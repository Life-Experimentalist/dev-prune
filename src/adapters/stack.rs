// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Haskell Stack build tree adapter.
//
// Stack keeps everything it builds for a project in `.stack-work/` beside
// `stack.yaml`: compiled modules, the project's own packages, per-snapshot GHC
// artifacts. The directory is written only by Stack and regenerates in full from
// `stack build`, which is why Stack's own documentation tells people to ignore it in
// version control. What pins the rebuild is the resolver: `stack.yaml` names a snapshot
// that fixes GHC and every package version, so the rebuild is reproducible in a way a
// bare `.cabal` file's version ranges are not.
//
// Opt-in, and held to `build_idle_days`: getting `.stack-work/` back means recompiling
// the project, and Haskell compiles are famously not quick.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The project file at every Stack project's root.
const MANIFEST: &str = "stack.yaml";

/// Stack's per-project build tree.
const CACHE_DIRS: &[&str] = &[".stack-work"];

/// Haskell Stack build tree adapter. Opt-in; see the module comment.
pub struct Stack;

impl PackageManager for Stack {
    fn name(&self) -> &'static str {
        "stack"
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

    /// What is checked is that `stack.yaml` still names a snapshot, because the
    /// snapshot is what makes the rebuild reproducible: it pins GHC and every package
    /// version the way a lockfile would. `resolver:` is the traditional key,
    /// `snapshot:` its newer synonym — a file with neither is not a project Stack can
    /// rebuild deterministically. Running `stack build` here to find out would start a
    /// compile in the middle of a delete pass.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to rebuild `.stack-work` from.")
        })?;
        let names_snapshot = content.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("resolver:") || line.starts_with("snapshot:")
        });
        if !names_snapshot {
            return Err(anyhow!(
                "`{MANIFEST}` names no `resolver:` or `snapshot:`: refusing to treat the \
                 build tree as reproducibly regenerable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("Stack's build tree regenerates on the next `stack build`");
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

    /// A project root holding a plausible `stack.yaml`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join(MANIFEST),
            "resolver: lts-22.33\n\npackages:\n- .\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Stack
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_project_file() {
        let dir = tempdir().unwrap();
        assert!(!Stack.detect(dir.path()));
        project(dir.path());
        assert!(Stack.detect(dir.path()));
    }

    #[test]
    fn the_build_tree_is_claimed_and_nothing_else() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join(".stack-work")).unwrap();
        fs::create_dir(root.join("src")).unwrap();

        assert_eq!(claimed(&root), vec![".stack-work"]);
    }

    #[test]
    fn the_newer_snapshot_key_also_passes() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(MANIFEST),
            "snapshot: nightly-2026-01-01\npackages:\n- .\n",
        )
        .unwrap();
        assert!(
            Stack
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn a_missing_or_snapshotless_project_file_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Stack.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "packages:\n- .\n").unwrap();
        assert!(Stack.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Stack.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn stack_is_opt_in() {
        assert!(Stack.opt_in());
    }
}
