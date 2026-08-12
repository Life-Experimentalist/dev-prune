// uv package manager adapter for Python projects.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command};
use anyhow::Result;
use std::fs;
use std::path::Path;

/// Adapter for uv-based Python projects.
pub struct Uv;

impl PackageManager for Uv {
    fn name(&self) -> &'static str {
        "uv"
    }

    fn detect(&self, path: &Path) -> bool {
        let uv_lock = path.join("uv.lock");
        if uv_lock.exists() {
            return true;
        }

        let pyproject = path.join("pyproject.toml");
        if pyproject.exists() {
            if let Ok(content) = fs::read_to_string(&pyproject) {
                if content.contains("[tool.uv]") {
                    return true;
                }
            }
        }

        false
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let venv_path = path.join(".venv");
        if venv_path.exists() {
            dirs.push(BloatDir {
                name: ".venv".to_string(),
                path: venv_path.clone(),
                size_bytes: dir_size(&venv_path),
            });
        }
        dirs
    }

    /// Enforces the lockfile without writing it.
    ///
    /// `uv lock --locked` asserts that `uv.lock` is already up to date with
    /// `pyproject.toml` and exits non-zero instead of rewriting it when it is not.
    /// Plain `uv lock` is the writing form, for the case where no lockfile exists yet.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = path.join("uv.lock");
        enforce_two_tier(
            &lockfile,
            "uv",
            &["lock", "--locked"],
            &["lock"],
            path,
            policy,
        )
    }

    fn restore(&self, path: &Path) -> Result<()> {
        run_command("uv", &["sync"], path)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["uv.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        let adapter = Uv;
        assert_eq!(adapter.name(), "uv");
    }

    #[test]
    fn test_detect_positive_lock() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("uv.lock")).unwrap();

        let adapter = Uv;
        assert!(adapter.detect(dir.path()));
    }

    #[test]
    fn test_detect_positive_toml() {
        let dir = tempdir().unwrap();
        let mut file = File::create(dir.path().join("pyproject.toml")).unwrap();
        writeln!(file, "[tool.uv]").unwrap();

        let adapter = Uv;
        assert!(adapter.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();

        let adapter = Uv;
        assert!(!adapter.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".venv")).unwrap();

        let adapter = Uv;
        let dirs = adapter.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".venv");
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();

        let adapter = Uv;
        let dirs = adapter.bloat_dirs(dir.path());
        assert!(dirs.is_empty());
    }
}
