// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Pruning engine — orchestrates the full prune pass.
//
// This module is the "brain" of dev-prune. It coordinates:
// 1. Git repo validation
// 2. Activity/idle checking
// 3. Adapter detection
// 4. Lockfile enforcement
// 5. Safe bloat directory deletion
//
// The engine enforces all safety invariants described in the project spec.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::adapters::BloatDir;
use crate::config::{Registry, RepoEntry};
use crate::constants;
use crate::scanner;
use crate::scanner::git;
use crate::workspace;

/// The outcome of pruning a single bloat directory.
#[derive(Debug, Clone)]
pub enum PruneStatus {
    /// Successfully deleted the bloat directory.
    Pruned,
    /// Skipped because the repo is still active (not idle).
    SkippedActive,
    /// Skipped because it's a dry run.
    SkippedDryRun,
    /// Skipped because lockfile enforcement failed.
    LockfileError(String),
    /// Skipped because the repository's last activity could not be determined.
    ///
    /// Not a `LockfileError`: that tag carries a `fix_command` an agent is told it can
    /// run, and "git failed to answer" has no such mechanical fix.
    ActivityCheckError(String),
    /// The registered path no longer exists on disk.
    PathMissing,
    /// Skipped because the bloat directory doesn't exist.
    NoBloat,
    /// Repo is disabled in the registry.
    Disabled,
    /// Repo has a `ignore.devprune.json` file — opted out.
    SkippedIgnored,
    /// Error during deletion.
    DeleteError(String),
    /// The bloat directory is a symlink or junction, so it was deliberately left alone.
    ///
    /// A skip, not an error: the storage it points at is not this repository's to
    /// delete, and the situation is permanent — reporting it as a failure made every
    /// scheduled pass over such a repo exit non-zero forever.
    SkippedSymlink(String),
    /// `.devprune.json` exists but could not be parsed, so the repo was left alone.
    ConfigError(String),
}

impl std::fmt::Display for PruneStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PruneStatus::Pruned => write!(f, "Pruned"),
            PruneStatus::SkippedActive => write!(f, "Skipped (active)"),
            PruneStatus::SkippedDryRun => write!(f, "Skipped (dry run)"),
            PruneStatus::LockfileError(e) => write!(f, "Lockfile error: {e}"),
            PruneStatus::ActivityCheckError(e) => write!(f, "Activity check failed: {e}"),
            PruneStatus::PathMissing => {
                write!(
                    f,
                    "Path no longer exists (`devp unlink --missing` clears it)"
                )
            }
            PruneStatus::NoBloat => write!(f, "No bloat found"),
            PruneStatus::Disabled => write!(f, "Disabled"),
            PruneStatus::SkippedIgnored => write!(
                f,
                "Ignored (ignore.devprune.json or ignore config in .devprune.json)"
            ),
            PruneStatus::DeleteError(e) => write!(f, "Delete error: {e}"),
            PruneStatus::SkippedSymlink(e) => write!(f, "Skipped (symlink): {e}"),
            PruneStatus::ConfigError(e) => write!(f, "Unreadable .devprune.json: {e}"),
        }
    }
}

/// Bytes in one mebibyte. The size floor is configured in MiB because that is the unit
/// `format_bytes` prints, so a user who sets `10` sees the same number they typed.
pub const BYTES_PER_MIB: u64 = 1024 * 1024;

/// Which package managers a pass is allowed to act on.
///
/// The default allows everything. Built through [`AdapterFilter::new`], which rejects
/// names no adapter answers to — a typo like `--only pmpm` silently matching nothing
/// looks exactly like "there was no bloat", and that is the wrong thing to believe about
/// a tool that deletes directories.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdapterFilter {
    only: Option<Vec<String>>,
    skip: Vec<String>,
}

impl AdapterFilter {
    /// Build a filter from comma-separated `--only` / `--skip` values.
    ///
    /// Names are matched case-insensitively. Listing the same adapter in both lists is
    /// a contradiction rather than a precedence puzzle, so it is rejected outright.
    pub fn new(only: Option<&str>, skip: Option<&str>) -> Result<Self> {
        let known: Vec<&'static str> = crate::adapters::get_all_adapters()
            .iter()
            .map(|a| a.name())
            .collect();

        let parse = |raw: &str, flag: &str| -> Result<Vec<String>> {
            let mut out = Vec::new();
            for token in raw.split(',') {
                let name = token.trim().to_lowercase();
                if name.is_empty() {
                    continue;
                }
                if !known.contains(&name.as_str()) {
                    anyhow::bail!(
                        "`--{flag} {name}` names no known package manager. Available: {}.",
                        known.join(", ")
                    );
                }
                if !out.contains(&name) {
                    out.push(name);
                }
            }
            if out.is_empty() {
                anyhow::bail!("`--{flag}` was given no adapter names.");
            }
            Ok(out)
        };

        let only = only.map(|raw| parse(raw, "only")).transpose()?;
        let skip = skip
            .map(|raw| parse(raw, "skip"))
            .transpose()?
            .unwrap_or_default();

        if let Some(only) = &only {
            if let Some(clash) = only.iter().find(|n| skip.contains(n)) {
                anyhow::bail!("`{clash}` is in both --only and --skip; pick one.");
            }
        }

        Ok(Self { only, skip })
    }

    /// Whether this filter would let `name` through.
    pub fn allows(&self, name: &str) -> bool {
        if self.skip.iter().any(|s| s == name) {
            return false;
        }
        match &self.only {
            Some(only) => only.iter().any(|o| o == name),
            None => true,
        }
    }

    /// Whether the filter restricts anything at all.
    pub fn is_unrestricted(&self) -> bool {
        self.only.is_none() && self.skip.is_empty()
    }

    /// Human-readable summary for the run header, or `None` when nothing is filtered.
    pub fn describe(&self) -> Option<String> {
        if self.is_unrestricted() {
            return None;
        }
        let mut parts = Vec::new();
        if let Some(only) = &self.only {
            parts.push(format!("only {}", only.join(", ")));
        }
        if !self.skip.is_empty() {
            parts.push(format!("skipping {}", self.skip.join(", ")));
        }
        Some(parts.join("; "))
    }
}

/// Everything that shapes a prune pass beyond the repository itself.
///
/// `Default` is written out rather than derived: `scan_depth` has a real default that is
/// not zero, and a derived one would have made every `..Default::default()` call site
/// quietly walk a single level and report a monorepo as empty.
#[derive(Debug, Clone)]
pub struct PruneOptions {
    /// Days of inactivity required before the repository is eligible.
    pub idle_days: u64,
    /// Report sizes and stop. Nothing is verified and nothing is deleted.
    pub dry_run: bool,
    /// Bypass the idle check. Lockfile verification still applies.
    pub force: bool,
    /// Restrict the pass to these repository-relative bloat directory labels.
    ///
    /// `Some` means a caller has already chosen — the interactive selector, or the
    /// second phase of `devp run`. The size floor is not applied on top of an explicit
    /// choice, because the caller has already decided these directories are wanted.
    pub only_dirs: Option<Vec<String>>,
    /// Which package managers may act.
    pub adapters: AdapterFilter,
    /// Smallest directory worth deleting. `0` disables the floor.
    pub min_size_bytes: u64,
    /// How deep to walk each repository looking for projects.
    ///
    /// The global setting. A repository's own `.devprune.json` may raise or lower it —
    /// see [`workspace::resolve_depth`], which this is fed into.
    pub scan_depth: usize,
    /// Whether an adapter may run the sync command that rewrites its tracked lockfile.
    pub allow_manifest_rewrite: bool,
    /// Ceiling on any one package-manager command, in seconds.
    ///
    /// The user's `command_timeout_secs`. It was settable, displayed by `devp status`
    /// and named in the timeout message long before anything actually read it here.
    pub command_timeout_secs: u64,
}

impl Default for PruneOptions {
    fn default() -> Self {
        Self {
            idle_days: 0,
            dry_run: false,
            force: false,
            only_dirs: None,
            adapters: AdapterFilter::default(),
            min_size_bytes: 0,
            scan_depth: crate::constants::DEFAULT_SCAN_DEPTH,
            allow_manifest_rewrite: crate::constants::DEFAULT_ALLOW_MANIFEST_REWRITE,
            command_timeout_secs: crate::constants::DEFAULT_COMMAND_TIMEOUT_SECS,
        }
    }
}

impl PruneOptions {
    /// The common case: prune everything eligible in this repository.
    pub fn new(idle_days: u64, dry_run: bool, force: bool) -> Self {
        Self {
            idle_days,
            dry_run,
            force,
            ..Self::default()
        }
    }
}

/// Result of pruning a single bloat directory in a single repo.
#[derive(Debug, Clone)]
pub struct PruneResult {
    /// Path to the repository.
    pub repo_path: PathBuf,
    /// Name of the adapter that handled this.
    pub adapter_name: String,
    /// The bloat directory that was (or would be) pruned.
    pub bloat_dir: String,
    /// Bytes freed (0 if not pruned).
    pub size_freed: u64,
    /// What happened.
    pub status: PruneStatus,
}

/// Prune a single repository. Returns results for each bloat directory found.
///
/// # Safety Invariants
/// 1. The path MUST contain a valid `.git` directory
/// 2. The repo must be idle (unless `force` is true)
/// 3. Lockfile enforcement MUST succeed before any deletion
pub fn prune_repo(
    repo_path: &Path,
    idle_days: u64,
    dry_run: bool,
    force: bool,
) -> Vec<PruneResult> {
    prune_repo_with(repo_path, &PruneOptions::new(idle_days, dry_run, force))
}

/// Prune a repository, optionally restricted to a specific set of bloat directories.
///
/// `only` is a list of `BloatDir::name` values. When `Some`, any bloat directory whose
/// name is not in the list is left untouched and produces no result — this is what makes
/// the interactive selector's per-directory choices meaningful. When `None`, every
/// detected bloat directory is pruned.
///
/// Same safety invariants as [`prune_repo`].
pub fn prune_repo_selected(
    repo_path: &Path,
    idle_days: u64,
    dry_run: bool,
    force: bool,
    only: Option<&[String]>,
) -> Vec<PruneResult> {
    prune_repo_with(
        repo_path,
        &PruneOptions {
            only_dirs: only.map(<[String]>::to_vec),
            ..PruneOptions::new(idle_days, dry_run, force)
        },
    )
}

/// Prune a repository under a full set of [`PruneOptions`].
///
/// This is the single implementation; [`prune_repo`] and [`prune_repo_selected`] are
/// thin wrappers for the two common shapes.
pub fn prune_repo_with(repo_path: &Path, opts: &PruneOptions) -> Vec<PruneResult> {
    let idle_days = opts.idle_days;
    let dry_run = opts.dry_run;
    let force = opts.force;
    let only = opts.only_dirs.as_deref();
    let mut results = Vec::new();

    // A registered path that is gone gets a visible line, not silence. Returning empty
    // results made the repository vanish from the run report entirely, which reads as
    // "handled" when the truth is "not found".
    if !repo_path.exists() {
        results.push(PruneResult {
            repo_path: repo_path.to_path_buf(),
            adapter_name: "-".to_string(),
            bloat_dir: "-".to_string(),
            size_freed: 0,
            status: PruneStatus::PathMissing,
        });
        return results;
    }

    // Safety check: must be a git repo
    if !scanner::is_git_repo(repo_path) {
        return results;
    }

    // Instant 0ms Check: if `ignore.devprune.json` exists in repo root, skip immediately without parsing any JSON files!
    if repo_path.join(constants::DEVPRUNE_IGNORE_FILE).exists() {
        results.push(PruneResult {
            repo_path: repo_path.to_path_buf(),
            adapter_name: "-".to_string(),
            bloat_dir: "-".to_string(),
            size_freed: 0,
            status: PruneStatus::SkippedIgnored,
        });
        return results;
    }

    // A `.devprune.json` that does not parse is a refusal to guess, not a missing file.
    // Falling back to defaults would drop `"ignore": true` and prune a repository the
    // user explicitly opted out of, so an unreadable config skips the repo entirely.
    let per_repo_config = match crate::config::PerRepoConfig::load_with_diagnostics(repo_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            results.push(PruneResult {
                repo_path: repo_path.to_path_buf(),
                adapter_name: "-".to_string(),
                bloat_dir: "-".to_string(),
                size_freed: 0,
                status: PruneStatus::ConfigError(e),
            });
            return results;
        }
    };
    if per_repo_config.as_ref().map(|c| c.ignore).unwrap_or(false) {
        results.push(PruneResult {
            repo_path: repo_path.to_path_buf(),
            adapter_name: "-".to_string(),
            bloat_dir: "-".to_string(),
            size_freed: 0,
            status: PruneStatus::SkippedIgnored,
        });
        return results;
    }

    // Effective idle days from per-repo config or parameter
    let effective_idle_days = per_repo_config
        .as_ref()
        .and_then(|c| c.override_idle_days)
        .unwrap_or(idle_days);

    // A repository may set its own floor, including `0` to opt out of a global one.
    // An explicit directory selection overrides both: the caller already chose.
    let min_size_bytes = if only.is_some() {
        0
    } else {
        per_repo_config
            .as_ref()
            .and_then(|c| c.min_size_mb)
            .map(|mb| mb.saturating_mul(BYTES_PER_MIB))
            .unwrap_or(opts.min_size_bytes)
    };

    // Check if repo is idle (skip if active, unless forced)
    if !force {
        match git::is_repo_idle(repo_path, effective_idle_days) {
            Ok(false) => {
                results.push(PruneResult {
                    repo_path: repo_path.to_path_buf(),
                    adapter_name: "-".to_string(),
                    bloat_dir: "-".to_string(),
                    size_freed: 0,
                    status: PruneStatus::SkippedActive,
                });
                return results;
            }
            Ok(true) => {} // Continue — repo is idle
            Err(e) => {
                results.push(PruneResult {
                    repo_path: repo_path.to_path_buf(),
                    adapter_name: "-".to_string(),
                    bloat_dir: "-".to_string(),
                    size_freed: 0,
                    status: PruneStatus::ActivityCheckError(e.to_string()),
                });
                return results;
            }
        }
    }

    // A repository can hold several projects at several depths — `frontend/` on pnpm,
    // `services/api/` on uv, `cli/` on cargo — and each is verified and pruned on its
    // own terms.
    let projects = workspace::discover_to_depth(
        repo_path,
        workspace::resolve_depth(repo_path, opts.scan_depth),
    );

    // Two adapters can legitimately claim the same directory (e.g. a cargo workspace
    // member and its workspace root both resolving to the same `target`). Without this
    // guard the size is counted twice and the second delete fails with "not found".
    let mut claimed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for project in &projects {
        for adapter in &project.adapters {
            if !opts.adapters.allows(adapter.name()) {
                continue;
            }

            // Labels are repo-relative (`node_modules`, `frontend/node_modules`) so that
            // two directories with the same basename in different projects stay
            // distinguishable — both on screen and in the `only` selection.
            //
            // The size floor is applied before `claimed`, so a directory rejected for
            // being too small does not also block a second adapter from considering it.
            let bloat_dirs: Vec<(String, BloatDir)> = adapter
                .bloat_dirs(&project.path)
                .into_iter()
                .map(|bd| (workspace::relative_label(repo_path, &bd.path), bd))
                .filter(|(label, _)| only.is_none_or(|names| names.contains(label)))
                .filter(|(_, bd)| bd.size_bytes >= min_size_bytes)
                .filter(|(_, bd)| claimed.insert(bd.path.clone()))
                .collect();

            if bloat_dirs.is_empty() {
                continue;
            }

            // Enforce lockfile BEFORE any deletion (skipped in dry-run — analysis only)
            if !dry_run {
                let policy = crate::adapters::EnforcePolicy {
                    allow_rewrite: opts.allow_manifest_rewrite,
                    timeout: std::time::Duration::from_secs(opts.command_timeout_secs),
                };
                if let Err(e) = adapter.enforce_lockfile(&project.path, policy) {
                    for (label, _) in &bloat_dirs {
                        results.push(PruneResult {
                            repo_path: repo_path.to_path_buf(),
                            adapter_name: adapter.name().to_string(),
                            bloat_dir: label.clone(),
                            size_freed: 0,
                            status: PruneStatus::LockfileError(e.to_string()),
                        });
                    }
                    continue;
                }
            }

            for (label, bd) in bloat_dirs {
                // A symlinked/junctioned bloat dir points at storage we do not own — in
                // a monorepo it is usually the workspace root's real `node_modules`.
                // Refuse rather than risk a recursive delete outside the repo. Checked
                // before the dry-run branch, because an analysis that counted the link
                // as reclaimable promised space the real pass then refused to touch.
                if fs::symlink_metadata(&bd.path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    results.push(PruneResult {
                        repo_path: repo_path.to_path_buf(),
                        adapter_name: adapter.name().to_string(),
                        bloat_dir: label,
                        size_freed: 0,
                        status: PruneStatus::SkippedSymlink(format!(
                            "`{}` is a symlink to storage dev-prune does not own — \
                             left alone. Remove the link yourself if you really want \
                             it gone.",
                            bd.path.display()
                        )),
                    });
                    continue;
                }

                if dry_run {
                    results.push(PruneResult {
                        repo_path: repo_path.to_path_buf(),
                        adapter_name: adapter.name().to_string(),
                        bloat_dir: label,
                        size_freed: bd.size_bytes,
                        status: PruneStatus::SkippedDryRun,
                    });
                    continue;
                }

                // Invariant 7 keeps the *walk* out of nested repositories, but the
                // directory about to be deleted can hold one inside it — a `file:`
                // dependency, a vendored checkout — with its own unpushed history.
                // No lockfile rebuilds somebody else's git history, so refuse.
                if let Some(nested) = find_nested_git(&bd.path) {
                    results.push(PruneResult {
                        repo_path: repo_path.to_path_buf(),
                        adapter_name: adapter.name().to_string(),
                        bloat_dir: label,
                        size_freed: 0,
                        status: PruneStatus::DeleteError(format!(
                            "`{}` contains a git repository at `{}` — refusing to \
                             delete it. Move or remove that checkout yourself if it \
                             holds nothing you need.",
                            bd.path.display(),
                            nested.display()
                        )),
                    });
                    continue;
                }

                let size = bd.size_bytes;
                // `remove_dir_all` is not atomic: one locked file — an antivirus scan,
                // an editor's file watcher — aborts it half-way, leaving a directory
                // that is neither usable nor gone. Retry once, because such locks are
                // usually released within moments of being hit.
                let delete = fs::remove_dir_all(&bd.path).or_else(|_| fs::remove_dir_all(&bd.path));
                match delete {
                    // "Not found" after a failed first attempt means the delete *did*
                    // complete — treat both the same.
                    Ok(()) => {
                        results.push(PruneResult {
                            repo_path: repo_path.to_path_buf(),
                            adapter_name: adapter.name().to_string(),
                            bloat_dir: label,
                            size_freed: size,
                            status: PruneStatus::Pruned,
                        });
                    }
                    Err(_) if !bd.path.exists() => {
                        results.push(PruneResult {
                            repo_path: repo_path.to_path_buf(),
                            adapter_name: adapter.name().to_string(),
                            bloat_dir: label,
                            size_freed: size,
                            status: PruneStatus::Pruned,
                        });
                    }
                    Err(e) => {
                        // Say what state the failure left behind. A half-deleted
                        // `node_modules` is corrupt whatever caused the abort, so the
                        // honest report is "no longer usable, rebuild it" — and the
                        // bytes already freed, so callers can record the partial pass
                        // and `devp restore` knows what to rebuild.
                        let remaining = crate::adapters::dir_size(&bd.path);
                        let freed = size.saturating_sub(remaining);
                        let message = if freed > 0 {
                            format!(
                                "{e} — `{}` was partially deleted ({} of {} remains) \
                                 and is no longer usable. Close whatever holds it open, \
                                 then run `devp restore` to rebuild it.",
                                bd.path.display(),
                                crate::output::format_bytes(remaining),
                                crate::output::format_bytes(size)
                            )
                        } else {
                            e.to_string()
                        };
                        results.push(PruneResult {
                            repo_path: repo_path.to_path_buf(),
                            adapter_name: adapter.name().to_string(),
                            bloat_dir: label,
                            size_freed: freed,
                            status: PruneStatus::DeleteError(message),
                        });
                    }
                }
            }
        }
    }

    // Nothing recognised, or recognised but nothing on disk to reclaim.
    if results.is_empty() {
        results.push(PruneResult {
            repo_path: repo_path.to_path_buf(),
            adapter_name: "-".to_string(),
            bloat_dir: "-".to_string(),
            size_freed: 0,
            status: PruneStatus::NoBloat,
        });
    }

    results
}

/// The first git repository found anywhere inside `dir`, if there is one.
///
/// `.git` as a directory is a full repository; as a file it is a submodule or worktree
/// gitlink. Either way the history it anchors lives (at least partly) in the tree that
/// is about to be deleted, and no lockfile can rebuild that.
fn find_nested_git(dir: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .flatten()
        .find(|e| e.file_name() == ".git")
        .map(|e| e.into_path())
}

/// Every bloat directory in a repository, across every nested project.
///
/// Returns the distinct adapter names in play alongside the deduplicated directories,
/// each labelled with its repository-relative path. Directories under `min_size_bytes`
/// are omitted so that what `devp status` reports as reclaimable is what `devp run`
/// would actually offer to delete.
fn collect_bloat(
    repo_path: &Path,
    min_size_bytes: u64,
    depth: usize,
) -> (Vec<String>, Vec<BloatDir>) {
    let mut adapter_names: Vec<String> = Vec::new();
    let mut bloat: Vec<BloatDir> = Vec::new();
    let mut claimed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for project in workspace::discover_to_depth(repo_path, depth) {
        for adapter in &project.adapters {
            let name = adapter.name();
            if !adapter_names.iter().any(|existing| existing == name) {
                adapter_names.push(name.to_string());
            }
            for bd in adapter.bloat_dirs(&project.path) {
                if bd.size_bytes < min_size_bytes {
                    continue;
                }
                if claimed.insert(bd.path.clone()) {
                    bloat.push(BloatDir {
                        name: workspace::relative_label(repo_path, &bd.path),
                        ..bd
                    });
                }
            }
        }
    }

    (adapter_names, bloat)
}

/// Run a prune pass across all registered repositories.
///
/// Each repository's own idle threshold applies; everything else in `opts` — the
/// adapter filter, the size floor, dry-run and force — is shared by the whole pass.
pub fn prune_all_with(registry: &mut Registry, opts: &PruneOptions) -> Vec<PruneResult> {
    let mut all_results = Vec::new();

    // Collect paths first to avoid borrow issues.
    //
    // Sorted, because `repositories` is a HashMap: without this the output of two
    // identical runs lists the same repositories in a different order, which makes the
    // summary hard to read and the JSON document impossible to diff.
    let mut repos: Vec<(PathBuf, u64, bool)> = registry
        .repositories
        .iter()
        .map(|(path, entry)| {
            let idle_days = entry
                .override_idle_days
                .unwrap_or(registry.settings.idle_days);
            (path.clone(), idle_days, entry.enabled)
        })
        .collect();
    repos.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, idle_days, enabled) in repos {
        if !enabled {
            all_results.push(PruneResult {
                repo_path: path.clone(),
                adapter_name: "-".to_string(),
                bloat_dir: "-".to_string(),
                size_freed: 0,
                status: PruneStatus::Disabled,
            });
            continue;
        }

        let results = prune_repo_with(
            &path,
            &PruneOptions {
                idle_days,
                ..opts.clone()
            },
        );

        let path_freed: u64 = results
            .iter()
            .filter(|r| matches!(r.status, PruneStatus::Pruned))
            .map(|r| r.size_freed)
            .sum();

        if path_freed > 0 {
            registry.mark_pruned(&path, path_freed);
        }

        all_results.extend(results);
    }

    all_results
}

/// Run a prune pass across all registered repositories with default options.
pub fn prune_all(registry: &mut Registry, dry_run: bool, force: bool) -> Vec<PruneResult> {
    prune_all_with(registry, &PruneOptions::new(0, dry_run, force))
}

/// Restore dependencies across every project in a tree.
///
/// Mirrors pruning: if `frontend/`, `services/api/` and `cli/` were each pruned, each is
/// restored by its own manager. The returned label is the adapter name for a project at
/// the root and `adapter (relative/path)` for a nested one.
///
/// Restore must reach at least as deep as the prune did. A repository configured to a
/// depth of 10 and pruned at 10, then restored at the default 6, comes back with its
/// deepest projects still empty — and nothing would have said so. `timeout` is the
/// user's `command_timeout_secs` for the same reason: a full reinstall is the longest
/// command this tool ever runs, and it used to be the only one that ignored the setting.
pub fn restore_project_to_depth(
    project_path: &Path,
    global_depth: usize,
    timeout: std::time::Duration,
) -> Result<Vec<(String, Result<()>)>> {
    let depth = workspace::resolve_depth(project_path, global_depth);
    let projects = workspace::discover_to_depth(project_path, depth);

    if projects.is_empty() {
        anyhow::bail!(
            "No recognized package manager found in {}",
            project_path.display()
        );
    }

    let mut results = Vec::new();
    for project in &projects {
        for adapter in &project.adapters {
            let label = if project.relative == "." {
                adapter.name().to_string()
            } else {
                format!("{} ({})", adapter.name(), project.relative)
            };
            results.push((label, adapter.restore(&project.path, timeout)));
        }
    }

    Ok(results)
}

/// The project directory that owns a bloat directory, as a repository-relative label.
///
/// Every adapter puts its bloat directory immediately inside the project it belongs to —
/// `node_modules`, `target`, `.venv`, `vendor` — so the owner is the label's parent.
/// `"node_modules"` belongs to the repository root, `"frontend/node_modules"` to
/// `frontend/`. Labels are `/`-separated on every platform; see
/// [`workspace::relative_label`].
fn owning_project(bloat_label: &str) -> &str {
    match bloat_label.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => ".",
    }
}

/// Restore exactly the projects a previous pass emptied, and nothing else.
///
/// `deleted` is the `(bloat directory label, adapter name)` pairs recorded at prune time.
/// [`restore_project_to_depth`] would reinstall every project in the tree; a repository
/// where one of five projects was pruned does not want the other four rebuilt, which on a
/// monorepo is the difference between one `npm ci` and five.
///
/// A recorded pair that no longer matches anything in the tree — the project was deleted,
/// renamed, or its manifest removed since the prune — comes back as an `Err` under its
/// own label rather than being dropped, because a restore that silently skips half its
/// work is the failure mode this whole command exists to avoid.
pub fn restore_deleted(
    repo_path: &Path,
    deleted: &[(String, String)],
    global_depth: usize,
    timeout: std::time::Duration,
) -> Vec<(String, Result<()>)> {
    let depth = workspace::resolve_depth(repo_path, global_depth);
    let projects = workspace::discover_to_depth(repo_path, depth);

    let mut results = Vec::new();
    for (bloat_label, adapter_name) in deleted {
        let wanted = owning_project(bloat_label);
        let label = format!("{adapter_name} ({bloat_label})");
        // The deleted directory's own name, so an adapter that supports several
        // (venv's `.venv`/`venv`/`my_env`) rebuilds the one that was actually there.
        let dir_name = bloat_label
            .rsplit_once('/')
            .map_or(bloat_label.as_str(), |(_, name)| name);

        let found = projects
            .iter()
            .filter(|p| p.relative == wanted)
            .flat_map(|p| p.adapters.iter().map(move |a| (p, a)))
            .find(|(_, a)| a.name() == adapter_name);

        if let Some((project, adapter)) = found {
            results.push((
                label,
                adapter.restore_named(&project.path, dir_name, timeout),
            ));
            continue;
        }

        // Re-detection can fail *because* the prune succeeded: deleting a virtual
        // environment removes the very `pyvenv.cfg` that venv detection looks for. The
        // recorded adapter passed detection and lockfile verification at prune time, so
        // when the project directory still exists, trust the record over a re-detect
        // that is looking at the hole the prune left.
        let project_dir = if wanted == "." {
            repo_path.to_path_buf()
        } else {
            repo_path.join(wanted)
        };
        let recorded = crate::adapters::get_all_adapters()
            .into_iter()
            .find(|a| a.name() == adapter_name);
        match recorded {
            Some(adapter) if project_dir.is_dir() => {
                results.push((
                    label,
                    adapter.restore_named(&project_dir, dir_name, timeout),
                ));
            }
            _ => results.push((
                label,
                Err(anyhow::anyhow!(
                    "`{wanted}` in {} is no longer a {adapter_name} project — it may have been \
                     moved or removed since the prune. Restore it by hand if it still exists.",
                    repo_path.display()
                )),
            )),
        }
    }

    results
}

/// Reason why a repo was not selected as a prune candidate.
#[derive(Debug, Clone, PartialEq)]
pub enum SkipReason {
    /// Has pruneable bloat — this IS a candidate.
    Candidate,
    /// Repo has been active recently.
    Active,
    /// Opted out: either registry-disabled OR `ignore.devprune.json` file present OR `.devprune.json` ignore config.
    /// Both are treated identically.
    Ignored,
    /// No recognised package manager / bloat dirs found.
    NoBloat,
    /// Path no longer exists on disk.
    PathMissing,
    /// `.devprune.json` exists but does not parse, so nothing about this repo is known.
    ConfigError(String),
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::Candidate => write!(f, "Candidate"),
            SkipReason::Active => write!(f, "Active (not idle)"),
            SkipReason::Ignored => write!(f, "Ignored"),
            SkipReason::NoBloat => write!(f, "No bloat found"),
            SkipReason::PathMissing => write!(f, "Path missing"),
            SkipReason::ConfigError(_) => write!(f, "Unreadable .devprune.json"),
        }
    }
}

/// Full status entry for a single registered repository.
#[derive(Debug, Clone)]
pub struct RepoStatusEntry {
    /// Repository path.
    pub path: PathBuf,
    /// Registry metadata.
    pub entry: RepoEntry,
    /// Why this repo was/wasn't selected as a prune candidate.
    pub reason: SkipReason,
    /// Adapter names detected (e.g. ["npm", "uv"]).
    pub adapters: Vec<String>,
    /// Bloat directories and sizes (empty if not a candidate).
    pub bloat_dirs: Vec<BloatDir>,
    /// Total reclaimable bytes.
    pub reclaimable_bytes: u64,
    /// Last git/file-system activity time.
    pub last_activity: Option<DateTime<Utc>>,
    /// Idle threshold that applies to this repo (days).
    pub idle_days: u64,
}

/// Compute full status for ALL registered repositories.
///
/// Unlike `get_space_summary`, this includes every repo — active, disabled,
/// ignored, or missing — with a human-readable reason for each.
pub fn get_full_status(registry: &Registry) -> Vec<RepoStatusEntry> {
    let mut entries: Vec<RepoStatusEntry> = Vec::new();

    for (path, reg_entry) in &registry.repositories {
        let registry_idle_days = reg_entry
            .override_idle_days
            .unwrap_or(registry.settings.idle_days);

        // Path missing? Checked before the config is read, because a directory that is
        // gone has no config to read.
        if !path.exists() {
            entries.push(RepoStatusEntry {
                path: path.clone(),
                entry: reg_entry.clone(),
                reason: SkipReason::PathMissing,
                adapters: Vec::new(),
                bloat_dirs: Vec::new(),
                reclaimable_bytes: 0,
                last_activity: None,
                idle_days: registry_idle_days,
            });
            continue;
        }

        // The same refusal-to-guess the prune pass makes. Reading this with
        // `load_from_repo` treated a broken file as "no config", so a repo that
        // `devp run` would refuse to touch showed up in the dashboard as a healthy
        // candidate with a reclaimable size next to it.
        let per_repo_config = match crate::config::PerRepoConfig::load_with_diagnostics(path) {
            Ok(cfg) => cfg,
            Err(e) => {
                entries.push(RepoStatusEntry {
                    path: path.clone(),
                    entry: reg_entry.clone(),
                    reason: SkipReason::ConfigError(e),
                    adapters: Vec::new(),
                    bloat_dirs: Vec::new(),
                    reclaimable_bytes: 0,
                    last_activity: last_activity_time(path),
                    idle_days: registry_idle_days,
                });
                continue;
            }
        };
        let idle_days = per_repo_config
            .as_ref()
            .and_then(|c| c.override_idle_days)
            .unwrap_or(registry_idle_days);

        // Disabled in registry, ignore.devprune.json present, OR .devprune.json ignore=true
        let is_ignored = !reg_entry.enabled
            || path.join(constants::DEVPRUNE_IGNORE_FILE).exists()
            || per_repo_config.as_ref().map(|c| c.ignore).unwrap_or(false);
        if is_ignored {
            entries.push(RepoStatusEntry {
                path: path.clone(),
                entry: reg_entry.clone(),
                reason: SkipReason::Ignored,
                adapters: Vec::new(),
                bloat_dirs: Vec::new(),
                reclaimable_bytes: 0,
                last_activity: last_activity_time(path),
                idle_days,
            });
            continue;
        }

        // Activity check. One computation drives both the column and the decision —
        // they used to be computed separately, from different rules, so a repo with
        // uncommitted edits was correctly held back as "Active" while the column next
        // to it showed the last *commit*, months earlier.
        let activity = git::get_last_activity(path).ok().flatten();
        let activity_time = to_utc(activity);
        let is_idle = git::is_idle_at(activity, idle_days);

        // Detect adapters & bloat across every project in the repository
        let min_size_bytes = per_repo_config
            .as_ref()
            .and_then(|c| c.min_size_mb)
            .unwrap_or(registry.settings.min_size_mb)
            .saturating_mul(BYTES_PER_MIB);
        // Same resolution order as the size floor just above: the repository's own
        // config first, the global setting otherwise. The dashboard and a run must walk
        // to the same depth or `status` will list projects `run` never sees.
        let depth = workspace::clamp_depth(
            per_repo_config
                .as_ref()
                .and_then(|c| c.scan_depth)
                .unwrap_or(registry.settings.scan_depth),
        );
        let (adapter_names, all_bloat) = collect_bloat(path, min_size_bytes, depth);
        let reclaimable: u64 = all_bloat.iter().map(|b| b.size_bytes).sum();

        let reason = if !is_idle {
            SkipReason::Active
        } else if all_bloat.is_empty() {
            SkipReason::NoBloat
        } else {
            SkipReason::Candidate
        };

        entries.push(RepoStatusEntry {
            path: path.clone(),
            entry: reg_entry.clone(),
            reason,
            adapters: adapter_names,
            bloat_dirs: all_bloat,
            reclaimable_bytes: reclaimable,
            last_activity: activity_time,
            idle_days,
        });
    }

    // Sort: candidates first, then by path name
    entries.sort_by(|a, b| {
        let a_cand = matches!(a.reason, SkipReason::Candidate);
        let b_cand = matches!(b.reason, SkipReason::Candidate);
        b_cand.cmp(&a_cand).then_with(|| a.path.cmp(&b.path))
    });

    entries
}

/// The `n` repositories with the most reclaimable space, or all of them when `top` is
/// `None`.
///
/// `devp status` lists every registered repository, which on a machine tracking a hundred
/// of them pushes the handful actually worth pruning off the screen. Selection is by
/// reclaimable bytes, descending; the survivors are then put back into the order
/// [`get_full_status`] produced, so a truncated dashboard reads like a shorter version of
/// the full one rather than a differently-sorted one.
pub fn take_top(repos: &[RepoStatusEntry], top: Option<usize>) -> Vec<RepoStatusEntry> {
    let Some(n) = top else {
        return repos.to_vec();
    };

    let mut ranked: Vec<usize> = (0..repos.len()).collect();
    ranked.sort_by_key(|&i| std::cmp::Reverse(repos[i].reclaimable_bytes));
    ranked.truncate(n);
    ranked.sort_unstable();
    ranked.into_iter().map(|i| repos[i].clone()).collect()
}

/// Compute crisp, disambiguated project names for a repository path.
///
/// Uses `.devprune.json` custom `project_name` if present. Otherwise defaults to folder name.
/// If multiple repositories share the exact same folder name, disambiguates by including parent folder.
pub fn compute_display_name(repo_path: &Path, all_paths: &[PathBuf]) -> String {
    // A label, so a config that does not parse just falls through to the folder name —
    // the states that matter are reported by the caller.
    if let Some(cfg) = crate::config::PerRepoConfig::load_with_diagnostics(repo_path)
        .ok()
        .flatten()
    {
        if let Some(custom) = cfg.project_name {
            if !custom.trim().is_empty() {
                return custom;
            }
        }
    }

    let folder_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| crate::output::clean_path(repo_path));

    // Check if duplicate folder names exist
    let duplicate_count = all_paths
        .iter()
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .as_deref()
                == Some(&folder_name)
        })
        .count();

    if duplicate_count > 1 {
        if let Some(parent) = repo_path.parent() {
            if let Some(parent_name) = parent.file_name() {
                return format!("{}/{}", parent_name.to_string_lossy(), folder_name);
            }
        }
    }

    folder_name
}

/// Best-effort last activity time for a repo: the later of its last commit and the
/// newest source file mtime, which is the same value the idle check uses.
fn last_activity_time(path: &Path) -> Option<DateTime<Utc>> {
    to_utc(git::get_last_activity(path).ok().flatten())
}

/// A `SystemTime` as the UTC timestamp the status entries carry.
fn to_utc(system_time: Option<SystemTime>) -> Option<DateTime<Utc>> {
    system_time.map(|st| {
        let duration = st
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        DateTime::from_timestamp(duration.as_secs() as i64, 0).unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Restore in these tests either fails before running anything or runs against an
    /// empty project; none of them should ever sit anywhere near this long.
    const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

    fn create_git_repo_with_commit(path: &Path) {
        fs::create_dir_all(path).unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        fs::write(path.join("README.md"), "# Test").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@test.com",
                "commit",
                "-m",
                "initial",
            ])
            .current_dir(path)
            .output()
            .unwrap();
    }

    #[test]
    fn a_bloat_label_names_the_project_that_owns_it() {
        assert_eq!(owning_project("node_modules"), ".");
        assert_eq!(owning_project("frontend/node_modules"), "frontend");
        assert_eq!(
            owning_project("packages/@scope/app/.venv"),
            "packages/@scope/app"
        );
    }

    #[test]
    fn restore_deleted_touches_only_the_projects_that_were_pruned() {
        // Two npm projects, one of which was pruned. Restoring the whole tree would
        // reinstall both; only the recorded one may be attempted.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        for name in ["frontend", "docs"] {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), "{}").unwrap();
            fs::write(dir.join("package-lock.json"), "{}").unwrap();
        }

        let deleted = vec![("frontend/node_modules".to_string(), "npm".to_string())];
        let results = restore_deleted(root, &deleted, 4, TEST_TIMEOUT);

        assert_eq!(results.len(), 1, "one recorded directory, one attempt");
        assert_eq!(results[0].0, "npm (frontend/node_modules)");
    }

    #[test]
    fn restore_deleted_reports_a_project_that_is_no_longer_there() {
        // Recorded at prune time, gone by restore time. Reported, never dropped: a
        // restore that quietly skips half its work is the failure this command prevents.
        let tmp = TempDir::new().unwrap();
        let deleted = vec![("services/api/.venv".to_string(), "uv".to_string())];
        let results = restore_deleted(tmp.path(), &deleted, 4, TEST_TIMEOUT);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "uv (services/api/.venv)");
        let err = results[0].1.as_ref().unwrap_err().to_string();
        assert!(err.contains("services/api"), "names the missing project");
        assert!(err.contains("uv"), "names the adapter that owned it");
    }

    #[test]
    fn test_prune_status_display() {
        assert_eq!(PruneStatus::Pruned.to_string(), "Pruned");
        assert_eq!(PruneStatus::SkippedActive.to_string(), "Skipped (active)");
        assert_eq!(PruneStatus::SkippedDryRun.to_string(), "Skipped (dry run)");
    }

    #[test]
    fn test_prune_repo_non_git() {
        let tmp = TempDir::new().unwrap();
        let results = prune_repo(tmp.path(), 15, false, false);
        assert!(results.is_empty());
    }

    #[test]
    fn test_prune_repo_active_skipped() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        // Just committed — active
        let results = prune_repo(&repo, 15, false, false);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].status, PruneStatus::SkippedActive));
    }

    /// An unreadable `.devprune.json` must never fall back to defaults: the file may have
    /// said `"ignore": true`, and guessing would delete from a repo that opted out.
    #[test]
    fn test_unparseable_per_repo_config_skips_the_repo() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        fs::create_dir(repo.join("target")).unwrap();
        fs::write(repo.join("target").join("dummy"), "data").unwrap();
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"",
        )
        .unwrap();
        fs::write(repo.join("Cargo.lock"), "# lockfile").unwrap();
        // Trailing comma — valid-looking, but not valid JSON.
        fs::write(repo.join(".devprune.json"), "{ \"ignore\": true, }").unwrap();

        // Forced, non-dry-run: everything else would have this repo pruned.
        let results = prune_repo(&repo, 15, false, true);

        assert_eq!(results.len(), 1);
        assert!(
            matches!(results[0].status, PruneStatus::ConfigError(_)),
            "expected ConfigError, got {:?}",
            results[0].status
        );
        assert!(repo.join("target").exists(), "target must survive");
    }

    /// The dashboard and the prune pass have to agree about a broken config file.
    /// Reporting it as a healthy candidate with a size next to it invites the user to
    /// select a repository that `devp run` will then refuse to touch.
    #[test]
    fn a_broken_config_is_reported_by_status_and_not_as_a_candidate() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        create_python_project(&repo);
        fs::write(repo.join(".devprune.json"), "{ \"ignore\": true, }").unwrap();

        let mut registry = Registry::default();
        registry.add_repo(repo.clone());

        let entries = get_full_status(&registry);
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(entries[0].reason, SkipReason::ConfigError(_)),
            "expected ConfigError, got {:?}",
            entries[0].reason
        );
        assert_eq!(entries[0].reclaimable_bytes, 0);
    }

    #[test]
    fn test_prune_repo_dry_run() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        // Create target directory and Cargo.toml + Cargo.lock
        fs::create_dir(repo.join("target")).unwrap();
        fs::write(repo.join("target").join("dummy"), "data").unwrap();
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"",
        )
        .unwrap();
        fs::write(repo.join("Cargo.lock"), "# lockfile").unwrap();
        // Force + dry run → should report what WOULD be pruned
        let results = prune_repo(&repo, 15, true, true);
        let dry_run_results: Vec<_> = results
            .iter()
            .filter(|r| matches!(r.status, PruneStatus::SkippedDryRun))
            .collect();
        assert!(!dry_run_results.is_empty());
        // target should still exist
        assert!(repo.join("target").exists());
    }

    /// A Python project with a populated `requirements.txt` and a virtual environment.
    ///
    /// The venv adapter verifies its lockfile by reading files rather than by shelling
    /// out, so it is the one ecosystem that can be pruned for real inside a test.
    fn create_python_project(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("requirements.txt"), "requests==2.32.3\n").unwrap();
        let venv = dir.join(".venv");
        fs::create_dir_all(&venv).unwrap();
        fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
        fs::write(venv.join("payload.bin"), vec![0u8; 4096]).unwrap();
    }

    /// The `bloat_dir` labels of every result, sorted.
    fn labels(results: &[PruneResult]) -> Vec<String> {
        let mut out: Vec<String> = results.iter().map(|r| r.bloat_dir.clone()).collect();
        out.sort();
        out
    }

    #[test]
    fn test_prune_finds_several_ecosystems_at_the_repo_root() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);

        fs::write(repo.join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        fs::create_dir(repo.join("target")).unwrap();
        fs::write(repo.join("package.json"), "{}").unwrap();
        fs::write(repo.join("package-lock.json"), "{}").unwrap();
        fs::create_dir(repo.join("node_modules")).unwrap();
        create_python_project(&repo);

        let results = prune_repo(&repo, 15, true, true);
        assert_eq!(labels(&results), vec![".venv", "node_modules", "target"]);
    }

    #[test]
    fn test_prune_finds_ecosystems_at_different_depths() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);

        fs::create_dir_all(repo.join("frontend")).unwrap();
        fs::write(repo.join("frontend/package.json"), "{}").unwrap();
        fs::write(repo.join("frontend/pnpm-lock.yaml"), "").unwrap();
        fs::create_dir(repo.join("frontend/node_modules")).unwrap();

        fs::create_dir_all(repo.join("tools/cli")).unwrap();
        fs::write(repo.join("tools/cli/Cargo.toml"), "[package]\nname = \"y\"").unwrap();
        fs::create_dir(repo.join("tools/cli/target")).unwrap();

        create_python_project(&repo.join("services/api"));

        let results = prune_repo(&repo, 15, true, true);
        assert_eq!(
            labels(&results),
            vec![
                "frontend/node_modules",
                "services/api/.venv",
                "tools/cli/target",
            ]
        );
    }

    #[test]
    fn test_prune_deletes_only_the_selected_nested_directory() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        create_python_project(&repo.join("a"));
        create_python_project(&repo.join("b"));

        let results = prune_repo_selected(&repo, 0, false, true, Some(&["a/.venv".to_string()]));

        assert_eq!(labels(&results), vec!["a/.venv"]);
        assert!(matches!(results[0].status, PruneStatus::Pruned));
        assert!(!repo.join("a/.venv").exists());
        assert!(repo.join("b/.venv").exists());
    }

    #[test]
    fn test_prune_ignores_bloat_inside_a_nested_repository() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        create_python_project(&repo.join("outer"));

        // A submodule is its own repository with its own activity history — pruning it
        // as part of the parent would ignore that.
        let nested = repo.join("nested");
        create_git_repo_with_commit(&nested);
        create_python_project(&nested);

        let results = prune_repo(&repo, 15, true, true);
        assert_eq!(labels(&results), vec!["outer/.venv"]);
    }

    #[test]
    fn test_prune_repo_no_adapters() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        // Force prune but no package manager files
        let results = prune_repo(&repo, 15, false, true);
        assert!(
            results
                .iter()
                .any(|r| matches!(r.status, PruneStatus::NoBloat))
        );
    }

    #[test]
    fn test_prune_all_disabled() {
        let tmp = TempDir::new().unwrap();
        let _registry_path = tmp.path().join("registry.json");

        let mut registry = Registry::default();
        let repo_path = PathBuf::from("/nonexistent/repo");
        registry.add_repo(repo_path.clone());
        registry.repositories.get_mut(&repo_path).unwrap().enabled = false;

        let results = prune_all(&mut registry, false, false);
        assert!(
            results
                .iter()
                .any(|r| matches!(r.status, PruneStatus::Disabled))
        );
    }

    #[test]
    fn test_restore_project_no_adapters() {
        let tmp = TempDir::new().unwrap();
        let result = restore_project_to_depth(
            tmp.path(),
            crate::constants::DEFAULT_SCAN_DEPTH,
            TEST_TIMEOUT,
        );
        assert!(result.is_err());
    }

    #[test]
    fn restore_deleted_trusts_the_record_when_the_prune_erased_detection() {
        // Deleting a venv removes the very pyvenv.cfg that detection looks for, so
        // re-detection finds nothing. The recorded adapter must still be attempted —
        // not reported as "no longer a venv project".
        let tmp = TempDir::new().unwrap();
        let api = tmp.path().join("api");
        fs::create_dir_all(&api).unwrap();
        fs::write(api.join("requirements.txt"), "requests==2.32.3\n").unwrap();
        // No .venv on disk — the prune already removed it.

        let deleted = vec![("api/.venv".to_string(), "venv".to_string())];
        // A zero timeout kills the rebuild the moment it starts; the test is about
        // which branch routes, not whether python can build an environment here.
        let results = restore_deleted(tmp.path(), &deleted, 4, std::time::Duration::ZERO);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "venv (api/.venv)");
        if let Err(e) = &results[0].1 {
            assert!(
                !e.to_string().contains("no longer a"),
                "the recorded adapter must be attempted, got: {e}"
            );
        }
    }

    #[test]
    fn a_git_repository_inside_a_bloat_directory_refuses_the_delete() {
        // A vendored checkout inside the directory about to be deleted carries its own
        // history, which no lockfile rebuilds. The whole delete must be refused.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        create_python_project(&repo);
        fs::create_dir_all(repo.join(".venv/src/vendored/.git")).unwrap();

        let results = prune_repo_selected(&repo, 0, false, true, Some(&[".venv".to_string()]));

        assert_eq!(results.len(), 1);
        let PruneStatus::DeleteError(msg) = &results[0].status else {
            panic!("expected a refusal, got {:?}", results[0].status);
        };
        assert!(msg.contains("git repository"), "says why: {msg}");
        assert!(repo.join(".venv").exists(), "nothing may be deleted");
        assert_eq!(results[0].size_freed, 0);
    }

    fn status_entry(name: &str, reclaimable: u64) -> RepoStatusEntry {
        RepoStatusEntry {
            path: PathBuf::from(name),
            entry: RepoEntry::new(),
            reason: SkipReason::Candidate,
            adapters: Vec::new(),
            bloat_dirs: Vec::new(),
            reclaimable_bytes: reclaimable,
            last_activity: None,
            idle_days: 15,
        }
    }

    #[test]
    fn take_top_selects_by_size_but_keeps_the_dashboard_order() {
        let repos = [
            status_entry("small", 10),
            status_entry("big", 300),
            status_entry("mid", 200),
        ];
        let names: Vec<String> = take_top(&repos, Some(2))
            .iter()
            .map(|e| e.path.display().to_string())
            .collect();
        // Selection is by reclaimable bytes; the survivors come back in the order the
        // full dashboard had them, so a truncated list reads like a shorter version of
        // the full one rather than a differently-sorted one.
        assert_eq!(names, vec!["big", "mid"]);
    }

    #[test]
    fn take_top_without_a_limit_or_with_an_oversized_one_returns_everything() {
        let repos = [status_entry("a", 1), status_entry("b", 2)];
        assert_eq!(take_top(&repos, None).len(), 2);
        assert_eq!(take_top(&repos, Some(10)).len(), 2);
        assert_eq!(take_top(&repos, Some(0)).len(), 0);
    }

    #[test]
    fn test_restore_project_with_npm() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        let results = restore_project_to_depth(
            tmp.path(),
            crate::constants::DEFAULT_SCAN_DEPTH,
            TEST_TIMEOUT,
        );
        // Will fail because npm isn't available in test env, but shouldn't panic
        assert!(results.is_ok());
        let results = results.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "npm");
    }
}
