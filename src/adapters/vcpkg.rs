// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// vcpkg adapter, for C and C++ projects in manifest mode.
//
// Opt-in, for the same reason as Cargo, Gradle, Maven and SwiftPM: `vcpkg_installed/`
// is not a downloaded dependency tree. vcpkg builds every port from source, so what is
// in there is headers and compiled libraries, and `vcpkg install` puts them back by
// compiling them again — Boost or Qt is an afternoon, not a download. The binary cache
// beside the vcpkg installation often turns that back into a copy, but nothing here can
// prove it holds an archive matching this project's triplet and ABI, so the adapter
// assumes the expensive answer and the engine holds it to `build_idle_days`.
//
// Manifest mode only, which is the mode that puts anything inside a repository at all:
// classic mode installs into one tree beside vcpkg itself, shared by every project on
// the machine. `devp caches` reports that one instead.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// vcpkg adapter. Opt-in; see the module comment.
pub struct Vcpkg;

impl PackageManager for Vcpkg {
    fn name(&self) -> &'static str {
        "vcpkg"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("vcpkg.json").exists()
    }

    /// `vcpkg_installed/`, which vcpkg creates beside the manifest it read. `build/` next
    /// to it belongs to CMake, and the name alone never says whose it is.
    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let installed = path.join("vcpkg_installed");
        if !installed.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: "vcpkg_installed".to_string(),
            path: installed.clone(),
            size_bytes: dir_size(&installed),
            shared_bytes: 0,
        }]
    }

    /// The manifest is the proof, as it is for Maven and SwiftPM — with one extra
    /// condition. `vcpkg.json` is also the file every *port* carries, and a port manifest
    /// describes a package rather than an installation: nothing rebuilds a
    /// `vcpkg_installed/` from it. What separates the two is a `dependencies` list, so
    /// that is what is checked. A manifest declaring nothing to install cannot account
    /// for the directory beside it either, which is the same refusal for a different
    /// reason.
    ///
    /// Running `vcpkg install --dry-run` here instead would need a registry checkout and
    /// a network fetch in the middle of a delete pass, for no stronger answer than "the
    /// file that rebuilds this is present and names dependencies".
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join("vcpkg.json");
        let raw = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!(
                "`vcpkg.json` could not be read ({e}) — nothing to rebuild `vcpkg_installed/` from."
            )
        })?;
        let json: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            anyhow!(
                "`vcpkg.json` is not valid JSON ({e}) — `vcpkg install` could not read it either."
            )
        })?;
        let declares_dependencies = json
            .get("dependencies")
            .and_then(|d| d.as_array())
            .is_some_and(|d| !d.is_empty());
        if !declares_dependencies {
            return Err(anyhow!(
                "`vcpkg.json` declares no `dependencies` — refusing to treat \
                 `vcpkg_installed/` as rebuildable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("vcpkg vcpkg_installed/ will regenerate on the next `vcpkg install`");
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["vcpkg.json"]
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn manifest(dir: &Path, body: &str) {
        fs::write(dir.join("vcpkg.json"), body).unwrap();
    }

    fn enforced(dir: &Path) -> Result<()> {
        Vcpkg.enforce_lockfile(dir, EnforcePolicy::default())
    }

    #[test]
    fn detects_on_the_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Vcpkg.detect(dir.path()));
        manifest(dir.path(), r#"{"dependencies":["fmt"]}"#);
        assert!(Vcpkg.detect(dir.path()));
    }

    #[test]
    fn claims_the_manifest_install_tree_and_nothing_else() {
        // `build/` beside a vcpkg manifest is CMake's, and a directory named `build` is
        // as often a hand-written one as a generated one. Claiming it on the name would
        // be the guess this project does not make.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("vcpkg_installed")).unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        let names: Vec<String> = Vcpkg
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["vcpkg_installed"]);
    }

    #[test]
    fn a_port_manifest_is_not_an_installation() {
        // Every vcpkg *port* carries a `vcpkg.json` too. It describes a package rather
        // than an install root, so nothing rebuilds a `vcpkg_installed/` from it and a
        // port directory that happens to hold one must not be pruned under it.
        let dir = tempdir().unwrap();
        manifest(dir.path(), r#"{"name":"fmt","version":"10.1.1"}"#);
        assert!(enforced(dir.path()).is_err());
    }

    #[test]
    fn a_missing_empty_or_unreadable_manifest_is_refused() {
        let dir = tempdir().unwrap();
        assert!(enforced(dir.path()).is_err(), "no manifest at all");
        manifest(dir.path(), "{ not json");
        assert!(enforced(dir.path()).is_err(), "unparseable manifest");
        manifest(dir.path(), r#"{"dependencies":[]}"#);
        assert!(
            enforced(dir.path()).is_err(),
            "an empty dependency list rebuilds nothing"
        );
    }

    #[test]
    fn a_manifest_with_dependencies_is_the_proof() {
        let dir = tempdir().unwrap();
        manifest(
            dir.path(),
            r#"{"dependencies":["fmt","zlib"],
                "builtin-baseline":"3426db05b996481ca31e95fff3734cf23e0f51bc"}"#,
        );
        assert!(enforced(dir.path()).is_ok());

        // The object form is what a dependency with features or a host requirement looks
        // like, and it is commoner than the bare string in manifests that pull anything
        // substantial.
        manifest(
            dir.path(),
            r#"{"dependencies":[{"name":"boost-asio","features":["ssl"]}]}"#,
        );
        assert!(enforced(dir.path()).is_ok());
    }

    #[test]
    fn vcpkg_is_opt_in() {
        assert!(Vcpkg.opt_in());
    }
}
