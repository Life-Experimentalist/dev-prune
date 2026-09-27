// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Cocos Creator imported-asset cache adapter.
//
// A Cocos Creator project is anchored by a `package.json` at its root, the same file
// name every Node project has, which is why detection reads it: Creator 3.x writes a
// `"creator"` key into it, and Creator 2.x names `"cocos-creator-js"` as its engine.
// A `package.json` with neither is an ordinary Node project and none of this
// adapter's business; the npm adapter already handles it.
//
// Beside the manifest the editor writes `library/` (imported assets in engine format)
// and `temp/` (its scratch space). Both regenerate from the committed sources the
// next time the editor opens the project, and both are in Creator's own generated
// `.gitignore`. `build/` is not claimed: it is the published output a person exported
// on purpose. Unlike Unity there is no known on-disk marker for "the editor has this
// project open", so the idle window is the only guard against a live editor; the
// opt-in and the longer build window are what keep that risk small.
//
// Opt-in, and held to `build_idle_days`: getting `library/` back means the editor
// re-importing every asset on the next open.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The manifest at the project root, shared with every Node project, which is why
/// detection reads its contents instead of trusting the name.
const MANIFEST: &str = "package.json";

/// The editor's cache directories. Never `build/`, which is exported output.
const CACHE_DIRS: &[&str] = &["library", "temp"];

/// True when this `package.json` belongs to Cocos Creator rather than plain Node:
/// Creator 3.x writes a `"creator"` key, Creator 2.x a `"cocos-creator-js"` engine.
fn is_creator_manifest(content: &str) -> bool {
    content.contains("\"creator\"") || content.contains("cocos-creator")
}

/// Cocos Creator imported-asset cache adapter. Opt-in; see the module comment.
pub struct Cocos;

impl PackageManager for Cocos {
    fn name(&self) -> &'static str {
        "cocos"
    }

    fn detect(&self, path: &Path) -> bool {
        let manifest = path.join(MANIFEST);
        manifest.is_file()
            && fs::read_to_string(&manifest).is_ok_and(|content| is_creator_manifest(&content))
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

    /// What is checked is that `package.json` is still readable and still names Cocos
    /// Creator, because it is what marks the caches as the editor's to regenerate.
    /// Running the editor to find out would start the re-import in the middle of a
    /// delete pass.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to re-import the assets from.")
        })?;
        if !is_creator_manifest(&content) {
            return Err(anyhow!(
                "`{MANIFEST}` no longer names Cocos Creator: refusing to treat the \
                 imported-asset cache as regenerable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "Cocos Creator's imported-asset cache regenerates when the editor next \
             opens the project"
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

    /// A project root holding a plausible Creator 3.x `package.json`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join(MANIFEST),
            "{\n  \"name\": \"demo\",\n  \"creator\": {\n    \"version\": \"3.8.0\"\n  }\n}\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Cocos
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_a_creator_manifest_only() {
        let dir = tempdir().unwrap();
        assert!(!Cocos.detect(dir.path()));
        // A plain Node project's package.json is the npm adapter's, not this one's.
        fs::write(
            dir.path().join(MANIFEST),
            "{\n  \"name\": \"demo\",\n  \"dependencies\": {}\n}\n",
        )
        .unwrap();
        assert!(!Cocos.detect(dir.path()));
        project(dir.path());
        assert!(Cocos.detect(dir.path()));
    }

    #[test]
    fn a_creator_2x_engine_line_also_detects() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(MANIFEST),
            "{\n  \"name\": \"demo\",\n  \"engine\": \"cocos-creator-js\",\n  \"version\": \"2.4.13\"\n}\n",
        )
        .unwrap();
        assert!(Cocos.detect(dir.path()));
    }

    #[test]
    fn the_caches_are_claimed_and_the_exported_build_is_not() {
        // `build/` is what someone exported on purpose; the claim stops at the
        // editor's own caches.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("library")).unwrap();
        fs::write(root.join("library").join("imports.json"), "cache").unwrap();
        fs::create_dir(root.join("temp")).unwrap();
        fs::create_dir(root.join("build")).unwrap();
        fs::create_dir(root.join("assets")).unwrap();

        assert_eq!(claimed(&root), vec!["library", "temp"]);
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Cocos.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "{\"name\": \"demo\"}").unwrap();
        assert!(Cocos.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Cocos.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn cocos_is_opt_in() {
        assert!(Cocos.opt_in());
    }
}
