// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Git repository scanner and activity detection.
//
// This module provides functions to:
// - Detect valid Git repositories
// - Recursively scan directories for Git repos
// - Determine last activity time for a repository

pub mod git;

use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

/// Maximum directory depth `scan_for_repos` will descend.
///
/// Without a bound, pointing `devp init` at a home directory or a drive root walks the
/// entire filesystem. 8 levels comfortably covers `~/code/org/project/...` layouts.
const MAX_SCAN_DEPTH: usize = 8;

/// Check if a path is a valid Git repository.
///
/// `.git` is a directory in a normal clone but a *file* containing a `gitdir:` pointer
/// in linked worktrees and submodules. Accepting both means dev-prune stops silently
/// ignoring every worktree and submodule on the machine.
pub fn is_git_repo(path: &Path) -> bool {
    let dot_git = path.join(".git");
    dot_git.is_dir() || dot_git.is_file()
}

/// Recursively scan a root directory for Git repositories.
///
/// Returns a list of absolute paths to directories containing `.git`.
/// Automatically skips dot-directories (`.cache`, `.claude`, `.gemini`),
/// hidden system folders, and build bloat directories.
pub fn scan_for_repos(root: &Path) -> Result<Vec<PathBuf>> {
    let mut repos = Vec::new();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_entry(|entry| {
            // Always allow root
            if entry.path() == root {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            // Skip hidden dot-directories (e.g. .cache, .gemini, .claude, .cargo) and common bloat folders
            if name.starts_with('.')
                || matches!(
                    name.as_ref(),
                    "node_modules" | "venv" | "target" | "AppData"
                )
            {
                return false;
            }
            true
        });

    for entry in walker {
        let entry = entry?;
        if entry.file_type().is_dir() && is_git_repo(entry.path()) {
            repos.push(entry.path().to_path_buf());
        }
    }

    Ok(repos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_git_repo_true() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        assert!(is_git_repo(tmp.path()));
    }

    #[test]
    fn test_is_git_repo_false() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_git_repo(tmp.path()));
    }

    /// Linked worktrees and submodules have `.git` as a file holding a `gitdir:`
    /// pointer, not a directory. They are real repositories and must be detected.
    #[test]
    fn test_is_git_repo_worktree_gitfile() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".git"), "gitdir: /repo/.git/worktrees/wt").unwrap();
        assert!(is_git_repo(tmp.path()));
    }

    #[test]
    fn test_scan_for_repos_finds_repos() {
        let tmp = TempDir::new().unwrap();
        // Create two git repos
        let repo1 = tmp.path().join("project1");
        let repo2 = tmp.path().join("project2");
        fs::create_dir_all(repo1.join(".git")).unwrap();
        fs::create_dir_all(repo2.join(".git")).unwrap();
        // Create a non-repo directory
        fs::create_dir_all(tmp.path().join("not_a_repo")).unwrap();

        let repos = scan_for_repos(tmp.path()).unwrap();
        assert_eq!(repos.len(), 2);
    }

    #[test]
    fn test_scan_for_repos_skips_node_modules() {
        let tmp = TempDir::new().unwrap();
        // A real repo
        fs::create_dir_all(tmp.path().join("real_repo").join(".git")).unwrap();
        // A git repo inside node_modules (should be skipped)
        fs::create_dir_all(
            tmp.path()
                .join("real_repo")
                .join("node_modules")
                .join("some_pkg")
                .join(".git"),
        )
        .unwrap();

        let repos = scan_for_repos(tmp.path()).unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[test]
    fn test_scan_empty_directory() {
        let tmp = TempDir::new().unwrap();
        let repos = scan_for_repos(tmp.path()).unwrap();
        assert!(repos.is_empty());
    }
}
