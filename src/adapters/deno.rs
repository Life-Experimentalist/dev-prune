// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Deno adapter.
//
// For most of its history a Deno project had nothing local to prune: everything Deno
// downloaded went into one machine-wide directory (`DENO_DIR`) and the project directory
// held source and nothing else. npm interoperability changed that. A project with a
// `package.json`, or with `"nodeModulesDir": "auto"` in its config, gets a real
// `node_modules/` beside the source; a project with `"vendor": true` gets a `vendor/`
// tree holding a copy of every remote module it resolved. Both are rebuilt by `deno
// install`, and on anything using the npm ecosystem both routinely outweigh the source
// they sit next to.
//
// Detection is on `deno.lock` and nothing else. A `deno.json` without a lockfile is a
// project whose versions are decided by whatever the next resolve happens to find, and a
// `node_modules` only a fresh resolve can rebuild is not recoverable in the sense this
// tool promises.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size_with_hardlinks, refuse_if_manifest_stale,
    run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Deno adapter.
pub struct Deno;

/// The config files Deno reads, in the order it looks for them.
const CONFIGS: [&str; 3] = ["deno.json", "deno.jsonc", "package.json"];

impl Deno {
    /// Whether this project has asked Deno to write a `vendor/` directory.
    ///
    /// `vendor/` is claimed only when the config says so, rather than whenever the
    /// directory happens to exist. The name is not Deno's: Go and Composer both use a
    /// `vendor/` at the project root for something else entirely, and a repository that
    /// carried a `deno.lock` alongside either of those would otherwise have dev-prune
    /// delete Composer's dependency tree and offer `deno install` to put it back.
    fn vendors(path: &Path) -> bool {
        ["deno.json", "deno.jsonc"].iter().any(|name| {
            fs::read_to_string(path.join(name))
                .map(|config| {
                    config
                        .split_whitespace()
                        .collect::<String>()
                        .contains("\"vendor\":true")
                })
                .unwrap_or(false)
        })
    }
}

impl PackageManager for Deno {
    fn name(&self) -> &'static str {
        "deno"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("deno.lock").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        // Deno hardlinks out of `DENO_DIR` when it materialises `node_modules`, exactly
        // as bun does out of its own cache, so the shared/freed split is the honest
        // measurement rather than the whole tree.
        let node_modules = path.join("node_modules");
        if node_modules.is_dir() {
            let size = dir_size_with_hardlinks(&node_modules);
            dirs.push(BloatDir {
                name: "node_modules".to_string(),
                path: node_modules,
                size_bytes: size.freed_bytes,
                shared_bytes: size.shared_bytes,
            });
        }
        let vendor = path.join("vendor");
        if vendor.is_dir() && Self::vendors(path) {
            let size = dir_size_with_hardlinks(&vendor);
            dirs.push(BloatDir {
                name: "vendor".to_string(),
                path: vendor,
                size_bytes: size.freed_bytes,
                shared_bytes: size.shared_bytes,
            });
        }
        dirs
    }

    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let lock = path.join("deno.lock");
        let content = fs::read_to_string(&lock).map_err(|e| {
            anyhow!(
                "`deno.lock` could not be read ({e}) — without it `deno install` \
                 resolves afresh instead of restoring the versions being deleted."
            )
        })?;
        // Every Deno lockfile is a JSON object carrying a `version`. A file without one
        // is a fragment or a merge conflict, and `deno install` would silently start
        // over from the config rather than fail.
        let parsed: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            anyhow!("`deno.lock` is not valid JSON ({e}) — it cannot be a complete lockfile.")
        })?;
        if parsed.get("version").is_none() {
            return Err(anyhow!(
                "`deno.lock` has no `version` field — it is not a complete Deno \
                 lockfile, so the dependency tree cannot be proven rebuildable from it."
            ));
        }
        // `deno install` resolves and writes rather than reporting, and `--frozen` still
        // performs the whole install before it can disagree — a download, and then a
        // write, in the middle of a delete pass. The timestamps are the only offline
        // evidence there is, the same as for CocoaPods, Mix and pub.
        for name in CONFIGS {
            let manifest = path.join(name);
            if manifest.exists() {
                refuse_if_manifest_stale(&manifest, &lock, "deno install")?;
            }
        }
        Ok(())
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("deno", &["install"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["deno.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn lockfile(dir: &Path) {
        fs::write(dir.join("deno.lock"), r#"{"version":"5","specifiers":{}}"#).unwrap();
    }

    #[test]
    fn detects_on_the_lockfile_and_not_the_config() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("deno.json"), "{}").unwrap();
        assert!(!Deno.detect(dir.path()));
        lockfile(dir.path());
        assert!(Deno.detect(dir.path()));
    }

    #[test]
    fn claims_node_modules() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let names: Vec<String> = Deno
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["node_modules"]);
    }

    #[test]
    fn a_vendor_directory_is_claimed_only_when_the_config_asked_for_one() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("vendor")).unwrap();
        assert!(Deno.bloat_dirs(dir.path()).is_empty());

        fs::write(dir.path().join("deno.json"), "{ \"vendor\": true }").unwrap();
        let names: Vec<String> = Deno
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["vendor"]);
    }

    #[test]
    fn a_missing_or_malformed_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Deno.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("deno.lock"), "<<<<<<< HEAD\n").unwrap();
        assert!(
            Deno.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("deno.lock"), r#"{"specifiers":{}}"#).unwrap();
        assert!(
            Deno.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        lockfile(dir.path());
        assert!(
            Deno.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn is_not_opt_in_because_none_of_it_is_compiler_output() {
        // `node_modules` is not opt-in the way a compiler-output directory is: `deno
        // install` puts it back from the lockfile without recompiling anything.
        assert!(!Deno.opt_in());
    }
}
