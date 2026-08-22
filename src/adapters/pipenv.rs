// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Pipenv adapter for Python projects.
//
// Only the in-project environment is claimed — the `.venv` that appears when
// `PIPENV_VENV_IN_PROJECT` is set. Pipenv's default is a virtualenv in a shared
// directory under the user's home, keyed by a hash of the project path. That lives
// outside the repository and is left alone entirely: it is where other projects'
// dependencies are installed, not a cache, so no lockfile here can prove it recoverable.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::Result;
use std::path::Path;

/// Pipenv package manager adapter.
pub struct Pipenv;

impl PackageManager for Pipenv {
    fn name(&self) -> &'static str {
        "pipenv"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("Pipfile").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let venv = path.join(".venv");
        if !venv.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: ".venv".to_string(),
            path: venv.clone(),
            size_bytes: dir_size(&venv),
            shared_bytes: 0,
        }]
    }

    /// `pipenv verify` is exactly this check and nothing else: it compares the hash
    /// `Pipfile.lock` recorded against the current `Pipfile` and exits non-zero when
    /// they no longer match, writing nothing. `pipenv lock` is the write side.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        enforce_two_tier(
            &path.join("Pipfile.lock"),
            "pipenv",
            &["verify"],
            &["lock"],
            path,
            policy,
        )
    }

    /// `--deploy` refuses rather than re-resolving when the lockfile is out of date with
    /// the `Pipfile`, which is the right failure for a restore: putting back something
    /// other than what was deleted is worse than reporting that it cannot be done.
    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("pipenv", &["install", "--deploy"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["Pipfile.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_pipfile() {
        let dir = tempdir().unwrap();
        assert!(!Pipenv.detect(dir.path()));
        fs::write(dir.path().join("Pipfile"), "[packages]\n").unwrap();
        assert!(Pipenv.detect(dir.path()));
    }

    #[test]
    fn claims_only_an_in_project_environment() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Pipfile"), "").unwrap();
        assert!(Pipenv.bloat_dirs(dir.path()).is_empty());
        fs::create_dir(dir.path().join(".venv")).unwrap();
        let dirs = Pipenv.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".venv");
    }
}
