// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// PDM adapter for Python projects.
//
// PDM installs either into an in-project `.venv` or, in PEP 582 mode, into
// `__pypackages__/`. Both are rebuilt by `pdm install` from `pdm.lock`, so both are
// claimed and neither is opt-in.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::Result;
use std::fs;
use std::path::Path;

/// PDM package manager adapter.
pub struct Pdm;

/// The two places PDM puts a project's dependencies.
const PDM_ENV_DIRS: [&str; 2] = [".venv", "__pypackages__"];

impl PackageManager for Pdm {
    fn name(&self) -> &'static str {
        "pdm"
    }

    /// The lockfile, or a `pyproject.toml` that names PDM as the build backend or
    /// carries PDM's own settings table. A plain PEP 621 `pyproject.toml` belongs to
    /// whichever tool actually manages it, and is not claimed here.
    fn detect(&self, path: &Path) -> bool {
        if path.join("pdm.lock").exists() {
            return true;
        }
        fs::read_to_string(path.join("pyproject.toml"))
            .is_ok_and(|c| c.contains("[tool.pdm]") || c.contains("pdm.backend"))
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        PDM_ENV_DIRS
            .iter()
            .map(|name| (name, path.join(name)))
            .filter(|(_, dir)| dir.is_dir())
            .map(|(name, dir)| BloatDir {
                name: (*name).to_string(),
                path: dir.clone(),
                size_bytes: dir_size(&dir),
                shared_bytes: 0,
            })
            .collect()
    }

    /// `pdm lock --check` resolves `pyproject.toml` against `pdm.lock` and exits
    /// non-zero when they have drifted apart, without writing either file. Plain
    /// `pdm lock` is the write side.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        enforce_two_tier(
            &path.join("pdm.lock"),
            "pdm",
            &["lock", "--check"],
            &["lock"],
            path,
            policy,
        )
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("pdm", &["install"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["pdm.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_lockfile() {
        let dir = tempdir().unwrap();
        assert!(!Pdm.detect(dir.path()));
        fs::write(dir.path().join("pdm.lock"), "").unwrap();
        assert!(Pdm.detect(dir.path()));
    }

    #[test]
    fn detects_on_a_pdm_pyproject() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[tool.pdm]\n").unwrap();
        assert!(Pdm.detect(dir.path()));
    }

    #[test]
    fn leaves_a_plain_pep621_project_alone() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"x\"\n",
        )
        .unwrap();
        assert!(!Pdm.detect(dir.path()));
    }

    #[test]
    fn claims_both_environment_layouts() {
        let dir = tempdir().unwrap();
        assert!(Pdm.bloat_dirs(dir.path()).is_empty());
        fs::create_dir(dir.path().join(".venv")).unwrap();
        fs::create_dir(dir.path().join("__pypackages__")).unwrap();
        let names: Vec<String> = Pdm
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec![".venv", "__pypackages__"]);
    }
}
