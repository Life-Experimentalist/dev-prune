// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Cargo/Rust package manager adapter.
//
// Opt-in (`devp config set enable_cargo true`), and it is worth being clear about why,
// because `cargo metadata --locked` genuinely does prove the dependency graph resolves
// from `Cargo.lock`. What it does not prove is that anything comes back *cheaply*:
// `target/` holds compiler output, and the only way to get it back is to rebuild it.
// That puts cargo in the same class as gradle, maven and swift rather than with
// `node_modules` and `.venv`, so it waits for the longer `build_idle_days` window and
// nobody finds it deleted without having asked.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier};
use crate::declared::{Gap, RebuildCheck, on_path};
use anyhow::Result;
use std::fs;
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

    fn opt_in(&self) -> bool {
        true
    }
}

/// The rebuild check for `cargo` declarations.
pub(crate) struct CargoSubcommands;

impl RebuildCheck for CargoSubcommands {
    fn tools(&self) -> &'static [&'static str] {
        &["cargo"]
    }

    fn gap(&self, repo_path: &Path, _tool: &str, args: &[&str]) -> Option<Gap> {
        cargo_subcommand_gap(repo_path, args)
    }
}

/// Cargo's own subcommands, which never need a plugin behind them.
///
/// Only used to *stop* asking: a name on this list is accepted immediately. A name that
/// is not on it goes on to the `PATH` and alias checks rather than being refused, so a
/// list that falls behind cargo makes this slower, never wrong.
const CARGO_BUILTINS: &[&str] = &[
    "add",
    "b",
    "bench",
    "build",
    "c",
    "check",
    "clean",
    "clippy",
    "config",
    "d",
    "doc",
    "fetch",
    "fix",
    "fmt",
    "generate-lockfile",
    "help",
    "info",
    "init",
    "install",
    "locate-project",
    "login",
    "logout",
    "metadata",
    "miri",
    "new",
    "owner",
    "package",
    "pkgid",
    "publish",
    "r",
    "read-manifest",
    "remove",
    "report",
    "run",
    "rustc",
    "rustdoc",
    "search",
    "t",
    "test",
    "tree",
    "uninstall",
    "unpublish",
    "update",
    "vendor",
    "verify-project",
    "version",
    "yank",
];

/// A `cargo` subcommand nothing on this machine provides.
///
/// Deliberately not an attempt to enumerate cargo plugins — there is no list of those.
/// It only rules out the names that are definitively absent: not one of cargo's own, no
/// `cargo-<name>` program on `PATH`, and no `[alias]` table anywhere cargo reads one.
fn cargo_subcommand_gap(repo_path: &Path, args: &[&str]) -> Option<Gap> {
    let mut i = 0;
    // `cargo +nightly build` picks a toolchain before naming the subcommand.
    while args.get(i).is_some_and(|arg| arg.starts_with('+')) {
        i += 1;
    }
    let sub = *args.get(i)?;
    if sub.starts_with('-') || CARGO_BUILTINS.contains(&sub) {
        return None;
    }
    // Asking `PATH` the same question cargo itself asks when it meets a name it does not
    // know. An installed plugin is found here.
    if on_path(&format!("cargo-{sub}")) {
        return None;
    }
    if cargo_aliases_exist(repo_path) {
        return None;
    }
    Some(Gap {
        what: format!("`{sub}` is not a cargo subcommand and no `cargo-{sub}` is on this machine"),
        fix: format!("Install whatever provides `cargo {sub}`, or fix the command."),
    })
}

/// Whether any config cargo would read declares an `[alias]` table.
///
/// Its mere presence is enough to stop asking. An alias can name anything, and reading
/// one machine's table to decide whether a committed declaration is honoured is exactly
/// the kind of answer this module would rather not give.
fn cargo_aliases_exist(repo_path: &Path) -> bool {
    let mut candidates = vec![
        repo_path.join(".cargo").join("config.toml"),
        repo_path.join(".cargo").join("config"),
    ];
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|dir| dir.join(".cargo")));
    if let Some(home) = home {
        candidates.push(home.join("config.toml"));
        candidates.push(home.join("config"));
    }
    candidates.iter().any(|path| {
        fs::read_to_string(path)
            .map(|text| text.lines().any(|l| l.trim_start().starts_with("[alias")))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn cargo_is_opt_in() {
        // `target/` is compiler output: proving the crates resolve is not the same as
        // getting the compiled artefacts back for free.
        assert!(Cargo.opt_in());
    }

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
