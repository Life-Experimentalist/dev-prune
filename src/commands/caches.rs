// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune caches`.
//
// Every package manager keeps a machine-wide download cache outside any repository:
// npm's `_cacache`, pnpm's content-addressable store, the Go module cache, cargo's
// registry, Maven's local repository, NuGet's global packages folder. They are
// frequently the largest reclaimable thing on a developer's disk and
// nobody notices, because nothing ever mentions them — a 4 GiB `GOMODCACHE` looks like
// free space that simply went missing.
//
// This command finds them, sizes them, and prints the command that clears each one.
//
// **Nothing here ever runs on its own.** A cache is shared by every project on the
// machine, so its contents are not something dev-prune can prove is recoverable for any
// one repository — which is the bar every deletion in the prune path has to clear. So no
// scheduler, no Git hook and no `devp run` will ever touch one, and `devp caches` on its
// own still deletes nothing.
//
// `devp caches clear <manager>` exists because typing the command this report already
// prints is the whole of what it does. It names what it is about to empty, says what
// that costs — a cleared cache turns the next `devp restore` into a download — and asks
// before it does it.
//
// Clearing prefers the manager's own subcommand (`npm cache clean --force`, `go clean
// -modcache`) over deleting a directory: the manager knows what is safe to keep, and its
// own bookkeeping stays consistent. The managers that ship no such subcommand — cargo,
// gradle, vcpkg — are cleared by removing the directory, and the path removed is the one
// this command resolved and sized, never a string handed to a shell.
//
// Maven is reported and never cleared. `~/.m2/repository` is an install target as well
// as a download cache, and `MAVEN_MANUAL` below is the long version of why that puts it
// out of reach of a tool that deletes only what it can prove is recoverable.
//
// Each manager is asked where its own cache lives rather than being assumed — a
// `CARGO_HOME`, a `--cache-dir`, a corporate `.npmrc` all move it. Every one of those
// queries is read-only, and a manager that is not installed falls back to the
// conventional location, so a cache left behind by an uninstalled manager still shows up.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use colored::Colorize as _;

use crate::adapters;
use crate::constants;
use crate::i18n;
use crate::json;
use crate::output;

/// One cache directory that exists on this machine.
pub struct CacheReport {
    /// The package manager that owns it.
    pub manager: &'static str,
    /// Which of that manager's caches this is, when it keeps more than one.
    pub kind: &'static str,
    /// Where it actually is, as resolved on this machine.
    pub path: PathBuf,
    /// Total size on disk.
    pub bytes: u64,
    /// The command that empties it, as a human would type it.
    ///
    /// Owned rather than borrowed because one row's command names a path: a pnpm store
    /// on a volume of its own is emptied by `pnpm store prune --store-dir <that store>`,
    /// and no fixed string can say which one.
    pub clear_command: String,
    /// How `devp caches clear` empties it.
    pub clear: Clear,
    /// What the user gives up by running that command, when it is more than time.
    pub note: Option<&'static str>,
    /// The size cap set for this manager in `cache_max_gb`, in gibibytes.
    ///
    /// `None` when none is set, which is the default and means this cache is never
    /// called too big.
    pub cap_gb: Option<u64>,
    /// Whether this manager's caches add up to more than [`Self::cap_gb`].
    ///
    /// Per *manager*, not per row: cargo keeps a registry cache and an unpacked source
    /// tree, go keeps a build cache and a module cache, and "cargo is over ten
    /// gigabytes" is a statement about the pair. Every row of an over-cap manager is
    /// marked, because clearing only one of them is not what the cap asked for.
    pub over_cap: bool,
    /// How many registered repositories use this manager, or `None` where dev-prune
    /// cannot say.
    ///
    /// `None` is not zero. It is the honest answer for the caches no adapter is named
    /// after — `pip`, `conda`, `nuget`, `conan`, `hex`, `playwright`, `puppeteer`,
    /// `huggingface`, `cypress` and `electron` — where deciding which projects feed them
    /// would mean inventing a mapping dev-prune has never verified, and it is the answer
    /// again when there is no registry to compare against. Only
    /// `Some(0)` means "nothing registered on this machine needs this", and that is the
    /// one reading `devp caches clear --unused` is allowed to act on.
    pub dependents: Option<usize>,
    /// Arguments appended to [`Self::clear`]'s command for this row alone.
    ///
    /// Empty for every cache a manager finds on its own. It exists for the one that a
    /// manager does *not*: `pnpm store prune` prunes the store for the filesystem it is
    /// run on, so emptying a store on another volume means naming it. Appended to both
    /// the command dev-prune runs and the [`Self::clear_command`] it prints, so the two
    /// cannot say different things.
    pub extra_args: Vec<String>,
}

/// How one cache is emptied.
#[derive(Clone, Copy)]
pub enum Clear {
    /// The manager's own subcommand, as `(program, args)`. Preferred wherever one
    /// exists — `pnpm store prune` and `uv cache prune` keep what is still referenced,
    /// which no directory delete can work out.
    Command(&'static str, &'static [&'static str]),
    /// Delete the directory this command resolved and sized. Only for the managers that
    /// ship nothing equivalent.
    Directory,
    /// Report it, print the command, and refuse to run it. For the one store that is not
    /// a cache: see the maven entry for the reason a deletion here cannot be proven
    /// recoverable. `why` is printed to the user in place of doing it.
    Manual { why: &'static str },
}

/// How to find one cache.
struct Probe {
    manager: &'static str,
    kind: &'static str,
    /// The manager's own answer to "where is it?", as `(program, args)`.
    ///
    /// All of these print a path and exit; none of them writes anything or creates the
    /// directory. `None` means the ecosystem has no such query and only the conventional
    /// locations are available.
    query: Option<(&'static str, &'static [&'static str])>,
    /// Appended to whatever [`Self::query`] answered, for the managers that will only
    /// name a directory one level above their cache.
    ///
    /// `gem env gemdir` prints the gem home, which holds installed gems, binstubs and
    /// the `.gem` archives that are the cache; `poetry config cache-dir` prints a
    /// directory holding both `artifacts/` and `virtualenvs/`. Sizing or deleting either
    /// answer whole would take environments and installed packages with it, so the probe
    /// names the one subdirectory that is genuinely a download cache. Ignored by the
    /// fallback list, which already spells the full path.
    query_suffix: Option<&'static str>,
    clear_command: &'static str,
    clear: Clear,
    note: Option<&'static str>,
}

/// cargo ships no cache subcommand, so the only honest "how do I clear this" is the
/// deletion itself. `cargo build` re-downloads and re-extracts what it needs.
#[cfg(windows)]
const CARGO_CACHE_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:USERPROFILE\.cargo\registry\cache";
#[cfg(not(windows))]
const CARGO_CACHE_CLEAR: &str = "rm -rf ~/.cargo/registry/cache";

#[cfg(windows)]
const CARGO_SRC_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.cargo\registry\src";
#[cfg(not(windows))]
const CARGO_SRC_CLEAR: &str = "rm -rf ~/.cargo/registry/src";

/// Maven has no cache subcommand either — `mvn dependency:purge-local-repository`
/// exists, but it needs a project to run in and re-resolves as it purges, which is not
/// "clear the cache". The honest command is the deletion, so that is what gets printed —
/// but dev-prune does not run it. See [`MAVEN_MANUAL`].
#[cfg(windows)]
const MAVEN_REPO_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.m2\repository";
#[cfg(not(windows))]
const MAVEN_REPO_CLEAR: &str = "rm -rf ~/.m2/repository";

/// Why `devp caches clear maven` refuses.
///
/// `~/.m2/repository` is the one entry in this table that is not a cache, and Maven does
/// not call it one either — it is the *local repository*, and `mvn install` writes into
/// it. Two things live there that no remote can hand back:
///
/// * artifacts put there by `mvn install:install-file`, which is the documented way to
///   use a jar that is in no repository at all — a driver behind a click-through
///   licence, a partner SDK, an internal artifact from before there was an internal
///   Nexus. There is nothing to re-download them *from*.
/// * `-SNAPSHOT` builds of the user's own modules, which are recoverable only for as
///   long as the source that produced them is still on the machine and still builds.
///
/// Maven does record which remote each artifact came from, in a `_remote.repositories`
/// file it documents as internal and free to change without notice — and one written
/// only by Maven 3 and later, so an older or legacy-mode repository has none at all.
/// Deleting on the strength of that would mean betting the unrecoverable half of the
/// tree on a file format with no compatibility promise. Sizing it and printing the
/// command is the whole of what can be done honestly.
const MAVEN_MANUAL: &str = "`~/.m2/repository` is Maven's local repository, not a \
     download cache: `mvn install` and `install:install-file` write artifacts there \
     that exist nowhere else, and nothing in the tree tells them apart from the \
     downloaded ones reliably enough to delete around. dev-prune sizes it and prints \
     the command; running it is yours to decide.";

#[cfg(windows)]
const GRADLE_CACHE_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.gradle\caches";
#[cfg(not(windows))]
const GRADLE_CACHE_CLEAR: &str = "rm -rf ~/.gradle/caches";

#[cfg(windows)]
const GRADLE_DISTS_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:USERPROFILE\.gradle\wrapper\dists";
#[cfg(not(windows))]
const GRADLE_DISTS_CLEAR: &str = "rm -rf ~/.gradle/wrapper/dists";

#[cfg(windows)]
const VCPKG_ARCHIVES_CLEAR: &str = r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\vcpkg\archives";
#[cfg(not(windows))]
const VCPKG_ARCHIVES_CLEAR: &str = "rm -rf ~/.cache/vcpkg/archives";

/// Hex has no cache-clearing task. hexpm/hex#344 asked for one and there still is not
/// one, so the honest command is the deletion; `mix deps.get` re-fetches the tarballs.
#[cfg(windows)]
const HEX_CACHE_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.hex\packages";
#[cfg(not(windows))]
const HEX_CACHE_CLEAR: &str = "rm -rf ~/.hex/packages";

/// RubyGems has no command for this. `gem cleanup` removes *older versions of
/// installed gems*, which touches the installed tree and leaves the downloads alone —
/// the opposite of what is wanted here. The `.gem` archives under the gem home are the
/// cache, and deleting them is the only command there is.
#[cfg(windows)]
const BUNDLER_CACHE_CLEAR: &str = "Remove-Item -Recurse -Force \"$(gem env gemdir)\\cache\"";
#[cfg(not(windows))]
const BUNDLER_CACHE_CLEAR: &str = "rm -rf \"$(gem env gemdir)/cache\"";

/// `dart pub cache clean` empties the whole pub cache, including the `git/` checkouts
/// and the `bin/` shims `dart pub global activate` writes — none of which is a download
/// this tool can prove recoverable. `hosted/` is the part that is, so that is the part
/// reported and the part deleted.
#[cfg(windows)]
const DART_HOSTED_CLEAR: &str = r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\Pub\Cache\hosted";
#[cfg(not(windows))]
const DART_HOSTED_CLEAR: &str = "rm -rf ~/.pub-cache/hosted";

/// `swift package purge-cache` exists but has to be run from inside a package
/// directory, which makes it a per-project command for a machine-wide store. The
/// deletion is what a person can actually type.
#[cfg(windows)]
const SWIFTPM_CACHE_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\org.swift.swiftpm";
#[cfg(target_os = "macos")]
const SWIFTPM_CACHE_CLEAR: &str = "rm -rf ~/Library/Caches/org.swift.swiftpm";
#[cfg(not(any(windows, target_os = "macos")))]
const SWIFTPM_CACHE_CLEAR: &str = "rm -rf ~/.cache/org.swift.swiftpm";

#[cfg(windows)]
const TERRAFORM_PLUGIN_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:APPDATA\terraform.d\plugin-cache";
#[cfg(not(windows))]
const TERRAFORM_PLUGIN_CLEAR: &str = "rm -rf ~/.terraform.d/plugin-cache";

/// Poetry's cache directory holds `artifacts/` beside `virtualenvs/`. Only the first is
/// a cache; the second is where the environments themselves live, and nothing here will
/// go near it.
#[cfg(windows)]
const POETRY_ARTIFACTS_CLEAR: &str =
    "Remove-Item -Recurse -Force \"$(poetry config cache-dir)\\artifacts\"";
#[cfg(not(windows))]
const POETRY_ARTIFACTS_CLEAR: &str = "rm -rf \"$(poetry config cache-dir)/artifacts\"";

/// Playwright keeps one unpacked browser build per version and removes none of them on
/// upgrade, so this grows by a few hundred megabytes every time the dependency moves.
#[cfg(windows)]
const PLAYWRIGHT_CLEAR: &str = r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\ms-playwright";
#[cfg(target_os = "macos")]
const PLAYWRIGHT_CLEAR: &str = "rm -rf ~/Library/Caches/ms-playwright";
#[cfg(not(any(windows, target_os = "macos")))]
const PLAYWRIGHT_CLEAR: &str = "rm -rf ~/.cache/ms-playwright";

/// Puppeteer resolves its download directory from the home directory on every platform
/// rather than through the OS cache location, so this path is the same everywhere.
#[cfg(windows)]
const PUPPETEER_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.cache\puppeteer";
#[cfg(not(windows))]
const PUPPETEER_CLEAR: &str = "rm -rf ~/.cache/puppeteer";

/// `hf cache delete` — `huggingface-cli delete-cache` before it — is an interactive
/// picker: it draws a checklist and waits for a keypress. dev-prune runs its clear
/// commands with no terminal attached and a timeout, so driving that would hang until
/// the timeout and delete nothing. The directory delete is what can be done unattended.
#[cfg(windows)]
const HUGGINGFACE_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:USERPROFILE\.cache\huggingface\hub";
#[cfg(not(windows))]
const HUGGINGFACE_CLEAR: &str = "rm -rf ~/.cache/huggingface/hub";

#[cfg(windows)]
const CYPRESS_CLEAR: &str = r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\Cypress\Cache";
#[cfg(target_os = "macos")]
const CYPRESS_CLEAR: &str = "rm -rf ~/Library/Caches/Cypress";
#[cfg(not(any(windows, target_os = "macos")))]
const CYPRESS_CLEAR: &str = "rm -rf ~/.cache/Cypress";

#[cfg(windows)]
const ELECTRON_CLEAR: &str = r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\electron\Cache";
#[cfg(target_os = "macos")]
const ELECTRON_CLEAR: &str = "rm -rf ~/Library/Caches/electron";
#[cfg(not(any(windows, target_os = "macos")))]
const ELECTRON_CLEAR: &str = "rm -rf ~/.cache/electron";

#[cfg(windows)]
const ELECTRON_BUILDER_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:LOCALAPPDATA\electron-builder\Cache";
#[cfg(target_os = "macos")]
const ELECTRON_BUILDER_CLEAR: &str = "rm -rf ~/Library/Caches/electron-builder";
#[cfg(not(any(windows, target_os = "macos")))]
const ELECTRON_BUILDER_CLEAR: &str = "rm -rf ~/.cache/electron-builder";

const PROBES: &[Probe] = &[
    Probe {
        manager: "npm",
        kind: "cache",
        query: Some(("npm", &["config", "get", "cache"])),
        query_suffix: None,
        clear_command: "npm cache clean --force",
        clear: Clear::Command("npm", &["cache", "clean", "--force"]),
        note: None,
    },
    Probe {
        manager: "pnpm",
        kind: "store",
        query: Some(("pnpm", &["store", "path"])),
        query_suffix: None,
        clear_command: "pnpm store prune",
        clear: Clear::Command("pnpm", &["store", "prune"]),
        note: Some(
            "hardlinked into every node_modules it filled; emptying it is what makes the \
             next pnpm install a download",
        ),
    },
    Probe {
        manager: "yarn",
        kind: "cache",
        query: Some(("yarn", &["cache", "dir"])),
        query_suffix: None,
        clear_command: "yarn cache clean",
        clear: Clear::Command("yarn", &["cache", "clean"]),
        note: None,
    },
    Probe {
        manager: "bun",
        kind: "cache",
        query: Some(("bun", &["pm", "cache"])),
        query_suffix: None,
        clear_command: "bun pm cache rm",
        clear: Clear::Command("bun", &["pm", "cache", "rm"]),
        note: None,
    },
    Probe {
        manager: "uv",
        kind: "cache",
        query: Some(("uv", &["cache", "dir"])),
        query_suffix: None,
        // `prune` drops what nothing can use again and keeps the rest; `uv cache clean`
        // is the sledgehammer, and is not what most people mean by "clear the cache".
        clear_command: "uv cache prune",
        clear: Clear::Command("uv", &["cache", "prune"]),
        note: None,
    },
    Probe {
        manager: "pip",
        kind: "cache",
        query: Some(("pip", &["cache", "dir"])),
        query_suffix: None,
        clear_command: "pip cache purge",
        clear: Clear::Command("pip", &["cache", "purge"]),
        note: None,
    },
    // conda ships a command that prints the package directories, but it is `conda config
    // --show pkgs_dirs` and conda takes seconds to start on a cold shell — the same price
    // Maven charges, for the same read-only size report. So this row is the conventional
    // locations plus `CONDA_EXE`, which every conda shell exports and which names the
    // installation root wherever someone put it.
    Probe {
        manager: "conda",
        kind: "package cache",
        query: None,
        query_suffix: None,
        clear_command: "conda clean --packages --tarballs --yes",
        clear: Clear::Command("conda", &["clean", "--packages", "--tarballs", "--yes"]),
        note: Some(
            "unpacked packages and downloaded archives; conda keeps what its \
             environments use, except any it linked by symlink rather than hardlink",
        ),
    },
    Probe {
        manager: "cargo",
        kind: "registry cache",
        query: None,
        query_suffix: None,
        clear_command: CARGO_CACHE_CLEAR,
        clear: Clear::Directory,
        note: Some("the downloaded .crate archives; clearing them means downloading again"),
    },
    Probe {
        manager: "cargo",
        kind: "registry sources",
        query: None,
        query_suffix: None,
        clear_command: CARGO_SRC_CLEAR,
        clear: Clear::Directory,
        note: Some("unpacked copies of the archives above; cargo re-extracts these offline"),
    },
    Probe {
        manager: "go",
        kind: "module cache",
        query: Some(("go", &["env", "GOMODCACHE"])),
        query_suffix: None,
        clear_command: "go clean -modcache",
        clear: Clear::Command("go", &["clean", "-modcache"]),
        note: None,
    },
    Probe {
        manager: "go",
        kind: "build cache",
        query: Some(("go", &["env", "GOCACHE"])),
        query_suffix: None,
        clear_command: "go clean -cache",
        clear: Clear::Command("go", &["clean", "-cache"]),
        note: Some("compiled build artifacts; clearing them means the next build is a cold one"),
    },
    // `mvn help:evaluate -Dexpression=settings.localRepository` would answer precisely,
    // but it boots a JVM, resolves the help plugin over the network on first use, and
    // takes several seconds — the wrong trade for a read-only size report. A relocated
    // repository (settings.xml `<localRepository>`) is rare enough to miss.
    Probe {
        manager: "maven",
        kind: "local repository",
        query: None,
        query_suffix: None,
        clear_command: MAVEN_REPO_CLEAR,
        clear: Clear::Manual { why: MAVEN_MANUAL },
        note: Some(
            "every Maven build on the machine resolves from here, and `mvn install` writes here too — dev-prune will not delete it for you",
        ),
    },
    Probe {
        manager: "gradle",
        kind: "caches",
        query: None,
        query_suffix: None,
        clear_command: GRADLE_CACHE_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "downloaded dependencies and build caches shared by every Gradle project; rebuilt on demand",
        ),
    },
    Probe {
        manager: "gradle",
        kind: "wrapper distributions",
        query: None,
        query_suffix: None,
        clear_command: GRADLE_DISTS_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "one full Gradle per version any wrapper ever asked for; re-downloaded on demand",
        ),
    },
    // `dotnet nuget locals global-packages --list` answers `global-packages: <path>` —
    // a labelled line, not a bare path — so the conventional locations are simpler and
    // just as reliable. The clear command, however, is nuget's own.
    Probe {
        manager: "nuget",
        kind: "global packages",
        query: None,
        query_suffix: None,
        clear_command: "dotnet nuget locals global-packages --clear",
        clear: Clear::Command("dotnet", &["nuget", "locals", "global-packages", "--clear"]),
        note: Some(
            "every .NET project on the machine restores from here; re-downloaded on the next restore",
        ),
    },
    Probe {
        manager: "vcpkg",
        kind: "binary cache",
        query: None,
        query_suffix: None,
        clear_command: VCPKG_ARCHIVES_CLEAR,
        clear: Clear::Directory,
        note: Some("prebuilt package archives; vcpkg rebuilds from source what it cannot re-fetch"),
    },
    Probe {
        manager: "conan",
        kind: "package cache",
        query: None,
        query_suffix: None,
        clear_command: "conan remove \"*\" --confirm",
        clear: Clear::Command("conan", &["remove", "*", "--confirm"]),
        note: Some(
            "recipes and binaries shared by every Conan project; re-fetched on the next install",
        ),
    },
    // Composer will say where its cache is, and asking is the only way to get it right:
    // the directory moves with `COMPOSER_HOME`, with `COMPOSER_CACHE_DIR`, and with a
    // `cache-dir` written into the global config, and the default differs on all three
    // platforms. That is four ways to be wrong and one command that is not.
    Probe {
        manager: "composer",
        kind: "cache",
        query: Some(("composer", &["config", "--global", "cache-dir"])),
        query_suffix: None,
        clear_command: "composer clear-cache",
        clear: Clear::Command("composer", &["clear-cache"]),
        note: Some(
            "downloaded package archives and repository metadata; re-fetched by the next composer install",
        ),
    },
    // CocoaPods ships no command that prints the cache directory — `pod cache list`
    // prints its *contents* — so this row is the conventional location plus the
    // relocation variable. Emptying it is still CocoaPods' own job: the cache is keyed by
    // pod name and version and it keeps an index of what is in there.
    Probe {
        manager: "cocoapods",
        kind: "cache",
        query: None,
        query_suffix: None,
        clear_command: "pod cache clean --all",
        clear: Clear::Command("pod", &["cache", "clean", "--all"]),
        note: Some("downloaded pod sources, re-fetched by the next pod install"),
    },
    Probe {
        manager: "hex",
        kind: "package cache",
        query: None,
        query_suffix: None,
        clear_command: HEX_CACHE_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "package tarballs shared by every Mix project on the machine; re-fetched by the next mix deps.get",
        ),
    },
    // ---------------------------------------------------------------------------
    // Adapters that prune a project directory but whose machine-wide store had no
    // probe: `devp run` cleaned the project and `devp caches` could not see where the
    // bytes it had just freed came back from.
    // ---------------------------------------------------------------------------
    Probe {
        manager: "bundler",
        kind: "gem cache",
        query: Some(("gem", &["env", "gemdir"])),
        query_suffix: Some("cache"),
        clear_command: BUNDLER_CACHE_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "only the `cache/` subdirectory of the gem home, which holds the downloaded \
             `.gem` archives. The installed gems beside it, and the binstubs in `bin/`, \
             are untouched — including anything put there by `gem install ./local.gem`, \
             which no remote could hand back.",
        ),
    },
    Probe {
        manager: "dart",
        kind: "pub cache",
        query: None,
        query_suffix: None,
        clear_command: DART_HOSTED_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "only `hosted/`, the packages pub downloaded from a registry. The `git/` \
             checkouts and the executables `dart pub global activate` installed live \
             beside it and are left alone; `dart pub cache clean` would take all three.",
        ),
    },
    Probe {
        manager: "swift",
        kind: "swiftpm cache",
        query: None,
        query_suffix: None,
        clear_command: SWIFTPM_CACHE_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "the shared repository and manifest cache SwiftPM fills for every package on \
             the machine. `swift build` re-clones what it needs, so the first build after \
             this one is a network build.",
        ),
    },
    Probe {
        manager: "terraform",
        kind: "plugin cache",
        query: None,
        query_suffix: None,
        clear_command: TERRAFORM_PLUGIN_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "this exists only if you switched it on with `TF_PLUGIN_CACHE_DIR` or \
             `plugin_cache_dir` in `.terraformrc`. Without it Terraform downloads a \
             private copy of every provider into each project's `.terraform/` — which is \
             what `devp run` prunes — and there is no shared store to report.",
        ),
    },
    Probe {
        manager: "poetry",
        kind: "artifact cache",
        query: Some(("poetry", &["config", "cache-dir"])),
        query_suffix: Some("artifacts"),
        clear_command: POETRY_ARTIFACTS_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "only `artifacts/`, the wheels and sdists Poetry downloaded. `virtualenvs/` \
             sits in the same cache directory and holds the environments themselves — \
             deleting that would not be clearing a cache.",
        ),
    },
    Probe {
        manager: "pdm",
        kind: "cache",
        query: Some(("pdm", &["config", "cache_dir"])),
        query_suffix: None,
        clear_command: "pdm cache clear",
        clear: Clear::Command("pdm", &["cache", "clear"]),
        note: None,
    },
    // ---------------------------------------------------------------------------
    // Stores with no adapter of the same name, added because each of them routinely
    // reaches gigabytes and nothing ever removes an old entry from them. They get no
    // `dependents` count, for the reason `Dependents` gives.
    // ---------------------------------------------------------------------------
    Probe {
        manager: "playwright",
        kind: "browser bundles",
        query: None,
        query_suffix: None,
        clear_command: PLAYWRIGHT_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "a full Chromium, Firefox and WebKit build per Playwright version, and the \
             old ones are never removed when the dependency moves. `npx playwright \
             install` re-downloads only the versions the installed Playwright asks for, \
             which is usually why this comes back smaller than it was.",
        ),
    },
    Probe {
        manager: "puppeteer",
        kind: "browser bundles",
        query: None,
        query_suffix: None,
        clear_command: PUPPETEER_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "one Chrome build per Puppeteer version. `npx puppeteer browsers install` \
             re-downloads the current one.",
        ),
    },
    Probe {
        manager: "huggingface",
        kind: "hub cache",
        query: None,
        query_suffix: None,
        clear_command: HUGGINGFACE_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "model weights, not packages. This is the one row here where re-downloading \
             is measured in tens of gigabytes and can fail outright: a gated or \
             now-private repository will not hand the file back, and a checkpoint from a \
             revision that has since been deleted is gone. Worth looking at what is in it \
             before emptying it.",
        ),
    },
    Probe {
        manager: "cypress",
        kind: "binary cache",
        query: None,
        query_suffix: None,
        clear_command: CYPRESS_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "one unpacked Electron application per Cypress version, around half a \
             gigabyte each, and upgrading leaves the previous one in place. `npx cypress \
             install` re-downloads the version the project pins.",
        ),
    },
    Probe {
        manager: "electron",
        kind: "download cache",
        query: None,
        query_suffix: None,
        clear_command: ELECTRON_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "the prebuilt Electron archives every `electron` dependency downloads, kept \
             per version and per architecture. Re-downloaded on the next install.",
        ),
    },
    Probe {
        manager: "electron",
        kind: "builder cache",
        query: None,
        query_suffix: None,
        clear_command: ELECTRON_BUILDER_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "electron-builder's own store of signing tools, `winCodeSign`, `nsis` and the \
             runtimes it packages with. Re-downloaded on the next build, which makes that \
             build slow rather than broken.",
        ),
    },
    Probe {
        manager: "deno",
        kind: "cache",
        query: None,
        query_suffix: None,
        clear_command: "deno clean",
        clear: Clear::Command("deno", &["clean"]),
        note: None,
    },
];

/// Run the `caches` command, for the whole machine or for one drive.
pub fn run(json_output: bool, volume: Option<&str>) -> Result<()> {
    // Before the size walk, so a drive that was mistyped costs nothing.
    let volume = volume.map(resolve_volume).transpose()?;

    let reg = registered();
    let mut reports = collect(!json_output, reg.as_ref());
    apply_caps(&mut reports, &caps());
    let deps = reg.as_ref().map(|r| dependents(r, !json_output));
    apply_dependents(&mut reports, deps.as_ref());

    // Narrowed last, after every verdict is decided, so each one stays a statement about
    // the whole machine. A cap is per manager wherever that manager's caches are, and
    // "no registered repository uses this" must not become true just because the
    // repositories that do use it are on another drive.
    if let Some(root) = &volume {
        reports = on_volume(reports, root);
    }

    // Asked here rather than left to `devp caches containers`, because the mistake this
    // report exists to prevent is someone clearing 6 GB of npm cache while a stopped
    // Docker daemon holds 40 GB they were never told about. It costs one `system df` per
    // installed engine and nothing at all on a machine with none.
    //
    // Skipped under `--volume`: an engine reports its own disk from inside a VM image
    // that has no path on this filesystem, so there is no honest drive to file it under
    // and quietly filing it under this one would be a fabricated number.
    let engines = if volume.is_some() {
        Vec::new()
    } else {
        container_summary(!json_output)
    };

    if json_output {
        return json::emit(&json::caches_document(
            &reports,
            deps.as_ref().map(|d| d.repositories),
            &engines,
            volume.as_deref(),
        ));
    }

    print_report(&reports, deps.as_ref(), volume.as_deref());
    crate::commands::containers::print_summary(&engines);
    Ok(())
}

/// Turn what someone typed after `--volume` into the root of a drive or filesystem.
///
/// Anything on the volume is accepted, not just its root, because the question is "which
/// drive" and a path the reader already has in their hand answers it.
fn resolve_volume(arg: &str) -> Result<PathBuf> {
    let path = absolutize(normalize_volume_arg(arg));
    volume_root(&path).ok_or_else(|| {
        anyhow::Error::new(crate::UsageError(format!(
            "`{arg}` is not a drive or a path on one. Name a drive (`--volume V:`), a \
             mount point (`--volume /mnt/data`), or any path that sits on the one you \
             mean."
        )))
    })
}

/// A relative path names a volume as well as an absolute one, and `--volume .` — the
/// drive you are standing on — is the shortest way to say it. Neither [`volume_root`]
/// can see it: the Windows one reads a prefix a relative path has not got, and the Unix
/// one walks ancestors that stop at the working directory instead of the mount point.
///
/// Only a path that exists is expanded. Every unrecognised word is also a valid relative
/// path on Windows, so absolutising unconditionally would turn `--volume typo` into a
/// silent report about the current drive — and "that is not a drive" is the answer that
/// sends someone back to check what they typed.
fn absolutize(path: PathBuf) -> PathBuf {
    if path.is_absolute() || !path.exists() {
        return path;
    }
    std::path::absolute(&path).unwrap_or(path)
}

/// What a person types when asked which drive, turned into a path with a root.
///
/// `V:` is a drive-*relative* path to Windows — "wherever the current directory on V:
/// is" — and has no root component, so [`volume_root`] refuses it. It is also, with a
/// bare `V`, exactly what gets typed. Anything else is passed through untouched and
/// stands or falls as a path.
#[cfg(windows)]
fn normalize_volume_arg(arg: &str) -> PathBuf {
    let trimmed = arg.trim();
    let letter = match trimmed.as_bytes() {
        [c] if c.is_ascii_alphabetic() => Some(*c),
        [c, b':'] if c.is_ascii_alphabetic() => Some(*c),
        _ => None,
    };
    match letter {
        Some(c) => PathBuf::from(format!("{}:\\", c.to_ascii_uppercase() as char)),
        None => PathBuf::from(trimmed),
    }
}

/// Unix has no drive letters to expand, so a mount point is already a path.
#[cfg(unix)]
fn normalize_volume_arg(arg: &str) -> PathBuf {
    PathBuf::from(arg.trim())
}

/// Whether two volume roots are the same volume.
///
/// Compared by what the prefix *means*, never by its text. A cache path that came back
/// from `canonicalize` carries the verbatim prefix — `\?\C:\` — and the drive someone
/// typed is `C:\`; those are one drive, and a string comparison says they are two, which
/// is a filter that silently reports every machine as having no caches anywhere.
#[cfg(windows)]
fn same_volume(a: &Path, b: &Path) -> bool {
    match (volume_key(a), volume_key(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// What a Windows path's prefix names, with the verbatim spelling and the case removed.
#[cfg(windows)]
fn volume_key(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    Some(match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            (letter.to_ascii_uppercase() as char).to_string()
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
            "{}\\{}",
            server.to_string_lossy().to_uppercase(),
            share.to_string_lossy().to_uppercase()
        ),
        other => format!("{other:?}").to_uppercase(),
    })
}

/// Unix paths are bytes, and two mount points that differ in case are two mount points.
#[cfg(unix)]
fn same_volume(a: &Path, b: &Path) -> bool {
    a == b
}

/// The rows that sit on one volume.
///
/// A row whose volume cannot be determined is dropped rather than kept: `--volume V:`
/// asks for what is on V:, and "we could not tell" is not that.
fn on_volume(reports: Vec<CacheReport>, root: &Path) -> Vec<CacheReport> {
    reports
        .into_iter()
        .filter(|r| volume_root(&r.path).is_some_and(|v| same_volume(&v, root)))
        .collect()
}

/// What each volume holds, largest first.
///
/// Rows whose volume is unknown are left out entirely rather than gathered under a
/// heading, because a subtotal that does not add up to the total printed above it is
/// worse than a subtotal that is missing.
fn volume_totals(reports: &[CacheReport]) -> Vec<(String, u64)> {
    let mut totals: Vec<(String, u64)> = Vec::new();
    for r in reports {
        let Some(root) = volume_root(&r.path) else {
            continue;
        };
        let label = output::clean_path(&root);
        match totals.iter_mut().find(|(l, _)| *l == label) {
            Some((_, bytes)) => *bytes += r.bytes,
            None => totals.push((label, r.bytes)),
        }
    }
    totals.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    totals
}

/// The container engines on this machine, behind the report's own spinner.
fn container_summary(spinner: bool) -> Vec<crate::commands::containers::EngineReport> {
    let pb = spinner.then(|| output::create_spinner("Asking the container engines..."));
    let engines = crate::commands::containers::collect(None);
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    engines
}

/// The user's `cache_max_gb`, or an empty map when the registry cannot be read.
///
/// A cap is a preference, and a preference that cannot be loaded is not a reason to
/// refuse to report cache sizes — the command's whole job still works without it.
fn caps() -> BTreeMap<String, u64> {
    crate::config::Registry::load()
        .map(|r| r.settings.cache_max_gb)
        .unwrap_or_default()
}

/// Mark every row whose *manager* is over the cap set for it.
///
/// Split out from [`collect`] so the size walk stays a measurement and the verdict stays
/// a separate, testable step over it.
fn apply_caps(reports: &mut [CacheReport], caps: &BTreeMap<String, u64>) {
    let mut totals: BTreeMap<&str, u64> = BTreeMap::new();
    for r in reports.iter() {
        *totals.entry(r.manager).or_default() += r.bytes;
    }
    for r in reports.iter_mut() {
        let Some(&gb) = caps.get(r.manager) else {
            continue;
        };
        r.cap_gb = Some(gb);
        r.over_cap = totals.get(r.manager).copied().unwrap_or(0)
            > gb.saturating_mul(crate::constants::BYTES_PER_GIB);
    }
}

/// The registered repositories that are actually on this disk.
///
/// Two of the questions this command answers are questions about the machine's
/// repositories rather than about its caches — which filesystems hold projects, and
/// which managers those projects use — so the registry is read once and handed to both.
struct Registered {
    /// Registry paths that still exist.
    paths: Vec<PathBuf>,
    /// The machine-wide scan depth, before any repository's own override.
    scan_depth: usize,
}

/// Load the registry, or nothing when there is nothing in it worth loading.
///
/// `None` — not an empty list — for a registry that will not load, holds no
/// repositories, or holds only paths that are no longer on disk. All three would
/// otherwise make every cache on the machine read as used by nobody, and `--unused`
/// would offer to empty the lot on the strength of a registry someone had simply not
/// filled in yet.
fn registered() -> Option<Registered> {
    let registry = crate::config::Registry::load().ok()?;
    let paths: Vec<PathBuf> = registry
        .repositories
        .keys()
        .filter(|p| p.exists())
        .cloned()
        .collect();
    if paths.is_empty() {
        return None;
    }
    Some(Registered {
        paths,
        scan_depth: registry.settings.scan_depth,
    })
}

/// How many registered repositories still use each package manager.
///
/// The report answers "how big is it". This answers the question that follows and that
/// nothing else on the machine can: *who still needs it*. A cache with no repository
/// behind it is sediment — everything in it was downloaded for projects that are no
/// longer here — and it is the only kind this tool will offer to clear on the strength
/// of a count.
struct Dependents {
    /// Registered repositories that are actually on this disk, and the denominator of
    /// every count below.
    repositories: usize,
    /// Repositories in which an adapter of this name was detected, keyed by manager.
    ///
    /// Only names that are both a cache in [`PROBES`] and an adapter appear at all. The
    /// rest are absent rather than zero, which is what carries the difference between
    /// "nothing uses it" and "dev-prune has no way to tell".
    by_manager: BTreeMap<&'static str, usize>,
}

/// Count the repositories behind each cache.
///
/// Only ever called with a [`Registered`], which is the thing that carries "there is
/// something here to count against" — see [`registered`] for why the absence of one is
/// not the same as a count of zero.
fn dependents(reg: &Registered, spinner: bool) -> Dependents {
    let pb = spinner.then(|| output::create_spinner("Checking which caches are still in use..."));

    // Seeded at zero for every cache an adapter is named after, so a manager nothing uses
    // is a counted zero rather than a missing key. The ten that are absent — `pip`,
    // `conda`, `nuget`, `conan`, `hex`, `playwright`, `puppeteer`, `huggingface`,
    // `cypress` and `electron` — stay absent: dev-prune ships no adapter of those names,
    // and deciding that `venv` feeds `pip`, or that every `node_modules` on the disk
    // feeds the Playwright browser cache, would be a guess standing in for a
    // measurement.
    let mut by_manager: BTreeMap<&'static str, usize> = PROBES
        .iter()
        .map(|p| p.manager)
        .filter(|m| adapters::is_adapter_name(m))
        .map(|m| (m, 0))
        .collect();

    for path in &reg.paths {
        // The repository's own `scan_depth` where it sets one, read exactly as a prune
        // pass reads it: a monorepo that had to raise its depth to be pruned properly has
        // to be walked to that same depth here, or its projects are invisible and the
        // managers behind them are undercounted.
        let depth = crate::workspace::clamp_depth(
            crate::config::PerRepoConfig::load_with_diagnostics(path)
                .ok()
                .flatten()
                .and_then(|c| c.scan_depth)
                .unwrap_or(reg.scan_depth),
        );
        let mut here: HashSet<&'static str> = HashSet::new();
        for project in crate::workspace::discover_all_to_depth(path, depth) {
            for adapter in &project.adapters {
                here.insert(adapter.name());
            }
        }
        for (manager, count) in by_manager.iter_mut() {
            if here.contains(manager) {
                *count += 1;
            }
        }
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    Dependents {
        repositories: reg.paths.len(),
        by_manager,
    }
}

/// Hand each row the count for its manager, and leave the rest at `None`.
fn apply_dependents(reports: &mut [CacheReport], deps: Option<&Dependents>) {
    let Some(deps) = deps else {
        return;
    };
    for r in reports.iter_mut() {
        r.dependents = deps.by_manager.get(r.manager).copied();
    }
}

/// What each manager's caches add up to, and how many rows it took.
///
/// The same total the cap is measured against, and for the same reason: "cargo" is one
/// cache to a person and two rows to this command. The row count rides along because a
/// total that spans more than one row has to say so where it is printed — see
/// [`used_by`].
fn manager_totals(reports: &[CacheReport]) -> BTreeMap<&'static str, (u64, usize)> {
    let mut totals: BTreeMap<&'static str, (u64, usize)> = BTreeMap::new();
    for r in reports {
        let entry = totals.entry(r.manager).or_default();
        entry.0 += r.bytes;
        entry.1 += 1;
    }
    totals
}

/// Find and size every cache on this machine, largest first.
fn collect(spinner: bool, reg: Option<&Registered>) -> Vec<CacheReport> {
    let pb = spinner.then(|| output::create_spinner("Measuring package manager caches..."));
    let from = query_dir();

    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut reports = Vec::new();

    for probe in PROBES {
        let Some(path) = locate(probe, &from) else {
            continue;
        };
        // Canonical, because two probes can land on the same directory — `GOCACHE` and
        // `GOMODCACHE` are both under `~/.cache` on Linux, and a machine can be
        // configured to share them. Counting one twice would inflate the total, which is
        // the one number this command exists to get right. It also settles the spelling:
        // a manager answers in whatever case and separators it likes, and two rows
        // disagreeing about how to write `C:\Users` reads like a bug.
        let path = path.canonicalize().unwrap_or(path);
        if !seen.insert(path.clone()) {
            continue;
        }
        reports.push(CacheReport {
            manager: probe.manager,
            kind: probe.kind,
            bytes: adapters::dir_size(&path),
            path,
            clear_command: probe.clear_command.to_string(),
            clear: probe.clear,
            note: probe.note,
            cap_gb: None,
            over_cap: false,
            dependents: None,
            extra_args: Vec::new(),
        });
    }

    // After the probes, so the ordinary case — home and projects on one filesystem, one
    // store, already found — does not get reported twice.
    for store in reg.map(|r| volume_stores(&r.paths)).unwrap_or_default() {
        if !seen.insert(store.canonicalize().unwrap_or_else(|_| store.clone())) {
            continue;
        }
        reports.push(volume_store_report(store));
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    reports.sort_by_key(|r| std::cmp::Reverse(r.bytes));
    reports
}

/// Why a second pnpm store on one machine is not a duplicate.
const PNPM_VOLUME_NOTE: &str = "one store per filesystem, because a hardlink into node_modules cannot cross one; \
     this is the store for the projects on this volume";

/// One row for a pnpm store that lives on a volume of its own.
///
/// The printed command and the arguments dev-prune runs are built from the same path, in
/// one place, because the whole point of printing a command is that it is the one being
/// run.
fn volume_store_report(store: PathBuf) -> CacheReport {
    let named = output::clean_path(&store);
    CacheReport {
        manager: "pnpm",
        kind: "store",
        bytes: adapters::dir_size(&store),
        clear_command: format!("pnpm store prune --store-dir {}", shell_arg(&named)),
        extra_args: vec!["--store-dir".to_string(), named],
        path: store,
        clear: Clear::Command("pnpm", &["store", "prune"]),
        note: Some(PNPM_VOLUME_NOTE),
        cap_gb: None,
        over_cap: false,
        dependents: None,
    }
}

/// Quote a path for the command line this report prints, and only when it needs it.
///
/// Only for display. The command dev-prune runs passes the path as one argument and
/// never goes near a shell.
fn shell_arg(named: &str) -> String {
    if named.contains(' ') {
        format!("\"{named}\"")
    } else {
        named.to_string()
    }
}

/// pnpm stores sitting on a filesystem of their own, one per volume that holds a
/// registered repository.
///
/// pnpm hardlinks its store into every `node_modules` it fills, and a hardlink cannot
/// cross a filesystem. So a project that is not on the home directory's filesystem does
/// not use the store beside the home directory: pnpm puts one at the root of *that*
/// filesystem and fills it with everything those projects need. This is not a Windows
/// idea. It is the same rule for a second drive on Windows, a separate `/home` or
/// `/mnt/data` on Linux, and an external volume under `/Volumes` on macOS.
///
/// It has to be looked for, because the query the pnpm row otherwise trusts — `pnpm
/// store path` — answers for the filesystem it is run on, and it is run from the home
/// directory. On a machine whose projects all live on a second drive, that answer is a
/// nearly empty store and the real one, the multi-gigabyte one, is invisible.
fn volume_stores(repos: &[PathBuf]) -> Vec<PathBuf> {
    // The volume the command was run from counts as well as the registered ones. A
    // machine with nothing linked yet has no registry to read, and standing in the
    // project whose store this is is the one moment dev-prune can still find it.
    let mut roots = volume_roots(repos);
    if let Ok(here) = std::env::current_dir()
        && let Some(root) = volume_root(&here)
        && !roots.contains(&root)
    {
        roots.push(root);
    }
    roots
        .into_iter()
        .map(|root| root.join(constants::PNPM_VOLUME_STORE_DIR))
        .filter(|store| store.is_dir())
        .collect()
}

/// The distinct filesystems a set of repositories sits on, in the order first seen.
fn volume_roots(repos: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for repo in repos {
        if let Some(root) = volume_root(repo)
            && !roots.contains(&root)
        {
            roots.push(root);
        }
    }
    roots
}

/// The root of the filesystem `path` sits on.
///
/// Mount points are found by device number rather than by parsing a mount table:
/// `/proc/mounts` is Linux-only, the output of `mount` is not a format, and `st_dev` is
/// the same answer on every Unix. The highest ancestor still on the same device is where
/// the filesystem starts.
#[cfg(unix)]
pub(crate) fn volume_root(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    let dev = std::fs::metadata(path).ok()?.dev();
    let mut root = path.to_path_buf();
    for ancestor in path.ancestors().skip(1) {
        match std::fs::metadata(ancestor) {
            Ok(m) if m.dev() == dev => root = ancestor.to_path_buf(),
            _ => break,
        }
    }
    Some(root)
}

/// The root of the volume `path` sits on: `V:\`, or `\\server\share\` for a UNC path.
///
/// Windows can also mount a volume into an empty directory of another one, which this
/// does not see. A drive letter is what a developer with a second disk actually has, and
/// the cost of missing the other case is a cache that goes unreported rather than one
/// that is wrongly cleared.
#[cfg(windows)]
pub(crate) fn volume_root(path: &Path) -> Option<PathBuf> {
    use std::path::Component;

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let mut root = PathBuf::from(prefix.as_os_str());
    root.push(Component::RootDir.as_os_str());
    Some(root)
}

/// Where to run the "where is your cache?" queries from.
///
/// The home directory, not the current one. A project's `.npmrc` or `.cargo/config.toml`
/// can move the cache for that project alone, and answering with it would report a
/// directory that is not the machine's actual cache. Falling back to the current
/// directory is only for the case where there is no home directory at all.
fn query_dir() -> PathBuf {
    dirs::home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve one probe to a directory that exists, or nothing.
fn locate(probe: &Probe, from: &Path) -> Option<PathBuf> {
    if let Some((program, args)) = probe.query
        && adapters::binary_available(program)
    {
        let answered = adapters::capture_command_with_timeout(
            program,
            args,
            from,
            std::time::Duration::from_secs(constants::CACHE_QUERY_TIMEOUT_SECS),
        )
        .ok()
        .and_then(|raw| path_from_output(&raw))
        .map(|p| match probe.query_suffix {
            Some(suffix) => suffix.split('/').fold(p, |acc, seg| acc.join(seg)),
            None => p,
        })
        .filter(|p| p.is_dir());
        if answered.is_some() {
            return answered;
        }
    }

    // Either the manager is not installed, or it is and its cache has never been
    // populated. The conventional location is still worth checking: an uninstalled
    // manager leaves its cache behind, and that is exactly the multi-gigabyte directory
    // nobody remembers.
    fallbacks(probe.manager, probe.kind)
        .into_iter()
        .find(|p| p.is_dir())
}

/// Read a path out of a manager's answer.
///
/// The last non-empty line, because some managers print a notice first, and quotes are
/// stripped because `go env` quotes paths containing spaces on Windows.
fn path_from_output(raw: &str) -> Option<PathBuf> {
    let line = raw.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let line = line.trim_matches('"');
    // npm answers `undefined` for a config key it does not have, and a manager that
    // errored can print anything at all. A relative path is never a machine-wide cache.
    if line.is_empty() || line == "undefined" || !Path::new(line).is_absolute() {
        return None;
    }
    Some(PathBuf::from(line))
}

/// Conventional locations for a cache, most likely first.
fn fallbacks(manager: &str, kind: &str) -> Vec<PathBuf> {
    let home = dirs::home_dir();
    let local = dirs::data_local_dir();
    let cache = dirs::cache_dir();
    // `rel` is split rather than joined whole so a Windows path never comes out as
    // `C:\Users\dev\go\pkg/mod`. `Path::join` accepts the forward slashes, it just keeps
    // them, and a report that spells the same drive two ways reads like a bug.
    let under = |base: &Option<PathBuf>, rel: &str| {
        base.as_ref()
            .map(|b| rel.split('/').fold(b.clone(), |p, seg| p.join(seg)))
    };

    let candidates = match (manager, kind) {
        // `npm config get cache` answers `~/.npm` on Unix and `%LocalAppData%\npm-cache`
        // on Windows; the payload lives in `_cacache` underneath either one.
        ("npm", _) => vec![under(&local, "npm-cache"), under(&home, ".npm")],
        ("pnpm", _) => vec![
            under(&local, "pnpm/store"),
            under(&home, ".local/share/pnpm/store"),
            under(&home, "Library/pnpm/store"),
            under(&home, ".pnpm-store"),
        ],
        ("yarn", _) => vec![
            under(&home, ".yarn/berry/cache"),
            under(&local, "Yarn/Cache"),
            under(&cache, "yarn"),
        ],
        ("bun", _) => vec![under(&home, ".bun/install/cache")],
        ("uv", _) => vec![under(&cache, "uv"), under(&local, "uv/cache")],
        ("pip", _) => vec![under(&cache, "pip"), under(&local, "pip/Cache")],
        // `CONDA_PKGS_DIRS` names one directory in practice; conda's own multi-value
        // support for it is still a feature request, so this is not split on anything.
        // `CONDA_EXE` is `<root>/bin/conda` on Unix and `<root>\Scripts\conda.exe` on
        // Windows, so the grandparent is the installation root either way — the only way
        // to find a conda that is not in one of the default places. `~/.conda/pkgs` is
        // where conda falls back when the root is not writable, which is every managed
        // multi-user install.
        ("conda", _) => vec![
            std::env::var_os("CONDA_PKGS_DIRS").map(PathBuf::from),
            std::env::var_os("CONDA_EXE")
                .map(PathBuf::from)
                .and_then(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf))
                .map(|root| root.join("pkgs")),
            under(&home, "miniconda3/pkgs"),
            under(&home, "anaconda3/pkgs"),
            under(&home, "miniforge3/pkgs"),
            under(&home, "mambaforge/pkgs"),
            under(&home, ".conda/pkgs"),
        ],
        ("cargo", "registry cache") => vec![Some(cargo_home().join("registry").join("cache"))],
        ("cargo", _) => vec![Some(cargo_home().join("registry").join("src"))],
        ("go", "module cache") => vec![
            std::env::var_os("GOMODCACHE").map(PathBuf::from),
            std::env::var_os("GOPATH").map(|p| PathBuf::from(p).join("pkg").join("mod")),
            under(&home, "go/pkg/mod"),
        ],
        ("go", _) => vec![
            std::env::var_os("GOCACHE").map(PathBuf::from),
            under(&cache, "go-build"),
            under(&local, "go-build"),
        ],
        ("maven", _) => vec![under(&home, ".m2/repository")],
        // GRADLE_USER_HOME relocates the whole ~/.gradle tree, caches and wrapper both.
        ("gradle", "caches") => vec![
            std::env::var_os("GRADLE_USER_HOME").map(|p| PathBuf::from(p).join("caches")),
            under(&home, ".gradle/caches"),
        ],
        ("gradle", _) => vec![
            std::env::var_os("GRADLE_USER_HOME")
                .map(|p| PathBuf::from(p).join("wrapper").join("dists")),
            under(&home, ".gradle/wrapper/dists"),
        ],
        ("nuget", _) => vec![
            std::env::var_os("NUGET_PACKAGES").map(PathBuf::from),
            under(&home, ".nuget/packages"),
        ],
        ("vcpkg", _) => vec![
            std::env::var_os("VCPKG_DEFAULT_BINARY_CACHE").map(PathBuf::from),
            under(&local, "vcpkg/archives"),
            under(&cache, "vcpkg/archives"),
        ],
        // Conan 2 keeps packages under <CONAN_HOME>/p; pointing at `p` rather than the
        // whole home keeps profiles and remotes out of the size (and out of harm's way).
        ("conan", _) => vec![
            std::env::var_os("CONAN_HOME").map(|p| PathBuf::from(p).join("p")),
            under(&home, ".conan2/p"),
        ],
        // Only reached when `composer` is not installed, which is the case worth
        // covering: the cache a PHP toolchain left behind is the one nobody remembers.
        ("composer", _) => vec![
            std::env::var_os("COMPOSER_CACHE_DIR").map(PathBuf::from),
            std::env::var_os("COMPOSER_HOME").map(|p| PathBuf::from(p).join("cache")),
            under(&local, "Composer"),
            under(&cache, "composer"),
            under(&home, ".composer/cache"),
        ],
        // CocoaPods puts the cache under `~/Library/Caches` by name rather than through
        // the platform's cache directory, so this is `home` and not `cache` even on the
        // one platform where the two would agree.
        ("cocoapods", _) => vec![
            std::env::var_os("CP_CACHE_DIR").map(PathBuf::from),
            under(&home, "Library/Caches/CocoaPods"),
        ],
        // HEX_HOME moves the whole `.hex` tree; MIX_XDG puts it under the platform cache
        // directory instead. Both are checked because either can be set alone.
        ("hex", _) => vec![
            std::env::var_os("HEX_HOME").map(|p| PathBuf::from(p).join("packages")),
            under(&home, ".hex/packages"),
            under(&cache, "hex/packages"),
        ],
        // `gem env gemdir` answers this whenever Ruby is installed. These are the shapes
        // a user-install tree takes when it is not — which is the case worth covering,
        // since a gem cache nobody can account for is one left behind by a toolchain
        // that has since been removed.
        ("bundler", _) => gem_cache_dirs(),
        ("dart", _) => vec![
            std::env::var_os("PUB_CACHE").map(|p| PathBuf::from(p).join("hosted")),
            under(&local, "Pub/Cache/hosted"),
            under(&home, ".pub-cache/hosted"),
        ],
        // SwiftPM's shared cache moved under the platform cache directory; `~/.swiftpm`
        // is where older toolchains put it and where one can still be sitting.
        ("swift", _) => vec![
            under(&cache, "org.swift.swiftpm"),
            under(&home, ".swiftpm/cache"),
        ],
        // Terraform has no default here at all: without `TF_PLUGIN_CACHE_DIR` or a
        // `plugin_cache_dir` line there is no shared store, and finding nothing is the
        // correct answer rather than a gap. The two paths are the ones the documentation
        // uses in its own example.
        ("terraform", _) => vec![
            std::env::var_os("TF_PLUGIN_CACHE_DIR").map(PathBuf::from),
            under(&home, ".terraform.d/plugin-cache"),
            under(&dirs::data_dir(), "terraform.d/plugin-cache"),
        ],
        ("poetry", _) => vec![
            std::env::var_os("POETRY_CACHE_DIR").map(|p| PathBuf::from(p).join("artifacts")),
            under(&cache, "pypoetry/Cache/artifacts"),
            under(&cache, "pypoetry/artifacts"),
        ],
        ("pdm", _) => vec![
            std::env::var_os("PDM_CACHE_DIR").map(PathBuf::from),
            under(&cache, "pdm/Cache"),
            under(&cache, "pdm"),
        ],
        ("playwright", _) => vec![
            std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").map(PathBuf::from),
            under(&cache, "ms-playwright"),
        ],
        // Puppeteer joins `.cache` onto the home directory itself instead of asking the
        // platform where caches go, so this is one path on all three and a
        // `%LOCALAPPDATA%` guess would miss it on Windows.
        ("puppeteer", _) => vec![
            std::env::var_os("PUPPETEER_CACHE_DIR").map(PathBuf::from),
            under(&home, ".cache/puppeteer"),
        ],
        // Same shape, same reason: `huggingface_hub` defaults to `~/.cache/huggingface`
        // on every platform.
        ("huggingface", _) => vec![
            std::env::var_os("HF_HUB_CACHE").map(PathBuf::from),
            std::env::var_os("HF_HOME").map(|p| PathBuf::from(p).join("hub")),
            under(&home, ".cache/huggingface/hub"),
        ],
        // Cypress, Electron and electron-builder all take the platform cache directory
        // and add a `Cache` segment on Windows only. Both shapes are listed and the first
        // that exists wins, which is cheaper than a third `cfg` for one path segment.
        ("cypress", _) => vec![
            std::env::var_os("CYPRESS_CACHE_FOLDER").map(PathBuf::from),
            under(&cache, "Cypress/Cache"),
            under(&cache, "Cypress"),
        ],
        ("electron", "download cache") => vec![
            std::env::var_os("electron_config_cache").map(PathBuf::from),
            under(&cache, "electron/Cache"),
            under(&cache, "electron"),
        ],
        ("electron", _) => vec![
            under(&cache, "electron-builder/Cache"),
            under(&cache, "electron-builder"),
        ],
        ("deno", _) => vec![
            std::env::var_os("DENO_DIR").map(PathBuf::from),
            under(&cache, "deno"),
        ],
        _ => vec![],
    };

    candidates.into_iter().flatten().collect()
}

/// Every `cache/` directory under a conventional user-install gem tree.
///
/// The version segment in the middle — `~/.gem/ruby/3.4.0/cache` — is not something a
/// fixed path can name, so the parents are read instead of guessed at. The last entry is
/// unconditional so that this probe always has *some* conventional location to point at,
/// which is what keeps the "every probe can be found without its manager installed"
/// check meaningful rather than accidentally satisfied.
fn gem_cache_dirs() -> Vec<Option<PathBuf>> {
    let mut out = vec![std::env::var_os("GEM_HOME").map(|p| PathBuf::from(p).join("cache"))];
    let home = dirs::home_dir();
    for parent in [".gem/ruby", ".local/share/gem/ruby"] {
        let Some(base) = home
            .as_ref()
            .map(|h| parent.split('/').fold(h.clone(), |p, seg| p.join(seg)))
        else {
            continue;
        };
        if let Ok(entries) = std::fs::read_dir(&base) {
            out.extend(entries.flatten().map(|e| Some(e.path().join("cache"))));
        }
    }
    out.push(home.map(|h| h.join(".gem").join("cache")));
    out
}

/// `CARGO_HOME`, or the default cargo puts it in.
fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
}

fn print_report(reports: &[CacheReport], deps: Option<&Dependents>, volume: Option<&Path>) {
    output::print_header(i18n::t("caches.header"));

    if reports.is_empty() {
        println!();
        match volume {
            Some(root) => output::print_info(&format!(
                "No package manager cache on this machine sits on {}. They are somewhere \
                 else — run `devp caches` without `--volume` to see where.",
                output::clean_path(root)
            )),
            None => output::print_info(i18n::t("caches.nothing")),
        }
        return;
    }

    println!();
    let totals = manager_totals(reports);
    // One line per manager, not per row: cargo's registry cache and its sources have the
    // same repositories behind them, and saying so twice reads as two findings.
    let mut counted: HashSet<&'static str> = HashSet::new();
    for r in reports {
        let label = format!("{} {}", r.manager, r.kind);
        println!(
            "  {:<30} {:>10}  {}",
            label,
            output::format_bytes(r.bytes),
            output::clean_path(&r.path)
        );
        // The manager's own command was the only one printed here, which left the
        // wrapper missing from the one place someone reads before deciding to empty
        // something. It is not a synonym for the line below it: what `devp caches clear`
        // frees is added to the lifetime total in `devp stats`, and the same command
        // typed into a terminal is invisible to dev-prune, so the report a week later is
        // short by exactly the space this cleared. The manual command still gets its
        // line — dev-prune runs it, and hiding what it runs would be worse than
        // repeating it.
        // Repeated on both of a manager's rows rather than named once: the rows are
        // ordered by size, so cargo's two are rarely adjacent and a command printed
        // beside the first would be nowhere near the second.
        let clearable = !matches!(r.clear, Clear::Manual { .. });
        if clearable {
            println!(
                "  {:<30} {:>10}  clear: devp caches clear {}",
                "", "", r.manager
            );
        }
        let verb = if clearable { "runs: " } else { "clear:" };
        println!("  {:<30} {:>10}  {verb} {}", "", "", r.clear_command);
        if let Some(note) = r.note {
            println!("  {:<30} {:>10}  {}", "", "", note);
        }
        if r.over_cap
            && let Some(gb) = r.cap_gb
        {
            println!(
                "  {:<30} {:>10}  over the {gb} GiB cap you set for {}",
                "", "", r.manager
            );
        }
        if let Some(n) = r.dependents
            && counted.insert(r.manager)
        {
            println!("  {:<30} {:>10}  {}", "", "", used_by(r, n, deps, &totals));
        }
        println!();
    }

    let total: u64 = reports.iter().map(|r| r.bytes).sum();
    println!(
        "  {:<30} {:>10}  across {} {}",
        "Total",
        output::format_bytes(total),
        reports.len(),
        output::plural(reports.len(), "cache", "caches")
    );

    // Where the total actually is. On a machine whose projects live on a second disk,
    // "22 GiB of caches" is not the number that decides anything — the two gigabytes
    // sitting on the drive that is full is. Printed only when there is more than one
    // volume to tell apart, so it never appears under `--volume`, where there is one by
    // construction and the line would only repeat the total above it.
    let by_volume = volume_totals(reports);
    if by_volume.len() > 1 {
        let named = by_volume
            .iter()
            .map(|(label, bytes)| format!("{label} {}", output::format_bytes(*bytes)))
            .collect::<Vec<_>>()
            .join(" · ");
        println!("  {:<30} {:>10}  {named}", "By drive", "");
    }

    // A ranking, not a recommendation. Which of these is worth emptying depends on what
    // the reader is about to do with this machine, and inventing a threshold to call one
    // of them "too big" would be inventing a fact. Naming the order is enough.
    let ranked = costliest_per_repository(reports);
    if ranked.len() > 1 {
        let named = ranked
            .iter()
            .take(3)
            .map(|(m, b)| format!("{m} {}", output::format_bytes_weighted(*b)))
            .collect::<Vec<_>>()
            .join(" · ");
        println!("  {:<30} {:>10}  {named}", "Costliest per repository", "");
    }

    if reports.iter().any(|r| r.over_cap) {
        println!();
        output::print_info(
            "The caches marked above have outgrown the cap you set for them. `devp caches clear \
             --over-cap all` empties exactly those and leaves the rest alone.",
        );
    }

    if reports.iter().any(|r| r.dependents == Some(0)) {
        println!();
        output::print_info(
            "The caches above that no registered repository uses were filled for projects that \
             are not here any more. `devp caches clear --unused all` empties exactly those. It \
             counts only repositories dev-prune knows about, so `devp link` anything you keep \
             outside the registry before trusting the number.",
        );
    }

    if let Some(root) = volume {
        println!();
        output::print_info(&format!(
            "Only the caches on {} are above, and the container engines are not among \
             them: an engine reports its own disk from inside a VM image with no path on \
             this filesystem, so there is no drive to file it under — `devp caches \
             docker` has that number. The clear commands above are not drive-specific \
             either. They empty that manager's cache wherever it is, which for every \
             manager but pnpm is one place.",
            output::clean_path(root)
        ));
    }

    println!();
    output::print_info(
        "Nothing above was deleted. A cache is shared by every project on the machine, so \
         no single repository's lockfile can prove it is recoverable — and it is what \
         makes `devp restore` fast, which is why nothing dev-prune runs on a schedule \
         will ever touch one. When you want the space more than the speed, run a clear \
         command yourself, or `devp caches clear <manager>` — only the second is counted \
         in `devp stats`, because dev-prune never sees the first.",
    );
}

/// The one line that says who still needs this manager's caches.
///
/// The size beside the count is the manager's whole footprint divided by the number of
/// repositories behind it, which is the figure that actually decides anything: two
/// repositories holding a 12 GiB cache between them is 6 GiB each and worth a look; forty
/// repositories holding the same 12 GiB is 300 MiB each and is the cache doing its job.
///
/// Returned bold, because it is the conclusion of its block and everything above it is
/// plumbing — the path you already know and the command you only need once you have
/// decided. Set in the same weight as the rest, "cargo is used by 1 of 46 registered
/// repositories" was something you had to read the whole report to find. Weight and not
/// colour: whether a cache with one dependent is a problem depends on what the reader is
/// about to do, and a colour would answer that question for them.
fn used_by(
    r: &CacheReport,
    dependents: usize,
    deps: Option<&Dependents>,
    totals: &BTreeMap<&'static str, (u64, usize)>,
) -> String {
    if dependents == 0 {
        return format!("no registered repository uses {}", r.manager)
            .bold()
            .to_string();
    }
    let registered = deps.map_or(dependents, |d| d.repositories);
    let (total, rows) = totals.get(r.manager).copied().unwrap_or((r.bytes, 1));
    // Named rather than implied. The label column is blank on a continuation line, and
    // the figure is the manager's total across every row it has — so on go's two rows the
    // number beside "go build cache" is larger than that row's size and reads as an
    // arithmetic error until the sentence says what it summed.
    let share = output::format_bytes(total / dependents as u64);
    let each = if rows > 1 {
        format!("{share} each across its {rows} caches")
    } else {
        format!("{share} each")
    };
    format!(
        "{} is used by {dependents} of {registered} registered {} · {each}",
        r.manager,
        output::plural(registered, "repository", "repositories"),
    )
    .bold()
    .to_string()
}

/// Every manager something still needs, by what it costs one of them, worst first.
///
/// The report is ordered by total size, and the cache that costs a single repository the
/// most is routinely not the biggest one on the machine — a 2 GiB store serving one
/// project is a worse deal than a 10 GiB store serving eighteen, and the report as it
/// stands makes you read thirteen blocks and do that arithmetic yourself.
fn costliest_per_repository(reports: &[CacheReport]) -> Vec<(&'static str, u64)> {
    let totals = manager_totals(reports);
    // Per manager, not per row, for the same reason `used_by` prints once per manager:
    // cargo's registry cache and its sources are one cache with one set of dependents.
    let mut per: BTreeMap<&'static str, u64> = BTreeMap::new();
    for r in reports {
        if let Some(n) = r.dependents.filter(|n| *n > 0) {
            let total = totals.get(r.manager).map_or(r.bytes, |t| t.0);
            per.insert(r.manager, total / n as u64);
        }
    }
    let mut ranked: Vec<(&'static str, u64)> = per.into_iter().collect();
    // Size, then name: two managers costing the same must not swap places between runs.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    ranked
}

/// What happened to one cache.
pub struct ClearOutcome {
    /// The package manager that owned it.
    pub manager: &'static str,
    /// Which of that manager's caches this was.
    pub kind: &'static str,
    /// Where it is.
    pub path: PathBuf,
    /// Size before, as this command measured it.
    pub before: u64,
    /// Size after, measured again rather than assumed. `pnpm store prune` and `uv cache
    /// prune` deliberately keep what is still referenced, so subtracting is the only
    /// honest way to say what actually went.
    pub after: u64,
    /// `None` when it worked; otherwise why it did not, phrased for a human.
    pub problem: Option<String>,
}

impl ClearOutcome {
    /// Bytes given back to the disk.
    pub fn freed(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

/// Run `dev-prune caches clear <target>`.
///
/// `target` is a manager name or `all`. Everything about to be emptied is named and
/// sized first, and unless `--yes` answers for the user, it asks. `over_cap` narrows the
/// selection to managers that have outgrown their `cache_max_gb` entry, and `unused` to
/// managers no registered repository uses at all.
pub fn run_clear(
    target: &str,
    over_cap: bool,
    unused: bool,
    yes: bool,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    let all = target.eq_ignore_ascii_case("all");
    // An engine is cleared by the module that knows how to ask it questions; the dispatch
    // is here because `caches clear docker` is where a person looks for it.
    //
    // `all` deliberately does not reach it. A container store is the biggest thing on the
    // disk and the slowest to put back, and "clear all the caches" typed in a hurry
    // should not also mean re-pulling every base image tomorrow morning. Naming the
    // engine is the consent.
    if !all && crate::commands::containers::is_engine(target) {
        if over_cap || unused {
            return Err(anyhow::Error::new(crate::UsageError(format!(
                "`--over-cap` and `--unused` pick package manager caches by size cap and by \
                 which repositories still need them. Neither applies to {target}: an image \
                 belongs to no repository and `cache_max_gb` does not cover one. Run `devp \
                 caches clear {target}` on its own."
            ))));
        }
        return crate::commands::containers::run_clear(target, yes, dry_run, json_output);
    }
    if !all
        && !PROBES
            .iter()
            .any(|p| p.manager.eq_ignore_ascii_case(target))
    {
        return Err(anyhow::Error::new(crate::UsageError(format!(
            "`{target}` is not a manager dev-prune knows a cache for. Try one of: {}, or `all`.",
            known_managers().join(", ")
        ))));
    }
    // Naming a manager dev-prune only ever reports is asking for the one thing this
    // command does not do, so the reason is the answer — and it is the same answer
    // whether or not the store is on this machine, which is why it comes from the table
    // rather than from a size walk that would end in "nothing to clear".
    if !all
        && let Some(probe) = manual_only(target)
        && let Clear::Manual { why } = probe.clear
    {
        return Err(anyhow::Error::new(crate::UsageError(format!(
            "{why} The command is: {}",
            probe.clear_command
        ))));
    }

    // A prompt nobody can answer is a hang, and the "pass --yes" line printed in its
    // place would land in the middle of the JSON document and break the parse.
    if json_output && !yes && !dry_run {
        return Err(anyhow::Error::new(crate::UsageError(
            "`--json` cannot ask for confirmation — pass `--yes` as well, or `--dry-run` \
             to see what would go."
                .to_string(),
        )));
    }

    // Caps are applied to the whole measurement, before the name filter: a cap is per
    // manager and a manager's total is the sum of its rows, so narrowing first would let
    // `clear cargo --over-cap` compare a cap against half a cache.
    let reg = registered();
    let mut measured = collect(!json_output, reg.as_ref());
    apply_caps(&mut measured, &caps());

    // `--unused` is the only selection here that acts on a count rather than on a size,
    // so it refuses to run without one. An empty registry would otherwise make every
    // cache on the machine look unused, and this flag would agree to empty all of them.
    let deps = if unused {
        let Some(reg) = reg.as_ref() else {
            return Err(anyhow::Error::new(crate::UsageError(
                "`--unused` empties the caches no registered repository needs, and there are no \
                 registered repositories on this disk to check against — every cache would look \
                 unused. Register what you keep with `devp link` first."
                    .to_string(),
            )));
        };
        Some(dependents(reg, !json_output))
    } else {
        None
    };
    apply_dependents(&mut measured, deps.as_ref());

    // Split before anything is printed. A plan that lists a store dev-prune is never
    // going to empty is a promise it cannot keep, and the JSON record of the run would
    // carry the same lie.
    let (reports, kept): (Vec<CacheReport>, Vec<CacheReport>) = measured
        .into_iter()
        .filter(|r| all || r.manager.eq_ignore_ascii_case(target))
        .filter(|r| !over_cap || r.over_cap)
        .filter(|r| !unused || r.dependents == Some(0))
        .partition(|r| !matches!(r.clear, Clear::Manual { .. }));

    if reports.is_empty() {
        if json_output {
            return json::emit(&json::caches_clear_plan_document(&reports, &kept));
        }
        if unused {
            output::print_info(
                "Every cache on this machine is used by at least one registered repository, or \
                 is one dev-prune cannot attribute to any — nothing to clear.",
            );
            return Ok(());
        }
        if over_cap {
            // Two very different situations read the same from here — no caps set at
            // all, and caps set that nothing has reached — so say which one it is. The
            // first is a setting the user has not made yet; the second is good news.
            output::print_info(if caps().is_empty() {
                "No cache size caps are set, so nothing is over one. Set them with `devp config \
                 set cache_max_gb npm=10,uv=10`, or in `devp config wizard`."
            } else {
                "Every capped cache is under its cap — nothing to clear."
            });
            return Ok(());
        }
        output::print_info(&format!(
            "No {} cache on this machine — nothing to clear.",
            if all { "package manager" } else { target }
        ));
        return Ok(());
    }

    if dry_run {
        if json_output {
            return json::emit(&json::caches_clear_plan_document(&reports, &kept));
        }
        print_kept(&kept);
        print_clear_plan(&reports, true);
        return Ok(());
    }

    if !json_output {
        print_kept(&kept);
        print_clear_plan(&reports, false);
        if !confirm_clear(yes) {
            output::print_info("Nothing was cleared.");
            return Ok(());
        }
    }

    let outcomes: Vec<ClearOutcome> = reports.iter().map(clear_one).collect();
    // Before either output path, so both credit it. Everything above this line has
    // already returned — a dry run never reaches here, and neither does a
    // declined confirmation.
    record_cache_clear(outcomes.iter().map(ClearOutcome::freed).sum());

    if json_output {
        json::emit(&json::caches_clear_document(&outcomes, &kept))?;
    } else {
        print_clear_result(&outcomes);
    }

    // Reported first, then failed: the rows above are the useful part, and a caller
    // reading only the exit code still learns that something did not go.
    let failed = outcomes.iter().filter(|o| o.problem.is_some()).count();
    if failed > 0 {
        anyhow::bail!(
            "{failed} {} could not be cleared.",
            output::plural(failed, "cache", "caches")
        );
    }
    Ok(())
}

/// The entry to explain when every cache `target` names is one dev-prune only reports.
///
/// `None` for a manager with anything clearable under it, and for a name that matches
/// nothing — the caller has already rejected those.
fn manual_only(target: &str) -> Option<&'static Probe> {
    let matching: Vec<&Probe> = PROBES
        .iter()
        .filter(|p| p.manager.eq_ignore_ascii_case(target))
        .collect();
    if matching.is_empty()
        || matching
            .iter()
            .any(|p| !matches!(p.clear, Clear::Manual { .. }))
    {
        return None;
    }
    matching.first().copied()
}

/// Whether `name` is a cache manager dev-prune knows, for validating `cache_max_gb`.
pub fn is_cache_manager(name: &str) -> bool {
    PROBES.iter().any(|p| p.manager.eq_ignore_ascii_case(name))
}

/// Every manager name `clear` accepts, in report order, without repeats.
pub fn known_managers() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for probe in PROBES {
        if !names.contains(&probe.manager) {
            names.push(probe.manager);
        }
    }
    names
}

/// Empty one cache, and measure what that actually gave back.
fn clear_one(report: &CacheReport) -> ClearOutcome {
    let problem = match report.clear {
        Clear::Command(program, args) => run_clear_command(program, args, &report.extra_args),
        Clear::Directory => remove_cache_dir(&report.path),
        // `run_clear` filters these out before they reach here. Reporting the reason
        // rather than falling through to a delete keeps that a refactoring bug instead
        // of a silently emptied Maven repository.
        Clear::Manual { why } => Some(why.to_string()),
    };
    ClearOutcome {
        manager: report.manager,
        kind: report.kind,
        path: report.path.clone(),
        before: report.bytes,
        // Re-measured even after a failure: a clear that died half-way still freed
        // something, and calling that zero sends someone looking for space already back.
        after: adapters::dir_size(&report.path),
        problem,
    }
}

/// Hand the cache to the manager that owns it.
fn run_clear_command(program: &str, args: &[&str], extra: &[String]) -> Option<String> {
    if !adapters::binary_available(program) {
        return Some(format!(
            "`{program}` is not on PATH — only it knows what in this cache is still \
             referenced, so dev-prune will not delete the directory in its place."
        ));
    }
    // Whatever the row added to the printed command is added to this one too, or the
    // command a user was shown and the command that ran are two different commands.
    let mut all: Vec<&str> = args.to_vec();
    all.extend(extra.iter().map(String::as_str));
    adapters::run_command_with_timeout(
        program,
        &all,
        &query_dir(),
        std::time::Duration::from_secs(constants::CACHE_CLEAR_TIMEOUT_SECS),
    )
    .err()
    .map(|e| format!("{e:#}"))
}

/// Delete the directory, for the managers that ship no way to ask.
fn remove_cache_dir(path: &Path) -> Option<String> {
    // `remove_dir_all` is not atomic, and a machine-wide cache is exactly where an
    // antivirus scan or a background build is most likely to be holding a file open.
    // The same one retry as the prune pass, for the same reason.
    std::fs::remove_dir_all(path)
        .or_else(|_| {
            std::thread::sleep(std::time::Duration::from_millis(250));
            std::fs::remove_dir_all(path)
        })
        .err()
        // "Not found" on the retry means the first attempt did finish after all.
        .filter(|e| e.kind() != std::io::ErrorKind::NotFound)
        .map(|e| format!("{} could not be removed: {e}", output::clean_path(path)))
}

/// Name what was left alone, and why, before naming what is about to go.
fn print_kept(kept: &[CacheReport]) {
    for r in kept {
        let Clear::Manual { why } = r.clear else {
            continue;
        };
        println!();
        output::print_info(&format!(
            "Keeping {} {} ({} at {}). {why}",
            r.manager,
            r.kind,
            output::format_bytes(r.bytes),
            output::clean_path(&r.path)
        ));
    }
}

/// Name everything that is about to go, and what it costs, before any of it goes.
fn print_clear_plan(reports: &[CacheReport], dry_run: bool) {
    output::print_header(if dry_run {
        i18n::t("caches.header.would_clear")
    } else {
        i18n::t("caches.header.about_to_clear")
    });

    println!();
    for r in reports {
        println!(
            "  {:<30} {:>10}  {}",
            format!("{} {}", r.manager, r.kind),
            output::format_bytes(r.bytes),
            output::clean_path(&r.path)
        );
        println!("  {:<30} {:>10}  via: {}", "", "", r.clear_command);
    }

    println!();
    let total: u64 = reports.iter().map(|r| r.bytes).sum();
    println!(
        "  {:<30} {:>10}  across {} {}",
        "Total",
        output::format_bytes(total),
        reports.len(),
        output::plural(reports.len(), "cache", "caches")
    );

    println!();
    output::print_info(
        "Nothing in a cache is lost — every manager above re-downloads what it needs. \
         The cost is time: the next install, and the next `devp restore`, in every \
         project on this machine.",
    );
}

/// Add what was just emptied to the machine's running total, for `devp stats`.
///
/// Best-effort, and silent when it fails. The space is already back whether or not the
/// note about it lands, and a registry that cannot be written — a read-only
/// home directory, a disk that just filled — must not turn a successful
/// clear into a failed command.
fn record_cache_clear(bytes: u64) {
    if bytes == 0 {
        return;
    }
    if let Ok(mut registry) = crate::config::Registry::load() {
        registry.record_cache_clear(bytes);
        let _ = registry.save();
    }
}

/// What actually went.
fn print_clear_result(outcomes: &[ClearOutcome]) {
    println!();
    for o in outcomes {
        let label = format!("{} {}", o.manager, o.kind);
        println!(
            "  {:<30} {:>10}  {}",
            label,
            output::format_bytes(o.freed()),
            if o.problem.is_some() {
                "not cleared"
            } else {
                "cleared"
            }
        );
        if let Some(why) = &o.problem {
            println!("  {:<30} {:>10}  {why}", "", "");
        }
    }

    println!();
    let freed: u64 = outcomes.iter().map(ClearOutcome::freed).sum();
    output::print_success(&format!("Freed {}.", output::format_bytes(freed)));
}

/// Ask before anything is emptied. `--yes` answers for the user; a pipe or a script
/// without it gets a "no" plus the flag to pass next time.
pub fn confirm_clear(yes: bool) -> bool {
    use std::io::{IsTerminal, Write};
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        output::print_info("Not running in a terminal — pass `--yes` to clear these.");
        return false;
    }
    // Default no. Nothing here is unrecoverable, but it is every other project's time
    // being spent, and a reflexive Enter should not be what spends it. The question goes
    // to stderr so a piped stdout cannot eat it.
    eprint!("Clear them? [y/N]: ");
    if std::io::stderr().flush().is_err() {
        return false;
    }
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_probe_can_be_found_without_its_manager_installed() {
        // A probe with no query and no fallbacks is a row that can never appear, which
        // is a silent hole in the report rather than a test failure anywhere else.
        for probe in PROBES {
            assert!(
                !fallbacks(probe.manager, probe.kind).is_empty(),
                "{} {} has no conventional location",
                probe.manager,
                probe.kind
            );
        }
    }

    #[test]
    fn every_probe_names_the_command_that_clears_it() {
        for probe in PROBES {
            assert!(
                !probe.clear_command.trim().is_empty(),
                "{} {} reports a size with no way to act on it",
                probe.manager,
                probe.kind
            );
        }
    }

    #[test]
    fn the_probed_managers_with_no_adapter_of_the_same_name_are_pinned() {
        // The report, `--unused`, SKILL.md, the CLI reference and llms.txt all state this
        // split in prose, and it went out wrong once already: the docs named a manager
        // that had since grown an adapter and omitted one that never had. Pin the list
        // here so the next adapter makes the claim fail rather than quietly rot.
        let orphans: Vec<&str> = PROBES
            .iter()
            .map(|p| p.manager)
            .filter(|m| !adapters::is_adapter_name(m))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(
            orphans,
            [
                "conan",
                "conda",
                "cypress",
                "electron",
                "hex",
                "huggingface",
                "nuget",
                "pip",
                "playwright",
                "puppeteer",
            ]
        );
    }

    #[test]
    fn no_two_probes_describe_the_same_cache() {
        let mut keys: Vec<(&str, &str)> = PROBES.iter().map(|p| (p.manager, p.kind)).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two probes share a manager and kind");
    }

    #[test]
    fn a_managers_answer_is_read_off_the_last_line() {
        // npm prints notices before the value it was asked for.
        let raw = if cfg!(windows) {
            "npm warn config global deprecated\nC:\\Users\\dev\\AppData\\Local\\npm-cache\n"
        } else {
            "npm warn config global deprecated\n/home/dev/.npm\n"
        };
        assert!(path_from_output(raw).is_some());
    }

    #[test]
    fn quoted_paths_lose_their_quotes() {
        let raw = if cfg!(windows) {
            "\"C:\\Program Files\\go\\pkg\\mod\"\n"
        } else {
            "\"/opt/go path/pkg/mod\"\n"
        };
        let path = path_from_output(raw).expect("a quoted path is still a path");
        assert!(!path.to_string_lossy().contains('"'));
    }

    #[test]
    fn a_non_answer_is_not_mistaken_for_a_path() {
        // Each of these has been an actual answer from a package manager at some point,
        // and treating any of them as a directory would size the wrong thing.
        for raw in [
            "",
            "\n \n",
            "undefined\n",
            "not a command\n",
            "./relative\n",
        ] {
            assert!(
                path_from_output(raw).is_none(),
                "{raw:?} was accepted as a cache path"
            );
        }
    }

    #[test]
    fn the_cargo_rows_point_inside_the_registry() {
        // Both cargo rows are fallback-only — cargo has no "where is your cache" query —
        // so a wrong path here is a row that silently reports 0 B forever.
        for kind in ["registry cache", "registry sources"] {
            let path = fallbacks("cargo", kind).remove(0);
            assert!(
                path.starts_with(cargo_home().join("registry")),
                "{kind} resolved outside the cargo registry: {}",
                path.display()
            );
        }
    }

    #[test]
    fn the_conda_row_points_at_the_package_cache_and_not_the_installation() {
        // conda keeps its package cache *inside* the installation, so a location one
        // component short of `pkgs` names the environments, the interpreter and every
        // other thing conda put there. `conda clean` would never touch those, but the
        // row prints the path it sized as well, and a multi-gigabyte figure next to
        // `~/miniconda3` is an invitation to delete the wrong directory by hand.
        let home = dirs::home_dir().expect("a home directory");
        let found = fallbacks("conda", "package cache");

        for install in [
            "miniconda3",
            "anaconda3",
            "miniforge3",
            "mambaforge",
            ".conda",
        ] {
            let want = home.join(install).join("pkgs");
            assert!(
                found.contains(&want),
                "{} is not among conda's conventional locations",
                want.display()
            );
            assert!(
                !found.contains(&home.join(install)),
                "{} is the installation, not its package cache",
                home.join(install).display()
            );
        }
    }

    #[test]
    fn the_report_is_ordered_by_what_is_worth_clearing() {
        let mut reports = [
            CacheReport {
                manager: "npm",
                kind: "cache",
                path: PathBuf::from("/a"),
                bytes: 10,
                clear_command: "x".to_string(),
                clear: Clear::Command("npm", &["cache"]),
                note: None,
                cap_gb: None,
                over_cap: false,
                dependents: None,
                extra_args: Vec::new(),
            },
            CacheReport {
                manager: "go",
                kind: "module cache",
                path: PathBuf::from("/b"),
                bytes: 4_000,
                clear_command: "y".to_string(),
                clear: Clear::Directory,
                note: None,
                cap_gb: None,
                over_cap: false,
                dependents: None,
                extra_args: Vec::new(),
            },
        ];
        reports.sort_by_key(|r| std::cmp::Reverse(r.bytes));
        assert_eq!(reports[0].manager, "go");
    }

    /// A report row, with only the fields this ranking reads set to anything.
    fn sized(manager: &'static str, bytes: u64, dependents: Option<usize>) -> CacheReport {
        CacheReport {
            manager,
            kind: "cache",
            path: PathBuf::from("/x"),
            bytes,
            clear_command: "x".to_string(),
            clear: Clear::Directory,
            note: None,
            cap_gb: None,
            over_cap: false,
            dependents,
            extra_args: Vec::new(),
        }
    }

    #[test]
    fn the_costliest_cache_per_repository_is_not_the_biggest_one() {
        // The shape that made this worth printing at all: npm is five times the size of
        // the pnpm store and a fifth of the cost, because eighteen repositories share it.
        let reports = [
            sized("npm", 10_240, Some(18)),
            sized("pnpm", 2_048, Some(1)),
            sized("cargo", 300, Some(2)),
            sized("cargo", 100, Some(2)),
            sized("bun", 512, Some(0)),
            sized("nuget", 900, None),
        ];
        assert_eq!(
            costliest_per_repository(&reports),
            vec![("pnpm", 2_048), ("npm", 568), ("cargo", 200)],
        );
    }

    #[test]
    fn every_probe_clears_with_the_command_it_prints() {
        // The table tells you what to type and `clear` types it for you. If those two
        // ever name different programs, one of them is lying to the user.
        for probe in PROBES {
            let printed = probe.clear_command;
            match probe.clear {
                Clear::Command(program, args) => {
                    assert!(
                        printed.starts_with(program),
                        "{} {} prints `{printed}` but runs `{program}`",
                        probe.manager,
                        probe.kind
                    );
                    for arg in args {
                        // `conan remove "*"` is quoted for a shell and unquoted for a
                        // spawn, which is exactly the kind of drift worth catching.
                        assert!(
                            printed.contains(arg.trim_matches('"')),
                            "{} {} prints `{printed}` but passes `{arg}`",
                            probe.manager,
                            probe.kind
                        );
                    }
                }
                // A manual entry is still a directory delete — it is just one the user
                // runs. The printed command is the whole of what they get, so it has to
                // be there.
                Clear::Directory | Clear::Manual { .. } => assert!(
                    printed.contains("rm -rf") || printed.contains("Remove-Item"),
                    "{} {} deletes a directory but prints `{printed}`",
                    probe.manager,
                    probe.kind
                ),
            }
        }
    }

    #[test]
    fn the_maven_local_repository_is_never_emptied_by_dev_prune() {
        // `~/.m2/repository` is an install target as well as a download cache, and the
        // artifacts `mvn install:install-file` puts there exist nowhere else. It is
        // reported and sized like everything else and deleted by nothing.
        let maven: Vec<&Probe> = PROBES.iter().filter(|p| p.manager == "maven").collect();
        assert!(!maven.is_empty(), "maven is no longer reported at all");
        for probe in maven {
            assert!(
                matches!(probe.clear, Clear::Manual { .. }),
                "maven {} would be emptied by dev-prune",
                probe.kind
            );
        }
    }

    #[test]
    fn a_manual_report_that_reaches_the_clear_deletes_nothing() {
        // `run_clear` filters these out long before here. This is the last line of
        // defence: if a future refactor drops that filter, the failure has to be a
        // reported problem and not an emptied Maven repository.
        let dir = tempfile::tempdir().unwrap();
        let artifact = dir.path().join("app-1.0-SNAPSHOT.jar");
        std::fs::write(&artifact, b"nowhere else").unwrap();

        let outcome = clear_one(&CacheReport {
            manager: "maven",
            kind: "local repository",
            path: dir.path().to_path_buf(),
            bytes: 12,
            clear_command: MAVEN_REPO_CLEAR.to_string(),
            clear: Clear::Manual { why: MAVEN_MANUAL },
            note: None,
            cap_gb: None,
            over_cap: false,
            dependents: None,
            extra_args: Vec::new(),
        });

        assert!(artifact.exists(), "the store was emptied after all");
        assert!(
            outcome.problem.is_some(),
            "it reported success without doing anything"
        );
    }

    #[test]
    fn clearing_a_manual_only_manager_explains_itself_instead_of_reporting_nothing() {
        // The unhelpful failure this guards against is "No maven cache on this machine",
        // which is both untrue and no help at all.
        let err = run_clear("maven", false, false, true, true, false).unwrap_err();
        assert!(
            err.downcast_ref::<crate::UsageError>().is_some(),
            "expected a usage error, got: {err:#}"
        );
        let text = format!("{err}");
        assert!(
            text.contains("local repository") && text.contains(MAVEN_REPO_CLEAR),
            "the refusal names neither the reason nor the command: {text}"
        );
    }

    #[test]
    fn every_manager_in_the_report_can_be_named_to_clear() {
        let names = known_managers();
        for probe in PROBES {
            assert!(
                names.contains(&probe.manager),
                "{} is reported but `devp caches clear {}` would not find it",
                probe.manager,
                probe.manager
            );
        }
        // cargo, go and gradle each have two rows; naming one clears both, and offering
        // the name twice in the error message reads like a bug.
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "repeated manager in {names:?}");
    }

    #[test]
    fn an_unknown_manager_is_a_usage_error() {
        // Returns before anything is measured, so this touches nothing.
        let err = run_clear("nonesuch", false, false, true, true, false).unwrap_err();
        assert!(err.downcast_ref::<crate::UsageError>().is_some());
    }

    #[test]
    fn json_without_yes_is_a_usage_error_rather_than_a_prompt() {
        let err = run_clear("npm", false, false, false, false, true).unwrap_err();
        assert!(err.downcast_ref::<crate::UsageError>().is_some());
    }

    #[test]
    fn removing_a_directory_reports_nothing_when_it_worked() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("blob"), b"x").unwrap();

        assert!(remove_cache_dir(&cache).is_none());
        assert!(!cache.exists());
        // Already gone is not a failure: the retry can win the race the first attempt
        // lost, and reporting that as an error would fail a clear that succeeded.
        assert!(remove_cache_dir(&cache).is_none());
    }

    #[test]
    fn clearing_a_directory_reports_what_actually_went() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("store");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("blob"), vec![0u8; 4096]).unwrap();
        let before = adapters::dir_size(&cache);

        let outcome = clear_one(&CacheReport {
            manager: "cargo",
            kind: "registry cache",
            path: cache.clone(),
            bytes: before,
            clear_command: "rm -rf".to_string(),
            clear: Clear::Directory,
            note: None,
            cap_gb: None,
            over_cap: false,
            dependents: None,
            extra_args: Vec::new(),
        });

        assert!(outcome.problem.is_none());
        assert_eq!(outcome.after, 0);
        // Measured, not assumed: `before - after`, so a partial clear reports a partial
        // number instead of the whole directory.
        assert_eq!(outcome.freed(), before);
        assert!(!cache.exists());
    }

    #[test]
    fn a_manager_that_is_not_installed_is_reported_rather_than_deleted_around() {
        // The one case where dev-prune declines to fall back to deleting the directory:
        // only the manager knows what in its store is still referenced.
        let problem = run_clear_command("dev-prune-no-such-manager", &["cache", "clean"], &[]);
        assert!(problem.is_some_and(|p| p.contains("not on PATH")));
    }

    /// One row, sized in whole gibibytes so the arithmetic in these tests is readable.
    fn row(manager: &'static str, kind: &'static str, gib: u64) -> CacheReport {
        CacheReport {
            manager,
            kind,
            path: PathBuf::from("/cache").join(manager).join(kind),
            bytes: gib * crate::constants::BYTES_PER_GIB,
            clear_command: "x".to_string(),
            clear: Clear::Directory,
            note: None,
            cap_gb: None,
            over_cap: false,
            dependents: None,
            extra_args: Vec::new(),
        }
    }

    /// A count for every manager named, and nothing for the rest.
    fn counted(repositories: usize, counts: &[(&'static str, usize)]) -> Dependents {
        Dependents {
            repositories,
            by_manager: counts.iter().copied().collect(),
        }
    }

    #[test]
    fn a_cache_no_adapter_is_named_after_is_left_unanswered_rather_than_zeroed() {
        // `pip`, `nuget`, `conan`, `conda` and `hex` are caches dev-prune ships
        // no adapter for. Deciding that `venv` feeds `pip` or that `mix` feeds `hex`
        // would be a guess standing in for a measurement, and the guess that reads `0`
        // is the one that gets a cache on a machine full of Python cleared.
        let mut reports = vec![row("npm", "cache", 1), row("pip", "cache", 1)];
        apply_dependents(&mut reports, Some(&counted(4, &[("npm", 2)])));

        assert_eq!(reports[0].dependents, Some(2));
        assert_eq!(
            reports[1].dependents, None,
            "pip has no adapter of its name, so there is nothing to count"
        );
    }

    #[test]
    fn no_registry_leaves_every_count_unanswered() {
        // The failure this exists for: an empty registry counting to zero everywhere, and
        // `--unused` then offering to empty every cache on the machine.
        let mut reports = vec![row("npm", "cache", 1), row("go", "module cache", 1)];
        apply_dependents(&mut reports, None);
        assert!(reports.iter().all(|r| r.dependents.is_none()));
    }

    #[test]
    fn a_manager_nothing_uses_is_a_counted_zero() {
        // The one state `--unused` is allowed to act on, and the only thing that separates
        // it from the unanswered case above.
        let mut reports = vec![row("go", "module cache", 3)];
        apply_dependents(&mut reports, Some(&counted(9, &[("go", 0)])));
        assert_eq!(reports[0].dependents, Some(0));
        assert!(
            used_by(&reports[0], 0, None, &manager_totals(&reports))
                .contains("no registered repository uses go")
        );
    }

    #[test]
    fn the_per_repository_share_is_the_managers_whole_footprint() {
        // Same arithmetic as the cap, for the same reason: cargo is one cache to a person
        // and two rows to this command, so six plus six across two repositories is 6 GiB
        // each and not 3.
        let reports = vec![row("cargo", "registry", 6), row("cargo", "sources", 6)];
        let line = used_by(
            &reports[0],
            2,
            Some(&counted(2, &[("cargo", 2)])),
            &manager_totals(&reports),
        );
        assert!(
            line.contains("cargo is used by 2 of 2 registered repositories")
                && line.contains("6 GiB each across its 2 caches"),
            "{line}"
        );
    }

    #[test]
    fn one_cache_does_not_say_how_many_it_summed() {
        // The clause exists to explain a figure larger than the row above it. A manager
        // with a single row has no such gap, and "across its 1 caches" would be noise.
        let reports = vec![row("bun", "cache", 4)];
        let line = used_by(
            &reports[0],
            2,
            Some(&counted(2, &[("bun", 2)])),
            &manager_totals(&reports),
        );
        assert!(line.ends_with("2 GiB each"), "{line}");
    }

    #[test]
    fn a_volume_root_is_an_ancestor_of_what_sits_on_it() {
        // Whatever a filesystem's root turns out to be on this platform, a path can only
        // ever sit underneath its own. A root that is not an ancestor would send the
        // `.pnpm-store` probe at some unrelated directory.
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();

        let root = volume_root(&nested).expect("a real directory sits on some filesystem");
        assert!(
            nested.starts_with(&root),
            "{} is not under {}",
            nested.display(),
            root.display()
        );
        assert!(root.is_dir(), "{} is not a directory", root.display());
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_volume_root_is_the_drive_and_nothing_more() {
        // `V:\`, not `V:` and not `V:\Code`. The store this feeds is at the root of the
        // drive, so an answer one component too deep finds nothing and an answer with no
        // separator names the *current* directory on that drive instead of its root.
        let root = volume_root(Path::new(r"V:\Code\ProjectCode")).unwrap();
        assert_eq!(root, PathBuf::from("V:\\"));
        assert_eq!(volume_root(Path::new(r"Code\ProjectCode")), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_drive_someone_types_and_the_drive_on_disk_are_the_same_drive() {
        // Cache paths come back from `canonicalize`, which stamps the verbatim prefix on
        // them, and nobody types `\\?\C:\`. Comparing the two as text is a filter that
        // reports every machine as having no caches on any drive — which is exactly what
        // it did before this was a function.
        assert!(same_volume(Path::new(r"\\?\C:\"), Path::new(r"C:\")));
        assert!(same_volume(Path::new(r"c:\"), Path::new(r"C:\")));
        assert!(!same_volume(Path::new(r"C:\"), Path::new(r"V:\")));
    }

    #[cfg(windows)]
    #[test]
    fn a_bare_drive_letter_is_what_people_type_and_has_to_resolve() {
        // `V:` is drive-*relative* to Windows — the current directory on V:, with no root
        // component — so it is refused by the parser it has to survive. It is also the
        // first thing anyone types after `--volume`, as is a bare `V`.
        for typed in ["V", "v", "V:", "v:", r"V:\", r"V:\Code\ProjectCode"] {
            assert_eq!(
                resolve_volume(typed).unwrap(),
                PathBuf::from(r"V:\"),
                "{typed}"
            );
        }
        assert!(resolve_volume("not-a-path-at-all").is_err());
    }

    #[test]
    fn the_drive_you_are_standing_on_is_a_dot() {
        // `--volume .` is the shortest way to name the drive you are on, and it is what
        // the reference tells people to type. A relative path reaches neither
        // `volume_root`: one reads a prefix it has not got, the other walks ancestors
        // that stop at the working directory rather than the mount point.
        let here = std::env::current_dir().unwrap();
        assert_eq!(resolve_volume(".").unwrap(), volume_root(&here).unwrap());
        // A word that is not a path is still a usage error, and not a silent report
        // about the current drive.
        assert!(resolve_volume("definitely-not-a-drive").is_err());
    }

    #[test]
    fn narrowing_to_one_drive_keeps_only_what_is_on_it() {
        // The whole point of the flag: a drive that is nearly full is the only drive that
        // matters, and the twenty gigabytes on the other one are noise.
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("cache");
        std::fs::create_dir_all(&here).unwrap();
        let root = volume_root(&here).unwrap();

        let mut on_this_one = sized("pnpm", 2_048, Some(1));
        on_this_one.path = here;
        let mut nowhere = sized("npm", 10_240, Some(18));
        nowhere.path = PathBuf::from("relative/and/unresolvable");

        let kept = on_volume(vec![on_this_one, nowhere], &root);
        assert_eq!(
            kept.len(),
            1,
            "a row whose drive is unknown is not on this one"
        );
        assert_eq!(kept[0].manager, "pnpm");
    }

    #[test]
    fn the_per_drive_line_adds_up_and_leads_with_the_biggest() {
        // A subtotal a reader cannot add back up to the printed total is worse than no
        // subtotal, so a row whose drive cannot be determined is left out rather than
        // filed under a guess.
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("cache");
        std::fs::create_dir_all(&here).unwrap();

        let mut small = sized("pnpm", 100, None);
        small.path = here.clone();
        let mut large = sized("npm", 900, None);
        large.path = here;
        let mut unknown = sized("uv", 500, None);
        unknown.path = PathBuf::from("relative/and/unresolvable");

        let totals = volume_totals(&[small, large, unknown]);
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].1, 1_000);
    }

    #[test]
    fn one_volume_is_listed_once_however_many_repositories_are_on_it() {
        // Forty-six repositories on one drive is one store to look for, not forty-six
        // identical rows.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("one");
        let b = dir.path().join("two");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        assert_eq!(volume_roots(&[a.clone(), b, a]).len(), 1);
        assert!(volume_roots(&[]).is_empty());
    }

    #[test]
    fn a_volume_stores_printed_command_is_the_one_that_runs() {
        // The reason this row exists at all is that `pnpm store prune` on its own prunes
        // the store for the filesystem it is run on, which is not this one. Printing a
        // command that names the store and running one that does not would be worse than
        // never reporting it.
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join(".pnpm-store");
        std::fs::create_dir_all(&store).unwrap();

        let report = volume_store_report(store.clone());
        let named = output::clean_path(&store);
        assert_eq!(
            report.extra_args,
            vec!["--store-dir".to_string(), named.clone()]
        );
        assert!(
            report.clear_command.contains(&named),
            "the printed command does not name the store: {}",
            report.clear_command
        );
        assert!(matches!(
            report.clear,
            Clear::Command("pnpm", ["store", "prune"])
        ));
    }

    #[test]
    fn only_a_path_with_a_space_in_it_is_quoted() {
        // The quoting is for the human reading the line. dev-prune passes the path as one
        // argument and never hands it to a shell, so quoting everything would print a
        // command that differs from the one that ran for no reason at all.
        assert_eq!(shell_arg("/mnt/data/.pnpm-store"), "/mnt/data/.pnpm-store");
        assert_eq!(
            shell_arg("/mnt/my data/.pnpm-store"),
            "\"/mnt/my data/.pnpm-store\""
        );
    }

    #[test]
    fn a_cap_is_measured_against_the_managers_whole_footprint() {
        // cargo keeps a registry cache and an unpacked source tree, and "cargo is over
        // ten gigabytes" is a statement about the pair. Six plus six clears a cap of ten
        // that neither row reaches on its own.
        let mut reports = vec![row("cargo", "registry", 6), row("cargo", "sources", 6)];
        apply_caps(&mut reports, &BTreeMap::from([("cargo".to_string(), 10)]));
        assert!(
            reports.iter().all(|r| r.over_cap),
            "both rows belong to the manager that went over"
        );
        assert!(reports.iter().all(|r| r.cap_gb == Some(10)));
    }

    #[test]
    fn a_manager_under_its_cap_is_marked_with_the_cap_and_nothing_else() {
        let mut reports = vec![row("npm", "cache", 3)];
        apply_caps(&mut reports, &BTreeMap::from([("npm".to_string(), 10)]));
        // The cap is still reported, because "capped and fine" is worth seeing — it is
        // the difference between a setting that is working and one nobody made.
        assert_eq!(reports[0].cap_gb, Some(10));
        assert!(!reports[0].over_cap);
    }

    #[test]
    fn a_manager_with_no_cap_is_never_called_too_big() {
        // The default is an empty map, and an empty map has to mean "no opinion" rather
        // than "zero", or every cache on the machine would report as over-size.
        let mut reports = vec![row("uv", "cache", 40)];
        apply_caps(&mut reports, &BTreeMap::new());
        assert_eq!(reports[0].cap_gb, None);
        assert!(!reports[0].over_cap);
    }

    #[test]
    fn one_managers_cap_says_nothing_about_another() {
        let mut reports = vec![row("npm", "cache", 12), row("go", "module cache", 12)];
        apply_caps(&mut reports, &BTreeMap::from([("npm".to_string(), 10)]));
        assert!(reports[0].over_cap);
        assert!(
            !reports[1].over_cap,
            "go has no cap and did not acquire npm's"
        );
    }

    #[test]
    fn exactly_at_the_cap_is_not_over_it() {
        // A cap of ten means ten is allowed. Off by one here would mark a cache the
        // moment it hit the number the user chose as acceptable.
        let mut reports = vec![row("pnpm", "store", 10)];
        apply_caps(&mut reports, &BTreeMap::from([("pnpm".to_string(), 10)]));
        assert!(!reports[0].over_cap);
    }

    #[test]
    fn every_cache_manager_answers_to_its_own_name() {
        // `cache_max_gb` is validated against this, so a probe the check does not know
        // would be a manager `devp caches clear` accepts and `devp config set` rejects.
        for probe in PROBES {
            assert!(
                is_cache_manager(probe.manager),
                "{} is reported but cannot be capped",
                probe.manager
            );
        }
        assert!(!is_cache_manager("dev-prune-no-such-manager"));
    }
}
