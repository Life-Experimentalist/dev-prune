// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Configuration and registry management for dev-prune.
//
// This module handles persistent storage of:
// - Global settings (idle threshold, check interval, daemon toggle)
// - Registered repository paths and their metadata
//
// All data is stored in `~/.config/dev-prune/registry.json`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::constants;

/// Global settings that control prune behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Number of inactive days before a repo is eligible for pruning.
    pub idle_days: u64,
    /// Interval in days between automated daemon checks.
    pub check_interval_days: u64,
    /// Whether the setup pass installs the OS scheduler. On by default.
    pub auto_daemon: bool,
    /// Whether the setup pass installs the global Git hooks. On by default.
    #[serde(default = "default_auto_hooks")]
    pub auto_hooks: bool,
    /// Whether dev-prune installs its own missing integrations. On by default.
    #[serde(default = "default_auto_setup")]
    pub auto_setup: bool,
    /// Whether `link` and `init` write a default `.devprune.json` into repositories
    /// they register. Off by default; see [`constants::DEFAULT_AUTO_CONFIG`].
    #[serde(default = "default_auto_config")]
    pub auto_config: bool,
    /// Whether interactive confirmation is required before pruning.
    #[serde(default = "default_require_confirmation")]
    pub require_confirmation: bool,
    /// Timeout in seconds for lockfile enforcement / CLI commands (default 600s = 10m).
    #[serde(default = "default_command_timeout_secs")]
    pub command_timeout_secs: u64,
    /// Smallest bloat directory worth deleting, in MiB. `0` disables the floor.
    ///
    /// Below this size the reinstall costs more than the space is worth, so the
    /// directory is not offered as a candidate at all.
    #[serde(default = "default_min_size_mb")]
    pub min_size_mb: u64,
    /// Whether dev-prune asks GitHub for the latest release from time to time.
    ///
    /// On by default, and opt-*out* rather than opt-in: an out-of-date cleanup tool is a
    /// tool whose safety fixes you do not have. The request sends nothing but itself —
    /// no identifier, no configuration, no usage data. Turn it off with
    /// `devp config set update_check false`.
    #[serde(default = "default_update_check")]
    pub update_check: bool,
    /// How many directory levels below a repository root discovery descends.
    ///
    /// Six by default. A flat repository never notices; a monorepo that nests projects
    /// under `packages/@scope/name/app` does. Raise it when `devp status` does not list
    /// a project you know is there, and remember that the walk gets more expensive with
    /// every level. Clamped to [`constants::MAX_SCAN_DEPTH_LIMIT`].
    #[serde(default = "default_scan_depth")]
    pub scan_depth: usize,
    /// Whether cargo and go may run the sync command that rewrites tracked manifests.
    ///
    /// Off. See [`constants::DEFAULT_ALLOW_MANIFEST_REWRITE`] — with this off, both are
    /// verified read-only and a project with no lockfile at all is simply not pruned.
    #[serde(default = "default_allow_manifest_rewrite")]
    pub allow_manifest_rewrite: bool,
    /// Days between automatic release checks.
    ///
    /// Only the *automatic* check honours this; `devp update` always asks, because you
    /// are standing there waiting for the answer.
    #[serde(default = "default_update_check_interval_days")]
    pub update_check_interval_days: i64,
    /// How long the release check waits for GitHub before giving up, in seconds.
    ///
    /// Five is right on a normal connection and too short behind some corporate proxies,
    /// which is the whole reason this is a setting rather than a constant.
    #[serde(default = "default_update_check_timeout_secs")]
    pub update_check_timeout_secs: u64,
    /// Whether the setup pass may install the Git hooks *in front of* another tool's.
    ///
    /// Off. With it on, a `core.hooksPath` that belongs to husky is not a reason to skip:
    /// dev-prune takes the slot and forwards every hook back to the directory it
    /// displaced. Behaviour-preserving, but it is still someone else's setup, so it is
    /// asked for rather than assumed. Same thing as `devp hook install --chain`.
    #[serde(default = "default_auto_hooks_chain")]
    pub auto_hooks_chain: bool,
    /// Whether the opt-in Gradle build-tool adapter is active. Off by default:
    /// `build/` comes back by recompiling the project, so nobody should find it
    /// deleted without having asked. See [`crate::adapters::gradle`].
    #[serde(default)]
    pub enable_gradle: bool,
    /// Whether the opt-in Maven build-tool adapter is active. Off by default, for the
    /// same reason as `enable_gradle`. See [`crate::adapters::maven`].
    #[serde(default)]
    pub enable_maven: bool,
    /// Idle days required before *build-tool* directories (gradle, maven) are pruned.
    ///
    /// Separate from `idle_days` because the cost of being wrong is different: a
    /// deleted `node_modules` is one `npm ci` away, a deleted Android `build/` is a
    /// long recompile. Applied as `max(build_idle_days, idle_days)`.
    #[serde(default = "default_build_idle_days")]
    pub build_idle_days: u64,
    /// Whether `devp update --install` runs by itself at the end of a prune pass when
    /// a newer release is known. Off by default: replacing the binary is visible,
    /// channel-specific behaviour the user opts into.
    #[serde(default)]
    pub auto_update: bool,
}

fn default_build_idle_days() -> u64 {
    constants::DEFAULT_BUILD_IDLE_DAYS
}

fn default_require_confirmation() -> bool {
    constants::DEFAULT_REQUIRE_CONFIRMATION
}

fn default_command_timeout_secs() -> u64 {
    constants::DEFAULT_COMMAND_TIMEOUT_SECS
}

fn default_auto_hooks() -> bool {
    constants::DEFAULT_AUTO_HOOKS
}

fn default_auto_setup() -> bool {
    constants::DEFAULT_AUTO_SETUP
}

fn default_auto_config() -> bool {
    constants::DEFAULT_AUTO_CONFIG
}

fn default_update_check() -> bool {
    constants::DEFAULT_UPDATE_CHECK
}

fn default_min_size_mb() -> u64 {
    constants::DEFAULT_MIN_SIZE_MB
}

fn default_scan_depth() -> usize {
    constants::DEFAULT_SCAN_DEPTH
}

fn default_allow_manifest_rewrite() -> bool {
    constants::DEFAULT_ALLOW_MANIFEST_REWRITE
}

fn default_update_check_interval_days() -> i64 {
    constants::UPDATE_CHECK_INTERVAL_DAYS
}

fn default_update_check_timeout_secs() -> u64 {
    constants::UPDATE_CHECK_TIMEOUT_SECS
}

fn default_auto_hooks_chain() -> bool {
    constants::DEFAULT_AUTO_HOOKS_CHAIN
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_days: constants::DEFAULT_IDLE_DAYS,
            check_interval_days: constants::DEFAULT_CHECK_INTERVAL_DAYS,
            auto_daemon: constants::DEFAULT_AUTO_DAEMON,
            auto_hooks: constants::DEFAULT_AUTO_HOOKS,
            auto_setup: constants::DEFAULT_AUTO_SETUP,
            auto_config: constants::DEFAULT_AUTO_CONFIG,
            require_confirmation: constants::DEFAULT_REQUIRE_CONFIRMATION,
            command_timeout_secs: constants::DEFAULT_COMMAND_TIMEOUT_SECS,
            min_size_mb: constants::DEFAULT_MIN_SIZE_MB,
            update_check: constants::DEFAULT_UPDATE_CHECK,
            scan_depth: constants::DEFAULT_SCAN_DEPTH,
            allow_manifest_rewrite: constants::DEFAULT_ALLOW_MANIFEST_REWRITE,
            update_check_interval_days: constants::UPDATE_CHECK_INTERVAL_DAYS,
            update_check_timeout_secs: constants::UPDATE_CHECK_TIMEOUT_SECS,
            auto_hooks_chain: constants::DEFAULT_AUTO_HOOKS_CHAIN,
            enable_gradle: false,
            enable_maven: false,
            build_idle_days: constants::DEFAULT_BUILD_IDLE_DAYS,
            auto_update: false,
        }
    }
}

/// Metadata for a single registered repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoEntry {
    /// Timestamp when the repo was added to the registry.
    pub added_at: DateTime<Utc>,
    /// Timestamp of the last successful prune, if any.
    pub last_pruned_at: Option<DateTime<Utc>>,
    /// Per-repo override for idle days (overrides global setting).
    pub override_idle_days: Option<u64>,
    /// Whether this repo is enabled for pruning.
    pub enabled: bool,
    /// Cumulative bytes reclaimed from this repository.
    ///
    /// Recorded from 1.1.0 onward. Registries written by 1.0.0 have no such figure and
    /// deserialize to zero, so `devp stats` says where the number starts rather than
    /// implying a repository pruned last March never freed anything.
    #[serde(default)]
    pub total_freed_bytes: u64,
}

impl RepoEntry {
    /// Creates a new `RepoEntry` with the current timestamp.
    pub fn new() -> Self {
        Self {
            added_at: Utc::now(),
            last_pruned_at: None,
            override_idle_days: None,
            enabled: true,
            total_freed_bytes: 0,
        }
    }
}

impl Default for RepoEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve where a repository's shared git directory actually lives.
///
/// `.git` is a directory in an ordinary clone, but in worktrees and submodules it is a
/// one-line `gitdir: <path>` pointer file — and a worktree's private gitdir in turn
/// holds a `commondir` file pointing at the shared one, which is where `info/exclude`
/// lives. Returns `None` when the path is not inside a git repository at all.
fn git_common_dir(repo_path: &Path) -> Option<PathBuf> {
    let dot_git = repo_path.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let pointer = fs::read_to_string(&dot_git).ok()?;
        let target = pointer.strip_prefix("gitdir:")?.trim();
        let target = Path::new(target);
        if target.is_absolute() {
            target.to_path_buf()
        } else {
            repo_path.join(target)
        }
    };
    if let Ok(common) = fs::read_to_string(git_dir.join("commondir")) {
        let target = Path::new(common.trim());
        if target.is_absolute() {
            return Some(target.to_path_buf());
        }
        return Some(git_dir.join(target));
    }
    Some(git_dir)
}

/// Ensure an entry (e.g. ".devprune.json") is in the repository's `.git/info/exclude`.
///
/// The exclude file, not `.gitignore`: the config records one machine's preferences,
/// and `.gitignore` is a tracked file shared by everyone who clones the repository —
/// appending to it silently puts an uncommitted change in the user's diff. The exclude
/// file gives the same "never shows up in `git status`" result without touching
/// anything the repository tracks.
pub fn ensure_in_git_exclude(repo_path: &Path, entry: &str) -> Result<()> {
    let Some(git_dir) = git_common_dir(repo_path) else {
        return Ok(());
    };
    let info_dir = git_dir.join("info");
    fs::create_dir_all(&info_dir)?;
    let exclude_path = info_dir.join("exclude");
    if exclude_path.exists() {
        let content = fs::read_to_string(&exclude_path)?;
        if !content.lines().any(|line| line.trim() == entry) {
            let mut file = fs::OpenOptions::new().append(true).open(&exclude_path)?;
            let prefix = if content.ends_with('\n') || content.is_empty() {
                ""
            } else {
                "\n"
            };
            writeln!(file, "{prefix}{entry}")?;
        }
    } else {
        fs::write(&exclude_path, format!("{entry}\n"))?;
    }
    Ok(())
}

/// Normalise a repository path into the form used as a registry key.
///
/// Falls back to the path as given when it cannot be canonicalised (e.g. it no longer
/// exists), so entries for deleted repos stay addressable.
pub fn canonical_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Resolve `.` and `..` segments and anchor a relative path to the working directory,
/// for paths that no longer exist and so cannot be canonicalised whole. The deepest
/// ancestor that still exists is canonicalised and the missing tail re-appended:
/// registry keys are canonical, and a deleted repo named through a symlinked parent —
/// macOS's `/var` → `/private/var` temp tree being the everyday case — would otherwise
/// spell the same directory through a different root and never compare equal.
fn lexical_absolute(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    let mut prefix = out.as_path();
    while !prefix.as_os_str().is_empty() {
        if let Ok(real) = prefix.canonicalize() {
            if let Ok(tail) = out.strip_prefix(prefix) {
                return real.join(tail);
            }
            break;
        }
        match prefix.parent() {
            Some(parent) => prefix = parent,
            None => break,
        }
    }
    out
}

/// Whether two paths name the same directory, tolerating the differences
/// canonicalisation normally absorbs: the Windows `\\?\` prefix, separator style,
/// trailing separators, and case on Windows.
fn loose_path_eq(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.strip_prefix("//?/").unwrap_or(&s);
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// Expand a leading `~` to the user's home directory.
///
/// POSIX shells do this before the argument ever reaches a program, so on Linux and
/// macOS it is usually a no-op. PowerShell and cmd do not: they hand a native
/// executable the literal three characters `~/C`, and `devp init ~/Code` — the exact
/// line in the README and on the landing page — would register a directory called `~`
/// sitting in the current working directory. Quoting defeats the expansion in *every*
/// shell, so `devp init "~/Code"` needs this too.
///
/// Only a bare `~` or a `~` followed by a separator is expanded. `~alice` means "some
/// other user's home" in shell syntax and cannot be resolved portably, and `~backup` is
/// a perfectly ordinary directory name.
pub fn expand_tilde(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix('~') else {
        return raw.to_string();
    };
    if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')) {
        return raw.to_string();
    }
    let Some(home) = dirs::home_dir() else {
        // No home directory to expand to. Handing back the literal `~` lets the caller
        // fail with "no such directory", which is a better error than a silent guess.
        return raw.to_string();
    };
    if rest.is_empty() {
        return home.to_string_lossy().into_owned();
    }
    home.join(rest.trim_start_matches(['/', '\\']))
        .to_string_lossy()
        .into_owned()
}

/// Structured per-repository configuration file stored inside repo roots as `.devprune.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerRepoConfig {
    /// JSON Schema reference URL for IDE IntelliSense and validation.
    #[serde(rename = "$schema", default = "default_schema_url")]
    pub schema: String,
    /// Custom display name for this project in TUI and CLI status views.
    #[serde(default)]
    pub project_name: Option<String>,
    /// Whether this repository is ignored/excluded from pruning.
    #[serde(default)]
    pub ignore: bool,
    /// Disable global Git auto-registration hooks for this specific workspace.
    #[serde(default)]
    pub disable_hooks: bool,
    /// Disable background daemon automated pruning pass for this specific workspace.
    #[serde(default)]
    pub disable_daemon: bool,
    /// Custom override for idle days threshold (overrides global settings).
    #[serde(default)]
    pub override_idle_days: Option<u64>,
    /// Custom override for the size floor, in MiB (overrides global `min_size_mb`).
    ///
    /// `Some(0)` is a meaningful value: it turns the floor off for this repository even
    /// when a global floor is set.
    #[serde(default)]
    pub min_size_mb: Option<u64>,
    /// Custom override for how deep discovery walks this repository.
    ///
    /// The setting that most often needs to differ per repository rather than globally:
    /// one deeply-nested monorepo should not make every other repository pay for a
    /// deeper walk. Clamped to [`constants::MAX_SCAN_DEPTH_LIMIT`] like the global one.
    #[serde(default)]
    pub scan_depth: Option<usize>,
}

// Deliberately absent: `allow_manifest_rewrite`.
//
// Only the settings whose right value depends on the *project* have a per-repository
// form. `allow_manifest_rewrite` is a permission the user grants their own machine, and
// — exactly as with `post_prune_command` below — nothing stops a project from committing
// its `.devprune.json`: the `.git/info/exclude` entry [`PerRepoConfig::save_to_repo`]
// writes is local to one clone and excludes nothing already tracked. A
// per-repository form would therefore let a repository nobody has read grant itself the
// right to have `cargo generate-lockfile` / `go mod tidy` rewrite its tracked manifests
// during an unattended pass. The `auto_*` and `update_check*` settings describe the
// machine rather than a project and would mean nothing here either.

// Removed: `custom_bloat_dirs` and `post_prune_command`.
//
// Both were serialized, schema'd and documented but never read by any code path, so
// setting them did nothing. `post_prune_command` is also not a feature that should be
// reintroduced casually: nothing stops a project from committing its `.devprune.json`,
// so honouring it would mean cloning an untrusted repository and running `devp` hands
// that repository arbitrary code execution on the user's machine.

fn default_schema_url() -> String {
    if let Ok(config_dir) = Registry::config_dir() {
        let local_schema = config_dir.join("bin").join("devprune.schema.json");
        if local_schema.exists() {
            // `file://` + `/` + an absolute path. Unix paths already start with a
            // separator, so pasting them in unconditionally produced `file:////home/...`
            // — four slashes, which editors reject, leaving the `$schema` link dead and
            // no IntelliSense at all on the platform where most of them run.
            return file_uri(&crate::output::clean_path(&local_schema));
        }
    }
    constants::JSON_SCHEMA_URL.to_string()
}

/// A `file://` URI for an absolute path.
///
/// `file://` + `/` + the path. Unix paths already start with a separator, so pasting one
/// in unconditionally produced `file:////home/...` — four slashes, which editors reject,
/// leaving the `$schema` link dead and no IntelliSense at all on the platform where most
/// of them run.
fn file_uri(clean_path: &str) -> String {
    format!("file:///{}", clean_path.trim_start_matches('/'))
}

impl Default for PerRepoConfig {
    fn default() -> Self {
        Self {
            schema: default_schema_url(),
            project_name: None,
            ignore: false,
            disable_hooks: false,
            disable_daemon: false,
            override_idle_days: None,
            min_size_mb: None,
            scan_depth: None,
        }
    }
}

impl PerRepoConfig {
    /// Load per-repo config from `.devprune.json`, or `None` when there is no such file.
    ///
    /// This is the only loader. There used to be a second one that returned `None` for a
    /// file that failed to parse as well as for one that was absent, and every caller of
    /// it then went on to act as though the repository had no configuration: the prune
    /// pass ignored an `"ignore": true` it could not read, and the two workspace toggles
    /// wrote a fresh default file straight over the user's broken one, taking every
    /// override in it with them. A caller that genuinely does not care — the display-name
    /// lookup — says so with `.ok().flatten()`.
    pub fn load_with_diagnostics(repo_path: &Path) -> Result<Option<Self>, String> {
        let config_file = repo_path.join(constants::PER_REPO_CONFIG_FILE);
        if !config_file.exists() {
            return Ok(None);
        }
        let content =
            fs::read_to_string(&config_file).map_err(|e| format!("Failed to read file: {e}"))?;
        match serde_json::from_str::<Self>(&content) {
            Ok(cfg) => Ok(Some(cfg)),
            // `clean_path`, like every other path this tool shows. `Display` on a
            // canonicalised Windows path leaks the `\\?\` extended-length prefix into an
            // error message the user is being asked to act on.
            Err(e) => Err(format!(
                "Syntax error in `{}`: {e}",
                crate::output::clean_path(&config_file)
            )),
        }
    }

    /// Save per-repo config to `.devprune.json` in the repo root, and record it in the
    /// repository's `.git/info/exclude` so it never shows up in `git status`.
    pub fn save_to_repo(&self, repo_path: &Path) -> Result<()> {
        let config_file = repo_path.join(constants::PER_REPO_CONFIG_FILE);
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&config_file, content)?;
        let _ = ensure_in_git_exclude(repo_path, constants::PER_REPO_CONFIG_FILE);
        let _ = ensure_in_git_exclude(repo_path, constants::DEVPRUNE_IGNORE_FILE);
        Ok(())
    }
}

/// One directory a prune pass deleted.
///
/// Enough to put it back and nothing more: which repository it belonged to, which
/// project inside that repository owned it, and who verified it. No file list — the
/// lockfile is the record of the contents, which is the whole premise of the tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrunedDir {
    /// Repository root the directory belonged to.
    pub repo_path: PathBuf,
    /// Repository-relative label, `/`-separated: `node_modules`, `frontend/node_modules`.
    pub bloat_dir: String,
    /// Adapter that verified and deleted it.
    pub adapter: String,
    /// Bytes reclaimed.
    pub size_freed: u64,
}

/// What the most recent prune pass deleted, for `devp restore --last-run`.
///
/// Only passes that actually deleted something are recorded. A later run that frees
/// nothing — everything was active, everything was already clean — leaves this alone,
/// because "put back what you just took" should still mean the pass that took something.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastPrune {
    /// When the pass ran.
    pub at: DateTime<Utc>,
    /// Every directory it removed.
    pub dirs: Vec<PrunedDir>,
}

/// A one-line summary of a completed prune pass, for `devp stats`.
///
/// Deliberately not a second copy of [`LastPrune`]. That one exists so
/// `devp restore --last-run` can put files back, so it carries the full directory list
/// and only ever describes the most recent pass. This one is a trend line — four numbers
/// per pass, bounded by [`constants::PRUNE_HISTORY_LIMIT`] — and could not restore
/// anything if it wanted to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruneRunSummary {
    /// When the pass ran.
    pub at: DateTime<Utc>,
    /// Bytes reclaimed by the pass.
    pub bytes_freed: u64,
    /// How many directories it removed.
    pub dirs_removed: usize,
    /// How many distinct repositories it touched.
    pub repos_touched: usize,
}

/// The top-level registry structure persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    /// Schema version for forward compatibility.
    pub version: String,
    /// Global settings.
    pub settings: Settings,
    /// Map of canonical repo paths to their metadata.
    pub repositories: HashMap<PathBuf, RepoEntry>,
    /// Total cumulative bytes freed historically across all prune passes.
    #[serde(default)]
    pub total_freed_bytes: u64,
    /// How many prune passes have deleted something, ever.
    ///
    /// One per *pass*, not per repository and not per directory — a `devp run` that
    /// cleared eleven directories across four repositories counts once. Incremented in
    /// exactly one place, [`Registry::record_prune`], which is also where the pass is
    /// recorded for `devp restore --last-run`; keeping the two together is what stops
    /// them meaning different things depending on which command did the pruning.
    #[serde(default)]
    pub total_pruned_count: u64,
    /// List of repository paths added in the most recent init/link action (for devp undo).
    #[serde(default)]
    pub last_added_repos: Vec<PathBuf>,
    /// What the most recent prune pass deleted (for `devp restore --last-run`).
    #[serde(default)]
    pub last_prune: Option<LastPrune>,
    /// Summaries of recent prune passes, oldest first, for `devp stats`.
    ///
    /// Capped at [`constants::PRUNE_HISTORY_LIMIT`]. Recorded from 1.1.0 onward.
    #[serde(default)]
    pub prune_history: Vec<PruneRunSummary>,
    /// When the release check last ran, so it runs at most once every
    /// `UPDATE_CHECK_INTERVAL_DAYS` instead of on every command.
    #[serde(default)]
    pub last_update_check: Option<DateTime<Utc>>,
    /// The newest release seen by the last check, so the reminder survives until the
    /// user actually upgrades without needing the network again.
    #[serde(default)]
    pub latest_known_version: Option<String>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            settings: Settings::default(),
            repositories: HashMap::new(),
            total_freed_bytes: 0,
            total_pruned_count: 0,
            last_added_repos: Vec::new(),
            last_prune: None,
            prune_history: Vec::new(),
            last_update_check: None,
            latest_known_version: None,
        }
    }
}

impl Registry {
    /// Returns the path to the config directory (`~/.config/dev-prune/`).
    ///
    /// Uses the `dirs` crate to resolve the platform-specific config location:
    /// - Linux/macOS: `~/.config/dev-prune/`
    /// - Windows: `C:\Users\<user>\AppData\Roaming\dev-prune\` (or `~/.config/dev-prune/`)
    pub fn config_dir() -> Result<PathBuf> {
        if let Ok(override_dir) = std::env::var(constants::ENV_CONFIG_DIR_OVERRIDE) {
            return Ok(PathBuf::from(override_dir));
        }
        let base = dirs::config_dir().context("Could not determine config directory")?;
        Ok(base.join(constants::CONFIG_DIR_NAME))
    }

    /// Returns the full path to the registry file.
    pub fn registry_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join(constants::REGISTRY_FILENAME))
    }

    /// Loads the registry from disk, or the defaults when there is nothing to load.
    ///
    /// Reading does not write. This used to persist the default registry on the way
    /// out, which made `devp --dry-run init` create the very file it had just promised
    /// not to write and gave `devp status --json` — documented as a pure read — a side
    /// effect on first use. Every command that actually changes something calls
    /// [`Registry::save`], and that creates the directory as needed.
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::registry_path()?)
    }

    /// Loads the registry from a specific path (for testing or custom locations).
    ///
    /// Non-persisting, exactly like [`Registry::load`], which is implemented on top of
    /// it. The two used to disagree — this one wrote the defaults out when the file was
    /// missing — which is the sort of difference that makes a test pass while the
    /// behaviour it stands in for is broken.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Registry::default());
        }
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read registry at {}", path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse registry at {}", path.display()))
    }

    /// Saves the registry to disk atomically (write to temp, then rename).
    pub fn save(&self) -> Result<()> {
        let path = Self::registry_path()?;
        self.save_to(&path)
    }

    /// Saves the registry to a specific path (for testing or custom locations).
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
        }
        // Unique per process. A manual run and the scheduled daemon pass can save at the
        // same moment; with a shared `registry.json.tmp`, one process could rename the
        // other's half-written file into place as a torn, unparseable registry.
        let tmp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let contents =
            serde_json::to_string_pretty(self).context("Failed to serialize registry")?;
        {
            // `sync_all` before the rename, or the atomicity is only apparent: after a
            // power cut the rename can survive while the data does not, leaving the
            // registry as zero bytes — the one outcome this dance exists to prevent.
            use std::io::Write;
            let mut file = fs::File::create(&tmp_path)
                .with_context(|| format!("Failed to write temp registry {}", tmp_path.display()))?;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("Failed to write temp registry {}", tmp_path.display()))?;
            file.sync_all()
                .with_context(|| format!("Failed to flush temp registry {}", tmp_path.display()))?;
        }
        fs::rename(&tmp_path, path)
            .with_context(|| format!("Failed to rename temp registry to {}", path.display()))?;

        // A crash between write and rename strands that process's `.<pid>.tmp` forever.
        // Sweep siblings old enough that no live save can still own them.
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let prefix = format!("{}.", name.to_string_lossy());
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let file_name = file_name.to_string_lossy();
                    if file_name.starts_with(&prefix)
                        && file_name.ends_with(".tmp")
                        && entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age.as_secs() > 3600)
                    {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(())
    }

    /// Adds a repository to the registry. Returns `true` if newly added, `false` if already present.
    pub fn add_repo(&mut self, path: PathBuf) -> bool {
        // The registry is keyed by path, so `./foo`, `foo/`, and the absolute form
        // would otherwise register as three separate repositories.
        let path = canonical_key(&path);
        if self.repositories.contains_key(&path) {
            return false;
        }
        self.repositories.insert(path, RepoEntry::new());
        true
    }

    /// Removes a repository from the registry. Returns `true` if it was present.
    ///
    /// A repository that has been deleted from disk cannot be canonicalised any more,
    /// so `canonical_key` falls back to the path as typed — which never equals the
    /// canonical key it was registered under (on Windows those carry the `\\?\`
    /// prefix). Unlinking a deleted repository is the most ordinary reason to unlink
    /// at all, so a direct miss falls back to a lexical comparison.
    pub fn remove_repo(&mut self, path: &Path) -> bool {
        let target = lexical_absolute(path);
        let removed = if self.repositories.remove(&canonical_key(path)).is_some() {
            true
        } else {
            let found = self
                .repositories
                .keys()
                .find(|k| loose_path_eq(k, &target))
                .cloned();
            found.is_some_and(|k| self.repositories.remove(&k).is_some())
        };
        if removed {
            // The undo list stores the canonical `\\?\`-prefixed spelling, while a
            // deleted directory can only be named lexically — strict equality misses,
            // and the next `devp undo` "reverts" by removing nothing.
            self.last_added_repos.retain(|p| !loose_path_eq(p, &target));
        }
        removed
    }

    // Removed: `repo_paths` and `effective_idle_days`.
    //
    // Neither had a caller outside this file's own tests. `effective_idle_days` had also
    // drifted from the rule the engine actually applies: it looked the repository up by
    // the path as given, where every write to `repositories` goes through
    // `canonical_key`, so `devp`'s own relative paths would have missed the entry and
    // silently returned the global threshold instead of the repository's override.

    /// Credit `bytes_freed` to one repository, and to the machine-wide total.
    ///
    /// Safe to call once per repository or once per directory — every figure it touches
    /// is either a sum or a timestamp, so the two styles agree. Counting *passes* is
    /// deliberately not done here for exactly that reason; that lives in
    /// [`Registry::record_prune`], which is called once per pass.
    pub fn mark_pruned(&mut self, path: &Path, bytes_freed: u64) {
        // Same rule as every other accessor: the map is keyed by `canonical_key`, so a
        // raw lookup would silently skip the per-repo credit for a relative or
        // differently-spelled path while still growing the machine-wide total.
        if let Some(entry) = self.repositories.get_mut(&canonical_key(path)) {
            entry.last_pruned_at = Some(Utc::now());
            entry.total_freed_bytes += bytes_freed;
        }
        self.total_freed_bytes += bytes_freed;
    }

    /// Record what a prune pass deleted, replacing any earlier record.
    ///
    /// A pass that deleted nothing is not a pass worth remembering, so an empty list is
    /// ignored rather than stored — otherwise `devp run` on an already-clean machine
    /// would quietly throw away the record of the run the user actually wants back.
    ///
    /// This is the one place a prune pass is counted. It sets [`Registry::last_prune`],
    /// appends a [`PruneRunSummary`] to [`Registry::prune_history`] and bumps
    /// [`Registry::total_pruned_count`], because "a pass happened and it deleted things"
    /// is exactly the condition all three describe. Splitting them across call sites is
    /// how the counter previously came to mean repositories in `devp run` and directories
    /// in the `devp status` dashboard.
    pub fn record_prune(&mut self, dirs: Vec<PrunedDir>) {
        self.record_prune_progress(Utc::now(), dirs);
    }

    /// Record a pass's progress mid-flight, superseding this same pass's earlier record.
    ///
    /// `at` identifies the pass: a repeated call with the same timestamp replaces the
    /// history entry and `last_prune` it wrote before, rather than counting a second
    /// pass. This exists so a long pass can persist after every repository — a crash
    /// half-way through used to leave `devp restore --last-run` pointing at the
    /// *previous* pass, offering to reinstall directories that were never deleted while
    /// saying nothing about the ones that were.
    pub fn record_prune_progress(&mut self, at: DateTime<Utc>, dirs: Vec<PrunedDir>) {
        if dirs.is_empty() {
            return;
        }
        if self.prune_history.last().map(|s| s.at) == Some(at) {
            self.prune_history.pop();
        } else {
            self.total_pruned_count += 1;
        }

        self.prune_history.push(PruneRunSummary {
            at,
            bytes_freed: dirs.iter().map(|d| d.size_freed).sum(),
            dirs_removed: dirs.len(),
            repos_touched: dirs
                .iter()
                .map(|d| &d.repo_path)
                .collect::<HashSet<_>>()
                .len(),
        });
        // Oldest first, so the overflow comes off the front.
        if self.prune_history.len() > constants::PRUNE_HISTORY_LIMIT {
            let excess = self.prune_history.len() - constants::PRUNE_HISTORY_LIMIT;
            self.prune_history.drain(..excess);
        }

        self.last_prune = Some(LastPrune { at, dirs });
    }

    /// Returns the number of registered repositories.
    pub fn repo_count(&self) -> usize {
        self.repositories.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_registry_path(dir: &TempDir) -> PathBuf {
        dir.path().join("dev-prune").join("registry.json")
    }

    fn a_pruned_dir(label: &str) -> PrunedDir {
        PrunedDir {
            repo_path: PathBuf::from("/repo"),
            bloat_dir: label.to_string(),
            adapter: "npm".to_string(),
            size_freed: 42,
        }
    }

    #[test]
    fn a_prune_that_deleted_nothing_does_not_erase_the_last_one() {
        // Otherwise a second `devp run` on an already-clean machine throws away the
        // record of the pass the user actually wants to undo.
        let mut registry = Registry::default();
        registry.record_prune(vec![a_pruned_dir("node_modules")]);
        let recorded = registry.last_prune.clone().expect("first pass recorded");

        registry.record_prune(Vec::new());

        assert_eq!(registry.last_prune, Some(recorded));
    }

    #[test]
    fn a_later_prune_replaces_the_record() {
        let mut registry = Registry::default();
        registry.record_prune(vec![a_pruned_dir("node_modules")]);
        registry.record_prune(vec![a_pruned_dir("frontend/node_modules")]);

        let dirs = registry.last_prune.unwrap().dirs;
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].bloat_dir, "frontend/node_modules");
    }

    #[test]
    fn the_last_prune_record_survives_a_save_and_load() {
        // `restore --last-run` reads it out of a file written by a process that has
        // already exited, so the round trip is the whole feature.
        let dir = TempDir::new().unwrap();
        let path = test_registry_path(&dir);

        let mut registry = Registry::default();
        registry.record_prune(vec![a_pruned_dir("frontend/node_modules")]);
        registry.save_to(&path).unwrap();

        let loaded = Registry::load_from(&path).unwrap();
        assert_eq!(loaded.last_prune, registry.last_prune);
    }

    #[test]
    fn a_registry_written_before_the_field_existed_still_loads() {
        // The registry on disk predates `last_prune`; a missing key means "no pass
        // recorded", not a parse failure that would lock the user out of their config.
        let dir = TempDir::new().unwrap();
        let path = test_registry_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"version":"1.0","settings":{"idle_days":15,"check_interval_days":2,
               "auto_daemon":true},"repositories":{}}"#,
        )
        .unwrap();

        let loaded = Registry::load_from(&path).unwrap();
        assert_eq!(loaded.last_prune, None);
    }

    #[test]
    fn a_leading_tilde_becomes_the_home_directory() {
        // The whole reason this exists: PowerShell hands `devp init ~/Code` straight
        // through, so without expansion the registry gains a repository at `.\~\Code`.
        let home = dirs::home_dir().expect("test host has a home directory");

        assert_eq!(expand_tilde("~"), home.to_string_lossy());
        assert_eq!(
            expand_tilde("~/Code"),
            home.join("Code").to_string_lossy(),
            "forward slash, as typed in every shell"
        );
        assert_eq!(
            expand_tilde("~\\Code"),
            home.join("Code").to_string_lossy(),
            "backslash, as typed in PowerShell"
        );
    }

    #[test]
    fn a_tilde_that_is_not_a_home_reference_is_left_alone() {
        // `~alice` is another user's home in shell syntax and cannot be resolved
        // portably; `~backup` and `./~tmp` are ordinary directory names. Rewriting any
        // of them would silently point the user at the wrong directory.
        for raw in ["~alice/Code", "~backup", "./~tmp", "Code~", "", "."] {
            assert_eq!(expand_tilde(raw), raw, "{raw} must survive untouched");
        }
    }

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.idle_days, 15);
        assert_eq!(settings.check_interval_days, 2);
        // On by default: dev-prune installs its own integrations, once per version,
        // and only the ones it finds missing.
        assert!(settings.auto_daemon);
        assert!(settings.auto_hooks);
        assert!(settings.auto_setup);
    }

    #[test]
    fn settings_written_before_the_automation_toggles_existed_still_load() {
        // Real registries on disk predate `auto_hooks` / `auto_setup`; an upgrade must
        // read them rather than fail to parse and lose every registered repository.
        let json = r#"{
            "idle_days": 30,
            "check_interval_days": 2,
            "auto_daemon": false
        }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.idle_days, 30);
        assert!(!settings.auto_daemon, "an explicit opt-out is preserved");
        assert!(settings.auto_hooks, "a missing key takes the default");
        assert!(settings.auto_setup);
    }

    #[test]
    fn test_default_registry() {
        let registry = Registry::default();
        assert_eq!(registry.version, "1.0");
        assert_eq!(registry.settings, Settings::default());
        assert!(registry.repositories.is_empty());
    }

    #[test]
    fn test_repo_entry_new() {
        let entry = RepoEntry::new();
        assert!(entry.enabled);
        assert!(entry.last_pruned_at.is_none());
        assert!(entry.override_idle_days.is_none());
    }

    #[test]
    fn test_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let path = test_registry_path(&tmp);

        let mut registry = Registry::default();
        registry.add_repo(PathBuf::from("/test/repo"));
        registry.save_to(&path).unwrap();

        let loaded = Registry::load_from(&path).unwrap();
        assert_eq!(loaded.repo_count(), 1);
        assert!(
            loaded
                .repositories
                .contains_key(&PathBuf::from("/test/repo"))
        );
    }

    #[test]
    fn loading_a_missing_registry_yields_the_defaults_and_writes_nothing() {
        let tmp = TempDir::new().unwrap();
        let path = test_registry_path(&tmp);

        let loaded = Registry::load_from(&path).unwrap();
        assert_eq!(loaded, Registry::default());
        // Reading is not writing. `devp --dry-run` and `devp status --json` both promise
        // to leave the disk alone, and both start by loading the registry.
        assert!(!path.exists(), "loading the registry created it");
    }

    #[test]
    fn test_add_repo_returns_true_for_new() {
        let mut registry = Registry::default();
        assert!(registry.add_repo(PathBuf::from("/test/repo")));
    }

    #[test]
    fn test_add_repo_returns_false_for_duplicate() {
        let mut registry = Registry::default();
        registry.add_repo(PathBuf::from("/test/repo"));
        assert!(!registry.add_repo(PathBuf::from("/test/repo")));
    }

    #[test]
    fn test_remove_repo() {
        let mut registry = Registry::default();
        registry.add_repo(PathBuf::from("/test/repo"));
        assert!(registry.remove_repo(Path::new("/test/repo")));
        assert!(!registry.remove_repo(Path::new("/test/repo")));
        assert_eq!(registry.repo_count(), 0);
    }

    /// macOS puts temp trees behind `/var` → `/private/var`, so a repo registered
    /// through the symlink is keyed under the real path — and once deleted, the
    /// symlinked spelling cannot be canonicalised whole. The lexical fallback must
    /// resolve the surviving parent, or unlink reports "not registered" for a
    /// directory the user is looking at in their own prompt.
    #[cfg(unix)]
    #[test]
    fn a_deleted_repo_named_through_a_symlinked_parent_still_unlinks() {
        let tmp = TempDir::new().unwrap();
        let real_parent = tmp.path().join("real");
        std::fs::create_dir(&real_parent).unwrap();
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&real_parent, &alias).unwrap();

        let repo = real_parent.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let mut registry = Registry::default();
        registry.add_repo(alias.join("repo"));
        std::fs::remove_dir(&repo).unwrap();

        assert!(registry.remove_repo(&alias.join("repo")));
        assert_eq!(registry.repo_count(), 0);
    }

    #[test]
    fn test_mark_pruned() {
        let mut registry = Registry::default();
        registry.add_repo(PathBuf::from("/test/repo"));
        assert!(
            registry.repositories[&PathBuf::from("/test/repo")]
                .last_pruned_at
                .is_none()
        );
        registry.mark_pruned(Path::new("/test/repo"), 1024);
        assert!(
            registry.repositories[&PathBuf::from("/test/repo")]
                .last_pruned_at
                .is_some()
        );
        assert_eq!(registry.total_freed_bytes, 1024);
        // Not the pass counter — that is `record_prune`'s job, once per pass.
        assert_eq!(registry.total_pruned_count, 0);
    }

    #[test]
    fn a_pass_is_counted_once_however_much_it_deleted() {
        // The counter is published as `prune_passes`, and it used to be incremented once
        // per repository by `devp run` and once per *directory* by the status dashboard,
        // so the same work produced a different number depending on where it started.
        let mut registry = Registry::default();
        registry.add_repo(PathBuf::from("/repo"));

        registry.mark_pruned(Path::new("/repo"), 1024);
        registry.mark_pruned(Path::new("/repo"), 1024);
        registry.record_prune(vec![
            a_pruned_dir("node_modules"),
            a_pruned_dir("frontend/node_modules"),
        ]);

        assert_eq!(registry.total_pruned_count, 1);

        registry.record_prune(vec![a_pruned_dir("target")]);
        assert_eq!(registry.total_pruned_count, 2);

        // A pass that deleted nothing is not a pass.
        registry.record_prune(Vec::new());
        assert_eq!(registry.total_pruned_count, 2);
    }

    #[test]
    fn mark_pruned_credits_the_repo_under_its_canonical_key() {
        // On Windows, `canonicalize` yields a `\\?\`-prefixed path, so a registry keyed
        // by the canonical form and a `mark_pruned` looking up the raw form would miss —
        // growing the machine-wide total while the repository's own figure stayed zero.
        let tmp = TempDir::new().unwrap();
        let raw = tmp.path().to_path_buf();

        let mut registry = Registry::default();
        registry.add_repo(raw.clone());
        registry.mark_pruned(&raw, 1024);

        let entry = &registry.repositories[&canonical_key(&raw)];
        assert_eq!(entry.total_freed_bytes, 1024);
        assert!(entry.last_pruned_at.is_some());
        assert_eq!(registry.total_freed_bytes, 1024);
    }

    #[test]
    fn each_repository_accumulates_its_own_total() {
        // `devp stats` ranks repositories against each other, so the per-repo figure has
        // to be a running total and not the size of the most recent pass.
        let mut registry = Registry::default();
        registry.add_repo(PathBuf::from("/test/repo"));
        registry.add_repo(PathBuf::from("/test/other"));

        registry.mark_pruned(Path::new("/test/repo"), 1024);
        registry.mark_pruned(Path::new("/test/repo"), 2048);
        registry.mark_pruned(Path::new("/test/other"), 512);

        assert_eq!(
            registry.repositories[&PathBuf::from("/test/repo")].total_freed_bytes,
            3072
        );
        assert_eq!(
            registry.repositories[&PathBuf::from("/test/other")].total_freed_bytes,
            512
        );
        assert_eq!(registry.total_freed_bytes, 3584);
    }

    #[test]
    fn the_prune_history_summarises_the_pass() {
        let mut registry = Registry::default();
        registry.record_prune(vec![
            a_pruned_dir("node_modules"),
            a_pruned_dir("frontend/node_modules"),
        ]);

        let summary = registry.prune_history.last().expect("pass summarised");
        assert_eq!(summary.bytes_freed, 84);
        assert_eq!(summary.dirs_removed, 2);
        // Both fixtures live under `/repo`, so this is one repository, not two.
        assert_eq!(summary.repos_touched, 1);
    }

    #[test]
    fn the_prune_history_is_capped_and_drops_the_oldest() {
        // The registry is rewritten in full on every save, so an uncapped list would grow
        // the file forever on a machine running the scheduled pass.
        let mut registry = Registry::default();
        for _ in 0..constants::PRUNE_HISTORY_LIMIT + 5 {
            registry.record_prune(vec![a_pruned_dir("node_modules")]);
        }

        assert_eq!(registry.prune_history.len(), constants::PRUNE_HISTORY_LIMIT);
        let first = registry.prune_history.first().unwrap().at;
        let last = registry.prune_history.last().unwrap().at;
        assert!(first <= last, "oldest first");
    }

    #[test]
    fn test_repo_count() {
        let mut registry = Registry::default();
        assert_eq!(registry.repo_count(), 0);
        registry.add_repo(PathBuf::from("/a"));
        registry.add_repo(PathBuf::from("/b"));
        assert_eq!(registry.repo_count(), 2);
    }

    #[test]
    fn a_local_schema_uri_has_exactly_three_slashes_on_either_platform() {
        assert_eq!(
            file_uri("/home/dev/.config/dev-prune/bin/devprune.schema.json"),
            "file:///home/dev/.config/dev-prune/bin/devprune.schema.json"
        );
        assert_eq!(
            file_uri("C:/Users/dev/AppData/Roaming/dev-prune/bin/devprune.schema.json"),
            "file:///C:/Users/dev/AppData/Roaming/dev-prune/bin/devprune.schema.json"
        );
    }

    #[test]
    fn a_broken_per_repo_config_is_an_error_rather_than_an_absent_one() {
        // The distinction the whole tool leans on: "no config" means take the defaults,
        // "unreadable config" means refuse — never overwrite, never prune on a guess.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        assert_eq!(PerRepoConfig::load_with_diagnostics(repo), Ok(None));

        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "ignore": true, }"#,
        )
        .unwrap();
        let err = PerRepoConfig::load_with_diagnostics(repo).unwrap_err();
        assert!(err.contains("Syntax error"), "{err}");

        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "ignore": true }"#,
        )
        .unwrap();
        assert!(
            PerRepoConfig::load_with_diagnostics(repo)
                .unwrap()
                .unwrap()
                .ignore
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut registry = Registry::default();
        registry.settings.idle_days = 30;
        registry.add_repo(PathBuf::from("/test/repo"));

        let json = serde_json::to_string_pretty(&registry).unwrap();
        let deserialized: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(registry.settings.idle_days, deserialized.settings.idle_days);
        assert_eq!(registry.repo_count(), deserialized.repo_count());
    }

    #[test]
    fn test_atomic_save_leaves_no_tmp() {
        let tmp = TempDir::new().unwrap();
        let path = test_registry_path(&tmp);

        let registry = Registry::default();
        registry.save_to(&path).unwrap();

        assert!(path.exists());
        // Nothing but the registry itself may remain — a leftover `*.tmp` would mean the
        // rename never happened.
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.path() != path)
            .collect();
        assert!(leftovers.is_empty(), "leftover files: {leftovers:?}");
    }

    #[test]
    fn exclude_entry_lands_in_git_info_exclude_not_gitignore() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::create_dir(repo.join(".git")).unwrap();

        ensure_in_git_exclude(repo, ".devprune.json").unwrap();

        let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == ".devprune.json"));
        // The whole point of using the exclude file: the shared, tracked `.gitignore`
        // must never be created or touched.
        assert!(!repo.join(".gitignore").exists());
    }

    #[test]
    fn exclude_entry_is_appended_once_and_preserves_existing_lines() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::create_dir_all(repo.join(".git/info")).unwrap();
        // No trailing newline, deliberately — the append must not glue two entries
        // onto one line.
        fs::write(repo.join(".git/info/exclude"), "*.log").unwrap();

        ensure_in_git_exclude(repo, ".devprune.json").unwrap();
        ensure_in_git_exclude(repo, ".devprune.json").unwrap();

        let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        let lines: Vec<_> = exclude.lines().collect();
        assert_eq!(lines, vec!["*.log", ".devprune.json"]);
    }

    #[test]
    fn exclude_follows_a_gitdir_pointer_file() {
        // Worktrees and submodules have a one-line `.git` *file*, and a worktree's
        // private gitdir points at the shared one via `commondir` — where the real
        // `info/exclude` lives.
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("main-clone/.git");
        let worktree_gitdir = shared.join("worktrees/wt");
        fs::create_dir_all(&worktree_gitdir).unwrap();
        fs::write(worktree_gitdir.join("commondir"), "../..\n").unwrap();

        let wt = tmp.path().join("wt");
        fs::create_dir(&wt).unwrap();
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", worktree_gitdir.display()),
        )
        .unwrap();

        ensure_in_git_exclude(&wt, ".devprune.json").unwrap();

        let exclude = fs::read_to_string(shared.join("info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == ".devprune.json"));
    }

    #[test]
    fn exclude_is_a_no_op_outside_a_git_repository() {
        let tmp = TempDir::new().unwrap();

        ensure_in_git_exclude(tmp.path(), ".devprune.json").unwrap();

        assert!(!tmp.path().join(".git").exists());
        assert!(!tmp.path().join(".gitignore").exists());
    }
}
