// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// NPM adapter implementation.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::Result;
use std::fs;
use std::path::Path;

/// NPM package manager adapter.
pub struct Npm;

/// Refuse when `node_modules` holds packages `package-lock.json` never recorded.
///
/// `npm install --no-save` puts a package in the tree without touching the lockfile,
/// and `npm link` plants a symlink to a package that lives outside the project. Both
/// survive `npm ci --dry-run` — it checks the lockfile against `package.json`, not
/// against the tree — and neither comes back after deletion. npm writes its own record
/// of what it actually installed to `node_modules/.package-lock.json`; comparing that
/// against the real lockfile catches the `--no-save` case, and a scan for symlinked
/// entries catches `npm link`.
fn check_unrecorded_installs(project_dir: &Path) -> Result<()> {
    let node_modules = project_dir.join("node_modules");

    // `npm link` first: a symlinked package is outside the tree entirely, so the
    // hidden lockfile comparison below would not see it. Dot-entries are skipped —
    // `.bin` is symlinks by design.
    let mut linked: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&node_modules) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let is_link = |p: &Path| {
                fs::symlink_metadata(p)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
            };
            if is_link(&entry.path()) {
                linked.push(name);
            } else if name.starts_with('@') {
                // Scoped packages sit one level down: `@scope/pkg`.
                if let Ok(scoped) = fs::read_dir(entry.path()) {
                    for pkg in scoped.flatten() {
                        if is_link(&pkg.path()) {
                            linked.push(format!("{name}/{}", pkg.file_name().to_string_lossy()));
                        }
                    }
                }
            }
        }
    }
    if !linked.is_empty() {
        linked.sort();
        anyhow::bail!(
            "`{}` contains npm-linked package(s) ({}) — symlinks to code that lives \
             outside this project. `npm ci` after deletion would not re-link them. \
             Run `npm unlink` for each, or install them normally, then retry.",
            node_modules.display(),
            linked.join(", ")
        );
    }

    let extras = no_save_extras(project_dir);
    if extras.is_empty() {
        return Ok(());
    }
    let shown = extras
        .iter()
        .take(10)
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if extras.len() > 10 {
        format!(", … and {} more", extras.len() - 10)
    } else {
        String::new()
    };
    anyhow::bail!(
        "`node_modules` holds {} package(s) that package-lock.json does not record \
         ({shown}{suffix}) — likely installed with `npm install --no-save`. `npm ci` \
         after deletion would not bring them back. Run `npm install <pkg>` to save \
         them (or `npm install` to sync), then retry.",
        extras.len()
    );
}

/// The `--no-save` case, as data: entries npm's own install record
/// (`node_modules/.package-lock.json`) knows about that the committed lockfile does
/// not, sorted. Either file missing or unparseable means there is nothing to compare —
/// not evidence of drift — and answers empty.
fn no_save_extras(project_dir: &Path) -> Vec<String> {
    let package_names = |path: &Path| -> Option<std::collections::HashSet<String>> {
        let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        Some(
            json.get("packages")?
                .as_object()?
                .keys()
                .filter(|k| !k.is_empty())
                .cloned()
                .collect(),
        )
    };
    let (Some(installed), Some(recorded)) = (
        package_names(&project_dir.join("node_modules").join(".package-lock.json")),
        package_names(&project_dir.join("package-lock.json")),
    ) else {
        return Vec::new();
    };
    let mut extras: Vec<String> = installed.difference(&recorded).cloned().collect();
    extras.sort();
    extras
}

impl PackageManager for Npm {
    /// Returns the name of the package manager.
    fn name(&self) -> &'static str {
        "npm"
    }

    /// Detects if the project uses npm by checking for `package-lock.json`.
    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("package-lock.json").exists()
    }

    /// Returns the bloat directories for npm (node_modules).
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

    /// Enforces the lockfile without running install scripts, and without writing.
    ///
    /// `npm ci --dry-run` is the read-only check: it builds the tree the lockfile
    /// describes, fails outright when `package-lock.json` and `package.json` disagree,
    /// and — with `--dry-run` — neither installs nor writes. `--package-lock-only` is
    /// the writing form, kept for the no-lockfile case where there is nothing to
    /// preserve, and for the user who opted into rewriting.
    fn enforce_lockfile(&self, project_dir: &Path, policy: EnforcePolicy) -> Result<()> {
        check_unrecorded_installs(project_dir)?;
        let lockfile = project_dir.join("package-lock.json");
        enforce_two_tier(
            &lockfile,
            "npm",
            &["ci", "--dry-run", "--ignore-scripts"],
            &["install", "--package-lock-only", "--ignore-scripts"],
            project_dir,
            policy,
        )
    }

    /// Restores the dependencies using the lockfile.
    fn restore(&self, project_dir: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("npm", &["ci"], project_dir, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["package-lock.json"]
    }

    /// The `--no-save` comparison `enforce_lockfile` refuses on, as data. npm-linked
    /// packages are deliberately not listed here: a symlink to code outside the project
    /// is not something any lockfile edit can record, so it stays a prune-time refusal.
    fn drift(&self, project_dir: &Path) -> Vec<super::DriftReport> {
        let extras = no_save_extras(project_dir);
        if extras.is_empty() {
            return Vec::new();
        }
        vec![super::DriftReport {
            directory: "node_modules".to_string(),
            unrecorded: extras,
            record_command: "npm install <pkg> (or `npm install` to sync the lockfile)",
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        assert_eq!(Npm.name(), "npm");
    }

    /// npm used to run `npm install --package-lock-only` here, which *fixes* a lockfile
    /// that has drifted from `package.json` by rewriting it — during a pass that may
    /// have been started by the scheduler. Verification must now refuse instead.
    ///
    /// Skipped rather than failed when `npm` is absent from `PATH`.
    #[test]
    fn a_default_pass_never_rewrites_a_stale_lockfile() {
        if !super::super::binary_available("npm") {
            return;
        }
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"stale","version":"1.0.0","dependencies":{"left-pad":"^1.3.0"}}"#,
        )
        .unwrap();

        // A lockfile that never heard of `left-pad`, so it cannot rebuild the tree.
        let stale = r#"{"name":"stale","version":"1.0.0","lockfileVersion":3,"requires":true,"packages":{"":{"name":"stale","version":"1.0.0"}}}"#;
        fs::write(dir.path().join("package-lock.json"), stale).unwrap();

        let result = Npm.enforce_lockfile(dir.path(), EnforcePolicy::default());

        assert!(
            result.is_err(),
            "a lockfile out of sync with package.json must not pass verification"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("package-lock.json")).unwrap(),
            stale,
            "the read-only verification rewrote package-lock.json"
        );
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("package-lock.json")).unwrap();
        assert!(Npm.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        assert!(!Npm.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let bloat = Npm.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].path, dir.path().join("node_modules"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let bloat = Npm.bloat_dirs(dir.path());
        assert!(bloat.is_empty());
    }

    #[test]
    fn drift_reports_the_no_save_install_as_data() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package-lock.json"),
            r#"{"packages":{"":{},"node_modules/left-pad":{}}}"#,
        )
        .unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir(&nm).unwrap();
        fs::write(
            nm.join(".package-lock.json"),
            r#"{"packages":{"":{},"node_modules/left-pad":{},"node_modules/sneaky":{}}}"#,
        )
        .unwrap();

        let reports = Npm.drift(dir.path());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].directory, "node_modules");
        assert_eq!(reports[0].unrecorded, vec!["node_modules/sneaky"]);
    }

    /// A missing hidden lockfile means npm never recorded what it installed — that is
    /// "nothing to compare", not drift.
    #[test]
    fn drift_is_silent_without_npms_own_install_record() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package-lock.json"),
            r#"{"packages":{"":{}}}"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();

        assert!(Npm.drift(dir.path()).is_empty());
    }
}
