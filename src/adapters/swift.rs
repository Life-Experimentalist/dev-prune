// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Swift Package Manager adapter.
//
// Opt-in (`devp config set enable_swift true`), for the same reason as Gradle and
// Maven: `.build/` is not a dependency tree, it is a dependency tree *plus* every
// compiled module, and it comes back through `swift build` rather than a download. The
// engine also holds it to the longer `build_idle_days` window.
//
// `.swiftpm/` is deliberately not claimed — it holds editor and scheme configuration
// people commit and nothing regenerates.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Swift Package Manager adapter. Opt-in; see the module comment.
pub struct Swift;

impl PackageManager for Swift {
    fn name(&self) -> &'static str {
        "swift"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("Package.swift").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let build = path.join(".build");
        if !build.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: ".build".to_string(),
            path: build.clone(),
            size_bytes: dir_size(&build),
            shared_bytes: 0,
        }]
    }

    /// The manifest is the proof, as it is for Maven: `.build/` is derived entirely from
    /// `Package.swift`, the sources beside it and — when one exists — `Package.resolved`.
    /// Running `swift package resolve` here instead would fetch dependencies over the
    /// network in the middle of a delete pass, for no stronger answer.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join("Package.swift");
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`Package.swift` could not be read ({e}) — nothing to rebuild `.build/` from.")
        })?;
        if !content.contains("Package(") {
            return Err(anyhow!(
                "`Package.swift` declares no `Package(` — refusing to treat `.build/` as \
                 rebuildable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("SwiftPM .build/ will regenerate on the next `swift build`");
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["Package.resolved"]
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_package_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Swift.detect(dir.path()));
        fs::write(
            dir.path().join("Package.swift"),
            "// swift-tools-version:5.9",
        )
        .unwrap();
        assert!(Swift.detect(dir.path()));
    }

    #[test]
    fn claims_the_build_directory_and_not_the_swiftpm_one() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".build")).unwrap();
        fs::create_dir(dir.path().join(".swiftpm")).unwrap();
        let names: Vec<String> = Swift
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec![".build"]);
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Swift
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("Package.swift"), "let x = 1").unwrap();
        assert!(
            Swift
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(
            dir.path().join("Package.swift"),
            "let package = Package(name: \"x\")",
        )
        .unwrap();
        assert!(
            Swift
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn swift_is_opt_in() {
        assert!(Swift.opt_in());
    }
}
