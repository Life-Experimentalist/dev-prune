// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Terraform adapter.
//
// `.terraform/providers/` only, and deliberately not `.terraform/` itself. Three things
// live in that directory that a reinstall does not bring back the same:
//
// - `.terraform/environment` records the selected workspace. Delete it and Terraform
//   silently falls back to `default` — so the next `terraform apply` an operator runs
//   without looking targets the wrong environment. Nothing about that is recoverable in
//   the sense this tool means, and the failure is production, not a slow rebuild.
// - `.terraform/terraform.tfstate` is the backend's initialisation record. Rebuilding it
//   needs `terraform init` with the backend's credentials, which a prune has no business
//   assuming are present.
// - `.terraform/modules/` is fetched from module sources, and `.terraform.lock.hcl` does
//   not cover modules — only providers. An unpinned `git::` module source resolves to
//   whatever that branch says today, so deleting the directory can change what comes
//   back. That is exactly the thing this tool refuses to do.
//
// Providers are the bulk anyway — a handful of them is hundreds of megabytes of
// statically linked plugin binaries, per root module, and a repository with ten
// environments has ten copies.
//
// No manifest-staleness check, unlike Mix and CocoaPods: the nearest thing to a manifest
// is every `.tf` file in the directory, and those are edited constantly for reasons that
// have nothing to do with provider requirements. A lock file older than a `.tf` is the
// normal state of a healthy Terraform project, so refusing on it would refuse almost
// always — and a check that always fires teaches people to bypass it.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, run_command_with_timeout};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Terraform adapter.
pub struct Terraform;

impl PackageManager for Terraform {
    fn name(&self) -> &'static str {
        "terraform"
    }

    fn detect(&self, path: &Path) -> bool {
        // A root module is a directory with `.tf` files in it; there is no fixed manifest
        // filename to look for. `.tf.json` counts — it is the same language, and
        // generators emit it.
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return false;
            };
            name.ends_with(".tf") || name.ends_with(".tf.json")
        })
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let providers = path.join(".terraform").join("providers");
        if !providers.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: ".terraform/providers".to_string(),
            path: providers.clone(),
            size_bytes: dir_size(&providers),
            shared_bytes: 0,
        }]
    }

    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let lock = path.join(".terraform.lock.hcl");
        let content = fs::read_to_string(&lock).map_err(|e| {
            anyhow!(
                "`.terraform.lock.hcl` could not be read ({e}) — without it `terraform \
                 init` selects provider versions afresh instead of restoring the ones \
                 being deleted."
            )
        })?;
        // Every entry is a `provider "registry.terraform.io/..." { ... }` block. A file
        // with none is a lock file Terraform wrote before any provider was required, and
        // it proves nothing about the plugins on disk.
        if !content.contains("provider \"") {
            return Err(anyhow!(
                "`.terraform.lock.hcl` records no providers — it cannot prove \
                 `.terraform/providers` is rebuildable. Run `terraform init` and prune \
                 again."
            ));
        }
        Ok(())
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        // `-backend=false` because reinstalling plugins must not need the backend's
        // credentials. Only the providers were deleted, so only the providers are what
        // this has to put back; touching the backend would turn a local restore into a
        // request against somebody's state bucket.
        run_command_with_timeout("terraform", &["init", "-backend=false"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &[".terraform.lock.hcl"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const LOCK: &str =
        "provider \"registry.terraform.io/hashicorp/aws\" {\n  version = \"5.0.0\"\n}\n";

    #[test]
    fn detects_on_any_terraform_source_file() {
        let dir = tempdir().unwrap();
        assert!(!Terraform.detect(dir.path()));
        fs::write(dir.path().join("README.md"), "not terraform").unwrap();
        assert!(!Terraform.detect(dir.path()));
        fs::write(
            dir.path().join("main.tf"),
            "resource \"null_resource\" \"a\" {}",
        )
        .unwrap();
        assert!(Terraform.detect(dir.path()));
    }

    #[test]
    fn detects_generated_json_configuration_too() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.tf.json"), "{}").unwrap();
        assert!(Terraform.detect(dir.path()));
    }

    #[test]
    fn claims_the_provider_cache_and_nothing_else_under_dot_terraform() {
        let dir = tempdir().unwrap();
        let dot = dir.path().join(".terraform");
        fs::create_dir_all(dot.join("providers")).unwrap();
        fs::create_dir_all(dot.join("modules")).unwrap();
        fs::write(dot.join("environment"), "production").unwrap();
        fs::write(dot.join("terraform.tfstate"), "{}").unwrap();

        let dirs = Terraform.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".terraform/providers");
        assert_eq!(dirs[0].path, dot.join("providers"));
    }

    #[test]
    fn an_uninitialised_project_offers_nothing_to_prune() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.tf"), "").unwrap();
        assert!(Terraform.bloat_dirs(dir.path()).is_empty());
    }

    #[test]
    fn a_missing_or_providerless_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Terraform
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(
            dir.path().join(".terraform.lock.hcl"),
            "# This file is maintained automatically by \"terraform init\".\n",
        )
        .unwrap();
        assert!(
            Terraform
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join(".terraform.lock.hcl"), LOCK).unwrap();
        assert!(
            Terraform
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }
}
