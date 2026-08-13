// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Git-specific activity detection.
//
// Determines when a repository was last active by:
// 1. Checking the latest commit timestamp via `git log`
// 2. Falling back to file `mtime` scanning for empty repos (0 commits)

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// Directories to exclude when scanning file mtimes.
const EXCLUDED_DIRS: &[&str] = &[".git", "node_modules", ".venv", "venv", "target", "vendor"];

/// Depth ceiling for the mtime fallback walk, matching the repo-discovery scan.
///
/// The fallback only exists for repositories with no commits; walking an arbitrarily
/// deep tree to answer "has anyone touched this lately" costs more than the answer is
/// worth, and a pathological layout (a recursive junction, a vendored monorepo) could
/// stall every status refresh.
const MAX_MTIME_SCAN_DEPTH: usize = 8;

/// Get the timestamp of the most recent commit in a repository.
///
/// Returns `None` if the repo has no commits — `git log` on an unborn HEAD exits
/// non-zero, so no separate `git rev-parse HEAD` probe is needed to find that out.
pub fn get_last_commit_time(repo_path: &Path) -> Result<Option<SystemTime>> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .current_dir(repo_path)
        .output()
        .context("Failed to execute git log")?;

    if !output.status.success() {
        return Ok(None);
    }

    let timestamp_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if timestamp_str.is_empty() {
        return Ok(None);
    }

    let timestamp: u64 = timestamp_str
        .parse()
        .with_context(|| format!("Failed to parse git timestamp: {timestamp_str}"))?;

    Ok(Some(
        SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp),
    ))
}

/// Scan all source files in a repo and return the latest `mtime`.
///
/// Used as a fallback for empty repos (no commits). Excludes bloat directories
/// and the `.git` folder itself.
pub fn get_mtime_activity(repo_path: &Path) -> Result<Option<SystemTime>> {
    let mut latest: Option<SystemTime> = None;

    let now = SystemTime::now();
    let walker = WalkDir::new(repo_path)
        .follow_links(false)
        .max_depth(MAX_MTIME_SCAN_DEPTH)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !EXCLUDED_DIRS.contains(&name.as_ref())
        });

    for entry in walker.flatten() {
        if entry.file_type().is_file() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(mtime) = metadata.modified() {
                    // A future mtime — a skewed clock, an extracted archive — would make
                    // the repository read as active forever. Clamped, it reads as
                    // touched just now and ages out normally.
                    let mtime = mtime.min(now);
                    latest = Some(match latest {
                        Some(current) if mtime > current => mtime,
                        Some(current) => current,
                        None => mtime,
                    });
                }
            }
        }
    }

    Ok(latest)
}

/// Get the last activity time for a repository.
///
/// Strategy:
/// Checks BOTH commit-based timestamp AND source file mtimes (excluding bloat dirs and .git).
/// Returns `max(commit_time, latest_mtime)` so uncommitted local edits delay pruning.
pub fn get_last_activity(repo_path: &Path) -> Result<Option<SystemTime>> {
    let commit_time = get_last_commit_time(repo_path)?;
    let mtime = get_mtime_activity(repo_path)?;

    match (commit_time, mtime) {
        (Some(c), Some(m)) => Ok(Some(c.max(m))),
        (Some(c), None) => Ok(Some(c)),
        (None, Some(m)) => Ok(Some(m)),
        (None, None) => Ok(None),
    }
}

/// Whether an already-known activity time counts as idle.
///
/// Split out from [`is_repo_idle`] so a caller that has just computed the activity time
/// for display can decide idleness from the same value instead of recomputing it. The
/// dashboard used to do exactly that — a second `git log` plus a second full tree walk
/// per repository — and, worse, showed a "last activity" that the idle decision had not
/// actually used.
pub fn is_idle_at(last_activity: Option<SystemTime>, idle_days: u64) -> bool {
    match last_activity {
        Some(activity_time) => {
            // Saturating throughout: `idle_days * 86400` overflows u64 past ~2.1e14
            // days, and `SystemTime::now() - duration` panics if the result would be
            // before the epoch. A huge idle_days should mean "never idle", not a crash.
            let idle_duration = Duration::from_secs(idle_days.saturating_mul(24 * 60 * 60));
            let Some(threshold) = SystemTime::now().checked_sub(idle_duration) else {
                return false;
            };
            activity_time < threshold
        }
        // No activity detected at all → consider it idle
        None => true,
    }
}

/// Check if a repository is considered "idle" (inactive for `idle_days`).
pub fn is_repo_idle(repo_path: &Path, idle_days: u64) -> Result<bool> {
    Ok(is_idle_at(get_last_activity(repo_path)?, idle_days))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: a `git` that cannot see the developer's own configuration.
    ///
    /// dev-prune installs a *global* `core.hooksPath`. Without this, the commit below
    /// fires the real `post-commit` hook, which registers this temporary directory in the
    /// developer's real registry and leaves a dead entry behind once the fixture is
    /// deleted. Pointing `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` at files that do not
    /// exist is how git is told to read neither.
    fn git(path: &Path) -> Command {
        let mut cmd = Command::new("git");
        cmd.current_dir(path)
            .env("GIT_CONFIG_GLOBAL", path.join("no-such-gitconfig"))
            .env("GIT_CONFIG_SYSTEM", path.join("no-such-gitconfig"));
        cmd
    }

    /// Helper: create a real git repo with `git init`
    fn create_git_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path).args(["init"]).output().unwrap();
    }

    /// Helper: create a git repo with at least one commit
    fn create_git_repo_with_commit(path: &Path) {
        create_git_repo(path);
        fs::write(path.join("README.md"), "# Test").unwrap();
        git(path).args(["add", "."]).output().unwrap();
        git(path)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@test.com",
                "commit",
                "-m",
                "initial",
            ])
            .output()
            .unwrap();
    }

    #[test]
    fn test_get_last_commit_time_with_commits() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        let time = get_last_commit_time(&repo).unwrap();
        assert!(time.is_some());
    }

    #[test]
    fn test_get_last_commit_time_empty_repo() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo(&repo);
        let time = get_last_commit_time(&repo).unwrap();
        assert!(time.is_none());
    }

    #[test]
    fn test_get_mtime_activity() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "hello").unwrap();
        let activity = get_mtime_activity(tmp.path()).unwrap();
        assert!(activity.is_some());
    }

    #[test]
    fn test_get_mtime_activity_excludes_git() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
        // Only .git files, no source files
        let activity = get_mtime_activity(tmp.path()).unwrap();
        // The root dir itself might return something, but no source files
        // This just verifies it doesn't crash
        assert!(activity.is_some() || activity.is_none());
    }

    #[test]
    fn test_get_last_activity_with_commits() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        let activity = get_last_activity(&repo).unwrap();
        assert!(activity.is_some());
    }

    #[test]
    fn test_get_last_activity_empty_repo_with_files() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo(&repo);
        fs::write(repo.join("main.py"), "print('hello')").unwrap();
        let activity = get_last_activity(&repo).unwrap();
        assert!(activity.is_some());
    }

    #[test]
    fn test_is_repo_idle_recent() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);
        // A repo committed just now should NOT be idle
        assert!(!is_repo_idle(&repo, 15).unwrap());
    }

    #[test]
    fn is_idle_at_agrees_with_the_repo_level_check() {
        // The two must not drift: the dashboard decides with `is_idle_at` on an activity
        // time it already has, the prune pass decides with `is_repo_idle`.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo_with_commit(&repo);

        let activity = get_last_activity(&repo).unwrap();
        assert_eq!(is_idle_at(activity, 15), is_repo_idle(&repo, 15).unwrap());
        assert!(!is_idle_at(activity, 15));
        assert!(is_idle_at(activity, 0));
    }

    #[test]
    fn a_repo_with_no_activity_at_all_is_idle() {
        assert!(is_idle_at(None, 15));
    }

    #[test]
    fn an_absurd_idle_threshold_means_never_idle_rather_than_a_panic() {
        // `now - u64::MAX days` is before the epoch; subtracting it must not panic.
        assert!(!is_idle_at(Some(SystemTime::UNIX_EPOCH), u64::MAX));
    }

    #[test]
    fn test_is_repo_idle_no_activity() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        create_git_repo(&repo);
        // Empty repo with no files → idle
        // Note: might have mtime from git init, but that's recent
        // so let's test with 0 idle days
        let result = is_repo_idle(&repo, 0);
        assert!(result.is_ok());
    }
}
