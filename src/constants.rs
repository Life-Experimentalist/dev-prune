// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Centralized application constants and default configurations.
//
// Serves as the single source of truth for app metadata, versioning,
// default thresholds, and file paths.

/// Application crate version derived dynamically from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Minimum supported Rust version, derived dynamically from `rust-version` in
/// `Cargo.toml` — which is the single source of truth for the MSRV. Only `cargo
/// install` users ever meet it; every other channel ships a prebuilt binary.
pub const MSRV: &str = env!("CARGO_PKG_RUST_VERSION");

/// Application name.
pub const APP_NAME: &str = "dev-prune";

/// Author of dev-prune.
pub const AUTHOR: &str = "VKrishna04";

/// Canonical source repository.
///
/// The one in `Cargo.toml` is only visible to people who already found the crate. This
/// one is compiled into the binary, so a copy of the executable still says where it came
/// from.
pub const REPO_URL: &str = "https://github.com/Life-Experimentalist/dev-prune";

/// Project homepage.
pub const HOMEPAGE_URL: &str = "https://devprune.vkrishna04.me";

/// The one-line credit printed under interactive output.
///
/// Deliberately plain text in plain sight: it is not obfuscated, not assembled at
/// runtime, and not checked anywhere. Anyone may fork this project and change this line
/// — the Apache-2.0 licence says so, and nothing in the code argues. It exists so that
/// the common case, someone running the published binary, shows where it came from.
pub const ATTRIBUTION_LINE: &str =
    "dev-prune · made with ♥ by VKrishna04 · github.com/Life-Experimentalist/dev-prune";

/// The licence sentence under the declaration, in both wizard paths.
///
/// Apache-2.0 needs no click-through and this is not one: the licence governs use
/// whether or not anybody reads this line. It is here because the screen it sits on is
/// the first thing a new user meets, and a tool that deletes directories should say what
/// it does and does not promise at that moment rather than in a file called LICENSE.md.
/// Sections 7 and 8 are named so the disclaimer can be checked rather than taken on
/// trust.
pub const LICENCE_NOTICE: &str =
    "Apache-2.0 sections 7-8: no warranty, no liability. Using it accepts that.";

/// The body of `devp --version`.
///
/// Built at runtime rather than with `concat!`, which only takes literals and would mean
/// spelling the author and the URL a second time. Two copies of a string are two things
/// that can disagree, and this one exists precisely so that a stray copy of the binary
/// can still be traced back.
pub static LONG_VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "{VERSION}\n\
         author:     {AUTHOR}\n\
         repository: {REPO_URL}\n\
         homepage:   {HOMEPAGE_URL}\n\
         license:    Apache-2.0"
    )
});

/// How many prune passes `devp stats` keeps a summary of.
///
/// The registry is rewritten in full on every save, so this list is a file-size decision
/// as much as a display one. Fifty passes is roughly a year of a fortnightly schedule.
pub const PRUNE_HISTORY_LIMIT: usize = 50;

/// How many restore measurements one adapter's throughput average is worth.
///
/// Past this, the running totals are halved before the new sample is added, so the
/// average follows the machine rather than remembering a disk it no longer has. A plain
/// lifetime mean would quote a spinning-rust number years after the SSD went in.
pub const RESTORE_RATE_SAMPLE_CAP: u32 = 20;

/// The shortest restore worth learning a throughput from, in milliseconds.
///
/// A restore that returned in under this is a manager deciding it had nothing to do —
/// the packages were still in its cache, or already on disk. Averaging those in would
/// claim a rate no cold restore can reach.
pub const RESTORE_RATE_MIN_MILLIS: u64 = 250;

/// The release that started recording per-repository totals and the pass history.
///
/// A machine that pruned for months on 1.0.0 has a large lifetime total and no history at
/// all, and reading that as "nothing was ever pruned here" would be wrong. Both `devp
/// stats` and its `--json` document quote this version so the gap is explained rather than
/// looking like data loss. It is deliberately not [`VERSION`]: it names the release the
/// format changed in, and does not move again.
pub const HISTORY_STARTS_AT: &str = "1.1.0";

/// Default idle threshold in days before a repository is eligible for pruning.
pub const DEFAULT_IDLE_DAYS: u64 = 15;

/// Default idle threshold for *build-tree* directories, in days.
///
/// Applies to every adapter that answers [`crate::adapters::PackageManager::opt_in`].
/// Deliberately longer than [`DEFAULT_IDLE_DAYS`],
/// because those directories come back by recompiling the whole project rather than by
/// re-downloading a dependency tree, so the bar for "nobody will miss this" sits
/// higher. The engine applies `max(build_idle_days, idle_days)`.
///
/// Three times the dependency window and no more: 60 days was long enough that a
/// project touched once a quarter never became a candidate at all, which is not
/// caution, it is the feature never firing.
pub const DEFAULT_BUILD_IDLE_DAYS: u64 = 45;

/// The per-manager cache size cap `devp config wizard` offers, in gibibytes.
///
/// Not a default: `cache_max_gb` is empty until someone puts a number in it, and an
/// empty map means no cache is ever called too big. This is only the figure the wizard
/// pre-fills and the docs recommend, and 10 GiB is where it sits because that is the
/// size at which a download cache stops being a time-saver and starts being the largest
/// directory on the disk — a `uv` cache measured on the author's machine had passed it
/// while every project it served fit in a tenth of that.
pub const RECOMMENDED_CACHE_MAX_GB: u64 = 10;

/// One gibibyte, for turning `cache_max_gb` into the byte count a cache is measured in.
pub const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

/// Shown while `devp trust` and the configurator ask the OS about the scheduler and
/// Git hooks, then overwritten in place. Kept here because its width is what the
/// erase writes over, so the two must not be able to drift apart.
pub const READING_MACHINE: &str = "Reading this machine (scheduler, Git hooks)...";

/// Default interval in days between background daemon prune runs.
pub const DEFAULT_CHECK_INTERVAL_DAYS: u64 = 2;

/// Whether the setup pass installs the OS scheduler.
///
/// On. A pruner that has to be remembered is a pruner that never runs; the scheduled
/// pass is the product, not an extra. It is still bounded by everything an interactive
/// run is bounded by — idle threshold, lockfile verification, per-repo opt-outs — and
/// `devp daemon uninstall` (or `devp config set auto_daemon false`) removes it.
pub const DEFAULT_AUTO_DAEMON: bool = true;

/// Whether the setup pass installs the global Git auto-registration hooks.
///
/// On, but conditionally: installation is skipped, not forced, when `core.hooksPath`
/// already belongs to husky, pre-commit or lefthook.
pub const DEFAULT_AUTO_HOOKS: bool = true;

/// Whether dev-prune installs its missing integrations by itself.
///
/// On. The pass runs once per installed version — on first run, and again after an
/// upgrade — and only creates what is absent. Set to `false`, or export
/// `DEV_PRUNE_NO_AUTO_SETUP`, to manage the integrations entirely by hand.
///
/// The environment variable is symmetric: with it set, `devp uninstall` leaves those
/// same hand-managed integrations — the scheduler, the agent skills, the install
/// directories guessed from the home folder — alone too, and says so. "Entirely by
/// hand" has to include the removal, or the variable's promise only holds until the
/// day you uninstall.
pub const DEFAULT_AUTO_SETUP: bool = true;

/// Default for whether `link` and `init` write a `.devprune.json` into repositories
/// they register.
///
/// Off: most repositories are fine on the defaults, and a config file dropped into
/// every repo is litter. The setting exists for people who tune repositories
/// individually often enough that creating the file by hand every time is the chore.
pub const DEFAULT_AUTO_CONFIG: bool = false;

/// Default setting for requiring interactive confirmation before pruning.
pub const DEFAULT_REQUIRE_CONFIRMATION: bool = true;

/// Language for dev-prune's own headings and summary lines.
///
/// English, and English is also the fallback for every key a translation has not
/// reached yet -- see [`crate::i18n`]. Deliberately not derived from the operating
/// system locale: a machine configured in one language has said nothing about what
/// language its owner wants their build tools in.
pub const DEFAULT_LANGUAGE: &str = "en";

/// Default size floor, in MiB, below which a bloat directory is left alone.
///
/// Zero — every recognised directory is a candidate. Raising it trades a little disk
/// space for fewer reinstalls: deleting a 3 MiB `node_modules` costs a full `npm ci`
/// and reclaims almost nothing.
pub const DEFAULT_MIN_SIZE_MB: u64 = 0;

/// How far below a repository root project discovery descends, by default.
///
/// Six covers `packages/scope/name/…` monorepo layouts with room to spare while keeping
/// the walk bounded on repositories with deep source trees. Configurable with
/// `devp config set scan_depth`, and per repository with `"scan_depth"` in
/// `.devprune.json`, because "deep enough" is a property of the layout, not of the tool.
pub const DEFAULT_SCAN_DEPTH: usize = 6;

/// Upper bound accepted for `scan_depth`.
///
/// Not a matter of taste. The walk is breadth-first over every directory that is not
/// excluded, so cost grows with the tree, and a repository with a deep generated tree
/// (a Bazel `bazel-out`, a `.terraform` provider cache) can turn an unbounded walk into
/// a multi-minute stall on a background pass nobody is watching.
pub const MAX_SCAN_DEPTH_LIMIT: usize = 32;

/// Ceiling on the threads `devp status` uses to size repositories.
///
/// The scan is one independent file-system walk per repository, so it is bound by the
/// disk rather than the CPU and oversubscribing the cores is what makes it fast. The
/// ceiling exists anyway: past this point a spinning disk spends its time seeking
/// between trees instead of reading them, and a laptop under a background pass should
/// still be usable.
pub const STATUS_SCAN_MAX_THREADS: usize = 32;

/// Threads per reported core for the status scan. Above one because the scan waits on
/// the disk far more than on the CPU; well below what the disk will queue, because past
/// that point the extra threads only take turns.
pub const STATUS_SCAN_THREADS_PER_CORE: usize = 2;

/// Overrides the computed status-scan thread count. Clamped to
/// [`STATUS_SCAN_MAX_THREADS`]; `1` forces a sequential scan.
pub const STATUS_SCAN_THREADS_ENV: &str = "DEV_PRUNE_SCAN_THREADS";

/// How many lines of a failed command's output are relayed into a report.
///
/// A failing `npm ci` prints its whole usage screen. Six lines is enough for the error
/// and its cause, and short enough that the prune report around it is still readable;
/// the rest is reachable through the log file the condensed output still names.
pub const TOOL_OUTPUT_MAX_LINES: usize = 6;

/// Directory names that mean "the contents of this are disposable".
///
/// Matched against a repository's *ancestors*, never against the repository directory
/// itself: a project may legitimately be called `cache`, but a checkout sitting inside
/// one is something a tool put there. Editor and agent plugin managers clone into
/// directories like `~/.claude/plugins/cache/temp_git_<id>`, which is nowhere near the OS
/// temp directory and is deleted just as fast; twenty-eight of those reached one
/// registry before this list existed.
pub const EPHEMERAL_ANCESTORS: &[&str] = &["cache", ".cache", "Cache", "Caches", "tmp", ".tmp"];

/// Repository directory *names* that mean the same thing, wherever they are.
///
/// The ancestor list above only catches a throwaway clone that a tool was polite enough
/// to put under a directory called `cache`. These prefixes catch the clone itself:
/// `temp_git_1787245534782` is a checkout some plugin manager made, named after the
/// millisecond it made it, and it will be gone before the next prune pass. Registering
/// one strands a dead entry in the registry the moment the tool cleans up after itself.
///
/// A prefix rather than a substring, and deliberately narrow: `temporary-fixes` and
/// `my-temp-git-notes` are real repositories somebody named badly, and refusing to track
/// a real workspace is the worse of the two errors.
pub const EPHEMERAL_REPO_PREFIXES: &[&str] = &["temp_git_", "tmp_git_"];

/// Whether an adapter whose sync command edits tracked manifests may run it.
///
/// Off. `cargo generate-lockfile` re-resolves every dependency and rewrites
/// `Cargo.lock`; `go mod tidy` edits `go.mod` and `go.sum` and can drop requirements.
/// A cleanup tool that silently changes files Git tracks has done something the user
/// did not ask for, so these run read-only and this switch is the informed opt-in.
pub const DEFAULT_ALLOW_MANIFEST_REWRITE: bool = false;

/// Whether the setup pass installs the Git hooks in front of another tool's.
///
/// Off. Chaining is behaviour-preserving — every hook is forwarded on and
/// `devp hook uninstall` restores the original `core.hooksPath` — but it still rewires
/// somebody else's Git configuration, which is not a thing to do unasked. Turn it on
/// with `devp config set auto_hooks_chain true`, or do it once with
/// `devp hook install --chain`.
pub const DEFAULT_AUTO_HOOKS_CHAIN: bool = false;

/// GitHub releases page, shown whenever an upgrade is relevant.
pub const RELEASES_URL: &str = "https://github.com/Life-Experimentalist/dev-prune/releases";

/// The shell installer, printed as an upgrade command and re-run by `devp update
/// --install` when the running binary came from it.
pub const INSTALL_SH_URL: &str = "https://devprune.vkrishna04.me/install.sh";

/// The PowerShell installer — same two callers as [`INSTALL_SH_URL`].
pub const INSTALL_PS1_URL: &str = "https://devprune.vkrishna04.me/install.ps1";

/// GitHub API endpoint for the latest published release.
///
/// Contacted by `devp update` and by the interval-gated check behind
/// `run`/`status`/`init` (off via `update_check false` or `DEV_PRUNE_OFFLINE`). See the
/// network policy in `docs/PRIVACY.md`.
///
/// This answers with the newest release GitHub has marked *latest*, which is the newest
/// binary release and nothing else: the extension's releases are published with
/// `make_latest: false` precisely so they never surface here. They must not. This URL's
/// answer is fed to [`compare_versions`](crate::commands::update::compare_versions)
/// after a leading `v` is stripped, and a `vscode-v0.4.0` tag arriving here would leave
/// every installed copy unable to compare its own version for as long as that release
/// stayed newest.
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/Life-Experimentalist/dev-prune/releases/latest";

/// GitHub API endpoint listing releases newest-first, for the extension `.vsix`.
///
/// The extension ships on its own tags (`vscode-v*`) and its own release page, because
/// its version is its own and it changes on its own schedule — see
/// `.github/workflows/release-extension.yml`. That is also why this is a listing rather
/// than [`LATEST_RELEASE_API_URL`]: there is no "latest release whose tag starts with"
/// endpoint, so the caller walks the page and takes the first match.
///
/// One page is enough by a wide margin. The extension would have to go a hundred binary
/// releases without a single release of its own before its newest fell off the end, and
/// the fallback degrades to "install it by hand" rather than to anything wrong.
pub const RELEASES_LIST_API_URL: &str =
    "https://api.github.com/repos/Life-Experimentalist/dev-prune/releases?per_page=100";

/// Tag prefix identifying a release of the VS Code extension rather than of the binary.
pub const VSCODE_RELEASE_TAG_PREFIX: &str = "vscode-v";

/// Where a release's assets live, with `{tag}` standing in for `v1.4.0`.
///
/// Used by `devp update --install` to fetch the uncompressed binary and its `.sha256`
/// sidecar. Deliberately the plain `releases/download` path rather than an API endpoint:
/// it needs no token, is not rate-limited per-IP the way `api.github.com` is, and is the
/// same URL a person would click on the release page.
pub const RELEASE_DOWNLOAD_BASE: &str =
    "https://github.com/Life-Experimentalist/dev-prune/releases/download";

/// Where a SHA-256 becomes a scan report, with the digest appended as the last segment.
///
/// `devp trust` prints one of these per executable it owns. It is a *lookup* by hash and
/// nothing more: the digest is computed locally, the URL is printed rather than fetched,
/// and no part of dev-prune ever uploads a file anywhere. The reason it exists is that an
/// antivirus judges the bytes on the disk in front of it, not the asset on a release
/// page — so the only hash worth showing someone is the one their own copy has.
pub const VIRUSTOTAL_FILE_BASE: &str = "https://www.virustotal.com/gui/file";

/// The release-asset name for one platform, without the `.sha256` suffix.
///
/// This is a contract with the packaging steps in `.github/workflows/release.yml`, which
/// build these exact names. A mismatch is not a compile error and not a test failure —
/// it is a self-update that 404s on the day of a release — so the two are commented as
/// referring to each other.
///
/// `None` on a platform the release does not build for, which is how a source build on,
/// say, FreeBSD declines the direct route instead of downloading a Linux binary.
pub fn release_asset_name(version: &str) -> Option<String> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        _ => return None,
    };
    // The release publishes `x64`/`arm64`/`x86`, not Rust's target-arch spellings.
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => return None,
    };
    // Only Windows ships a 32-bit build; on any other OS `x86` has no asset.
    if arch == "x86" && os != "windows" {
        return None;
    }
    let ext = if os == "windows" { ".exe" } else { "" };
    Some(format!("dev-prune-v{version}-{os}-{arch}{ext}"))
}

/// Whether the periodic release check runs. On by default — see `Settings::update_check`.
pub const DEFAULT_UPDATE_CHECK: bool = true;

/// Whether a known-newer release installs itself at the end of a prune pass. On by
/// default — see `Settings::auto_update`.
pub const DEFAULT_AUTO_UPDATE: bool = true;

/// Whether the installed version is pinned where it is. Off by default, and the only
/// setting that outranks every other update path -- see `Settings::version_lock`.
pub const DEFAULT_VERSION_LOCK: bool = false;

/// Default interval, in days, between automatic release checks.
///
/// A week. Frequent enough that a security fix is not missed for long, rare enough that
/// it is invisible in day-to-day use. Override with
/// `devp config set update_check_interval_days`.
pub const UPDATE_CHECK_INTERVAL_DAYS: i64 = 7;

/// Default timeout for the release check. Short on purpose — this is a convenience,
/// and a user waiting on a hung socket is worse than not knowing. Override with
/// `devp config set update_check_timeout_secs` when a proxy needs longer.
pub const UPDATE_CHECK_TIMEOUT_SECS: u64 = 5;

/// Timeout for downloading a release binary in `devp update --install`.
///
/// Far longer than [`UPDATE_CHECK_TIMEOUT_SECS`], which only reads a few hundred bytes
/// of JSON: this pulls several megabytes, and a slow connection is not an error.
pub const UPDATE_DOWNLOAD_TIMEOUT_SECS: u64 = 300;

/// Name of the registry JSON file.
pub const REGISTRY_FILENAME: &str = "registry.json";

/// Config directory name under user config root.
pub const CONFIG_DIR_NAME: &str = "dev-prune";

/// Global environment variable name to override config directory location.
pub const ENV_CONFIG_DIR_OVERRIDE: &str = "DEV_PRUNE_CONFIG_DIR";

/// Environment variable that suppresses the automatic setup pass entirely.
///
/// For images, CI and anyone who wants the binary and nothing else. `devp setup` still
/// works when it is set — this only governs the unattended pass.
pub const ENV_NO_AUTO_SETUP: &str = "DEV_PRUNE_NO_AUTO_SETUP";

/// Environment variable that keeps the process off the network entirely — the release
/// check and the extension-download fallback alike. Set by the test suites, useful on
/// air-gapped machines; the durable per-user switch is
/// `devp config set update_check false`.
pub const ENV_OFFLINE: &str = "DEV_PRUNE_OFFLINE";

/// Environment variable that stops both install scripts from asking anything.
///
/// The scripts offer to migrate a copy another package manager owns, and `devp install
/// --channel installer` re-runs the script that made the offer. Without this the offer
/// would be made again by that inner run, to a user who has already answered it — the
/// same copy is still on PATH until the uninstall at the end. It is also the switch for
/// a provisioning script that wants the install and none of the conversation.
///
/// Read by `scripts/install.sh`, `scripts/install.ps1`, and set by
/// `src/commands/install.rs` on the child it spawns.
pub const ENV_NO_MIGRATE_PROMPT: &str = "DEV_PRUNE_NO_MIGRATE_PROMPT";

/// The install receipt, written beside the managed binary by whichever installer put it
/// there. See [`crate::receipt`].
///
/// Also written by `scripts/install.sh` and `scripts/install.ps1`, by hand, in their own
/// languages — which is why the field names have a test of their own.
pub const INSTALL_RECEIPT_FILE: &str = "install.json";

/// Set to any value to keep every full-screen view from opening, so the line-by-line
/// fallback runs instead.
///
/// For agents and wrappers that hold a real terminal — the terminal test alone cannot
/// tell them apart from a person, and a full-screen view waiting on a keypress from
/// something that will never send one is a hang.
pub const ENV_NO_TUI: &str = "DEV_PRUNE_NO_TUI";

/// Environment variable that overrides the `language` setting for one invocation.
///
/// What a script or a CI job sets when it wants output in a known language whatever the
/// machine is configured for. An unrecognised code falls back to [`DEFAULT_LANGUAGE`]
/// rather than failing, because this is read before the command runs and a typo in a
/// cosmetic setting should not stop a prune.
pub const ENV_LANGUAGE: &str = "DEV_PRUNE_LANG";

/// Windows environment variable that carries the *machine's* architecture when the
/// running process is emulated.
///
/// `std::env::consts::ARCH` is baked in at compile time and only ever describes the
/// binary. Windows sets this one under WOW64 and under ARM64 emulation, which is the
/// only way a 32-bit build can tell that it is running on a 64-bit machine.
/// `scripts/install.ps1` reads the same variable to choose which asset to download.
pub const ENV_NATIVE_ARCH: &str = "PROCESSOR_ARCHITEW6432";

/// Filename that, when present in a repo root, causes dev-prune to skip that repo entirely.
///
/// Create this file with: `touch ignore.devprune.json`
pub const DEVPRUNE_IGNORE_FILE: &str = "ignore.devprune.json";

/// Home-relative directory names that conventionally hold a developer's repositories.
///
/// Probed by name — existence is one `stat` each — when discovery has no registered
/// repository to work outwards from. Deliberately a list of conventions rather than a
/// walk of the home directory: `~` also contains `Library`, `AppData` and whatever a
/// cloud-sync client has decided to materialise, and none of that is anybody's code.
///
/// `Documents/GitHub` is GitHub Desktop's default, `source/repos` is Visual Studio's,
/// and `go/src` is the layout every pre-modules Go install still has.
pub const CODE_ROOT_NAMES: &[&str] = &[
    "Code",
    "code",
    "Projects",
    "projects",
    "Developer",
    "Development",
    "dev",
    "src",
    "repos",
    "git",
    "work",
    "workspace",
    "Documents/GitHub",
    "source/repos",
    "go/src",
];

/// Fewest path components a directory must have before discovery will scan it.
///
/// A repository cloned directly into the home directory has `~` as its parent, and
/// "scan the parent of every registered repository" would then mean walking the whole
/// home directory — every cache, every application-support tree, every cloud mount. The
/// depth floor is what stops one repository in the wrong place from turning a cheap
/// neighbourhood scan into a full-disk crawl.
pub const MIN_DISCOVERY_ROOT_DEPTH: usize = 2;

/// Default for whether the scheduled pass looks for unregistered repositories by itself.
///
/// On. The Git hook registers a repository the first time you commit in it, which leaves
/// out every repository you cloned and have not committed to — exactly the idle ones
/// worth pruning. Discovery is also the only way an assistant driving the tool learns
/// about repositories nobody has mentioned to it.
///
/// Safe to have on by default because discovery only *registers*. A newly registered
/// repository is still pruned only once it is idle past `idle_days`, only where a
/// lockfile proves every directory recoverable, and only after the safety invariants
/// pass — so the worst outcome of a wrong guess is a row in `devp status`.
pub const DEFAULT_AUTO_DISCOVER: bool = true;

/// Default timeout in seconds for lockfile enforcement / CLI commands (10 minutes).
pub const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 600;

/// Directory name pnpm gives a store it has to put on a volume of its own.
///
/// pnpm hardlinks its store into every `node_modules` it fills, and a hardlink cannot
/// cross a filesystem. A project on a filesystem that is not the home directory's
/// therefore gets a store at the root of *its* filesystem instead — `V:\\.pnpm-store` on
/// a second Windows drive, `/mnt/data/.pnpm-store` on Linux, `/Volumes/Work/.pnpm-store`
/// on macOS. `devp caches` looks for one on every volume that holds a registered
/// repository, because `pnpm store path` only ever answers for the volume it is run on.
pub const PNPM_VOLUME_STORE_DIR: &str = ".pnpm-store";

/// Timeout for the "where does your cache live?" queries `devp caches` makes.
///
/// Deliberately not `command_timeout_secs`. That ceiling is sized for `npm ci` and
/// `cargo metadata`; `npm config get cache` prints one line and returns. A query that
/// has not answered in five seconds is a broken installation, and the report is better
/// off falling back to the conventional path than waiting ten minutes for it.
pub const CACHE_QUERY_TIMEOUT_SECS: u64 = 5;

/// Timeout for asking a container engine how much disk it is using.
///
/// Longer than [`CACHE_QUERY_TIMEOUT_SECS`], because `docker system df` is not a config
/// lookup: the daemon walks every image layer, container and build-cache record to
/// answer it, and on a store with hundreds of images that is genuinely a few seconds. A
/// daemon that is not running refuses in milliseconds either way, which is the case this
/// ceiling is not for.
pub const CONTAINER_QUERY_TIMEOUT_SECS: u64 = 20;

/// Ceiling for one `devp caches clear` step.
///
/// Ten minutes, not the five seconds a query gets: `go clean -modcache` and deleting a
/// multi-gigabyte `~/.gradle/caches` are genuinely slow, and on Windows every file in a
/// module cache is read-only, so the delete is a chmod-and-unlink per file. A clear that
/// has not finished in ten minutes is wedged, and killing it leaves a partially emptied
/// cache the manager refills on its own.
pub const CACHE_CLEAR_TIMEOUT_SECS: u64 = 600;

/// Documentation URL for troubleshooting lockfile and pruning failures.
pub const TROUBLESHOOTING_URL: &str = "https://devprune.vkrishna04.me/docs/troubleshooting";
/// Name of the structured per-repository configuration file stored inside repo roots.
pub const PER_REPO_CONFIG_FILE: &str = ".devprune.json";

/// Name of the committed, team-wide half of a repository's configuration.
///
/// Same shape and same schema as [`PER_REPO_CONFIG_FILE`], and the opposite intent.
/// `.devprune.json` is written into `.git/info/exclude` so one person's answer stays one
/// person's; this one is meant to be `git add`-ed, so that "nobody prunes this
/// repository" is a fact a fresh clone already knows rather than something every
/// teammate has to be told. The `project.` prefix rather than a new extension is what
/// keeps one JSON schema covering both files.
pub const PROJECT_REPO_CONFIG_FILE: &str = "project.devprune.json";

/// The adapter name the declared-directory pass answers to.
///
/// Not a package manager and not in `get_all_adapters()`, but it appears in the same
/// column of the same report, so it needs the same kind of name — and `--only`,
/// `--skip` and `disabled_adapters` all match on that column. Naming it here is what
/// keeps the filter, the engine and the report agreeing on the spelling.
pub const DECLARED_ADAPTER_NAME: &str = "declared";

/// Public URL for the JSON Schema used by IDEs for .devprune.json IntelliSense.
pub const JSON_SCHEMA_URL: &str = "https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json";

/// Directory under the user's home that marks a Claude Code installation.
///
/// Its presence is how the setup pass decides the machine has an agent to install the
/// skill for; the directory itself is only ever created by Claude Code.
pub const CLAUDE_HOME_DIR: &str = ".claude";

/// Subdirectory of an agent's home where Agent Skills live, one directory per skill.
pub const AGENT_SKILLS_SUBDIR: &str = "skills";

/// Marketplace identifier (`publisher.name`) of the VS Code extension, as understood
/// by `code --install-extension`.
pub const VSCODE_EXTENSION_ID: &str = "VKrishna04.dev-prune";

/// The WinGet package identifier, as published in `packaging/winget/` and in the
/// manifests submitted to microsoft/winget-pkgs. `devp update` and `devp uninstall` name
/// it back to the user, so a typo here sends someone to a command that does not resolve.
pub const WINGET_PACKAGE_ID: &str = "VKrishna04.dev-prune";

/// The Homebrew tap that carries the formula, as `brew tap` takes it. Not homebrew-core:
/// plain `brew install dev-prune` resolves there, and that has a notability bar this
/// project has not cleared. See `docs/DISTRIBUTION.md`.
pub const HOMEBREW_TAP: &str = "Life-Experimentalist/tap";

/// The Scoop bucket name and the repository behind it. `scoop bucket add` takes both,
/// and the name is what `scoop install` then resolves `dev-prune` against.
pub const SCOOP_BUCKET_NAME: &str = "life-experimentalist";
/// Repository URL for [`SCOOP_BUCKET_NAME`].
pub const SCOOP_BUCKET_URL: &str = "https://github.com/Life-Experimentalist/scoop-bucket";

/// Where a person can read about the extension before installing it.
///
/// The offer prints all three. The two registries carry the same build — the release
/// `.vsix` — but a machine that trusts one may not have the other, and someone who
/// wants to read the source before letting anything into their editor needs neither.
pub const VSCODE_MARKETPLACE_URL: &str =
    "https://marketplace.visualstudio.com/items?itemName=VKrishna04.dev-prune";
/// The Open VSX listing, which is what VSCodium, Cursor and Windsurf resolve against.
pub const OPENVSX_URL: &str = "https://open-vsx.org/extension/VKrishna04/dev-prune";

/// Name of the Windows Task Scheduler task the daemon registers.
pub const WINDOWS_TASK_NAME: &str = "DevPrune";
/// File name of the windowless scheduler binary — the same CLI built for the GUI
/// subsystem, the relationship `pythonw.exe` has to `python.exe`. A `[[bin]]` target, so
/// it ships in the Windows archives and `cargo install` places it; nothing generates it
/// on a user's machine.
pub const WINDOWS_WINDOWLESS_BIN: &str = "devpw.exe";
/// Marker file (in the config directory) recording that this machine's Task Scheduler
/// refused the sessionless (S4U) task registration, so setup keeps the console task
/// instead of retrying the upgrade on every pass.
///
/// Named for what the task *is* and not for what it is not: this value lands in the
/// binary's string table a few bytes from `devpw.exe`, and "devpw" next to "hidden" is
/// what a reviewer running `strings` reads first. Nothing here is concealed from
/// anyone — the task is listed in Task Scheduler under its own name and the marker is
/// an empty file in the config directory — so the vocabulary must not imply otherwise.
pub const SCHEDULER_WINDOWLESS_REFUSED_MARKER: &str = "scheduler-windowless-refused";

/// Label of the macOS LaunchAgent the daemon registers (also names its plist file).
pub const MACOS_LAUNCHD_LABEL: &str = "com.devprune.daemon";

/// Where `devp skill --agent cursor` writes the per-repository rules.
pub const CURSOR_RULES_FILE: &str = ".cursor/rules/dev-prune.mdc";

/// Where `devp skill --agent windsurf` writes the per-repository rules.
pub const WINDSURF_RULES_FILE: &str = ".windsurf/rules/dev-prune.md";

/// Where `devp skill --agent antigravity` writes the per-repository rules —
/// Antigravity (Google) reads workspace rules from `.agent/rules/`.
pub const ANTIGRAVITY_RULES_FILE: &str = ".agent/rules/dev-prune.md";

/// Where `devp skill --agent cline` writes its rules (Cline reads every file in the
/// `.clinerules/` directory).
pub const CLINE_RULES_FILE: &str = ".clinerules/dev-prune.md";

/// Where `devp skill --agent agents-md` writes its marked block — the cross-tool
/// convention read by Codex, Jules, Amp, Antigravity and others.
pub const AGENTS_MD_FILE: &str = "AGENTS.md";

/// Where `devp skill --agent roo` writes its rules (Roo Code reads every file in
/// `.roo/rules/`).
pub const ROO_RULES_FILE: &str = ".roo/rules/dev-prune.md";

/// Where `devp skill --agent kilocode` writes its rules (Kilo Code reads every file in
/// `.kilocode/rules/`).
pub const KILOCODE_RULES_FILE: &str = ".kilocode/rules/dev-prune.md";

/// Where `devp skill --agent continue` writes its rules (Continue reads every file in
/// `.continue/rules/`).
pub const CONTINUE_RULES_FILE: &str = ".continue/rules/dev-prune.md";

/// Where `devp skill --agent amazon-q` writes its rules (Amazon Q Developer reads every
/// file in `.amazonq/rules/`).
pub const AMAZON_Q_RULES_FILE: &str = ".amazonq/rules/dev-prune.md";

/// Where `devp skill --agent kiro` writes its rules (Kiro reads every file in
/// `.kiro/steering/`).
pub const KIRO_STEERING_FILE: &str = ".kiro/steering/dev-prune.md";

/// Where `devp skill --agent trae` writes its rules (Trae reads every file in
/// `.trae/rules/`).
pub const TRAE_RULES_FILE: &str = ".trae/rules/dev-prune.md";

/// The shared file `devp skill --agent junie` owns a marked block inside — JetBrains
/// Junie reads one guidelines file, not a directory.
pub const JUNIE_GUIDELINES_FILE: &str = ".junie/guidelines.md";

/// The shared file `devp skill --agent gemini` owns a marked block inside — the Gemini
/// CLI reads one context file per repository.
pub const GEMINI_MD_FILE: &str = "GEMINI.md";

/// The shared file `devp skill --agent zed` owns a marked block inside. Zed reads
/// `.rules` ahead of every other convention, so a repository that also has an
/// `AGENTS.md` still needs this one.
pub const ZED_RULES_FILE: &str = ".rules";

/// The shared file `devp skill --agent aider` owns a marked block inside. Aider is the
/// one target that does not read its file on its own: `CONVENTIONS.md` is loaded only
/// by `aider --read CONVENTIONS.md` or a `read: CONVENTIONS.md` line in
/// `.aider.conf.yml`, which is why writing it prints that instruction.
pub const AIDER_CONVENTIONS_FILE: &str = "CONVENTIONS.md";

/// The shared file `devp skill --agent copilot` owns a marked block inside.
pub const COPILOT_INSTRUCTIONS_FILE: &str = ".github/copilot-instructions.md";

/// Markers around the block in [`COPILOT_INSTRUCTIONS_FILE`] that dev-prune manages.
/// Everything outside them belongs to the user and is never touched.
pub const RULES_BLOCK_START: &str = "<!-- dev-prune:rules:start -->";
pub const RULES_BLOCK_END: &str = "<!-- dev-prune:rules:end -->";

/// The phrase Git uses when it refuses a working tree owned by another account.
///
/// Matched against Git's own stderr rather than parsed: the message is twelve lines
/// long, ten of which are identical for every repository refused, and `devp run`
/// groups every repository sharing this cause under one explanation and one fix
/// instead of reprinting those ten lines per repository.
pub const GIT_DUBIOUS_OWNERSHIP: &str = "detected dubious ownership";

/// The phrase Git uses when the path it was pointed at is not a working tree at all.
///
/// Same reasoning as [`GIT_DUBIOUS_OWNERSHIP`]: one cause, one fix, one paragraph.
pub const GIT_NOT_A_REPOSITORY: &str = "not a git repository";
