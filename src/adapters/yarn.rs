// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Yarn adapter implementation.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command};
use anyhow::Result;
use std::path::Path;

/// Yarn package manager adapter.
pub struct Yarn;

impl PackageManager for Yarn {
    /// Returns the name of the package manager.
    fn name(&self) -> &'static str {
        "yarn"
    }

    /// Detects if the project uses yarn by checking for `yarn.lock`.
    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("yarn.lock").exists()
    }

    /// Returns the bloat directories for yarn (node_modules).
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

    /// Enforces the lockfile without installing anything.
    ///
    /// `--mode update-lockfile` resolves `package.json` and writes only `yarn.lock`.
    /// It is a Yarn Berry (2+) flag; Yarn Classic rejects it. Classic offers no
    /// resolve-only mode at all — its nearest equivalent, `yarn install
    /// --frozen-lockfile`, performs a full install and runs every dependency's
    /// lifecycle scripts, which is not something to do as a precondition for deleting
    /// that same tree. So on Classic an existing `yarn.lock` is itself the proof that
    /// `node_modules` is rebuildable, and that is what we require.
    ///
    /// On Berry, `--immutable` is what keeps the resolution read-only: it fails when
    /// `yarn.lock` would change rather than writing the change out.
    fn enforce_lockfile(&self, project_dir: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = project_dir.join("yarn.lock");
        let berry = enforce_two_tier(
            &lockfile,
            "yarn",
            &["install", "--immutable", "--mode", "update-lockfile"],
            &["install", "--mode", "update-lockfile"],
            project_dir,
            policy,
        );
        if berry.is_err() && !lockfile.exists() {
            anyhow::bail!(
                "`yarn install --mode update-lockfile` failed and there is no \
                 `yarn.lock` to fall back on. Cannot prove `node_modules` is \
                 rebuildable — run `yarn install` and commit the lockfile first."
            );
        }
        Ok(())
    }

    /// Restores the dependencies using the lockfile.
    fn restore(&self, project_dir: &Path) -> Result<()> {
        run_command("yarn", &["install", "--immutable"], project_dir)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["yarn.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        assert_eq!(Yarn.name(), "yarn");
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("yarn.lock")).unwrap();
        assert!(Yarn.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        assert!(!Yarn.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let bloat = Yarn.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].path, dir.path().join("node_modules"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let bloat = Yarn.bloat_dirs(dir.path());
        assert!(bloat.is_empty());
    }
}
