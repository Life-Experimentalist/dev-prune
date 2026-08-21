// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Poetry package manager adapter for Python projects.
//
// Poetry only leaves something to prune when the virtualenv is *in-project*
// (`virtualenvs.in-project = true`, the `.venv` directory). Environments kept in
// poetry's own cache live outside the repository and are not this tool's business, so
// `bloat_dirs` simply finds nothing there.
//
// Priority: `uv` wins when both detect (see `resolve_python_conflict`); the plain
// `venv` adapter already refuses poetry projects at `detect`.

use super::uv::{lockfile_package_names, unlocked_packages};
use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Adapter for poetry-based Python projects.
pub struct Poetry;

/// Whether `pyproject.toml` declares a `[tool.poetry]` table.
///
/// A textual check rather than a TOML parse, exactly like the venv adapter's: the table
/// header only ever appears at the start of a line, and this needs a yes/no.
fn declares_poetry(path: &Path) -> bool {
    fs::read_to_string(path.join("pyproject.toml"))
        .map(|c| {
            c.lines()
                .any(|l| l.trim_start().starts_with("[tool.poetry"))
        })
        .unwrap_or(false)
}

/// The project's own package name from `pyproject.toml`, normalised.
///
/// `poetry.lock` records the dependency closure but — unlike `uv.lock` — never the
/// project itself, while `poetry install` does install the project into `.venv`. Without
/// this the drift check would flag every poetry project as holding an unrecorded copy of
/// itself.
fn project_name(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path.join("pyproject.toml")).ok()?;
    let mut in_name_table = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            // Both spellings carry the name: `[tool.poetry]` (poetry's own table) and
            // `[project]` (PEP 621, which poetry 2.x also reads).
            in_name_table = line == "[tool.poetry]" || line == "[project]";
            continue;
        }
        if !in_name_table {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(super::venv::normalize_package_name(value));
            }
        }
    }
    None
}

impl PackageManager for Poetry {
    fn name(&self) -> &'static str {
        "poetry"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("poetry.lock").exists() || declares_poetry(path)
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
    /// `poetry check --lock` asserts that `poetry.lock` is consistent with
    /// `pyproject.toml` and exits non-zero instead of rewriting anything when it is not.
    /// Plain `poetry lock` is the writing form, for the case where no lockfile exists
    /// yet — but never over an existing environment, below.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = path.join("poetry.lock");

        // Generating a lockfile from `pyproject.toml` only proves the *declared*
        // dependencies resolve — it says nothing about what is actually installed in
        // `.venv`. Unlike uv, poetry leaves no stamp in `pyvenv.cfg`, so there is no way
        // to tell whether this environment even matches the manifest. Refuse instead of
        // manufacturing proof.
        if !lockfile.exists() && path.join(".venv").exists() {
            return Err(anyhow!(
                "`pyproject.toml` declares `[tool.poetry]` but there is no `poetry.lock` \
                 — a generated lockfile could not prove the environment's contents are \
                 recoverable. Lock and rebuild it first: `poetry lock` then \
                 `poetry install`."
            ));
        }

        enforce_two_tier(
            &lockfile,
            "poetry",
            &["check", "--lock"],
            &["lock"],
            path,
            policy,
        )?;

        // The environment can hold packages the lockfile never recorded — a
        // `poetry run pip install foo` nobody wrote back. Anything installed but absent
        // from `poetry.lock` is recoverable from nowhere, which is exactly what this
        // tool promises never to delete.
        if let Some(locked) = locked_names_with_project(path, &lockfile) {
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
                    "`.venv` holds {} package(s) that poetry.lock does not record \
                     ({shown}{suffix}). They were installed ad hoc and `poetry install` \
                     would not bring them back. Record them first: `poetry add <package>`.",
                    extras.len()
                ));
            }
        }
        Ok(())
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("poetry", &["install"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["poetry.lock"]
    }

    /// The comparison `enforce_lockfile` refuses on, as data: distributions in `.venv`
    /// that `poetry.lock` does not pin.
    fn drift(&self, path: &Path) -> Vec<super::DriftReport> {
        let Some(locked) = locked_names_with_project(path, &path.join("poetry.lock")) else {
            return Vec::new();
        };
        let extras = unlocked_packages(path, &locked);
        if extras.is_empty() {
            return Vec::new();
        }
        vec![super::DriftReport {
            directory: ".venv".to_string(),
            unrecorded: extras,
            record_command: "poetry add <package>",
        }]
    }
}

/// The lockfile's package names plus the project's own, since `poetry install` installs
/// the project but `poetry.lock` never lists it.
fn locked_names_with_project(path: &Path, lockfile: &Path) -> Option<HashSet<String>> {
    let mut locked = lockfile_package_names(lockfile)?;
    if let Some(own) = project_name(path) {
        locked.insert(own);
    }
    Some(locked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn detect_positive_lock() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("poetry.lock")).unwrap();
        assert!(Poetry.detect(dir.path()));
    }

    #[test]
    fn detect_positive_toml() {
        let dir = tempdir().unwrap();
        let mut file = File::create(dir.path().join("pyproject.toml")).unwrap();
        writeln!(file, "[tool.poetry]").unwrap();
        assert!(Poetry.detect(dir.path()));
    }

    #[test]
    fn detect_negative_on_a_plain_pep621_project() {
        let dir = tempdir().unwrap();
        let mut file = File::create(dir.path().join("pyproject.toml")).unwrap();
        writeln!(file, "[project]\nname = \"plain\"").unwrap();
        assert!(!Poetry.detect(dir.path()));
    }

    #[test]
    fn bloat_dirs_only_claims_an_in_project_venv() {
        let dir = tempdir().unwrap();
        assert!(Poetry.bloat_dirs(dir.path()).is_empty());
        fs::create_dir(dir.path().join(".venv")).unwrap();
        let dirs = Poetry.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".venv");
    }

    #[test]
    fn a_venv_without_a_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        let mut file = File::create(dir.path().join("pyproject.toml")).unwrap();
        writeln!(file, "[tool.poetry]\nname = \"proj\"").unwrap();
        fs::create_dir(dir.path().join(".venv")).unwrap();

        let err = Poetry
            .enforce_lockfile(dir.path(), EnforcePolicy::default())
            .unwrap_err();
        assert!(err.to_string().contains("poetry lock"));
    }

    #[test]
    fn the_projects_own_name_is_not_drift() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.poetry]\nname = \"My_Proj\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("poetry.lock"),
            "[[package]]\nname = \"requests\"\nversion = \"2.32.3\"\n",
        )
        .unwrap();
        let sp = dir.path().join(".venv").join("Lib").join("site-packages");
        fs::create_dir_all(sp.join("requests-2.32.3.dist-info")).unwrap();
        fs::create_dir_all(sp.join("my_proj-0.1.0.dist-info")).unwrap();

        assert!(Poetry.drift(dir.path()).is_empty());
    }

    #[test]
    fn an_ad_hoc_install_is_reported_as_drift() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.poetry]\nname = \"proj\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("poetry.lock"),
            "[[package]]\nname = \"requests\"\nversion = \"2.32.3\"\n",
        )
        .unwrap();
        let sp = dir.path().join(".venv").join("Lib").join("site-packages");
        fs::create_dir_all(sp.join("requests-2.32.3.dist-info")).unwrap();
        fs::create_dir_all(sp.join("sneaky_pkg-1.0.dist-info")).unwrap();

        let reports = Poetry.drift(dir.path());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].unrecorded, vec!["sneaky-pkg"]);
        assert_eq!(reports[0].record_command, "poetry add <package>");
    }

    #[test]
    fn the_project_name_reads_from_either_table() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[build-system]\nname = \"not-this\"\n\n[project]\nname = \"pep621-name\"\n",
        )
        .unwrap();
        assert_eq!(project_name(dir.path()).as_deref(), Some("pep621-name"));
    }
}
