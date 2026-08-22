// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// uv package manager adapter for Python projects.

use super::venv::{BASELINE_DISTRIBUTIONS, installed_distributions, normalize_package_name};
use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Adapter for uv-based Python projects.
pub struct Uv;

/// Whether `.venv` was created by uv itself — uv stamps a `uv = <version>` key into
/// `pyvenv.cfg`. A venv without the stamp was built by some other tool, and uv can make
/// no claims about what is inside it.
fn venv_is_uv_managed(path: &Path) -> bool {
    fs::read_to_string(path.join(".venv").join("pyvenv.cfg"))
        .map(|content| {
            content.lines().any(|line| {
                line.trim_start()
                    .strip_prefix("uv")
                    .is_some_and(|rest| rest.trim_start().starts_with('='))
            })
        })
        .unwrap_or(false)
}

/// Every package name `uv.lock` pins, normalised.
///
/// `uv.lock` records the full transitive closure — including the project itself — so a
/// plain name scan is enough; no dependency graph needed. `None` when no names could be
/// read at all, in which case the caller skips the drift comparison rather than refusing
/// on a lockfile format this scan does not understand.
pub(super) fn lockfile_package_names(lockfile: &Path) -> Option<HashSet<String>> {
    let content = fs::read_to_string(lockfile).ok()?;
    let mut names = HashSet::new();
    let mut in_package = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[[package]]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                names.insert(normalize_package_name(value));
            }
            // Only the first `name` in each `[[package]]` entry is the package's own.
            in_package = false;
        }
    }
    (!names.is_empty()).then_some(names)
}

/// Installed distributions in `.venv` that `uv.lock` does not record.
///
/// Those were installed ad hoc (`uv pip install …`) and `uv sync` after deletion would
/// not bring them back — exactly what this tool promises never to lose.
pub(super) fn unlocked_packages(path: &Path, locked: &HashSet<String>) -> Vec<String> {
    let Some(installed) = installed_distributions(&path.join(".venv")) else {
        return Vec::new();
    };
    let mut extras: Vec<String> = installed
        .keys()
        .filter(|name| !locked.contains(*name) && !BASELINE_DISTRIBUTIONS.contains(&name.as_str()))
        .cloned()
        .collect();
    extras.sort();
    extras
}

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
        if pyproject.exists()
            && let Ok(content) = fs::read_to_string(&pyproject)
            && content.contains("[tool.uv]")
        {
            return true;
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
                shared_bytes: 0,
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

        // Generating a lockfile from `pyproject.toml` only proves the *declared*
        // dependencies resolve — it says nothing about what is actually installed in
        // `.venv`. When the venv was not even created by uv, `uv sync` against that
        // fresh lock could rebuild a different environment than the one deleted, so
        // refuse instead of manufacturing proof.
        if !lockfile.exists() && path.join(".venv").exists() && !venv_is_uv_managed(path) {
            return Err(anyhow!(
                "`pyproject.toml` declares `[tool.uv]` but there is no `uv.lock`, and \
                 `.venv` was not created by uv — a generated lockfile could not prove the \
                 environment's contents are recoverable. Rebuild the environment under uv \
                 first: `uv lock` then `uv sync`."
            ));
        }

        enforce_two_tier(
            &lockfile,
            "uv",
            &["lock", "--locked"],
            &["lock"],
            path,
            policy,
        )?;

        // The environment can hold packages the lockfile never recorded — a
        // `uv pip install foo` nobody wrote back. `uv.lock` pins the full transitive
        // closure, so anything installed but absent from it is recoverable from
        // nowhere, which is exactly what this tool promises never to delete.
        if let Some(locked) = lockfile_package_names(&lockfile) {
            let extras = unlocked_packages(path, &locked);
            if !extras.is_empty() {
                let shown = extras
                    .iter()
                    .take(10)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if extras.len() > 10 {
                    format!(", … and {} more", extras.len() - 10)
                } else {
                    String::new()
                };
                return Err(anyhow!(
                    "`.venv` holds {} package(s) that uv.lock does not record \
                     ({shown}{suffix}). They were installed ad hoc and `uv sync` would \
                     not bring them back. Record them first: `uv add <package>`.",
                    extras.len()
                ));
            }
        }
        Ok(())
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        self.restore_named(path, ".venv", None, timeout)
    }

    /// `uv sync --python 3.12` rebuilds on that interpreter and, unlike every other
    /// route here, *downloads* it when the machine does not have it — which is why the
    /// tag is passed straight through without an availability check first.
    fn restore_named(
        &self,
        path: &Path,
        _dir_name: &str,
        runtime: Option<&str>,
        timeout: std::time::Duration,
    ) -> Result<()> {
        match runtime.filter(|tag| super::is_valid_runtime_tag(tag)) {
            Some(tag) => run_command_with_timeout("uv", &["sync", "--python", tag], path, timeout),
            None => run_command_with_timeout("uv", &["sync"], path, timeout),
        }
    }

    /// The interpreter `.venv` was built with, so a restore can rebuild on it.
    fn runtime_tag(&self, path: &Path, dir_name: &str) -> Option<String> {
        super::venv_runtime_tag(&path.join(dir_name))
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["uv.lock"]
    }

    /// The comparison `enforce_lockfile` refuses on, as data: distributions in `.venv`
    /// that `uv.lock` does not pin.
    fn drift(&self, path: &Path) -> Vec<super::DriftReport> {
        let Some(locked) = lockfile_package_names(&path.join("uv.lock")) else {
            return Vec::new();
        };
        let extras = unlocked_packages(path, &locked);
        if extras.is_empty() {
            return Vec::new();
        }
        vec![super::DriftReport {
            directory: ".venv".to_string(),
            unrecorded: extras,
            record_command: "uv add <package>",
        }]
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

    #[test]
    fn a_uv_stamped_pyvenv_cfg_marks_the_venv_as_uv_managed() {
        let dir = tempdir().unwrap();
        let venv = dir.path().join(".venv");
        fs::create_dir(&venv).unwrap();
        let mut cfg = File::create(venv.join("pyvenv.cfg")).unwrap();
        writeln!(cfg, "home = /usr/bin").unwrap();
        writeln!(cfg, "uv = 0.5.9").unwrap();
        assert!(venv_is_uv_managed(dir.path()));
    }

    #[test]
    fn a_pip_built_venv_is_not_mistaken_for_a_uv_one() {
        let dir = tempdir().unwrap();
        let venv = dir.path().join(".venv");
        fs::create_dir(&venv).unwrap();
        // `uvloop` starts with "uv" but is not the uv stamp.
        let mut cfg = File::create(venv.join("pyvenv.cfg")).unwrap();
        writeln!(cfg, "home = /usr/bin").unwrap();
        writeln!(cfg, "uvloop = 1.0").unwrap();
        assert!(!venv_is_uv_managed(dir.path()));
    }

    #[test]
    fn lockfile_names_come_from_package_entries_only() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("uv.lock");
        fs::write(
            &lock,
            "version = 1\n\n[[package]]\nname = \"Requests\"\nversion = \"2.32.3\"\n\n\
             [package.metadata]\nname = \"not-a-package\"\n\n[[package]]\nname = \"my-proj\"\n",
        )
        .unwrap();
        let names = lockfile_package_names(&lock).unwrap();
        assert!(names.contains("requests"));
        assert!(names.contains("my-proj"));
        assert!(!names.contains("not-a-package"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn an_ad_hoc_install_missing_from_the_lockfile_is_flagged() {
        let dir = tempdir().unwrap();
        let sp = dir.path().join(".venv").join("Lib").join("site-packages");
        fs::create_dir_all(sp.join("requests-2.32.3.dist-info")).unwrap();
        fs::create_dir_all(sp.join("sneaky_pkg-1.0.dist-info")).unwrap();
        fs::create_dir_all(sp.join("pip-24.0.dist-info")).unwrap();

        let locked: HashSet<String> = ["requests".to_string()].into();
        assert_eq!(unlocked_packages(dir.path(), &locked), vec!["sneaky-pkg"]);
    }

    #[test]
    fn a_foreign_venv_next_to_tool_uv_without_a_lock_is_refused() {
        let dir = tempdir().unwrap();
        let mut file = File::create(dir.path().join("pyproject.toml")).unwrap();
        writeln!(file, "[tool.uv]").unwrap();
        let venv = dir.path().join(".venv");
        fs::create_dir(&venv).unwrap();
        File::create(venv.join("pyvenv.cfg")).unwrap();

        let err = Uv
            .enforce_lockfile(dir.path(), EnforcePolicy::default())
            .unwrap_err();
        assert!(err.to_string().contains("not created by uv"));
    }

    #[test]
    fn drift_reports_the_ad_hoc_install_as_data() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("uv.lock"),
            "[[package]]\nname = \"requests\"\nversion = \"2.32.3\"\n",
        )
        .unwrap();
        let sp = dir.path().join(".venv").join("Lib").join("site-packages");
        fs::create_dir_all(sp.join("requests-2.32.3.dist-info")).unwrap();
        fs::create_dir_all(sp.join("sneaky_pkg-1.0.dist-info")).unwrap();

        let reports = Uv.drift(dir.path());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].directory, ".venv");
        assert_eq!(reports[0].unrecorded, vec!["sneaky-pkg"]);
        assert_eq!(reports[0].record_command, "uv add <package>");
    }

    #[test]
    fn drift_is_silent_when_the_lockfile_records_everything() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("uv.lock"),
            "[[package]]\nname = \"requests\"\nversion = \"2.32.3\"\n",
        )
        .unwrap();
        let sp = dir.path().join(".venv").join("Lib").join("site-packages");
        fs::create_dir_all(sp.join("requests-2.32.3.dist-info")).unwrap();

        assert!(Uv.drift(dir.path()).is_empty());
    }
}
