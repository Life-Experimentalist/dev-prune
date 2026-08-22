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
// own bookkeeping stays consistent. Only the managers that ship no such subcommand —
// cargo, maven, gradle, vcpkg — are cleared by removing the directory, and the path
// removed is the one this command resolved and sized, never a string handed to a shell.
//
// Each manager is asked where its own cache lives rather than being assumed — a
// `CARGO_HOME`, a `--cache-dir`, a corporate `.npmrc` all move it. Every one of those
// queries is read-only, and a manager that is not installed falls back to the
// conventional location, so a cache left behind by an uninstalled manager still shows up.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::adapters;
use crate::constants;
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
    pub clear_command: &'static str,
    /// How `devp caches clear` empties it.
    pub clear: Clear,
    /// What the user gives up by running that command, when it is more than time.
    pub note: Option<&'static str>,
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
/// "clear the cache". The honest command is the deletion.
#[cfg(windows)]
const MAVEN_REPO_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.m2\repository";
#[cfg(not(windows))]
const MAVEN_REPO_CLEAR: &str = "rm -rf ~/.m2/repository";

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

const PROBES: &[Probe] = &[
    Probe {
        manager: "npm",
        kind: "cache",
        query: Some(("npm", &["config", "get", "cache"])),
        clear_command: "npm cache clean --force",
        clear: Clear::Command("npm", &["cache", "clean", "--force"]),
        note: None,
    },
    Probe {
        manager: "pnpm",
        kind: "store",
        query: Some(("pnpm", &["store", "path"])),
        clear_command: "pnpm store prune",
        clear: Clear::Command("pnpm", &["store", "prune"]),
        note: Some(
            "hardlinked into every node_modules on the machine; emptying it is what makes \
             the next pnpm install a download",
        ),
    },
    Probe {
        manager: "yarn",
        kind: "cache",
        query: Some(("yarn", &["cache", "dir"])),
        clear_command: "yarn cache clean",
        clear: Clear::Command("yarn", &["cache", "clean"]),
        note: None,
    },
    Probe {
        manager: "bun",
        kind: "cache",
        query: Some(("bun", &["pm", "cache"])),
        clear_command: "bun pm cache rm",
        clear: Clear::Command("bun", &["pm", "cache", "rm"]),
        note: None,
    },
    Probe {
        manager: "uv",
        kind: "cache",
        query: Some(("uv", &["cache", "dir"])),
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
        clear_command: "pip cache purge",
        clear: Clear::Command("pip", &["cache", "purge"]),
        note: None,
    },
    Probe {
        manager: "cargo",
        kind: "registry cache",
        query: None,
        clear_command: CARGO_CACHE_CLEAR,
        clear: Clear::Directory,
        note: Some("the downloaded .crate archives; clearing them means downloading again"),
    },
    Probe {
        manager: "cargo",
        kind: "registry sources",
        query: None,
        clear_command: CARGO_SRC_CLEAR,
        clear: Clear::Directory,
        note: Some("unpacked copies of the archives above; cargo re-extracts these offline"),
    },
    Probe {
        manager: "go",
        kind: "module cache",
        query: Some(("go", &["env", "GOMODCACHE"])),
        clear_command: "go clean -modcache",
        clear: Clear::Command("go", &["clean", "-modcache"]),
        note: None,
    },
    Probe {
        manager: "go",
        kind: "build cache",
        query: Some(("go", &["env", "GOCACHE"])),
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
        clear_command: MAVEN_REPO_CLEAR,
        clear: Clear::Directory,
        note: Some(
            "every Maven build on the machine resolves from here; the next build re-downloads what it needs",
        ),
    },
    Probe {
        manager: "gradle",
        kind: "caches",
        query: None,
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
        clear_command: VCPKG_ARCHIVES_CLEAR,
        clear: Clear::Directory,
        note: Some("prebuilt package archives; vcpkg rebuilds from source what it cannot re-fetch"),
    },
    Probe {
        manager: "conan",
        kind: "package cache",
        query: None,
        clear_command: "conan remove \"*\" --confirm",
        clear: Clear::Command("conan", &["remove", "*", "--confirm"]),
        note: Some(
            "recipes and binaries shared by every Conan project; re-fetched on the next install",
        ),
    },
];

/// Run the `caches` command.
pub fn run(json_output: bool) -> Result<()> {
    let reports = collect(!json_output);

    if json_output {
        return json::emit(&json::caches_document(&reports));
    }

    print_report(&reports);
    Ok(())
}

/// Find and size every cache on this machine, largest first.
fn collect(spinner: bool) -> Vec<CacheReport> {
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
            clear_command: probe.clear_command,
            clear: probe.clear,
            note: probe.note,
        });
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    reports.sort_by_key(|r| std::cmp::Reverse(r.bytes));
    reports
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
        _ => vec![],
    };

    candidates.into_iter().flatten().collect()
}

/// `CARGO_HOME`, or the default cargo puts it in.
fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
}

fn print_report(reports: &[CacheReport]) {
    output::print_header("Package manager caches");

    if reports.is_empty() {
        println!();
        output::print_info("No package manager caches found on this machine.");
        return;
    }

    println!();
    for r in reports {
        let label = format!("{} {}", r.manager, r.kind);
        println!(
            "  {:<30} {:>10}  {}",
            label,
            output::format_bytes(r.bytes),
            output::clean_path(&r.path)
        );
        println!("  {:<30} {:>10}  clear: {}", "", "", r.clear_command);
        if let Some(note) = r.note {
            println!("  {:<30} {:>10}  {}", "", "", note);
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

    println!();
    output::print_info(
        "Nothing above was deleted. A cache is shared by every project on the machine, so \
         no single repository's lockfile can prove it is recoverable — and it is what \
         makes `devp restore` fast, which is why nothing dev-prune runs on a schedule \
         will ever touch one. When you want the space more than the speed, run a clear \
         command yourself, or `devp caches clear <manager>`.",
    );
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
/// sized first, and unless `--yes` answers for the user, it asks.
pub fn run_clear(target: &str, yes: bool, dry_run: bool, json_output: bool) -> Result<()> {
    let all = target.eq_ignore_ascii_case("all");
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
    // A prompt nobody can answer is a hang, and the "pass --yes" line printed in its
    // place would land in the middle of the JSON document and break the parse.
    if json_output && !yes && !dry_run {
        return Err(anyhow::Error::new(crate::UsageError(
            "`--json` cannot ask for confirmation — pass `--yes` as well, or `--dry-run` \
             to see what would go."
                .to_string(),
        )));
    }

    let reports: Vec<CacheReport> = collect(!json_output)
        .into_iter()
        .filter(|r| all || r.manager.eq_ignore_ascii_case(target))
        .collect();

    if reports.is_empty() {
        if json_output {
            return json::emit(&json::caches_clear_plan_document(&reports));
        }
        output::print_info(&format!(
            "No {} cache on this machine — nothing to clear.",
            if all { "package manager" } else { target }
        ));
        return Ok(());
    }

    if dry_run {
        if json_output {
            return json::emit(&json::caches_clear_plan_document(&reports));
        }
        print_clear_plan(&reports, true);
        return Ok(());
    }

    if !json_output {
        print_clear_plan(&reports, false);
        if !confirm_clear(yes) {
            output::print_info("Nothing was cleared.");
            return Ok(());
        }
    }

    let outcomes: Vec<ClearOutcome> = reports.iter().map(clear_one).collect();

    if json_output {
        json::emit(&json::caches_clear_document(&outcomes))?;
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

/// Every manager name `clear` accepts, in report order, without repeats.
fn known_managers() -> Vec<&'static str> {
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
        Clear::Command(program, args) => run_clear_command(program, args),
        Clear::Directory => remove_cache_dir(&report.path),
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
fn run_clear_command(program: &str, args: &[&str]) -> Option<String> {
    if !adapters::binary_available(program) {
        return Some(format!(
            "`{program}` is not on PATH — only it knows what in this cache is still \
             referenced, so dev-prune will not delete the directory in its place."
        ));
    }
    adapters::run_command_with_timeout(
        program,
        args,
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

/// Name everything that is about to go, and what it costs, before any of it goes.
fn print_clear_plan(reports: &[CacheReport], dry_run: bool) {
    output::print_header(if dry_run {
        "Would clear"
    } else {
        "About to clear"
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
fn confirm_clear(yes: bool) -> bool {
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
    fn the_report_is_ordered_by_what_is_worth_clearing() {
        let mut reports = [
            CacheReport {
                manager: "npm",
                kind: "cache",
                path: PathBuf::from("/a"),
                bytes: 10,
                clear_command: "x",
                clear: Clear::Command("npm", &["cache"]),
                note: None,
            },
            CacheReport {
                manager: "go",
                kind: "module cache",
                path: PathBuf::from("/b"),
                bytes: 4_000,
                clear_command: "y",
                clear: Clear::Directory,
                note: None,
            },
        ];
        reports.sort_by_key(|r| std::cmp::Reverse(r.bytes));
        assert_eq!(reports[0].manager, "go");
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
                Clear::Directory => assert!(
                    printed.contains("rm -rf") || printed.contains("Remove-Item"),
                    "{} {} deletes a directory but prints `{printed}`",
                    probe.manager,
                    probe.kind
                ),
            }
        }
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
        let err = run_clear("nonesuch", true, true, false).unwrap_err();
        assert!(err.downcast_ref::<crate::UsageError>().is_some());
    }

    #[test]
    fn json_without_yes_is_a_usage_error_rather_than_a_prompt() {
        let err = run_clear("npm", false, false, true).unwrap_err();
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
            clear_command: "rm -rf",
            clear: Clear::Directory,
            note: None,
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
        let problem = run_clear_command("dev-prune-no-such-manager", &["cache", "clean"]);
        assert!(problem.is_some_and(|p| p.contains("not on PATH")));
    }
}
