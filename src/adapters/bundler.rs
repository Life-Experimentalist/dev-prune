// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Bundler adapter for Ruby projects.
//
// Only the *vendored* install is claimed. Bundler's default is a shared gem home
// outside the repository (rbenv's, rvm's, or the system one), which is not this tool's
// business and is shared with every other project on the machine. A repository only
// has gems inside it when someone ran `bundle config set path vendor/bundle`, and then
// `bundle install` puts them back from `Gemfile.lock`.
//
// `.bundle/` is deliberately not claimed: it holds that path configuration, and
// deleting it would send the next `bundle install` to the shared gem home instead of
// back where the project asked for it.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command_with_timeout,
};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Bundler package manager adapter.
pub struct Bundler;

/// Where a vendored bundle lives, relative to the repository root.
fn vendor_bundle(path: &Path) -> PathBuf {
    path.join("vendor").join("bundle")
}

impl PackageManager for Bundler {
    fn name(&self) -> &'static str {
        "bundler"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("Gemfile").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let bundle = vendor_bundle(path);
        if !bundle.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: "vendor/bundle".to_string(),
            path: bundle.clone(),
            size_bytes: dir_size(&bundle),
            shared_bytes: 0,
        }]
    }

    /// `bundle lock --check` is Bundler's own read-only answer: it resolves the
    /// `Gemfile` against `Gemfile.lock`, exits non-zero when the lockfile no longer
    /// satisfies it, and writes nothing either way. Plain `bundle lock` is the write
    /// side, reached only when there is no lockfile to preserve or the user opted into
    /// rewrites.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        enforce_two_tier(
            &path.join("Gemfile.lock"),
            "bundle",
            &["lock", "--check"],
            &["lock"],
            path,
            policy,
        )
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("bundle", &["install"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["Gemfile.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_gemfile() {
        let dir = tempdir().unwrap();
        assert!(!Bundler.detect(dir.path()));
        fs::write(dir.path().join("Gemfile"), "source :rubygems").unwrap();
        assert!(Bundler.detect(dir.path()));
    }

    #[test]
    fn claims_only_a_vendored_bundle() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Gemfile"), "").unwrap();
        assert!(Bundler.bloat_dirs(dir.path()).is_empty());
        fs::create_dir_all(vendor_bundle(dir.path())).unwrap();
        let dirs = Bundler.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "vendor/bundle");
    }

    #[test]
    fn never_claims_the_bundle_config_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".bundle")).unwrap();
        assert!(Bundler.bloat_dirs(dir.path()).is_empty());
    }
}
