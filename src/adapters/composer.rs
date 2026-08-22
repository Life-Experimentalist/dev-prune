// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Composer adapter for PHP projects.
//
// Not opt-in, unlike the build-tool adapters: `vendor/` is a *download* restore.
// `composer install` reads `composer.lock` and puts back the exact versions recorded in
// it, the same relationship `package-lock.json` has with `node_modules`.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::Result;
use std::path::Path;

/// Composer package manager adapter.
pub struct Composer;

impl PackageManager for Composer {
    fn name(&self) -> &'static str {
        "composer"
    }

    /// The manifest, not the lockfile: a project that has never installed still has a
    /// `composer.json`, and reporting the manager it uses is useful before there is any
    /// bloat to report.
    fn detect(&self, path: &Path) -> bool {
        path.join("composer.json").exists()
    }

    /// `vendor/`, but only when it is Composer's own.
    ///
    /// Bundler configured with `bundle config set path vendor/bundle` puts its gems
    /// inside the same directory. Deleting `vendor/` in such a repository would take
    /// them with it under a proof that says nothing about them, and no
    /// `composer install` puts them back. Rare enough to decline rather than model:
    /// Bundler still claims `vendor/bundle` and prunes it under its own lockfile.
    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let vendor = path.join("vendor");
        if !vendor.is_dir() || vendor.join("bundle").is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: "vendor".to_string(),
            path: vendor.clone(),
            size_bytes: dir_size(&vendor),
            shared_bytes: 0,
        }]
    }

    /// `composer validate` is the read-only side: it writes nothing and fails when
    /// `composer.lock` is no longer in sync with `composer.json`, which is exactly the
    /// question a prune has to answer. `--no-check-publish` drops the "this package
    /// could not be published" complaints (missing `description`, `license`) and
    /// `--no-check-all` the constraint-style nags — neither has anything to do with
    /// whether the lockfile can rebuild `vendor/`.
    ///
    /// The write side is `composer update --no-install`, which re-resolves and writes
    /// the lockfile without touching `vendor/`.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        enforce_two_tier(
            &path.join("composer.lock"),
            "composer",
            &[
                "validate",
                "--no-check-publish",
                "--no-check-all",
                "--no-interaction",
            ],
            &["update", "--no-install", "--no-interaction"],
            path,
            policy,
        )
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("composer", &["install", "--no-interaction"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["composer.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Composer.detect(dir.path()));
        fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert!(Composer.detect(dir.path()));
    }

    #[test]
    fn claims_vendor_when_present() {
        let dir = tempdir().unwrap();
        assert!(Composer.bloat_dirs(dir.path()).is_empty());
        fs::create_dir(dir.path().join("vendor")).unwrap();
        let dirs = Composer.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "vendor");
    }

    #[test]
    fn declines_a_vendor_directory_bundler_is_living_in() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("vendor").join("bundle")).unwrap();
        assert!(Composer.bloat_dirs(dir.path()).is_empty());
    }
}
