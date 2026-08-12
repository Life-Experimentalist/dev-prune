// Centralized application constants and default configurations.
//
// Serves as the single source of truth for app metadata, versioning,
// default thresholds, and file paths.

/// Application crate version derived dynamically from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Application name.
pub const APP_NAME: &str = "dev-prune";

/// Default idle threshold in days before a repository is eligible for pruning.
pub const DEFAULT_IDLE_DAYS: u64 = 15;

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
pub const DEFAULT_AUTO_SETUP: bool = true;

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

/// GitHub API endpoint for the latest published release.
///
/// Contacted only by `devp update --check`, never on any other code path. See the
/// network policy in `docs/PRIVACY.md`.
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
