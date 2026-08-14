// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// PNPM adapter implementation.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size_with_hardlinks, enforce_two_tier,
    run_command_with_timeout,
};
use anyhow::Result;
use std::path::Path;

/// PNPM package manager adapter.
pub struct Pnpm;

impl PackageManager for Pnpm {
    /// Returns the name of the package manager.
    fn name(&self) -> &'static str {
        "pnpm"
    }

    /// Detects if the project uses pnpm by checking for `pnpm-lock.yaml`.
    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("pnpm-lock.yaml").exists()
    }

    /// Returns the bloat directories for pnpm (node_modules).
    ///
    /// pnpm does not copy packages into `node_modules` — it hardlinks them out of its
    /// content-addressable store whenever the store and the project sit on the same
    /// volume (NTFS included; Windows hardlinks work fine). Deleting such a tree frees
    /// only pnpm's own metadata and any genuinely copied files: the store keeps every
    /// linked byte. Counting apparent size here would promise gigabytes and deliver
    /// megabytes, so the split is measured per file via the link count.
    fn bloat_dirs(&self, project_dir: &Path) -> Vec<BloatDir> {
        let node_modules = project_dir.join("node_modules");
        if node_modules.exists() {
            let size = dir_size_with_hardlinks(&node_modules);
            vec![BloatDir {
                name: "node_modules".to_string(),
                path: node_modules,
                size_bytes: size.freed_bytes,
                shared_bytes: size.shared_bytes,
            }]
        } else {
            vec![]
        }
    }

    /// Enforces the lockfile without installing anything, and without writing.
    ///
    /// `--lockfile-only` resolves `package.json` and touches no `node_modules`, but on
    /// its own it *writes* the resolution back to `pnpm-lock.yaml`. `--frozen-lockfile`
    /// turns that write into a failure, which is the answer we actually want: a lockfile
    /// that no longer matches the manifest cannot rebuild the tree we are about to
    /// delete, so the prune should be refused rather than the file quietly fixed.
    fn enforce_lockfile(&self, project_dir: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = project_dir.join("pnpm-lock.yaml");
        enforce_two_tier(
            &lockfile,
            "pnpm",
            &["install", "--lockfile-only", "--frozen-lockfile"],
            &["install", "--lockfile-only"],
            project_dir,
            policy,
        )
    }

    /// Restores the dependencies using the lockfile.
    fn restore(&self, project_dir: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout(
            "pnpm",
            &["install", "--frozen-lockfile"],
            project_dir,
            timeout,
        )
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["pnpm-lock.yaml"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        assert_eq!(Pnpm.name(), "pnpm");
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("pnpm-lock.yaml")).unwrap();
        assert!(Pnpm.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        assert!(!Pnpm.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let bloat = Pnpm.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].path, dir.path().join("node_modules"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let bloat = Pnpm.bloat_dirs(dir.path());
        assert!(bloat.is_empty());
    }

    #[test]
    fn test_bloat_dirs_excludes_store_hardlinks() {
        // A miniature pnpm layout: one file hardlinked from a "store" outside
        // node_modules, one file pnpm wrote outright. Only the second is freed by
        // deleting the tree.
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let node_modules = dir.path().join("node_modules");
        fs::create_dir(&store).unwrap();
        fs::create_dir(&node_modules).unwrap();
        fs::write(store.join("pkg.js"), "0123456789").unwrap();
        fs::hard_link(store.join("pkg.js"), node_modules.join("pkg.js")).unwrap();
        fs::write(node_modules.join(".modules.yaml"), "y").unwrap();
        let bloat = Pnpm.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].size_bytes, 1);
        assert_eq!(bloat[0].shared_bytes, 10);
    }
}
