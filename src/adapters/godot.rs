// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Godot imported-resource cache adapter.
//
// Godot's editor keeps a cache of every imported asset (compressed textures, decoded
// audio, shader caches, editor metadata) beside the project it belongs to: `.godot/`
// in Godot 4, `.import/` in Godot 3. Both are written only by the editor, both sit
// beside `project.godot`, and both regenerate from the committed source assets the
// next time the editor opens the project, which is why the official starter
// `.gitignore` for Godot excludes them. On an asset-heavy project the cache is
// routinely larger than the sources it was imported from.
//
// The claim is deliberately narrow: only those two directory names, only directly
// beside a `project.godot`, and only when that manifest still parses as one (it always
// carries a `config_version=` line). There is nothing like CMake's cache file to read
// a source path out of, and nothing is needed: a hidden directory with the editor's
// own name beside the editor's own manifest is not something anybody writes by hand.
//
// Opt-in, and held to `build_idle_days`: getting the cache back means the editor
// re-importing every asset, which on a large project is a long sit, not a download.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The manifest at every Godot project's root, whatever the engine version.
const MANIFEST: &str = "project.godot";

/// The editor's cache directories: `.godot/` is Godot 4, `.import/` is Godot 3.
const CACHE_DIRS: &[&str] = &[".godot", ".import"];

/// Godot imported-resource cache adapter. Opt-in; see the module comment.
pub struct Godot;

impl PackageManager for Godot {
    fn name(&self) -> &'static str {
        "godot"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join(MANIFEST).is_file()
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

    /// What is checked is that `project.godot` is still readable and still reads as a
    /// Godot project file, because it is what the editor is about to be pointed at.
    /// Every version of the format carries a `config_version=` line, and a file
    /// without one is whatever it is, not a project the editor can re-import from.
    /// Running the editor here to find out would start an import in the middle of a
    /// delete pass, which is the opposite of what was asked for.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to re-import the assets from.")
        })?;
        if !content.contains("config_version=") {
            return Err(anyhow!(
                "`{MANIFEST}` has no `config_version=` line: refusing to treat the imported-\
                 resource cache as regenerable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "Godot's imported-resource cache regenerates when the editor next opens the \
             project; `godot --headless --import` does it without the window on 4.x"
        );
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &[MANIFEST]
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

    /// A project root holding a plausible `project.godot`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join(MANIFEST),
            "; Engine configuration file.\nconfig_version=5\n\n[application]\n\nconfig/name=\"Demo\"\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Godot
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_project_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Godot.detect(dir.path()));
        project(dir.path());
        assert!(Godot.detect(dir.path()));
    }

    #[test]
    fn both_engine_versions_caches_are_claimed() {
        // A project migrated from Godot 3 to 4 can be carrying both directories, and
        // both are the editor's to regenerate.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join(".godot")).unwrap();
        fs::write(root.join(".godot").join("uid_cache.bin"), "cache").unwrap();
        fs::create_dir(root.join(".import")).unwrap();

        assert_eq!(claimed(&root), vec![".godot", ".import"]);
    }

    #[test]
    fn nothing_else_beside_the_manifest_is_claimed() {
        // The claim is by these two names and nothing else: a directory of the user's
        // own is not this adapter's to reason about.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("assets")).unwrap();

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Godot.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "hello there").unwrap();
        assert!(Godot.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Godot.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn godot_is_opt_in() {
        assert!(Godot.opt_in());
    }
}
