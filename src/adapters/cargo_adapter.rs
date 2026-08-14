// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Cargo/Rust package manager adapter.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Adapter for Cargo-based Rust projects.
pub struct Cargo;

/// The `Cargo.lock` cargo itself would use for this project: the nearest one at or
/// above `path`, stopping at the repository boundary.
///
/// A workspace member has no lockfile of its own — the workspace root's covers it.
/// Treating the member as lockfile-less used to send enforcement down the
/// `generate-lockfile` tier, which re-resolves the whole workspace and rewrites the
/// *root* lockfile as a precondition for deleting one member's `target/`.
fn workspace_lockfile(path: &Path) -> Option<PathBuf> {
    let mut dir = path;
    loop {
        let candidate = dir.join("Cargo.lock");
        if candidate.exists() {
            return Some(candidate);
        }
        // Past the repository root, any lockfile found belongs to somebody else.
        if dir.join(".git").exists() {
            return None;
        }
        dir = dir.parent()?;
    }
}

impl PackageManager for Cargo {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("Cargo.toml").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let target_path = path.join("target");
        if target_path.exists() {
            dirs.push(BloatDir {
                name: "target".to_string(),
                path: target_path.clone(),
                size_bytes: dir_size(&target_path),
                shared_bytes: 0,
            });
        }
        dirs
    }

    /// `cargo metadata --locked` resolves the graph and fails if `Cargo.lock` would need
    /// updating — without ever writing to it. `generate-lockfile` re-resolves and
    /// rewrites it, which is what a user with a stale lockfile wants and what `metadata
    /// --locked` refuses to do for them; it is reached when there is no lockfile to
    /// preserve, or when they opted in. `--offline` is deliberately never passed —
    /// re-resolving against a stale local index is how you get a lockfile that does not
    /// build.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        // Criterion keeps its benchmark history in `target/criterion`. It is the one
        // thing under `target/` no build regenerates — the next `cargo bench` starts a
        // fresh baseline with nothing to compare against. Still recoverable-by-rebuild
        // in the sense that matters, so a warning, not a refusal.
        if path.join("target").join("criterion").is_dir() {
            crate::output::print_warning(&format!(
                "{}: `target/criterion` holds benchmark history that a rebuild does not \
                 bring back — copy it first if the baselines matter.",
                crate::output::clean_path(path)
            ));
        }
        let lockfile = workspace_lockfile(path).unwrap_or_else(|| path.join("Cargo.lock"));
        enforce_two_tier(
            &lockfile,
            "cargo",
            &["metadata", "--locked", "--format-version", "1"],
            &["generate-lockfile"],
            path,
            policy,
        )
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("Rust target/ will regenerate on next cargo build");
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["Cargo.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        let adapter = Cargo;
        assert_eq!(adapter.name(), "cargo");
    }

    /// The invariant the whole two-tier design exists for: a default pass may fail, but
    /// it may not leave the lockfile different from how it found it.
    ///
    /// Uses a lockfile that does not list the manifest's dependency, which is the exact
    /// state `--locked` is there to refuse. Skipped rather than failed when `cargo` is
    /// absent, so the suite still runs somewhere without a Rust toolchain on `PATH`.
    #[test]
    fn a_default_pass_never_rewrites_a_stale_lockfile() {
        if !super::super::binary_available("cargo") {
            return;
        }
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"stale\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("lib.rs"), "").unwrap();

        // A lockfile that knows only about the root package — `serde` is missing, so the
        // graph cannot be resolved from it.
        let stale = "version = 3\n\n[[package]]\nname = \"stale\"\nversion = \"0.1.0\"\n";
        fs::write(dir.path().join("Cargo.lock"), stale).unwrap();

        let result = Cargo.enforce_lockfile(dir.path(), EnforcePolicy::default());

        assert!(
            result.is_err(),
            "a lockfile that cannot resolve the manifest must not pass verification"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("Cargo.lock")).unwrap(),
            stale,
            "the read-only verification rewrote Cargo.lock"
        );
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("Cargo.toml")).unwrap();

        let adapter = Cargo;
        assert!(adapter.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();

        let adapter = Cargo;
        assert!(!adapter.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();

        let adapter = Cargo;
        let dirs = adapter.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "target");
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();

        let adapter = Cargo;
        let dirs = adapter.bloat_dirs(dir.path());
        assert!(dirs.is_empty());
    }
}
