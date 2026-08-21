// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Gradle build-tool adapter for Java/Kotlin/Android projects.
//
// Opt-in (`devp config set enable_gradle true`), for the same reason as Maven: `build/`
// comes back by recompiling, not by re-downloading, and on an Android project that can
// mean a very long first build. The engine holds these directories to the longer
// `build_idle_days` window on top of the opt-in.
//
// What it claims: `build/` (compiled outputs, all derived from the sources in the
// tree) and the project-local `.gradle/` (per-project caches: configuration cache,
// file-hash indexes — bookkeeping Gradle rebuilds on the next invocation). Gradle's
// *user-home* caches (`~/.gradle/caches`) live outside the repository and are never
// this tool's business.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::path::Path;

/// The manifests any Gradle project has at least one of.
const GRADLE_MANIFESTS: [&str; 4] = [
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];

/// Adapter for Gradle-based projects. Opt-in; see the module comment.
pub struct Gradle;

fn has_manifest(path: &Path) -> bool {
    GRADLE_MANIFESTS.iter().any(|m| path.join(m).exists())
}

impl PackageManager for Gradle {
    fn name(&self) -> &'static str {
        "gradle"
    }

    fn detect(&self, path: &Path) -> bool {
        has_manifest(path)
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        for name in ["build", ".gradle"] {
            let dir = path.join(name);
            if dir.is_dir() {
                dirs.push(BloatDir {
                    name: name.to_string(),
                    path: dir.clone(),
                    size_bytes: dir_size(&dir),
                    shared_bytes: 0,
                });
            }
        }
        dirs
    }

    /// Like Maven: the rebuild starts from the manifests in the tree, so their
    /// presence is the recoverability proof. Invoking Gradle itself here would run a
    /// configuration phase that can resolve plugins over the network mid-prune.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        if !has_manifest(path) {
            return Err(anyhow!(
                "no Gradle manifest (build.gradle[.kts] / settings.gradle[.kts]) — \
                 nothing to rebuild `build/` from."
            ));
        }
        Ok(())
    }

    fn restore(&self, path: &Path, _timeout: std::time::Duration) -> Result<()> {
        let wrapper = if cfg!(windows) {
            "gradlew.bat"
        } else {
            "gradlew"
        };
        let cmd = if path.join(wrapper).exists() {
            "./gradlew build"
        } else {
            "gradle build"
        };
        println!("Gradle build/ will regenerate on the next `{cmd}`");
        Ok(())
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_on_any_gradle_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Gradle.detect(dir.path()));
        fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert!(Gradle.detect(dir.path()));
    }

    #[test]
    fn claims_build_and_project_local_gradle_dir() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        fs::create_dir(dir.path().join(".gradle")).unwrap();
        let names: Vec<String> = Gradle
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["build", ".gradle"]);
    }

    #[test]
    fn a_vanished_manifest_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Gradle
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("settings.gradle"), "").unwrap();
        assert!(
            Gradle
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn gradle_is_opt_in() {
        assert!(Gradle.opt_in());
    }
}
