// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// pixi package manager adapter.
//
// pixi installs a project's conda and PyPI environments into `.pixi/` beside the
// manifest, pinned exactly by `pixi.lock`, and `pixi install` rebuilds the whole
// directory from that lockfile. A regular adapter, not opt-in: `.pixi/` is downloaded
// dependencies, not compiler output, the same trade as `node_modules` or a uv `.venv`.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, run_command_with_timeout};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Adapter for pixi-managed projects.
pub struct Pixi;

impl PackageManager for Pixi {
    fn name(&self) -> &'static str {
        "pixi"
    }

    fn detect(&self, path: &Path) -> bool {
        if path.join("pixi.toml").is_file() || path.join("pixi.lock").is_file() {
            return true;
        }
        // pixi can also live inside pyproject.toml, under its own tool table.
        let pyproject = path.join("pyproject.toml");
        pyproject.is_file()
            && fs::read_to_string(&pyproject)
                .map(|content| content.contains("[tool.pixi"))
                .unwrap_or(false)
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let pixi_dir = path.join(".pixi");
        if pixi_dir.is_dir() {
            dirs.push(BloatDir {
                name: ".pixi".to_string(),
                size_bytes: dir_size(&pixi_dir),
                path: pixi_dir,
                shared_bytes: 0,
            });
        }
        dirs
    }

    /// The check is a static read of `pixi.lock`, not `pixi lock --check`: re-solving
    /// the environments can reach the network, and the middle of a delete pass is the
    /// wrong moment to depend on a resolver being reachable. What the read establishes
    /// is that the lockfile is the format it claims (a `version:` line) and actually
    /// pins something (at least one conda or PyPI entry) — an empty or foreign file
    /// proves nothing about `.pixi/` being rebuildable.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let lockfile = path.join("pixi.lock");
        let content = fs::read_to_string(&lockfile).map_err(|e| {
            anyhow!(
                "`pixi.lock` could not be read ({e}): nothing proves the environments are \
                 recoverable. Write one first: `pixi lock`."
            )
        })?;
        let has_version = content
            .lines()
            .any(|line| line.trim_start().starts_with("version:"));
        let has_package = content.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("- conda") || line.starts_with("- pypi")
        });
        if !has_version || !has_package {
            return Err(anyhow!(
                "`pixi.lock` does not read as a pixi lockfile pinning any packages: \
                 refusing to treat `.pixi` as recoverable from it. Regenerate it: \
                 `pixi lock`."
            ));
        }
        Ok(())
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("pixi", &["install"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["pixi.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A plausible `pixi.lock` pinning one conda package.
    const LOCKFILE: &str = "version: 6\nenvironments:\n  default:\n    channels:\n\
                            - url: https://conda.anaconda.org/conda-forge/\n    packages:\n\
                            - conda: https://conda.anaconda.org/conda-forge/noarch/tzdata-2025a.conda\n";

    #[test]
    fn detects_on_manifest_lockfile_or_pyproject_table() {
        let dir = tempdir().unwrap();
        assert!(!Pixi.detect(dir.path()));

        fs::write(dir.path().join("pixi.toml"), "[project]\n").unwrap();
        assert!(Pixi.detect(dir.path()));
        fs::remove_file(dir.path().join("pixi.toml")).unwrap();

        fs::write(dir.path().join("pixi.lock"), LOCKFILE).unwrap();
        assert!(Pixi.detect(dir.path()));
        fs::remove_file(dir.path().join("pixi.lock")).unwrap();

        fs::write(dir.path().join("pyproject.toml"), "[tool.pixi.project]\n").unwrap();
        assert!(Pixi.detect(dir.path()));
    }

    #[test]
    fn a_plain_pyproject_is_not_claimed() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]\n").unwrap();
        assert!(!Pixi.detect(dir.path()));
    }

    #[test]
    fn the_environment_directory_is_claimed_and_nothing_else() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".pixi")).unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();

        let dirs = Pixi.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".pixi");
    }

    #[test]
    fn a_missing_or_empty_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Pixi.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(
            dir.path().join("pixi.lock"),
            "version: 6\nenvironments: {}\n",
        )
        .unwrap();
        assert!(Pixi.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join("pixi.lock"), LOCKFILE).unwrap();
        assert!(Pixi.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn a_foreign_file_under_the_lockfile_name_is_refused() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pixi.lock"), "hello there\n").unwrap();
        assert!(
            Pixi.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
    }

    #[test]
    fn pixi_is_not_opt_in() {
        assert!(!Pixi.opt_in());
    }
}
