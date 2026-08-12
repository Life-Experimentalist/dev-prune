// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// NPM adapter implementation.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command};
use anyhow::Result;
use std::path::Path;

/// NPM package manager adapter.
pub struct Npm;

impl PackageManager for Npm {
    /// Returns the name of the package manager.
    fn name(&self) -> &'static str {
        "npm"
    }

    /// Detects if the project uses npm by checking for `package-lock.json`.
    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("package-lock.json").exists()
    }

    /// Returns the bloat directories for npm (node_modules).
    fn bloat_dirs(&self, project_dir: &Path) -> Vec<BloatDir> {
        let node_modules = project_dir.join("node_modules");
        if node_modules.exists() {
            let size = dir_size(&node_modules);
            vec![BloatDir {
                name: "node_modules".to_string(),
                path: node_modules,
                size_bytes: size,
            }]
        } else {
            vec![]
        }
    }

    /// Enforces the lockfile without running install scripts, and without writing.
    ///
    /// `npm ci --dry-run` is the read-only check: it builds the tree the lockfile
    /// describes, fails outright when `package-lock.json` and `package.json` disagree,
    /// and — with `--dry-run` — neither installs nor writes. `--package-lock-only` is
    /// the writing form, kept for the no-lockfile case where there is nothing to
    /// preserve, and for the user who opted into rewriting.
    fn enforce_lockfile(&self, project_dir: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = project_dir.join("package-lock.json");
        enforce_two_tier(
            &lockfile,
            "npm",
            &["ci", "--dry-run", "--ignore-scripts"],
            &["install", "--package-lock-only", "--ignore-scripts"],
            project_dir,
            policy,
        )
    }

    /// Restores the dependencies using the lockfile.
    fn restore(&self, project_dir: &Path) -> Result<()> {
        run_command("npm", &["ci"], project_dir)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["package-lock.json"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        assert_eq!(Npm.name(), "npm");
    }

    /// npm used to run `npm install --package-lock-only` here, which *fixes* a lockfile
    /// that has drifted from `package.json` by rewriting it — during a pass that may
    /// have been started by the scheduler. Verification must now refuse instead.
    ///
    /// Skipped rather than failed when `npm` is absent from `PATH`.
    #[test]
    fn a_default_pass_never_rewrites_a_stale_lockfile() {
        if !super::super::binary_available("npm") {
            return;
        }
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"stale","version":"1.0.0","dependencies":{"left-pad":"^1.3.0"}}"#,
        )
        .unwrap();

        // A lockfile that never heard of `left-pad`, so it cannot rebuild the tree.
        let stale = r#"{"name":"stale","version":"1.0.0","lockfileVersion":3,"requires":true,"packages":{"":{"name":"stale","version":"1.0.0"}}}"#;
        fs::write(dir.path().join("package-lock.json"), stale).unwrap();

        let result = Npm.enforce_lockfile(dir.path(), EnforcePolicy::default());

        assert!(
            result.is_err(),
            "a lockfile out of sync with package.json must not pass verification"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("package-lock.json")).unwrap(),
            stale,
            "the read-only verification rewrote package-lock.json"
        );
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("package-lock.json")).unwrap();
        assert!(Npm.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        assert!(!Npm.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let bloat = Npm.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].path, dir.path().join("node_modules"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let bloat = Npm.bloat_dirs(dir.path());
        assert!(bloat.is_empty());
    }
}
