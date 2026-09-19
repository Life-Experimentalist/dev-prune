// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// sbt build tree adapter.
//
// sbt compiles a Scala project into `target/` beside `build.sbt`, and compiles the
// build definition itself into `project/target/`. Both are written only by sbt and
// regenerate in full from `sbt compile`, which is why sbt's own `.gitignore` guidance
// excludes them. The claim is exactly those two paths: sub-project `target/`
// directories deeper in a multi-module build are left to a later pass to learn about,
// because walking for them means guessing at what is a module and what is not.
//
// A repository that is both an sbt and a Maven or Gradle project deduplicates by path
// in the engine: whichever enabled adapter claims `target/` first owns it, and it is
// deleted once either way.
//
// Opt-in, and held to `build_idle_days`: getting the directories back means
// recompiling the project and the build definition.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The build definition at every sbt project's root.
const MANIFEST: &str = "build.sbt";

/// sbt build tree adapter. Opt-in; see the module comment.
pub struct Sbt;

impl PackageManager for Sbt {
    fn name(&self) -> &'static str {
        "sbt"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join(MANIFEST).is_file()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let target = path.join("target");
        if target.is_dir() {
            dirs.push(BloatDir {
                name: "target".to_string(),
                size_bytes: dir_size(&target),
                path: target,
                shared_bytes: 0,
            });
        }
        let project_target = path.join("project").join("target");
        if project_target.is_dir() {
            dirs.push(BloatDir {
                // Named by its path from the root: a second bare "target" row would
                // read as the same directory twice.
                name: "project/target".to_string(),
                size_bytes: dir_size(&project_target),
                path: project_target,
                shared_bytes: 0,
            });
        }
        dirs
    }

    /// What is checked is that the build definition still reads as one, because it is
    /// what `sbt compile` is about to execute. Nearly every `build.sbt` assigns a
    /// setting with `:=`; the rare one that does not (a bare aggregation root) is
    /// accepted when `project/build.properties` pins an `sbt.version`, which is sbt's
    /// own marker for the project root. Running sbt here to find out would start a
    /// compile, and JVM spin-up besides, in the middle of a delete pass.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to rebuild the targets from.")
        })?;
        if content.contains(":=") {
            return Ok(());
        }
        let properties = path.join("project").join("build.properties");
        if fs::read_to_string(&properties)
            .map(|p| p.contains("sbt.version"))
            .unwrap_or(false)
        {
            return Ok(());
        }
        Err(anyhow!(
            "`{MANIFEST}` assigns no settings and `project/build.properties` pins no \
             `sbt.version`: refusing to treat the build trees as regenerable from it."
        ))
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("sbt's build trees regenerate on the next `sbt compile`");
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

    /// A project root holding a plausible `build.sbt`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join(MANIFEST),
            "name := \"demo\"\n\nscalaVersion := \"3.4.2\"\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Sbt.bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_build_definition() {
        let dir = tempdir().unwrap();
        assert!(!Sbt.detect(dir.path()));
        project(dir.path());
        assert!(Sbt.detect(dir.path()));
    }

    #[test]
    fn both_target_trees_are_claimed_and_nothing_else() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("target")).unwrap();
        fs::create_dir_all(root.join("project").join("target")).unwrap();
        fs::create_dir(root.join("src")).unwrap();

        assert_eq!(claimed(&root), vec!["target", "project/target"]);
    }

    #[test]
    fn a_settingless_root_passes_when_build_properties_pins_sbt() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(MANIFEST), "// aggregation root\n").unwrap();
        fs::create_dir(dir.path().join("project")).unwrap();
        fs::write(
            dir.path().join("project").join("build.properties"),
            "sbt.version=1.10.1\n",
        )
        .unwrap();
        assert!(
            Sbt.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn a_missing_or_bogus_build_definition_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Sbt.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "// nothing assigned\n").unwrap();
        assert!(Sbt.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Sbt.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn sbt_is_opt_in() {
        assert!(Sbt.opt_in());
    }
}
