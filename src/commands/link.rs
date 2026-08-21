// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handlers for `dev-prune link` and `dev-prune unlink` commands.

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::{PerRepoConfig, Registry};
use crate::output;
use crate::scanner;

/// Run the `link` command — register a Git repository.
///
/// `quiet` is what the global Git hook passes. In that mode nothing is printed and a
/// repository whose `.devprune.json` sets `disable_hooks` is left unregistered — that
/// flag exists precisely to keep the hook out of a specific workspace.
pub fn run_link(path_str: &str, quiet: bool) -> Result<()> {
    let path = Path::new(path_str)
        .canonicalize()
        .with_context(|| format!("Path not found: {path_str}"))?;

    if !scanner::is_git_repo(&path) {
        if quiet {
            return Ok(());
        }
        // Non-zero, because nothing was linked. The hook path above still exits 0: a
        // commit in a directory dev-prune does not track is not a failed commit.
        anyhow::bail!(
            "`{}` is not a Git repository.\n  \
             Run `git init` there first, then `devp link .` again.",
            output::clean_path(&path)
        );
    }

    // A repository under the OS temporary directory is scratch by definition: a test
    // fixture, a `git clone` into `mktemp -d`, a build step. The hook fires on its first
    // commit, the directory is gone minutes later, and the registry keeps an entry that
    // can never be pruned and never be found again. Registering those is how a registry
    // fills with dead paths. An explicit `devp link` still works — this only declines to
    // do it *unasked*.
    if quiet && is_under_temp_dir(&path) {
        return Ok(());
    }

    // A config that does not parse also keeps the hook out. It may well be the file that
    // says `disable_hooks`, and a broken one is not licence to register the repo anyway.
    if quiet
        && !matches!(
            PerRepoConfig::load_with_diagnostics(&path),
            Ok(None)
                | Ok(Some(PerRepoConfig {
                    disable_hooks: false,
                    ..
                }))
        )
    {
        return Ok(());
    }

    let mut registry = Registry::load()?;

    if registry.add_repo(path.clone()) {
        registry.last_added_repos = vec![path.clone()];
        registry.save()?;
        if !quiet {
            output::print_success(&format!("Linked: {}", output::clean_path(&path)));
            if registry.settings.auto_config {
                ensure_default_repo_config(&path);
            }
        }
    } else if !quiet {
        output::print_info(&format!("Already linked: {}", output::clean_path(&path)));
    }

    Ok(())
}

/// Write a default `.devprune.json` into a newly registered repository, when
/// `auto_config` asks for it.
///
/// Never over an existing file — broken or not, it is the user's — and a write failure
/// is a note rather than a failed registration: the repository is tracked either way,
/// the config was only ever a convenience.
pub(crate) fn ensure_default_repo_config(path: &Path) {
    if path.join(crate::constants::PER_REPO_CONFIG_FILE).exists() {
        return;
    }
    match PerRepoConfig::default().save_to_repo(path) {
        Ok(()) => output::print_info(&format!(
            "auto_config: wrote a default `.devprune.json` in {}",
            output::clean_path(path)
        )),
        Err(e) => output::print_warning(&format!(
            "auto_config: could not write `.devprune.json` in {}: {e}",
            output::clean_path(path)
        )),
    }
}

/// Is `path` inside the OS temporary directory?
///
/// `path` is expected to be canonical already; the temp directory is canonicalised here
/// because macOS reports it as `/var/folders/…`, a symlink to `/private/var/folders/…`.
/// A temp directory that cannot be resolved is treated as no match: declining to register
/// a real workspace would be the worse error of the two.
fn is_under_temp_dir(path: &Path) -> bool {
    std::env::temp_dir()
        .canonicalize()
        .is_ok_and(|tmp| path.starts_with(tmp))
}

/// Run `unlink --missing`: drop every registered path that no longer exists.
///
/// Nothing on disk is touched — the directories are already gone. Registries accumulate
/// these from clones that were deleted, drives that were reformatted, and workspaces that
/// were moved; `devp doctor` counts them and sends the user here rather than printing one
/// `devp unlink` line per entry.
pub fn run_unlink_missing() -> Result<()> {
    let mut registry = Registry::load()?;

    let gone: Vec<_> = registry
        .repositories
        .keys()
        .filter(|p| !p.exists())
        .cloned()
        .collect();

    if gone.is_empty() {
        output::print_success("Every registered repository still exists — nothing to remove.");
        return Ok(());
    }

    for path in &gone {
        registry.remove_repo(path);
        output::print_info(&format!("Unlinked: {}", output::clean_path(path)));
    }
    // `undo` reverts the last `init`/`link` by unregistering what it added. A path in that
    // list that no longer exists can never be reverted into anything, so leaving it there
    // only sets `undo` up to report that it removed nothing.
    registry.last_added_repos.retain(|p| p.exists());
    registry.save()?;

    output::print_success(&format!(
        "Removed {} registry {} pointing at directories that no longer exist.",
        gone.len(),
        output::plural(gone.len(), "entry", "entries")
    ));
    Ok(())
}

/// Run the `unlink` command — unregister a repository.
pub fn run_unlink(path_str: &str) -> Result<()> {
    // A deleted directory still has to be removable from the registry, so an
    // uncanonicalisable path falls back to what the user typed rather than failing.
    let path = Path::new(path_str)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(path_str).to_path_buf());

    let mut registry = Registry::load()?;

    if registry.remove_repo(&path) {
        registry.save()?;
        output::print_success(&format!("Unlinked: {}", output::clean_path(&path)));
    } else {
        output::print_warning(&format!("Not in registry: {}", output::clean_path(&path)));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_repository_under_the_temp_directory_is_recognised_as_scratch() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        assert!(
            is_under_temp_dir(&repo),
            "{} should be seen as scratch — TempDir builds under std::env::temp_dir()",
            repo.display()
        );
    }

    #[test]
    fn a_repository_outside_the_temp_directory_is_not() {
        // The crate's own source tree: a real workspace by any definition.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        assert!(!is_under_temp_dir(&here));
    }
}
