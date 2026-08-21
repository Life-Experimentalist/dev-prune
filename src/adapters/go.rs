// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Go package manager adapter.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::path::Path;

/// Adapter for Go modules.
pub struct Go;

/// Whether `vendor/` carries uncommitted changes Git knows about.
///
/// A team that commits its vendor tree sometimes patches a dependency in place. Until
/// that patch is committed it exists nowhere but the worktree, and `go mod vendor` would
/// regenerate the tree from the module cache without it. Untracked entries (`??`) are
/// not counted: an untracked vendor tree is the ordinary gitignored-or-fresh case, and
/// `vendor/modules.txt` already vouches for how it was built.
/// This is the one refusal in this adapter that guards real data, so it fails
/// closed: when git cannot answer (missing binary, dubious-ownership refusal), the
/// answer is an error, not "no changes" — the old fail-open reading deleted a
/// patched vendor tree precisely when git was least able to vouch for it.
fn vendor_has_uncommitted_changes(path: &Path) -> Result<bool> {
    let output = crate::scanner::git::git_in(path)
        .args(["status", "--porcelain", "--", "vendor"])
        .output()
        .map_err(|e| anyhow!("could not run `git status` to check `vendor/`: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "`git status` could not inspect `vendor/` for uncommitted changes: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| !line.trim().is_empty() && !line.starts_with("??")))
}

impl PackageManager for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("go.mod").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let vendor_path = path.join("vendor");
        // Only a `go mod vendor` product carries `modules.txt`. A vendor tree without it
        // was assembled some other way, and `go mod vendor` makes no promise of
        // recreating whatever that was — so it is not this adapter's to delete.
        if vendor_path.exists() && vendor_path.join("modules.txt").exists() {
            dirs.push(BloatDir {
                name: "vendor".to_string(),
                path: vendor_path.clone(),
                size_bytes: dir_size(&vendor_path),
                shared_bytes: 0,
            });
        }
        dirs
    }

    /// `go mod tidy` reconciles `go.mod` and `go.sum` against the real imports, and can
    /// *remove* a requirement nothing imports any more — which is exactly why it is not
    /// the default. With `go.sum` present the module cache is verified instead, which
    /// never touches tracked files. `tidy` is reached only when there is no `go.sum` to
    /// bootstrap from, or when the user opted in.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        // An in-place patch to a committed vendor tree exists nowhere but the worktree;
        // `go mod vendor` after deletion would rebuild the tree from the module cache
        // without it.
        if path.join("vendor").exists() && vendor_has_uncommitted_changes(path)? {
            return Err(anyhow!(
                "`vendor/` has uncommitted changes — deleting it would lose them, and \
                 `go mod vendor` would rebuild the tree without them. Commit or stash \
                 the changes first."
            ));
        }
        enforce_two_tier(
            &path.join("go.sum"),
            "go",
            &["mod", "download"],
            &["mod", "tidy"],
            path,
            policy,
        )
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        if path.join("vendor").exists() {
            run_command_with_timeout("go", &["mod", "vendor"], path, timeout)
        } else {
            run_command_with_timeout("go", &["mod", "download"], path, timeout)
        }
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["go.sum"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        let adapter = Go;
        assert_eq!(adapter.name(), "go");
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("go.mod")).unwrap();

        let adapter = Go;
        assert!(adapter.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();

        let adapter = Go;
        assert!(!adapter.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("vendor")).unwrap();
        File::create(dir.path().join("vendor").join("modules.txt")).unwrap();

        let adapter = Go;
        let dirs = adapter.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "vendor");
    }

    #[test]
    fn a_vendor_tree_without_modules_txt_is_not_claimed() {
        // `go mod vendor` always writes modules.txt; a tree without it was assembled by
        // hand and cannot be promised back.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("vendor")).unwrap();
        File::create(dir.path().join("vendor").join("some_pkg.go")).unwrap();

        assert!(Go.bloat_dirs(dir.path()).is_empty());
    }

    #[test]
    fn staged_vendor_changes_refuse_the_prune() {
        let dir = tempdir().unwrap();
        let vendor = dir.path().join("vendor");
        fs::create_dir(&vendor).unwrap();
        fs::write(vendor.join("modules.txt"), "# github.com/x/y v1.0.0\n").unwrap();

        // Outside a repo git cannot answer, and "cannot answer" is an error rather
        // than a silent all-clear — the refusal fails closed…
        assert!(vendor_has_uncommitted_changes(dir.path()).is_err());

        // …and inside one, a staged-but-uncommitted vendor entry is a refusal. Staging
        // is enough to move the entry past `??` without needing commit identity.
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        git(&["add", "vendor"]);
        assert!(vendor_has_uncommitted_changes(dir.path()).unwrap());
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();

        let adapter = Go;
        let dirs = adapter.bloat_dirs(dir.path());
        assert!(dirs.is_empty());
    }
}
