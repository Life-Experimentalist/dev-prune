// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Unity imported-asset cache adapter.
//
// Unity keeps its imported-asset database in `Library/` beside the project: compressed
// textures, converted meshes, the asset database itself, shader caches. `Temp/` is the
// editor's scratch space. Both are written only by the editor, both regenerate from the
// committed sources the next time it opens the project, and both are in Unity's own
// recommended `.gitignore`. On an asset-heavy project `Library/` is routinely larger
// than the sources it was imported from.
//
// Detection is `ProjectSettings/ProjectVersion.txt`, which every Unity project has had
// since well before 2018; `Packages/manifest.json` does not go back that far, so it is
// not the anchor. The one thing the manifest check cannot see is a *running* editor,
// because the idle gate is Git-based and an open editor commits nothing, so the
// verification also refuses while `Temp/UnityLockfile` exists, which is the editor's
// own "this project is open" marker. Deleting `Library/` under a live editor corrupts
// the asset database instead of clearing a cache.
//
// Opt-in, and held to `build_idle_days`: getting `Library/` back means the editor
// re-importing every asset, which on a large project is a long sit, not a download.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Present at every Unity project's root, whatever the editor version.
const MANIFEST: &str = "ProjectSettings/ProjectVersion.txt";

/// The editor's own "this project is open in an editor right now" marker.
const EDITOR_LOCK: &str = "Temp/UnityLockfile";

/// The editor's cache directories: the imported-asset database and its scratch space.
const CACHE_DIRS: &[&str] = &["Library", "Temp"];

/// Unity imported-asset cache adapter. Opt-in; see the module comment.
pub struct Unity;

impl PackageManager for Unity {
    fn name(&self) -> &'static str {
        "unity"
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

    /// Two refusals. `ProjectVersion.txt` must still read as one (every version of the
    /// file opens with `m_EditorVersion:`), because it is what the editor will rebuild
    /// `Library/` from. And `Temp/UnityLockfile` must not exist: it means the editor
    /// has the project open *now*, which the Git-based idle gate cannot see, and
    /// deleting the asset database under a live editor corrupts it.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to re-import the assets from.")
        })?;
        if !content.contains("m_EditorVersion:") {
            return Err(anyhow!(
                "`{MANIFEST}` has no `m_EditorVersion:` line: refusing to treat the \
                 imported-asset cache as regenerable from it."
            ));
        }
        if path.join(EDITOR_LOCK).exists() {
            return Err(anyhow!(
                "`{EDITOR_LOCK}` exists: the Unity editor has this project open, and \
                 the asset database is not deletable under it. Close the editor first."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "Unity's imported-asset cache regenerates when the editor next opens the \
             project; expect a full re-import on the first open"
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

    /// A project root holding a plausible `ProjectSettings/ProjectVersion.txt`.
    fn project(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir.join("ProjectSettings")).unwrap();
        fs::write(
            dir.join(MANIFEST),
            "m_EditorVersion: 2022.3.10f1\nm_EditorVersionWithRevision: 2022.3.10f1 (ff3792e53c62)\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Unity
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_project_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Unity.detect(dir.path()));
        project(dir.path());
        assert!(Unity.detect(dir.path()));
    }

    #[test]
    fn the_asset_database_and_scratch_space_are_claimed() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("Library")).unwrap();
        fs::write(root.join("Library").join("ArtifactDB"), "cache").unwrap();
        fs::create_dir(root.join("Temp")).unwrap();

        assert_eq!(claimed(&root), vec!["Library", "Temp"]);
    }

    #[test]
    fn nothing_else_beside_the_manifest_is_claimed() {
        // `Assets/` is the sources the cache is imported *from*; claiming anything by
        // a name not in the list would be deleting somebody's work.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("Assets")).unwrap();

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Unity.enforce_lockfile(dir.path(), policy).is_err());
        fs::create_dir_all(dir.path().join("ProjectSettings")).unwrap();
        fs::write(dir.path().join(MANIFEST), "hello there").unwrap();
        assert!(Unity.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Unity.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn an_open_editor_is_refused() {
        // The idle gate reads Git, and an open editor commits nothing, so this is the
        // one place a live editor can be caught.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        let policy = EnforcePolicy::default();
        assert!(Unity.enforce_lockfile(&root, policy).is_ok());
        fs::create_dir(root.join("Temp")).unwrap();
        fs::write(root.join(EDITOR_LOCK), "").unwrap();
        assert!(Unity.enforce_lockfile(&root, policy).is_err());
    }

    #[test]
    fn unity_is_opt_in() {
        assert!(Unity.opt_in());
    }
}
