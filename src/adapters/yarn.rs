// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Yarn adapter implementation.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::Result;
use std::path::Path;

/// Yarn package manager adapter.
pub struct Yarn;

/// Whether the project is on Yarn Berry (2+) rather than Classic (1.x).
///
/// Berry projects carry a `.yarnrc.yml` or a `.yarn/` directory, and their lockfiles
/// open with a `__metadata:` block that Classic's `# yarn lockfile v1` format never
/// contains. Checked from the project's files rather than `yarn --version`, because the
/// globally installed yarn is routinely Classic while the project pins Berry through
/// Corepack.
fn is_berry_project(project_dir: &Path) -> bool {
    if project_dir.join(".yarnrc.yml").exists() || project_dir.join(".yarn").is_dir() {
        return true;
    }
    std::fs::read_to_string(project_dir.join("yarn.lock"))
        .map(|c| c.lines().take(30).any(|l| l.starts_with("__metadata:")))
        .unwrap_or(false)
}

impl PackageManager for Yarn {
    /// Returns the name of the package manager.
    fn name(&self) -> &'static str {
        "yarn"
    }

    /// Detects if the project uses yarn by checking for `yarn.lock`.
    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("yarn.lock").exists()
    }

    /// Returns the bloat directories for yarn (node_modules).
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

    /// Enforces the lockfile without installing anything.
    ///
    /// `--mode update-lockfile` resolves `package.json` and writes only `yarn.lock`.
    /// It is a Yarn Berry (2+) flag; Yarn Classic rejects it. Classic offers no
    /// resolve-only mode at all — its nearest equivalent, `yarn install
    /// --frozen-lockfile`, performs a full install and runs every dependency's
    /// lifecycle scripts, which is not something to do as a precondition for deleting
    /// that same tree. So on Classic an existing `yarn.lock` is itself the proof that
    /// `node_modules` is rebuildable, and that is what we require.
    ///
    /// On Berry, `--immutable` is what keeps the resolution read-only: it fails when
    /// `yarn.lock` would change rather than writing the change out — and that failure
    /// must reach the caller. An earlier version of this method decided Classic-vs-Berry
    /// by whether the Berry invocation errored, which swallowed every genuine Berry
    /// verification failure and made this the one adapter that could never say no.
    fn enforce_lockfile(&self, project_dir: &Path, policy: EnforcePolicy) -> Result<()> {
        // detect() required yarn.lock to exist, so on Classic the lockfile-as-proof
        // tier is already satisfied.
        if !is_berry_project(project_dir) {
            return Ok(());
        }
        enforce_two_tier(
            &project_dir.join("yarn.lock"),
            "yarn",
            &["install", "--immutable", "--mode", "update-lockfile"],
            &["install", "--mode", "update-lockfile"],
            project_dir,
            policy,
        )
    }

    /// Restores the dependencies using the lockfile. The two lines of yarn spell
    /// "install exactly what the lockfile says" differently, and each rejects the
    /// other's flag.
    fn restore(&self, project_dir: &Path, timeout: std::time::Duration) -> Result<()> {
        if is_berry_project(project_dir) {
            run_command_with_timeout("yarn", &["install", "--immutable"], project_dir, timeout)
        } else {
            run_command_with_timeout(
                "yarn",
                &["install", "--frozen-lockfile"],
                project_dir,
                timeout,
            )
        }
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["yarn.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        assert_eq!(Yarn.name(), "yarn");
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("yarn.lock")).unwrap();
        assert!(Yarn.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        assert!(!Yarn.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let bloat = Yarn.bloat_dirs(dir.path());
        assert_eq!(bloat.len(), 1);
        assert_eq!(bloat[0].path, dir.path().join("node_modules"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let bloat = Yarn.bloat_dirs(dir.path());
        assert!(bloat.is_empty());
    }

    #[test]
    fn a_classic_lockfile_alone_is_not_berry() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile v1\n").unwrap();
        assert!(!is_berry_project(dir.path()));
    }

    #[test]
    fn a_yarnrc_yml_marks_the_project_as_berry() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile v1\n").unwrap();
        fs::File::create(dir.path().join(".yarnrc.yml")).unwrap();
        assert!(is_berry_project(dir.path()));
    }

    #[test]
    fn a_dot_yarn_directory_marks_the_project_as_berry() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile v1\n").unwrap();
        fs::create_dir(dir.path().join(".yarn")).unwrap();
        assert!(is_berry_project(dir.path()));
    }

    #[test]
    fn a_metadata_block_in_the_lockfile_marks_the_project_as_berry() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
        assert!(is_berry_project(dir.path()));
    }

    // On Classic the lockfile's existence is the whole proof — no yarn binary runs, so
    // this passes on a machine with no yarn at all. Berry is the branch that shells
    // out, and the one whose failures must reach the caller (the old version swallowed
    // them by treating any Berry error as "must be Classic then").
    #[test]
    fn enforce_on_classic_needs_no_yarn_binary() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("yarn.lock"),
            "# yarn lockfile v1\n\nleft-pad@^1.3.0:\n  version \"1.3.0\"\n",
        )
        .unwrap();
        assert!(
            Yarn.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }
}
