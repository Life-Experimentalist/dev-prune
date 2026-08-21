// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Maven build-tool adapter for Java projects.
//
// Opt-in (`devp config set enable_maven true`), unlike every package-manager adapter:
// `target/` is rebuilt by *recompiling the whole project*, not by re-downloading a
// dependency tree, so deleting it trades disk for a full `mvn package` — minutes, not
// seconds. The engine also holds these directories to the longer `build_idle_days`
// window for the same reason.
//
// Recoverability rests on a different proof than the lockfile adapters use: `target/`
// is derived entirely from the sources and `pom.xml` sitting in the repository, and
// Maven refuses to build a module whose `pom.xml` declares dependencies without literal
// versions resolvable from a repository. There is nothing inside `target/` that a
// rebuild does not regenerate.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Adapter for Maven-based Java projects. Opt-in; see the module comment.
pub struct Maven;

impl PackageManager for Maven {
    fn name(&self) -> &'static str {
        "maven"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("pom.xml").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let target = path.join("target");
        if target.exists() {
            dirs.push(BloatDir {
                name: "target".to_string(),
                path: target.clone(),
                size_bytes: dir_size(&target),
                shared_bytes: 0,
            });
        }
        dirs
    }

    /// The proof here is the manifest, not a lockfile: `target/` is derived from the
    /// working tree, so what must exist is a readable `pom.xml` for the rebuild to
    /// start from. Running `mvn validate` instead would resolve plugins over the
    /// network — a download in the middle of a delete pass, for no stronger answer.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let pom = path.join("pom.xml");
        let content = fs::read_to_string(&pom).map_err(|e| {
            anyhow!("`pom.xml` could not be read ({e}) — nothing to rebuild `target/` from.")
        })?;
        if !content.contains("<project") {
            return Err(anyhow!(
                "`pom.xml` does not look like a Maven manifest (no `<project` element) — \
                 refusing to treat `target/` as rebuildable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("Maven target/ will regenerate on the next `mvn package` (or `mvn compile`)");
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["pom.xml"]
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
    fn detects_on_pom_xml_only() {
        let dir = tempdir().unwrap();
        assert!(!Maven.detect(dir.path()));
        fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        assert!(Maven.detect(dir.path()));
    }

    #[test]
    fn claims_target_when_present() {
        let dir = tempdir().unwrap();
        assert!(Maven.bloat_dirs(dir.path()).is_empty());
        fs::create_dir(dir.path().join("target")).unwrap();
        let dirs = Maven.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "target");
    }

    #[test]
    fn a_missing_or_bogus_manifest_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Maven
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("pom.xml"), "not xml at all").unwrap();
        assert!(
            Maven
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("pom.xml"), "<project></project>").unwrap();
        assert!(
            Maven
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn maven_is_opt_in() {
        assert!(Maven.opt_in());
    }
}
