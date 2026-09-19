// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Unreal Engine derived-data adapter.
//
// An Unreal project is anchored by a `<Name>.uproject` file at its root: a JSON
// document the launcher, the editor and the build tool all read. Beside it the engine
// writes `DerivedDataCache/` (compiled shaders, cooked texture formats) and
// `Intermediate/` (generated project files, UnrealBuildTool object output). Both are
// derived from the committed sources and both are in Epic's own recommended
// `.gitignore`; the editor and UnrealBuildTool rebuild them on the next open and the
// next build.
//
// The claim is those two names at the project root only. Never `Saved/`, which holds
// config, screenshots and local saved games nothing can regenerate; never `Binaries/`,
// whose loss turns "open the editor" into "compile the engine modules first"; and
// never a plugin's own `Intermediate/`, because the claim stops at the root. Two
// honest caveats: the *default* Derived Data Cache lives in a machine-global
// directory, so a project that never set a local DDC has little of it here to
// reclaim; and there is no fixed manifest name, so `devp doctor` reports that no
// single file identifies this manager rather than pretending one does.
//
// Opt-in, and held to `build_idle_days`: shaders recompile and object files rebuild,
// and on a large project that is hours, not a download.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

/// The derived directories at the project root. Nothing else, and never `Saved/`.
const CACHE_DIRS: &[&str] = &["DerivedDataCache", "Intermediate"];

/// The `<Name>.uproject` at this root, if there is exactly the kind of file the
/// editor itself would open. Case-insensitive on the extension because Windows is.
fn uproject_in(path: &Path) -> Option<PathBuf> {
    fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("uproject"))
        })
}

/// Unreal Engine derived-data adapter. Opt-in; see the module comment.
pub struct Unreal;

impl PackageManager for Unreal {
    fn name(&self) -> &'static str {
        "unreal"
    }

    fn detect(&self, path: &Path) -> bool {
        uproject_in(path).is_some()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        CACHE_DIRS
            .iter()
            .map(|name| path.join(name))
            .filter(|dir| dir.is_dir())
            .map(|dir| BloatDir {
                name: dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                size_bytes: dir_size(&dir),
                path: dir,
                shared_bytes: 0,
            })
            .collect()
    }

    /// The `.uproject` must still read as one: every version of the format is a JSON
    /// object carrying a `"FileVersion"` key, and a file without it is whatever it
    /// is, not a project the editor can rebuild derived data for. Running the editor
    /// or UnrealBuildTool to find out would start the build this pass exists to
    /// postpone.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = uproject_in(path).ok_or_else(|| {
            anyhow!("no `.uproject` file here: nothing to rebuild the derived data from.")
        })?;
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!(
                "`{}` could not be read ({e}): nothing to rebuild the derived data from.",
                manifest.display()
            )
        })?;
        if !content.contains("\"FileVersion\"") {
            return Err(anyhow!(
                "`{}` has no `\"FileVersion\"` key: refusing to treat the derived data \
                 as regenerable from it.",
                manifest.display()
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "Unreal rebuilds DerivedDataCache/ and Intermediate/ on the next editor \
             open and build; for a C++ project, regenerate project files first \
             (right-click the .uproject, Generate Project Files) because the generated \
             solution lived in Intermediate/"
        );
        Ok(())
    }

    /// Empty on purpose: the manifest is `<Name>.uproject`, a name this adapter cannot
    /// know ahead of time, and `devp doctor` says so rather than guessing.
    fn lockfiles(&self) -> &'static [&'static str] {
        &[]
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A project root holding a plausible `Demo.uproject`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join("Demo.uproject"),
            "{\n\t\"FileVersion\": 3,\n\t\"EngineAssociation\": \"5.3\",\n\t\"Category\": \"\"\n}\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Unreal
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_any_uproject_file() {
        let dir = tempdir().unwrap();
        assert!(!Unreal.detect(dir.path()));
        project(dir.path());
        assert!(Unreal.detect(dir.path()));
    }

    #[test]
    fn a_uproject_directory_is_not_a_manifest() {
        // The editor opens a file; a directory that happens to end in `.uproject`
        // is somebody's naming choice, not a project.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("Demo.uproject")).unwrap();
        assert!(!Unreal.detect(dir.path()));
    }

    #[test]
    fn only_the_derived_directories_are_claimed() {
        // `Saved/` holds config and local saved games nothing regenerates, and
        // `Binaries/` is what lets the editor open without a compile: neither is
        // this adapter's to claim, and the test is what keeps that true.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("DerivedDataCache")).unwrap();
        fs::write(root.join("DerivedDataCache").join("DDC.udd"), "cache").unwrap();
        fs::create_dir(root.join("Intermediate")).unwrap();
        fs::create_dir(root.join("Saved")).unwrap();
        fs::create_dir(root.join("Binaries")).unwrap();
        fs::create_dir(root.join("Content")).unwrap();

        assert_eq!(claimed(&root), vec!["DerivedDataCache", "Intermediate"]);
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Unreal.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join("Demo.uproject"), "hello there").unwrap();
        assert!(Unreal.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Unreal.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn unreal_is_opt_in() {
        assert!(Unreal.opt_in());
    }
}
