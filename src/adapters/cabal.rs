// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Haskell Cabal build tree adapter.
//
// `cabal build` keeps everything it compiles for a project in `dist-newstyle/` beside
// `cabal.project`: compiled modules, per-GHC artifacts, the build plan. The directory
// is written only by cabal-install and regenerates in full from `cabal build`, which is
// why the standard Haskell `.gitignore` excludes it.
//
// Detection is on `cabal.project`, not on a lone `*.cabal` file: the project file is a
// fixed name at a fixed place, marks the root the way `stack.yaml` does for Stack, and
// a Stack project wrapping the same `.cabal` files is claimed by the stack adapter
// instead. A repository driving cabal directly off one `.cabal` file with no project
// file is left alone — narrower is safer than clever.
//
// Opt-in, and held to `build_idle_days`: getting `dist-newstyle/` back means
// recompiling the project.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The project file at a Cabal project's root.
const MANIFEST: &str = "cabal.project";

/// cabal-install's per-project build tree.
const CACHE_DIRS: &[&str] = &["dist-newstyle"];

/// Haskell Cabal build tree adapter. Opt-in; see the module comment.
pub struct Cabal;

impl PackageManager for Cabal {
    fn name(&self) -> &'static str {
        "cabal"
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

    /// What is checked is that `cabal.project` still declares its packages, because
    /// `packages:` is the one field every project file carries and the one `cabal
    /// build` rebuilds from. A file without it is whatever it is, not a project
    /// cabal-install can rebuild. Running `cabal build` here to find out would start a
    /// compile in the middle of a delete pass.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!(
                "`{MANIFEST}` could not be read ({e}): nothing to rebuild `dist-newstyle` from."
            )
        })?;
        if !content.contains("packages") {
            return Err(anyhow!(
                "`{MANIFEST}` declares no `packages:`: refusing to treat the build tree \
                 as regenerable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("Cabal's build tree regenerates on the next `cabal build`");
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

    /// A project root holding a plausible `cabal.project`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(dir.join(MANIFEST), "packages: .\n").unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Cabal
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_project_file() {
        let dir = tempdir().unwrap();
        assert!(!Cabal.detect(dir.path()));
        project(dir.path());
        assert!(Cabal.detect(dir.path()));
    }

    #[test]
    fn the_build_tree_is_claimed_and_nothing_else() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("dist-newstyle")).unwrap();
        fs::create_dir(root.join("src")).unwrap();

        assert_eq!(claimed(&root), vec!["dist-newstyle"]);
    }

    #[test]
    fn a_missing_or_packageless_project_file_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Cabal.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "-- just a comment\n").unwrap();
        assert!(Cabal.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Cabal.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn cabal_is_opt_in() {
        assert!(Cabal.opt_in());
    }
}
