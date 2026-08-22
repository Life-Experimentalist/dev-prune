// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Mix adapter for Elixir and Erlang projects.
//
// `deps/` only. `_build/` is compiled output — beam files this tool has no lockfile
// proof for — and dev-prune does not delete compiled output outside the two adapters
// that ask for it by name. Deleting `deps/` alone is safe with `_build/` left in place:
// the next `mix deps.get` refetches the sources and Mix recompiles only what changed.
//
// The proof is offline, for the same reason as CocoaPods: `mix deps.get` fixes drift by
// fetching rather than reporting it, so what is checked is that `mix.lock` exists, is a
// lockfile rather than a fragment, and is not older than the `mix.exs` it came from.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, refuse_if_manifest_stale,
    run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Mix adapter.
pub struct Mix;

impl PackageManager for Mix {
    fn name(&self) -> &'static str {
        "mix"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("mix.exs").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let deps = path.join("deps");
        if !deps.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: "deps".to_string(),
            path: deps.clone(),
            size_bytes: dir_size(&deps),
            shared_bytes: 0,
        }]
    }

    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let lock = path.join("mix.lock");
        let content = fs::read_to_string(&lock).map_err(|e| {
            anyhow!(
                "`mix.lock` could not be read ({e}) — without it `mix deps.get` resolves \
                 afresh instead of restoring the versions being deleted."
            )
        })?;
        // A `mix.lock` is a single Elixir map literal: `%{"dep" => {:hex, ...}}`. A file
        // that does not open one is a fragment or a merge conflict, not a lockfile.
        if !content.contains("%{") {
            return Err(anyhow!(
                "`mix.lock` is not an Elixir map literal — it is not a complete Mix \
                 lockfile, so `deps/` cannot be proven rebuildable from it."
            ));
        }
        refuse_if_manifest_stale(&path.join("mix.exs"), &lock, "mix deps.get")
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("mix", &["deps.get"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["mix.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_mix_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Mix.detect(dir.path()));
        fs::write(dir.path().join("mix.exs"), "defmodule X.MixProject do end").unwrap();
        assert!(Mix.detect(dir.path()));
    }

    #[test]
    fn claims_deps_and_never_the_build_tree() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("deps")).unwrap();
        fs::create_dir(dir.path().join("_build")).unwrap();
        let names: Vec<String> = Mix
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["deps"]);
    }

    #[test]
    fn a_missing_or_malformed_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Mix.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("mix.lock"), "<<<<<<< HEAD\n").unwrap();
        assert!(
            Mix.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("mix.lock"), "%{\n  \"jason\": {:hex},\n}\n").unwrap();
        assert!(
            Mix.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }
}
