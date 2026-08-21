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

/// Default idle threshold for *build-tool* directories (gradle, maven), in days.
///
/// Deliberately much longer than [`DEFAULT_IDLE_DAYS`]: those directories come back
/// by recompiling the whole project, not by re-downloading a dependency tree, so the
/// bar for "nobody will miss this" sits higher. The engine applies
/// `max(build_idle_days, idle_days)`.
pub const DEFAULT_BUILD_IDLE_DAYS: u64 = 60;

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
/// Contacted by `devp update`, by the interval-gated check behind `run`/`status`/`init`
/// (off via `update_check false` or `DEV_PRUNE_OFFLINE`), and by the one-time
/// extension-install offer's `.vsix` fallback. See the network policy in
/// `docs/PRIVACY.md`.
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/Life-Experimentalist/dev-prune/releases/latest";

/// Whether the periodic release check runs. On by default — see `Settings::update_check`.
pub const DEFAULT_UPDATE_CHECK: bool = true;

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

/// Filename that, when present in a repo root, causes dev-prune to skip that repo entirely.
///
/// Create this file with: `touch ignore.devprune.json`
pub const DEVPRUNE_IGNORE_FILE: &str = "ignore.devprune.json";

/// Default timeout in seconds for lockfile enforcement / CLI commands (10 minutes).
pub const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 600;

/// Timeout for the "where does your cache live?" queries `devp caches` makes.
///
/// Deliberately not `command_timeout_secs`. That ceiling is sized for `npm ci` and
/// `cargo metadata`; `npm config get cache` prints one line and returns. A query that
/// has not answered in five seconds is a broken installation, and the report is better
/// off falling back to the conventional path than waiting ten minutes for it.
pub const CACHE_QUERY_TIMEOUT_SECS: u64 = 5;

/// Documentation URL for troubleshooting lockfile and pruning failures.
pub const TROUBLESHOOTING_URL: &str = "https://devprune.vkrishna04.me/docs/troubleshooting";
/// Name of the structured per-repository configuration file stored inside repo roots.
pub const PER_REPO_CONFIG_FILE: &str = ".devprune.json";

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
/// File name of the windowless scheduler binary — `dev-prune.exe` with its PE subsystem
/// set to GUI, the same relationship `pythonw.exe` has to `python.exe`. Generated
/// locally beside the managed binary; never shipped in any archive.
pub const WINDOWS_HIDDEN_BIN: &str = "devpw.exe";
/// Marker file (in the config directory) recording that this machine's Task Scheduler
/// refused the hidden (S4U) task registration, so setup keeps the visible task instead
/// of retrying the upgrade on every pass.
pub const SCHEDULER_HIDDEN_REFUSED_MARKER: &str = "scheduler-hidden-refused";

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

/// The shared file `devp skill --agent copilot` owns a marked block inside.
pub const COPILOT_INSTRUCTIONS_FILE: &str = ".github/copilot-instructions.md";

/// Markers around the block in [`COPILOT_INSTRUCTIONS_FILE`] that dev-prune manages.
/// Everything outside them belongs to the user and is never touched.
pub const RULES_BLOCK_START: &str = "<!-- dev-prune:rules:start -->";
pub const RULES_BLOCK_END: &str = "<!-- dev-prune:rules:end -->";
