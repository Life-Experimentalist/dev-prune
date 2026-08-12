// PNPM adapter implementation.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command};
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
    fn restore(&self, project_dir: &Path) -> Result<()> {
        run_command("pnpm", &["install", "--frozen-lockfile"], project_dir)
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
}
