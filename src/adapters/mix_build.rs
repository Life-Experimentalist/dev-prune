// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Mix build-tree adapter for Elixir and Erlang projects.
//
// Opt-in (`devp config set enable_mix_build true`), and separate from the `mix` adapter
// on purpose: `deps/` is downloaded and `_build/` is compiled. The plain adapter can
// delete `deps/` with `_build/` still in place because Mix refetches the sources and
// recompiles only what changed. Deleting `_build/` itself costs a full recompile of the
// project *and* every dependency, which on a Phoenix application is minutes — so it
// waits for someone to ask for it, and then for the longer `build_idle_days` window on
// top of that.
//
// What it claims is `_build/` and nothing else. Hex's package cache lives in
// `~/.hex/packages`, outside the repository, and is `devp caches` business rather than
// this adapter's.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::path::Path;

/// Adapter for the Mix build tree. Opt-in; see the module comment.
pub struct MixBuild;

impl PackageManager for MixBuild {
    fn name(&self) -> &'static str {
        "mix_build"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("mix.exs").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let build = path.join("_build");
        if !build.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: "_build".to_string(),
            path: build.clone(),
            size_bytes: dir_size(&build),
            shared_bytes: 0,
        }]
    }

    /// Like Gradle and Maven: the rebuild starts from the sources and `mix.exs` in the
    /// tree, so their presence is the recoverability proof. `mix.lock` is checked too —
    /// a recompile of the dependencies needs the same versions back, and that is the
    /// file that pins them.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        if !path.join("mix.exs").exists() {
            return Err(anyhow!("no `mix.exs` — nothing to rebuild `_build/` from."));
        }
        if !path.join("mix.lock").exists() {
            return Err(anyhow!(
                "`mix.lock` is missing — recompiling `_build/` needs the dependency \
                 versions it pins, and without it `mix deps.get` resolves afresh."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("Mix _build/ will regenerate on the next `mix compile`");
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["mix.lock"]
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
    fn detects_on_the_mix_manifest() {
        let dir = tempdir().unwrap();
        assert!(!MixBuild.detect(dir.path()));
        fs::write(dir.path().join("mix.exs"), "defmodule X.MixProject do end").unwrap();
        assert!(MixBuild.detect(dir.path()));
    }

    #[test]
    fn claims_the_build_tree_and_never_deps() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("deps")).unwrap();
        fs::create_dir(dir.path().join("_build")).unwrap();
        let names: Vec<String> = MixBuild
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["_build"]);
    }

    #[test]
    fn a_missing_manifest_or_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            MixBuild
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("mix.exs"), "").unwrap();
        assert!(
            MixBuild
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("mix.lock"), "%{}\n").unwrap();
        assert!(
            MixBuild
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn the_build_tree_is_opt_in() {
        assert!(MixBuild.opt_in());
        assert!(!crate::adapters::mix::Mix.opt_in());
    }
}
