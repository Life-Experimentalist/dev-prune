// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Configuration and registry management for dev-prune.
//
// This module handles persistent storage of:
// - Global settings (idle threshold, check interval, daemon toggle)
// - Registered repository paths and their metadata
//
// All data is stored in `~/.config/dev-prune/registry.json`.

use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// Whether the scheduled pass looks for unregistered repositories by itself.
    /// On by default; see [`constants::DEFAULT_AUTO_DISCOVER`].
    #[serde(default = "default_auto_discover")]
    pub auto_discover: bool,
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
    /// Whether the opt-in Cargo adapter is active. Off by default, and the reason is
    /// the same one that keeps `enable_gradle` off: Rust's `target/` is compiler
    /// output. `cargo metadata --locked` proves the *crates* come back from
    /// `Cargo.lock`, but nothing downloads a compiled artefact — the directory returns
    /// only by rebuilding, which on a large workspace is minutes rather than the
    /// seconds a dependency reinstall costs. See [`crate::adapters::cargo_adapter`].
    #[serde(default)]
    pub enable_cargo: bool,
    /// Whether the opt-in Gradle build-tool adapter is active. Off by default:
    /// `build/` comes back by recompiling the project, so nobody should find it
    /// deleted without having asked. See [`crate::adapters::gradle`].
    #[serde(default)]
    pub enable_gradle: bool,
    /// Whether the opt-in Maven build-tool adapter is active. Off by default, for the
    /// same reason as `enable_gradle`. See [`crate::adapters::maven`].
    #[serde(default)]
    pub enable_maven: bool,
    /// Whether the opt-in Swift Package Manager adapter is active. Off by default, for
    /// the same reason as `enable_gradle`: `.build/` holds compiled modules and comes
    /// back through `swift build`. See [`crate::adapters::swift`].
    #[serde(default)]
    pub enable_swift: bool,
    /// Whether the opt-in Dart and Flutter adapter is active. Off by default: the pub
    /// metadata in `.dart_tool/` is a second's work to restore, but the `build_runner`
    /// and `flutter_build` caches beside it are compiler output and come back only by
    /// recompiling. See [`crate::adapters::dart`].
    #[serde(default)]
    pub enable_dart: bool,
    /// Whether the opt-in Mix build-tree adapter is active. Off by default, and separate
    /// from the always-on `mix` adapter: that one deletes `deps/`, which comes back by
    /// downloading, while `_build/` comes back only by recompiling the project and every
    /// dependency in it. See [`crate::adapters::mix_build`].
    #[serde(default)]
    pub enable_mix_build: bool,
    /// Whether the opt-in vcpkg adapter is active. Off by default: vcpkg builds every
    /// port from source, so `vcpkg_installed/` comes back by compiling Boost or Qt
    /// again rather than by downloading them. See [`crate::adapters::vcpkg`].
    #[serde(default)]
    pub enable_vcpkg: bool,

    /// Whether the opt-in CMake build-tree adapter is active. Off by default: a build
    /// tree is object files and linked binaries, and it comes back by compiling the
    /// project again. See [`crate::adapters::cmake_build`].
    #[serde(default)]
    pub enable_cmake_build: bool,
    /// Idle days required before *build-tree* directories — everything the opt-in
    /// adapters claim — are pruned.
    ///
    /// Separate from `idle_days` because the cost of being wrong is different: a
    /// deleted `node_modules` is one `npm ci` away, a deleted Android `build/` is a
    /// long recompile. Applied as `max(build_idle_days, idle_days)`.
    #[serde(default = "default_build_idle_days")]
    pub build_idle_days: u64,
    /// Whether a newer release installs itself at the end of a prune pass, once the
    /// periodic check has found one.
    ///
    /// On by default. A pruner that runs on a schedule is exactly the kind of tool
    /// nobody thinks to upgrade, and an old one keeps whatever bug it shipped with
    /// forever. What runs here is the download-and-replace half only: see
    /// [`crate::commands::update::maybe_auto_update`], which never hands the machine to
    /// a package manager unattended and stands aside entirely on WinGet, Scoop and
    /// Homebrew, where the manager owns the upgrade.
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    /// Whether this copy stays on the version it is, whatever else is configured.
    ///
    /// Off by default, and turned on only by a person typing
    /// `devp config set version_lock true`. While it is on, `auto_update` does not run
    /// however it is set, `devp update --install` refuses, `devp install --channel`
    /// refuses because moving channels installs the latest release, and the install
    /// scripts leave the binary exactly where they find it. There is no flag that
    /// bypasses it: releasing the pin is the same kind of decision as setting it, and
    /// belongs to the same person.
    ///
    /// It exists because `auto_update = false` was never the whole answer. That setting
    /// stops one path; a machine that has to keep shipping the same tool for a year --
    /// a CI image, a reproduction that stops reproducing the moment the tool changes
    /// underneath it, a locked-down build box -- also has to survive someone re-running
    /// the install one-liner out of habit.
    #[serde(default)]
    pub version_lock: bool,
    /// Adapters switched off by name, whatever their lockfiles say.
    ///
    /// A deny-list rather than twenty `enable_*` booleans, because the answer for
    /// almost everyone is "none of them" and a list of exceptions says that in one
    /// place. It is a *preference*, and the opposite of `enable_gradle` and friends:
    /// those are off until asked for because deleting a build tree is expensive to
    /// undo, whereas `node_modules` is safe to prune and merely something a particular
    /// person may not want touched.
    ///
    /// Names are the adapter names `--only`/`--skip` take. Applied in
    /// [`crate::adapters::detect_adapters`], so a disabled adapter is invisible to
    /// every command at once rather than listed by `status` and skipped by `run`.
    #[serde(default)]
    pub disabled_adapters: Vec<String>,
    /// Per-adapter idle windows, in days, keyed by adapter name.
    ///
    /// The one dial that is neither global nor per-repository: "wait longer before
    /// touching Rust" is a statement about a *toolchain*, not about one checkout, and
    /// before this it could only be said by moving the global window for everything.
    ///
    /// **A floor, never a bypass.** The value is applied as
    /// `max(idle_days, adapter_idle_days[name])`, so it can only make an adapter wait
    /// longer than the repository-level check already requires. A smaller number is
    /// accepted and simply has no effect — the repository gate runs first and is the
    /// same gate for every adapter, and letting one adapter lower it would be a
    /// bypass of the idle check rather than a preference.
    ///
    /// `BTreeMap` rather than `HashMap` so the JSON round-trips in a stable order and
    /// a diff of the registry file shows what actually changed.
    #[serde(default)]
    pub adapter_idle_days: BTreeMap<String, u64>,
    /// Per-manager cache size caps, in gibibytes, keyed by cache manager name.
    ///
    /// A download cache is a bet that re-downloading costs more than the disk it
    /// occupies, and the bet stops paying somewhere: a `uv` cache past ten gigabytes is
    /// keeping wheels for Python versions the machine no longer has, and no repository's
    /// lockfile will ever say so. This is where that ceiling is written down.
    ///
    /// **It never deletes anything on its own.** `devp caches` marks a cache over its
    /// cap and `devp caches clear --over-cap` empties exactly those; nothing dev-prune
    /// runs on a schedule touches a cache, which is a promise `devp caches` prints in
    /// so many words and a size cap is not a reason to break.
    ///
    /// Keyed by the names `devp caches clear <MANAGER>` takes, not by adapter name.
    /// They mostly agree — `npm`, `uv`, `cargo`, `go` — but `pip`, `nuget`, `conan`,
    /// `conda`, `vcpkg` and `hex` are caches with no adapter, and `venv`, `terraform`
    /// and `dart` are adapters with no cache. Empty by default: no cache is too big
    /// until someone says what too big is.
    #[serde(default)]
    pub cache_max_gb: BTreeMap<String, u64>,
    /// Language for dev-prune's own headings and summary lines.
    ///
    /// English by default, and English wherever a translation has not reached a string
    /// yet -- see [`crate::i18n`] for what is translated and, more importantly, what is
    /// not: `--json`, exit codes, flag names, config keys and the sentences a refusal
    /// prints stay in English in every language, because they are a contract or a
    /// diagnosis rather than prose.
    ///
    /// `DEV_PRUNE_LANG` overrides this for one invocation. An unrecognised code falls
    /// back to English rather than failing.
    #[serde(default = "default_language")]
    pub language: String,
    /// Settings this build has never heard of, carried through a save verbatim.
    ///
    /// A registry written by a newer dev-prune can hold keys this build does not
    /// know, and every save rewrites the whole `settings` object — so without this,
    /// one run of an older binary (a pinned CI image, a machine `version_lock` holds
    /// back) silently erased the newer binary's configuration. `BTreeMap` for the
    /// same stable-diff reason as `adapter_idle_days`.
    #[serde(flatten, default)]
    pub unknown_keys: BTreeMap<String, serde_json::Value>,
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

fn default_auto_discover() -> bool {
    constants::DEFAULT_AUTO_DISCOVER
}

fn default_update_check() -> bool {
    constants::DEFAULT_UPDATE_CHECK
}

fn default_auto_update() -> bool {
    constants::DEFAULT_AUTO_UPDATE
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

fn default_language() -> String {
    constants::DEFAULT_LANGUAGE.to_string()
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
            auto_discover: constants::DEFAULT_AUTO_DISCOVER,
            require_confirmation: constants::DEFAULT_REQUIRE_CONFIRMATION,
            command_timeout_secs: constants::DEFAULT_COMMAND_TIMEOUT_SECS,
            min_size_mb: constants::DEFAULT_MIN_SIZE_MB,
            update_check: constants::DEFAULT_UPDATE_CHECK,
            scan_depth: constants::DEFAULT_SCAN_DEPTH,
            allow_manifest_rewrite: constants::DEFAULT_ALLOW_MANIFEST_REWRITE,
            update_check_interval_days: constants::UPDATE_CHECK_INTERVAL_DAYS,
            update_check_timeout_secs: constants::UPDATE_CHECK_TIMEOUT_SECS,
            auto_hooks_chain: constants::DEFAULT_AUTO_HOOKS_CHAIN,
            enable_cargo: false,
            enable_gradle: false,
            enable_maven: false,
            enable_swift: false,
            enable_dart: false,
            enable_mix_build: false,
            enable_vcpkg: false,
            enable_cmake_build: false,
            build_idle_days: constants::DEFAULT_BUILD_IDLE_DAYS,
            auto_update: constants::DEFAULT_AUTO_UPDATE,
            version_lock: constants::DEFAULT_VERSION_LOCK,
            disabled_adapters: Vec::new(),
            adapter_idle_days: BTreeMap::new(),
            cache_max_gb: BTreeMap::new(),
            language: constants::DEFAULT_LANGUAGE.to_string(),
            unknown_keys: BTreeMap::new(),
        }
    }
}

/// Outcome of recording a repository's identity when it was registered.
///
/// Reported rather than silent: a registration that quietly absorbed another entry's
/// prune history would be indistinguishable from one that lost it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// No dead entry claimed this identity.
    Nothing,
    /// This registration took over the history of a path that no longer exists.
    Moved(PathBuf),
    /// More than one dead entry claims the identity, so none was chosen.
    Ambiguous,
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
    /// The repository's root commit, recorded when it was registered.
    ///
    /// A repository that is moved keeps this; its path does not. Without it a moved
    /// workspace registers as a brand new repository and its prune history is stranded
    /// on a path that will never exist again. Registries written before 1.4.0 have none,
    /// and re-registering the repository is what fills it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
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
            identity: None,
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
    /// What this project declares prunable beyond what an adapter can recognise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prunable: Option<Prunable>,
}

/// The nested half of a repository's config: what this project says is rebuildable.
///
/// A section rather than a top-level key, because the keys above it are the whole of
/// what a repository could say in 1.0.0 and the list of things it might want to say is
/// not finished. Everything that arrives later and describes *what to delete* belongs
/// under this heading with `directories`, so the file grows a section at a time instead
/// of a scatter of top-level names nobody can group by eye.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prunable {
    /// Directories dev-prune would never find on its own, each with its way back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directories: Vec<DeclaredDir>,
    /// Declared paths to leave alone on this machine, whoever declared them.
    ///
    /// `project.devprune.json` is committed, so one person's `scratch` is everybody's
    /// `scratch`, and the teammate whose copy is holding something had no way to say so
    /// short of editing a file the whole team shares. Same spelling as a `path` in
    /// `directories`; the entry it names is skipped entirely.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

/// One directory a project declares prunable, and the command that puts it back.
///
/// Every adapter in this tool earns the right to delete a directory by finding a
/// lockfile that can rebuild it. A declaration is the same bargain made by hand: the
/// project states the directory, and states what rebuilds it, and dev-prune checks that
/// the stated command is one this machine could actually run before it deletes anything.
///
/// `rebuild` is required, and required is the point. An optional one would have made
/// "delete this, I have no idea how to get it back" the path of least resistance in a
/// file that gets committed and cloned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeclaredDir {
    /// Repository-relative, `/`-separated. Never absolute, never `..`, never `.git`.
    pub path: String,
    /// The command that rebuilds it. Shown, never run — see [`crate::declared`].
    pub rebuild: String,
    /// Why this is safe to lose, in the project's own words. Printed beside the path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
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
            prunable: None,
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
        Ok(RepoConfigLayers::load(repo_path)?.effective())
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

    /// Which of a repository's config files exist and do not parse, and why.
    ///
    /// [`load_with_diagnostics`](Self::load_with_diagnostics) collapses both into one
    /// refusal, which is the right answer for every reader: a config that cannot be read
    /// is a repository dev-prune will not touch, whichever file it was in. `devp doctor`
    /// is the one caller that has to know which, because it repairs the personal file by
    /// renaming it aside and must never do that to a file the user has committed.
    pub fn broken_files(repo_path: &Path) -> Vec<(&'static str, String)> {
        [
            constants::PROJECT_REPO_CONFIG_FILE,
            constants::PER_REPO_CONFIG_FILE,
        ]
        .into_iter()
        .filter_map(|name| match read_layer(&repo_path.join(name)) {
            Err(e) => Some((name, e)),
            Ok(_) => None,
        })
        .collect()
    }

    /// Keys a repository config file spells out that dev-prune does not read.
    ///
    /// Unknown keys are tolerated on purpose — a file written by a newer dev-prune must
    /// not stop an older one from reading the keys it does know — so
    /// `deny_unknown_fields` is the one fix this must never become. The cost of that
    /// tolerance is that a typo (`idle_days` for `override_idle_days`) silently does
    /// nothing, and nothing on this machine ever tells its author why. This is the
    /// diagnostic half: `devp doctor` names each stray key, and behaviour changes
    /// nowhere.
    pub fn unknown_keys(repo_path: &Path) -> Vec<(&'static str, String)> {
        const KNOWN: &[&str] = &[
            "$schema",
            "project_name",
            "ignore",
            "disable_hooks",
            "disable_daemon",
            "override_idle_days",
            "min_size_mb",
            "scan_depth",
            "prunable",
        ];
        const KNOWN_PRUNABLE: &[&str] = &["directories", "exclude"];
        const KNOWN_DIRECTORY: &[&str] = &["path", "rebuild", "why"];

        let mut out = Vec::new();
        for name in [
            constants::PROJECT_REPO_CONFIG_FILE,
            constants::PER_REPO_CONFIG_FILE,
        ] {
            let Ok(content) = fs::read_to_string(repo_path.join(name)) else {
                continue;
            };
            let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&content) else {
                continue;
            };
            for key in map.keys().filter(|k| !KNOWN.contains(&k.as_str())) {
                out.push((name, key.clone()));
            }
            let Some(serde_json::Value::Object(prunable)) = map.get("prunable") else {
                continue;
            };
            for key in prunable
                .keys()
                .filter(|k| !KNOWN_PRUNABLE.contains(&k.as_str()))
            {
                out.push((name, format!("prunable.{key}")));
            }
            if let Some(serde_json::Value::Array(dirs)) = prunable.get("directories") {
                for entry in dirs.iter().filter_map(|d| d.as_object()) {
                    for key in entry
                        .keys()
                        .filter(|k| !KNOWN_DIRECTORY.contains(&k.as_str()))
                    {
                        out.push((name, format!("prunable.directories[].{key}")));
                    }
                }
            }
        }
        out
    }

    /// The personal `.devprune.json` alone, for a caller about to write it back.
    ///
    /// [`load_with_diagnostics`](Self::load_with_diagnostics) answers "what is in force
    /// here", which is the merge of both files and the right answer for everything that
    /// reads. It is the wrong answer for anything that writes: saving it copies the
    /// project file's values into the personal one, and the next edit to the project
    /// file leaves that copy behind, silently overriding the file it was copied from.
    pub fn load_personal_for_write(repo_path: &Path) -> Result<Option<Self>, String> {
        Ok(RepoConfigLayers::load(repo_path)?
            .personal_config()
            .cloned())
    }
}

/// Write a starter `project.devprune.json`: a schema link, and the empty section.
///
/// Deliberately not a serialized [`PerRepoConfig::default`]. Every scalar key the
/// project file names is a key it wins, so writing all of them out would have `--team`
/// quietly take over every setting in the `.devprune.json` beside it — including the
/// ones that file was created to hold. An empty team file decides nothing until the team
/// decides something, and the `$schema` link is what makes deciding it a matter of
/// autocomplete rather than of remembering the key names.
///
/// The one thing written out is the empty `prunable.directories`, which decides nothing
/// either — an empty list adds no directories. It is there because a section nobody can
/// see is a section nobody fills in, and this is the file a person or an agent is
/// expected to fill in.
///
/// No `ensure_in_git_exclude`, and that omission is the entire point. [`PerRepoConfig::
/// save_to_repo`] hides what it writes because one person's overrides are nobody else's
/// business; hiding this one would leave it identical to the file beside it and useful
/// to nobody.
pub fn write_project_starter(repo_path: &Path) -> Result<()> {
    let file = repo_path.join(constants::PROJECT_REPO_CONFIG_FILE);
    let starter = serde_json::json!({
        "$schema": default_schema_url(),
        "prunable": { "directories": [] },
    });
    fs::write(&file, serde_json::to_string_pretty(&starter)?)?;
    Ok(())
}

/// Which of a repository's two config files an effective value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// Spelled out in the committed `project.devprune.json`.
    Project,
    /// Spelled out in the git-excluded `.devprune.json`.
    Personal,
    /// In neither file, so whatever the global setting or the built-in default says.
    Default,
}

impl ConfigSource {
    /// The file this answer came from, or where to look when it came from no file.
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => constants::PROJECT_REPO_CONFIG_FILE,
            Self::Personal => constants::PER_REPO_CONFIG_FILE,
            Self::Default => "global setting",
        }
    }
}

/// A repository's configuration as the two files that can contribute to it.
///
/// The project file wins every scalar key it names, and the personal file answers the
/// rest. That is the inverse of the usual local-overrides-committed convention, and
/// deliberately so: the settings here are the ones a *project* decides, and a team that
/// has written down "this repository is not worth pruning" wants that to survive a
/// teammate's stale personal file rather than lose to it.
///
/// "Names a key" means the key is literally in the file. A project file silent on
/// `ignore` does not overrule the personal one with serde's `false`, because a default
/// filled in by the deserializer is not something anybody wrote down.
///
/// `prunable.directories` is the one thing that unions instead of winning. It is a list
/// of separate declarations rather than a single decided value, so there is nothing for
/// one file to win: "the team says this cache is rebuildable" and "so is this one on my
/// machine" are both true at once, and a rule that let the committed file silence the
/// personal list would delete somebody's own declaration the day their team wrote their
/// first one. `prunable.exclude` unions for the opposite reason: a veto only ever
/// deletes less, so it is safe to honour from whichever file wrote it.
///
/// Nothing here widens what a repository can ask for. Both files deserialize into the
/// same [`PerRepoConfig`], so the two settings the type deliberately does not carry —
/// `allow_manifest_rewrite` and `post_prune_command` — are still absent from both, and
/// every field that is present is either display-only or scope-shaping. A committed
/// `.devprune.json` has had exactly this reach since 1.0.0, since the `.git/info/exclude`
/// entry is local to one clone and excludes nothing already tracked; the shared file
/// makes that reach a named, documented file instead of an accident.
pub struct RepoConfigLayers {
    /// The committed file and the keys it actually spells out.
    project: Option<(PerRepoConfig, HashSet<String>)>,
    /// The git-excluded file and the keys it actually spells out.
    personal: Option<(PerRepoConfig, HashSet<String>)>,
}

impl RepoConfigLayers {
    /// Read both files. `Err` if either exists and does not parse.
    pub fn load(repo_path: &Path) -> Result<Self, String> {
        Ok(Self {
            project: read_layer(&repo_path.join(constants::PROJECT_REPO_CONFIG_FILE))?,
            personal: read_layer(&repo_path.join(constants::PER_REPO_CONFIG_FILE))?,
        })
    }

    /// The merged configuration, or `None` when the repository has neither file.
    ///
    /// `None` rather than the defaults, because every caller of this treats "no config"
    /// and "a config that happens to match the defaults" as the same thing to act on but
    /// not the same thing to report.
    pub fn effective(&self) -> Option<PerRepoConfig> {
        if self.project.is_none() && self.personal.is_none() {
            return None;
        }
        let base = self
            .personal
            .as_ref()
            .map(|(c, _)| c.clone())
            .unwrap_or_default();
        let Some((project, keys)) = &self.project else {
            return Some(base);
        };
        let said = |k: &str| keys.contains(k);
        let declared = merge_declarations(project.prunable.as_ref(), base.prunable);
        Some(PerRepoConfig {
            // Never taken from the project file. `$schema` points at a validator, and the
            // one this clone should resolve is the one this machine has —
            // `default_schema_url` prefers a local copy when there is one, which a
            // teammate's committed absolute path would override with a file that does
            // not exist here.
            schema: base.schema,
            project_name: pick(
                said("project_name"),
                &project.project_name,
                base.project_name,
            ),
            ignore: pick(said("ignore"), &project.ignore, base.ignore),
            disable_hooks: pick(
                said("disable_hooks"),
                &project.disable_hooks,
                base.disable_hooks,
            ),
            disable_daemon: pick(
                said("disable_daemon"),
                &project.disable_daemon,
                base.disable_daemon,
            ),
            override_idle_days: pick(
                said("override_idle_days"),
                &project.override_idle_days,
                base.override_idle_days,
            ),
            min_size_mb: pick(said("min_size_mb"), &project.min_size_mb, base.min_size_mb),
            scan_depth: pick(said("scan_depth"), &project.scan_depth, base.scan_depth),
            prunable: declared,
        })
    }

    /// The committed file as it stands, before the personal one fills any gaps in.
    pub fn project_config(&self) -> Option<&PerRepoConfig> {
        self.project.as_ref().map(|(c, _)| c)
    }

    /// The personal file as it stands, before the project one overrules any of it.
    pub fn personal_config(&self) -> Option<&PerRepoConfig> {
        self.personal.as_ref().map(|(c, _)| c)
    }

    /// Which file each setting's effective value came from.
    ///
    /// This is what gets shown instead of copying the project file's values into
    /// `.devprune.json` as a visible "mirror". A second copy of a value is a second copy
    /// free to drift from the first, and the question somebody actually has in front of
    /// two config files is not "what does each say" but "which one won".
    pub fn rows(&self) -> Vec<(&'static str, String, ConfigSource)> {
        let cfg = self.effective().unwrap_or_default();
        vec![
            (
                "project_name",
                opt(&cfg.project_name),
                self.source_of("project_name"),
            ),
            ("ignore", cfg.ignore.to_string(), self.source_of("ignore")),
            (
                "disable_hooks",
                cfg.disable_hooks.to_string(),
                self.source_of("disable_hooks"),
            ),
            (
                "disable_daemon",
                cfg.disable_daemon.to_string(),
                self.source_of("disable_daemon"),
            ),
            (
                "override_idle_days",
                opt(&cfg.override_idle_days),
                self.source_of("override_idle_days"),
            ),
            (
                "min_size_mb",
                opt(&cfg.min_size_mb),
                self.source_of("min_size_mb"),
            ),
            (
                "scan_depth",
                opt(&cfg.scan_depth),
                self.source_of("scan_depth"),
            ),
        ]
    }

    /// Which file spelled this key out, in precedence order.
    pub fn source_of(&self, key: &str) -> ConfigSource {
        if self.project.as_ref().is_some_and(|(_, k)| k.contains(key)) {
            ConfigSource::Project
        } else if self.personal.as_ref().is_some_and(|(_, k)| k.contains(key)) {
            ConfigSource::Personal
        } else {
            ConfigSource::Default
        }
    }
}

/// `project` when the project file named this key, `personal` otherwise.
fn pick<T: Clone>(project_said_so: bool, project: &T, personal: T) -> T {
    if project_said_so {
        project.clone()
    } else {
        personal
    }
}

/// Both files' declarations, the committed ones first, one entry per path.
///
/// Deduplicated by path rather than by whole entry: two files naming the same directory
/// with two different `rebuild` commands is one directory, and the committed one is the
/// answer — a teammate whose personal file still names last year's build script should
/// get the project's current one, not a second delete of the same path.
///
/// `exclude` unions the same way and from either file. It can only ever take a directory
/// out of play, so there is nothing for the committed file to protect by winning it —
/// and the person who needs one is by definition the person that file is wrong for.
fn merge_declarations(project: Option<&Prunable>, personal: Option<Prunable>) -> Option<Prunable> {
    let mut directories: Vec<DeclaredDir> =
        project.map(|p| p.directories.clone()).unwrap_or_default();
    let mut exclude: Vec<String> = project.map(|p| p.exclude.clone()).unwrap_or_default();
    let personal = personal.unwrap_or_default();
    for dir in personal.directories {
        if !directories.iter().any(|d| d.path == dir.path) {
            directories.push(dir);
        }
    }
    for path in personal.exclude {
        if !exclude.contains(&path) {
            exclude.push(path);
        }
    }
    if directories.is_empty() && exclude.is_empty() {
        None
    } else {
        Some(Prunable {
            directories,
            exclude,
        })
    }
}

/// How an unset optional reads in the provenance table.
fn opt<T: std::fmt::Display>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map_or_else(|| "not set".to_string(), ToString::to_string)
}

/// Parse one config file into its values and the set of keys it actually spells out.
fn read_layer(path: &Path) -> Result<Option<(PerRepoConfig, HashSet<String>)>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {e}"))?;
    // `clean_path`, like every other path this tool shows. `Display` on a canonicalised
    // Windows path leaks the `\\?\` extended-length prefix into an error message the user
    // is being asked to act on.
    let cfg = serde_json::from_str::<PerRepoConfig>(&content)
        .map_err(|e| format!("Syntax error in `{}`: {e}", crate::output::clean_path(path)))?;
    // The same text just deserialized into a struct, so it is a JSON object and this
    // cannot fail; it is parsed a second time only because serde has by then thrown away
    // the difference between a key the file set and a key it defaulted.
    let keys = serde_json::from_str::<HashMap<String, serde_json::Value>>(&content)
        .map(|m| m.into_keys().collect())
        .unwrap_or_default();
    Ok(Some((cfg, keys)))
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
    /// The language runtime the deleted directory was built against — `"3.12"` for a
    /// virtual environment created by Python 3.12 — so a restore can rebuild on that
    /// interpreter instead of on whatever happens to be first on `PATH` today.
    ///
    /// `None` for every manager that pins its own toolchain in the lockfile (cargo, npm,
    /// go) and for anything pruned before 1.4.0. Optional rather than required for that
    /// second reason: a `registry.json` written by an older version has to keep loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
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
    /// Total bytes given back by `devp caches clear`, ever, on this machine.
    ///
    /// Kept apart from `total_freed_bytes` rather than folded into it, because the two
    /// cost different things to undo. A prune deletes what a lockfile proves it can
    /// rebuild, and getting it back is one reinstall in one repository; emptying a shared
    /// cache costs a download in every project on the disk. Not keyed by repository for
    /// the same reason: a package manager's cache belongs to none of them.
    ///
    /// Recorded from 1.9.0 onward, so a registry written before then deserializes to
    /// zero and starts counting from the next clear.
    #[serde(default)]
    pub total_cache_freed_bytes: u64,
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
    /// How fast each adapter has actually restored on this machine.
    ///
    /// Measured by `devp restore --last-run`, which is the one command that knows both
    /// how long a restore took and how many bytes it put back. Local only: nothing here
    /// is uploaded, compared against anyone else's machine, or used for anything except
    /// the estimate `devp status` prints. See `docs/PRIVACY.md`.
    #[serde(default)]
    pub restore_rates: BTreeMap<String, RestoreRate>,
}

/// One adapter's observed restore throughput on this machine.
///
/// Totals rather than a stored average, because that is what lets a new measurement be
/// folded in without keeping the individual samples — and the individual samples are
/// per-repository, which is exactly the shape of data this tool has no business keeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RestoreRate {
    /// How many restores this average is made of.
    pub samples: u32,
    /// Bytes those restores put back.
    pub bytes: u64,
    /// Milliseconds they took.
    pub millis: u64,
}

impl RestoreRate {
    /// Bytes per second, or `None` when the record cannot support the division.
    pub fn bytes_per_sec(&self) -> Option<f64> {
        (self.samples > 0 && self.millis > 0 && self.bytes > 0)
            .then(|| self.bytes as f64 * 1000.0 / self.millis as f64)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            settings: Settings::default(),
            repositories: HashMap::new(),
            total_freed_bytes: 0,
            total_cache_freed_bytes: 0,
            total_pruned_count: 0,
            last_added_repos: Vec::new(),
            last_prune: None,
            prune_history: Vec::new(),
            last_update_check: None,
            latest_known_version: None,
            restore_rates: BTreeMap::new(),
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

    /// Record `identity` against a registered repository, and hand it the history of the
    /// entry it moved away from.
    ///
    /// Called after `add_repo` from both `link` and `init`. When exactly one registered
    /// path no longer exists on disk and carries the same root commit, that entry is the
    /// same repository at its old location: its `added_at`, prune history and settings
    /// move across and the dead row is removed. Two dead entries claiming one identity
    /// is a clone, not a move, so nothing is guessed — the caller says so instead.
    ///
    /// Also the backfill path. Entries registered before 1.4.0 have no identity, so
    /// nothing they do can be recognised as a move; re-registering them records one, and
    /// a single `devp init ~/code` backfills the whole registry.
    pub fn adopt_moved_entry(&mut self, path: &Path, identity: Option<String>) -> Adoption {
        let key = canonical_key(path);
        let Some(identity) = identity else {
            return Adoption::Nothing;
        };

        let mut claimants: Vec<PathBuf> = self
            .repositories
            .iter()
            .filter(|(p, e)| {
                **p != key && e.identity.as_deref() == Some(identity.as_str()) && !p.exists()
            })
            .map(|(p, _)| p.clone())
            .collect();
        // Deterministic: two dead entries with one identity is a report, not a coin toss,
        // and the report must read the same twice.
        claimants.sort();

        let adopted = match claimants.len() {
            0 => Adoption::Nothing,
            1 => Adoption::Moved(claimants.remove(0)),
            _ => Adoption::Ambiguous,
        };

        if let Adoption::Moved(ref old) = adopted
            && let Some(previous) = self.repositories.remove(old)
        {
            if let Some(entry) = self.repositories.get_mut(&key) {
                // Everything the old path had earned. `enabled` and the idle override
                // come across too: a repository the user had switched off did not switch
                // itself back on by being moved.
                entry.added_at = previous.added_at;
                entry.last_pruned_at = previous.last_pruned_at;
                entry.override_idle_days = previous.override_idle_days;
                entry.enabled = previous.enabled;
                entry.total_freed_bytes = previous.total_freed_bytes;
            }
            self.last_added_repos.retain(|p| p != old);
        }

        if let Some(entry) = self.repositories.get_mut(&key) {
            entry.identity = Some(identity);
        }
        adopted
    }

    /// Whether a registered repository still has no recorded identity.
    ///
    /// The global Git hook runs `devp link --quiet` on every commit, and backfilling
    /// unconditionally would shell out to git and rewrite the registry once per commit
    /// forever. This makes it once per repository.
    pub fn needs_identity(&self, path: &Path) -> bool {
        self.repositories
            .get(&canonical_key(path))
            .is_some_and(|e| e.identity.is_none())
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

    /// Credit `bytes` to the machine's running cache-clear total.
    pub fn record_cache_clear(&mut self, bytes: u64) {
        self.total_cache_freed_bytes += bytes;
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
    /// Fold one measured restore into an adapter's running average.
    ///
    /// Ignores anything too quick to have been real work — see
    /// [`constants::RESTORE_RATE_MIN_MILLIS`] — because a manager that found everything
    /// still in its cache returns in a moment and would teach a throughput no cold
    /// restore can reach. That is the difference between an estimate that is optimistic
    /// and one that is wrong.
    pub fn record_restore(&mut self, adapter: &str, bytes: u64, millis: u64) {
        if bytes == 0 || millis < constants::RESTORE_RATE_MIN_MILLIS {
            return;
        }
        let rate = self.restore_rates.entry(adapter.to_string()).or_default();
        if rate.samples >= constants::RESTORE_RATE_SAMPLE_CAP {
            rate.samples /= 2;
            rate.bytes /= 2;
            rate.millis /= 2;
        }
        rate.samples += 1;
        rate.bytes = rate.bytes.saturating_add(bytes);
        rate.millis = rate.millis.saturating_add(millis);
    }

    /// How long putting back `by_adapter` would take, from what this machine has
    /// measured.
    ///
    /// Returns the seconds and the bytes those seconds account for. Anything from an
    /// adapter that has never been timed here is left out of both, so a caller can say
    /// how much of the estimate is actually covered rather than quietly quoting a
    /// number for half the work. `None` when nothing is covered at all — an estimate
    /// with no measurement behind it is a guess, and this command does not print
    /// guesses.
    pub fn estimate_restore(&self, by_adapter: &[(String, u64)]) -> Option<(f64, u64)> {
        let mut secs = 0.0;
        let mut covered = 0u64;
        for (adapter, bytes) in by_adapter {
            let Some(rate) = self
                .restore_rates
                .get(adapter)
                .and_then(|r| r.bytes_per_sec())
            else {
                continue;
            };
            secs += *bytes as f64 / rate;
            covered = covered.saturating_add(*bytes);
        }
        (covered > 0).then_some((secs, covered))
    }

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
            runtime: None,
        }
    }

    #[test]
    fn cache_clears_accumulate_separately_from_prunes() {
        let dir = TempDir::new().expect("temp dir");
        let path = test_registry_path(&dir);

        let mut registry = Registry::default();
        registry.mark_pruned(Path::new("/repo"), 42);
        registry.record_cache_clear(6_000_000_000);
        registry.record_cache_clear(2_000_000_000);
        registry.save_to(&path).expect("saved");

        let reloaded = Registry::load_from(&path).expect("reloaded");
        assert_eq!(reloaded.total_cache_freed_bytes, 8_000_000_000);
        // The prune total is untouched by either clear. `devp stats` prints them as two
        // lines because emptying a shared cache is not the same promise as pruning one
        // repository, and one combined figure would answer neither question.
        assert_eq!(reloaded.total_freed_bytes, 42);
    }

    #[test]
    fn a_registry_written_before_1_9_0_reads_the_cache_total_as_zero() {
        // The `#[serde(default)]`, exercised. Without it every registry on every machine
        // that upgraded would fail to parse, and `devp stats` would exit 1.
        let dir = TempDir::new().expect("temp dir");
        let path = test_registry_path(&dir);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("config dir");

        // Built by removing the one key 1.8.0 did not write, rather than hand-typed, so
        // this stays a test of the `default` and not of whichever unrelated field is
        // added to `Settings` next.
        let mut older = Registry {
            total_freed_bytes: 99,
            ..Default::default()
        };
        older.record_cache_clear(500);
        let mut document: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&older).expect("serialized"))
                .expect("re-parsed");
        assert!(
            document
                .as_object_mut()
                .expect("an object")
                .remove("total_cache_freed_bytes")
                .is_some(),
            "the field this test is about must be in the document to begin with"
        );
        std::fs::write(&path, document.to_string()).expect("wrote an older registry");

        let registry = Registry::load_from(&path).expect("an older registry still parses");
        assert_eq!(registry.total_cache_freed_bytes, 0);
        assert_eq!(registry.total_freed_bytes, 99);
    }

    #[test]
    fn a_setting_from_a_newer_version_survives_this_version_saving() {
        // The `#[serde(flatten)]` catch-all, exercised. A registry written by a newer
        // dev-prune can hold settings keys this build has never heard of, and every
        // save rewrites the whole `settings` object — so before the catch-all, one
        // run of an older binary (a pinned CI image, a machine `version_lock` holds
        // back) silently erased the newer binary's configuration.
        let dir = TempDir::new().expect("temp dir");
        let path = test_registry_path(&dir);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("config dir");

        let mut document: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Registry::default()).expect("serialized"))
                .expect("re-parsed");
        document["settings"]["from_the_future"] = serde_json::json!({ "answer": 42 });
        std::fs::write(&path, document.to_string()).expect("wrote a newer registry");

        let loaded = Registry::load_from(&path).expect("a newer registry still parses");
        loaded.save_to(&path).expect("saved");

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read back"))
                .expect("parsed");
        assert_eq!(saved["settings"]["from_the_future"]["answer"], 42);
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

    /// A typo'd key must be pointed out and must not refuse the file: the same
    /// tolerance that lets a newer dev-prune's file load in an older one is what makes
    /// the typo silent everywhere else.
    #[test]
    fn a_typo_key_is_reported_but_never_refused() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "ignore": true, "idle_days": 30, "prunable": { "directores": [] } }"#,
        )
        .unwrap();

        let cfg = PerRepoConfig::load_with_diagnostics(repo).unwrap().unwrap();
        assert!(cfg.ignore, "the keys the file spells right still apply");

        let unknown: Vec<String> = PerRepoConfig::unknown_keys(repo)
            .into_iter()
            .map(|(_, k)| k)
            .collect();
        assert_eq!(unknown, vec!["idle_days", "prunable.directores"]);
    }

    /// Drift guard: a field added to [`PerRepoConfig`] without extending the known-key
    /// list would make doctor warn about a key the tool itself wrote.
    #[test]
    fn every_key_the_type_serializes_is_a_known_key() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        let full = PerRepoConfig {
            project_name: Some("x".into()),
            ignore: true,
            disable_hooks: true,
            disable_daemon: true,
            override_idle_days: Some(1),
            min_size_mb: Some(1),
            scan_depth: Some(1),
            prunable: Some(Prunable {
                directories: vec![DeclaredDir {
                    path: "scratch".into(),
                    rebuild: "echo not needed".into(),
                    why: Some("scratch".into()),
                }],
                exclude: vec!["dist".into()],
            }),
            ..PerRepoConfig::default()
        };
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            serde_json::to_string_pretty(&full).unwrap(),
        )
        .unwrap();
        assert_eq!(PerRepoConfig::unknown_keys(repo), Vec::new());
    }

    #[test]
    fn the_project_file_wins_the_keys_it_names_and_no_others() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();

        // A team file that says one thing, and a personal file that says three.
        fs::write(
            repo.join(constants::PROJECT_REPO_CONFIG_FILE),
            r#"{ "ignore": true }"#,
        )
        .unwrap();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "ignore": false, "scan_depth": 12, "project_name": "mine" }"#,
        )
        .unwrap();

        let cfg = PerRepoConfig::load_with_diagnostics(repo).unwrap().unwrap();
        assert!(cfg.ignore, "the committed file decides the key it names");
        assert_eq!(
            cfg.scan_depth,
            Some(12),
            "and decides nothing about the keys it does not"
        );
        assert_eq!(cfg.project_name.as_deref(), Some("mine"));

        let layers = RepoConfigLayers::load(repo).unwrap();
        assert_eq!(layers.source_of("ignore"), ConfigSource::Project);
        assert_eq!(layers.source_of("scan_depth"), ConfigSource::Personal);
        assert_eq!(layers.source_of("min_size_mb"), ConfigSource::Default);
    }

    #[test]
    fn a_serde_default_is_not_a_project_decision() {
        // The whole reason the key set is carried around. Serde fills `ignore` in as
        // `false` for a file that never mentioned it, and a merge that could not tell
        // those apart would have every project file silently un-ignoring repositories
        // its author had said nothing about.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::write(
            repo.join(constants::PROJECT_REPO_CONFIG_FILE),
            r#"{ "scan_depth": 3 }"#,
        )
        .unwrap();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "ignore": true }"#,
        )
        .unwrap();

        let cfg = PerRepoConfig::load_with_diagnostics(repo).unwrap().unwrap();
        assert!(cfg.ignore);
        assert_eq!(cfg.scan_depth, Some(3));
    }

    #[test]
    fn a_broken_project_file_is_named_and_never_healed_in_place() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::write(
            repo.join(constants::PROJECT_REPO_CONFIG_FILE),
            r#"{ "ignore": true, }"#,
        )
        .unwrap();

        // Same refusal as a broken personal file: nothing reads a config it cannot
        // parse, whichever file it was in.
        assert!(
            PerRepoConfig::load_with_diagnostics(repo)
                .unwrap_err()
                .contains("Syntax error")
        );

        // But the repair path has to know which file, because one of them is tracked.
        let broken = PerRepoConfig::broken_files(repo);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].0, constants::PROJECT_REPO_CONFIG_FILE);
    }

    #[test]
    fn a_new_project_file_is_visible_to_git_and_decides_nothing() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::create_dir_all(repo.join(".git").join("info")).unwrap();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "ignore": true, "scan_depth": 9 }"#,
        )
        .unwrap();

        write_project_starter(repo).unwrap();

        // `save_to_repo` hides what it writes; this one must not, or the file is a
        // per-machine file with a misleading name.
        let exclude = repo.join(".git").join("info").join("exclude");
        let listed = fs::read_to_string(&exclude).unwrap_or_default();
        assert!(
            !listed.contains(constants::PROJECT_REPO_CONFIG_FILE),
            "the committed file must never be excluded: {listed}"
        );

        // And creating it must not have quietly taken over the file beside it. A
        // serialized `PerRepoConfig::default()` would name all seven keys and therefore
        // win all seven.
        let layers = RepoConfigLayers::load(repo).unwrap();
        assert_eq!(layers.source_of("ignore"), ConfigSource::Personal);
        let cfg = layers.effective().unwrap();
        assert!(cfg.ignore);
        assert_eq!(cfg.scan_depth, Some(9));

        // The empty section is written so it can be seen and filled in, which means it
        // has to be inert until somebody fills it in.
        assert!(cfg.prunable.is_none(), "an empty list declares nothing");
    }

    #[test]
    fn a_write_back_never_copies_the_project_answer_into_the_personal_file() {
        // The drift this feature would otherwise create: `devp config --update` and the
        // workspace toggles all read-modify-write `.devprune.json`, and a merged read
        // would bake the team's value into one person's file, where it outlives the
        // next edit to the file it came from.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::write(
            repo.join(constants::PROJECT_REPO_CONFIG_FILE),
            r#"{ "ignore": true }"#,
        )
        .unwrap();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "scan_depth": 4 }"#,
        )
        .unwrap();

        let personal = PerRepoConfig::load_personal_for_write(repo)
            .unwrap()
            .unwrap();
        assert!(!personal.ignore, "the project answer must not travel");
        assert_eq!(personal.scan_depth, Some(4));
    }

    #[test]
    fn a_personal_exclusion_vetoes_a_declaration_the_project_committed() {
        // The conflict the key exists for: the committed file says `scratch` is
        // rebuildable, and on this one machine `scratch` is holding something. The way
        // out must not be editing a file the whole team shares.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::write(
            repo.join(constants::PROJECT_REPO_CONFIG_FILE),
            r#"{ "prunable": { "directories": [
                 { "path": "scratch", "rebuild": "make scratch" }
               ] } }"#,
        )
        .unwrap();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "prunable": { "exclude": ["scratch"] } }"#,
        )
        .unwrap();

        let prunable = PerRepoConfig::load_with_diagnostics(repo)
            .unwrap()
            .unwrap()
            .prunable
            .unwrap();

        // The declaration survives the merge and is vetoed when it is resolved, so
        // deleting the exclusion later puts the directory back in play without anyone
        // having to re-declare it.
        assert_eq!(prunable.directories.len(), 1);
        assert_eq!(prunable.exclude, ["scratch"]);
    }

    #[test]
    fn declarations_from_both_files_add_up_rather_than_one_silencing_the_other() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        fs::write(
            repo.join(constants::PROJECT_REPO_CONFIG_FILE),
            r#"{ "prunable": { "directories": [
                 { "path": "tools/vendor", "rebuild": "make vendor" },
                 { "path": ".cache/shared", "rebuild": "make cache" }
               ] } }"#,
        )
        .unwrap();
        fs::write(
            repo.join(constants::PER_REPO_CONFIG_FILE),
            r#"{ "prunable": { "directories": [
                 { "path": ".cache/shared", "rebuild": "an old script I wrote" },
                 { "path": "scratch", "rebuild": "make scratch" }
               ] } }"#,
        )
        .unwrap();

        let dirs = PerRepoConfig::load_with_diagnostics(repo)
            .unwrap()
            .unwrap()
            .prunable
            .unwrap()
            .directories;

        // Every key above this one is decided by one file or the other. A list is not a
        // decision, so nobody's entry is dropped for having been written by the wrong
        // person.
        let paths: Vec<&str> = dirs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["tools/vendor", ".cache/shared", "scratch"]);

        // One path is still one directory, and the committed answer is the current one.
        assert_eq!(dirs[1].rebuild, "make cache");
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
    /// A repository that moved is recognised, and arrives with everything it had earned.
    #[test]
    fn adopt_moved_entry_transfers_history() {
        let mut reg = Registry::default();
        let old = PathBuf::from("/nowhere/old-home/project");
        let mut entry = RepoEntry::new();
        entry.identity = Some("abc1234def".into());
        entry.total_freed_bytes = 4096;
        entry.enabled = false;
        entry.override_idle_days = Some(90);
        reg.repositories.insert(old.clone(), entry);

        let new = std::env::temp_dir().join("devprune-adopt-live");
        reg.repositories.insert(new.clone(), RepoEntry::new());

        let outcome = reg.adopt_moved_entry(&new, Some("abc1234def".into()));
        assert_eq!(outcome, Adoption::Moved(old.clone()));
        assert!(!reg.repositories.contains_key(&old));

        let moved = &reg.repositories[&canonical_key(&new)];
        assert_eq!(moved.total_freed_bytes, 4096);
        // A repository the user had switched off did not switch itself back on by
        // being moved.
        assert!(!moved.enabled);
        assert_eq!(moved.override_idle_days, Some(90));
        assert_eq!(moved.identity.as_deref(), Some("abc1234def"));
    }

    /// Two dead entries with one root commit are clones, not a move. Nothing is guessed.
    #[test]
    fn adopt_moved_entry_refuses_to_guess_between_two() {
        let mut reg = Registry::default();
        for name in ["/nowhere/a", "/nowhere/b"] {
            let mut entry = RepoEntry::new();
            entry.identity = Some("shared".into());
            reg.repositories.insert(PathBuf::from(name), entry);
        }
        let new = std::env::temp_dir().join("devprune-adopt-ambiguous");
        reg.repositories.insert(new.clone(), RepoEntry::new());

        assert_eq!(
            reg.adopt_moved_entry(&new, Some("shared".into())),
            Adoption::Ambiguous
        );
        assert_eq!(reg.repositories.len(), 3);
        // The identity is still recorded, so the next registration can recognise it
        // once the duplicates are cleared.
        assert_eq!(
            reg.repositories[&canonical_key(&new)].identity.as_deref(),
            Some("shared")
        );
    }

    /// An entry whose path still exists is not a move, however matching its history.
    #[test]
    fn adopt_moved_entry_never_takes_from_a_live_path() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live");
        std::fs::create_dir(&live).unwrap();

        let mut reg = Registry::default();
        let mut entry = RepoEntry::new();
        entry.identity = Some("same".into());
        entry.total_freed_bytes = 999;
        reg.repositories.insert(canonical_key(&live), entry);

        let other = dir.path().join("other");
        std::fs::create_dir(&other).unwrap();
        reg.repositories
            .insert(canonical_key(&other), RepoEntry::new());

        assert_eq!(
            reg.adopt_moved_entry(&other, Some("same".into())),
            Adoption::Nothing
        );
        assert_eq!(
            reg.repositories[&canonical_key(&live)].total_freed_bytes,
            999
        );
    }

    /// A repository with no commits has no identity, so nothing is adopted and nothing
    /// is recorded — a guess would be worse than the dead entry it replaced.
    #[test]
    fn adopt_moved_entry_ignores_a_missing_identity() {
        let mut reg = Registry::default();
        let mut entry = RepoEntry::new();
        entry.identity = Some("orphan".into());
        reg.repositories
            .insert(PathBuf::from("/nowhere/gone"), entry);
        let new = std::env::temp_dir().join("devprune-adopt-unborn");
        reg.repositories.insert(new.clone(), RepoEntry::new());

        assert_eq!(reg.adopt_moved_entry(&new, None), Adoption::Nothing);
        assert_eq!(reg.repositories.len(), 2);
        assert!(reg.needs_identity(&new));
    }

    #[test]
    fn a_restore_too_quick_to_be_real_teaches_nothing() {
        // A manager that found everything still in its cache returns in a moment. Folding
        // that into the average would claim a throughput no cold restore can reach, and
        // the estimate exists precisely to describe a cold one.
        let mut reg = Registry::default();
        reg.record_restore("npm", 500_000_000, 10);
        reg.record_restore("npm", 0, 60_000);
        assert!(reg.restore_rates.is_empty(), "{:?}", reg.restore_rates);

        reg.record_restore("npm", 500_000_000, 60_000);
        assert_eq!(reg.restore_rates["npm"].samples, 1);
    }

    #[test]
    fn the_average_forgets_the_disk_the_machine_no_longer_has() {
        let mut reg = Registry::default();
        for _ in 0..constants::RESTORE_RATE_SAMPLE_CAP {
            reg.record_restore("npm", 1_000_000, 1_000);
        }
        assert_eq!(
            reg.restore_rates["npm"].samples,
            constants::RESTORE_RATE_SAMPLE_CAP
        );

        // The cap is a halving, not a ceiling: the next sample still lands, on top of
        // half of what came before.
        reg.record_restore("npm", 1_000_000, 1_000);
        let rate = &reg.restore_rates["npm"];
        assert_eq!(rate.samples, constants::RESTORE_RATE_SAMPLE_CAP / 2 + 1);
        assert!(rate.bytes_per_sec().is_some());
    }

    #[test]
    fn an_estimate_with_nothing_measured_is_not_offered() {
        // Never a zero and never a guess: a machine that has not restored anything yet
        // has no honest answer to "how long is this to undo", so it does not print one.
        let reg = Registry::default();
        assert!(reg.estimate_restore(&[("npm".into(), 1_000_000)]).is_none());
    }

    #[test]
    fn an_untimed_adapter_is_left_out_of_the_coverage() {
        // Half an answer, reported as half. Counting cargo's bytes at npm's speed would
        // be the one thing worse than saying nothing.
        let mut reg = Registry::default();
        reg.record_restore("npm", 10_000_000, 10_000);
        let (secs, covered) = reg
            .estimate_restore(&[("npm".into(), 10_000_000), ("cargo".into(), 90_000_000)])
            .expect("npm alone is enough to answer for npm");
        assert_eq!(covered, 10_000_000, "cargo has never been timed here");
        assert!((secs - 10.0).abs() < 0.01, "{secs}");
    }
}
