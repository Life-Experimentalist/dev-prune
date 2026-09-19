// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Defold build-output adapter.
//
// A Defold project is anchored by `game.project` at its root, an INI-style file the
// editor and the command-line builder (bob) both read. Beside it they write `build/`:
// compiled scripts, compressed textures, everything the next build regenerates from
// the committed sources. Defold's own documentation has it ignored, and either tool
// rebuilds it in full on the next build of the project.
//
// The claim is that one name, directly beside a `game.project` that still parses as
// one (the format always opens its sections with headers like `[project]`). Nothing
// else: `.internal/` is the editor's own state and small, and the assets are the
// sources the build is *from*.
//
// Opt-in, and held to `build_idle_days`: the cost of getting it back is a full
// rebuild, not a download.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The manifest at every Defold project's root.
const MANIFEST: &str = "game.project";

/// The build output the editor and bob regenerate.
const CACHE_DIRS: &[&str] = &["build"];

/// Defold build-output adapter. Opt-in; see the module comment.
pub struct Defold;

impl PackageManager for Defold {
    fn name(&self) -> &'static str {
        "defold"
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

    /// What is checked is that `game.project` is still readable and still reads as
    /// one: the format always carries a `[project]` section, and a file without it is
    /// whatever it is, not a project the builder can rebuild from. Running bob to find
    /// out would start the build this pass exists to postpone.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to rebuild from.")
        })?;
        if !content.contains("[project]") {
            return Err(anyhow!(
                "`{MANIFEST}` has no `[project]` section: refusing to treat the build \
                 output as regenerable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "Defold's build output regenerates on the next build, in the editor or \
             with bob"
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

    /// A project root holding a plausible `game.project`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join(MANIFEST),
            "[project]\ntitle = Demo\nversion = 1.0\n\n[display]\nwidth = 960\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Defold
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_project_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Defold.detect(dir.path()));
        project(dir.path());
        assert!(Defold.detect(dir.path()));
    }

    #[test]
    fn the_build_output_is_claimed() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("build")).unwrap();
        fs::write(root.join("build").join("game.arcd"), "output").unwrap();

        assert_eq!(claimed(&root), vec!["build"]);
    }

    #[test]
    fn nothing_else_beside_the_manifest_is_claimed() {
        // `.internal/` is the editor's own state and the assets are the sources; the
        // claim is one name and the test keeps it one name.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join(".internal")).unwrap();
        fs::create_dir(root.join("assets")).unwrap();

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Defold.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "hello there").unwrap();
        assert!(Defold.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Defold.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn defold_is_opt_in() {
        assert!(Defold.opt_in());
    }
}
