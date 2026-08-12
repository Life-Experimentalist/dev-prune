// Bun adapter implementation.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, lock_sync_or_verify_with_timeout,
    run_command,
};
use anyhow::Result;
use std::path::Path;

/// Bun package manager adapter.
pub struct Bun;

impl PackageManager for Bun {
    /// Returns the name of the package manager.
    fn name(&self) -> &'static str {
        "bun"
    }

    /// Detects if the project uses bun by checking for `bun.lockb` or `bun.lock`.
    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("bun.lockb").exists() || project_dir.join("bun.lock").exists()
    }

    /// Returns the bloat directories for bun (node_modules).
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

    /// Verifies the lockfile resolves cleanly, without installing anything.
    ///
    /// `--dry-run` is essential, not cosmetic. A plain `bun install --frozen-lockfile`
    /// is a real install: it downloads every dependency and runs their lifecycle
    /// scripts — third-party code executed as a precondition for *deleting* the very
    /// tree it just built. With `--dry-run`, bun still resolves `package.json` against
    /// the lockfile and fails when they disagree, but writes nothing.
    ///
    /// bun is the one manager whose natural check was already read-only, so there is no
    /// writing form to opt into and `allow_rewrite` changes nothing here.
    fn enforce_lockfile(&self, project_dir: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = if project_dir.join("bun.lockb").exists() {
            project_dir.join("bun.lockb")
        } else {
            project_dir.join("bun.lock")
        };
        lock_sync_or_verify_with_timeout(
            &lockfile,
            "bun",
            &[
                "install",
                "--frozen-lockfile",
                "--dry-run",
                "--ignore-scripts",
            ],
            project_dir,
            policy.timeout,
        )
    }

    /// Restores the dependencies using the lockfile.
    fn restore(&self, project_dir: &Path) -> Result<()> {
        run_command("bun", &["install", "--frozen-lockfile"], project_dir)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["bun.lockb", "bun.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        assert_eq!(Bun.name(), "bun");
    }

    #[test]
    fn test_detect_positive_lockb() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("bun.lockb")).unwrap();
        assert!(Bun.detect(dir.path()));
    }

    #[test]
    fn test_detect_positive_lock() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("bun.lock")).unwrap();
        assert!(Bun.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        assert!(!Bun.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let bloat = Bun.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].path, dir.path().join("node_modules"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let bloat = Bun.bloat_dirs(dir.path());
        assert!(bloat.is_empty());
    }
}
