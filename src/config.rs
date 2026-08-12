// Configuration and registry management for dev-prune.
//
// This module handles persistent storage of:
// - Global settings (idle threshold, check interval, daemon toggle)
// - Registered repository paths and their metadata
//
// All data is stored in `~/.config/dev-prune/registry.json`.

use std::collections::HashMap;
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
            require_confirmation: constants::DEFAULT_REQUIRE_CONFIRMATION,
            command_timeout_secs: constants::DEFAULT_COMMAND_TIMEOUT_SECS,
            min_size_mb: constants::DEFAULT_MIN_SIZE_MB,
            update_check: constants::DEFAULT_UPDATE_CHECK,
            scan_depth: constants::DEFAULT_SCAN_DEPTH,
            allow_manifest_rewrite: constants::DEFAULT_ALLOW_MANIFEST_REWRITE,
            update_check_interval_days: constants::UPDATE_CHECK_INTERVAL_DAYS,
            update_check_timeout_secs: constants::UPDATE_CHECK_TIMEOUT_SECS,
            auto_hooks_chain: constants::DEFAULT_AUTO_HOOKS_CHAIN,
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
}

impl RepoEntry {
    /// Creates a new `RepoEntry` with the current timestamp.
    pub fn new() -> Self {
        Self {
            added_at: Utc::now(),
            last_pruned_at: None,
            override_idle_days: None,
            enabled: true,
        }
    }
}

impl Default for RepoEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to ensure an entry (e.g. ".devprune.json") is present in the repository's `.gitignore`.
/// If `.gitignore` doesn't exist, it creates it and adds the entry.
pub fn ensure_in_gitignore(repo_path: &Path, entry: &str) -> Result<()> {
    let gitignore_path = repo_path.join(".gitignore");
    if gitignore_path.exists() {
        let content = fs::read_to_string(&gitignore_path)?;
        if !content.lines().any(|line| line.trim() == entry) {
            let mut file = fs::OpenOptions::new().append(true).open(&gitignore_path)?;
            let prefix = if content.ends_with('\n') || content.is_empty() {
                ""
            } else {
                "\n"
            };
            writeln!(file, "{prefix}{entry}")?;
        }
    } else {
        fs::write(&gitignore_path, format!("{entry}\n"))?;
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
// its `.devprune.json` despite the `.gitignore` entry [`PerRepoConfig::save`] writes. A
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

    /// Save per-repo config to `.devprune.json` in the repo root and auto-update `.gitignore`.
    pub fn save_to_repo(&self, repo_path: &Path) -> Result<()> {
        let config_file = repo_path.join(constants::PER_REPO_CONFIG_FILE);
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&config_file, content)?;
        let _ = ensure_in_gitignore(repo_path, constants::PER_REPO_CONFIG_FILE);
        let _ = ensure_in_gitignore(repo_path, constants::DEVPRUNE_IGNORE_FILE);
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
    /// Total count of successful prune operations executed historically.
    #[serde(default)]
    pub total_pruned_count: u64,
    /// List of repository paths added in the most recent init/link action (for devp undo).
    #[serde(default)]
    pub last_added_repos: Vec<PathBuf>,
    /// What the most recent prune pass deleted (for `devp restore --last-run`).
    #[serde(default)]
    pub last_prune: Option<LastPrune>,
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
        let tmp_path = path.with_extension("json.tmp");
        let contents =
            serde_json::to_string_pretty(self).context("Failed to serialize registry")?;
        fs::write(&tmp_path, &contents)
            .with_context(|| format!("Failed to write temp registry {}", tmp_path.display()))?;
        fs::rename(&tmp_path, path)
            .with_context(|| format!("Failed to rename temp registry to {}", path.display()))?;
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
    pub fn remove_repo(&mut self, path: &Path) -> bool {
        self.repositories.remove(&canonical_key(path)).is_some()
    }

    // Removed: `repo_paths` and `effective_idle_days`.
    //
    // Neither had a caller outside this file's own tests. `effective_idle_days` had also
    // drifted from the rule the engine actually applies: it looked the repository up by
    // the path as given, where every write to `repositories` goes through
    // `canonical_key`, so `devp`'s own relative paths would have missed the entry and
    // silently returned the global threshold instead of the repository's override.

    /// Marks a repo as pruned with the current timestamp and updates cumulative historical metrics.
    pub fn mark_pruned(&mut self, path: &Path, bytes_freed: u64) {
        if let Some(entry) = self.repositories.get_mut(path) {
            entry.last_pruned_at = Some(Utc::now());
        }
        self.total_freed_bytes += bytes_freed;
        self.total_pruned_count += 1;
    }

    /// Record what a prune pass deleted, replacing any earlier record.
    ///
    /// A pass that deleted nothing is not a pass worth remembering, so an empty list is
    /// ignored rather than stored — otherwise `devp run` on an already-clean machine
    /// would quietly throw away the record of the run the user actually wants back.
    pub fn record_prune(&mut self, dirs: Vec<PrunedDir>) {
        if dirs.is_empty() {
            return;
        }
        self.last_prune = Some(LastPrune {
            at: Utc::now(),
            dirs,
        });
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
        assert_eq!(registry.total_pruned_count, 1);
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

        let tmp_path = path.with_extension("json.tmp");
        assert!(!tmp_path.exists());
        assert!(path.exists());
    }
}
