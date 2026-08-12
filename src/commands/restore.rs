// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune restore` command.
//
// Detects package managers in a project and restores dependencies from lockfiles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Registry;
use crate::engine;
use crate::output;

/// Run the `restore` command.
pub fn run(path_str: &str) -> Result<()> {
    let path = Path::new(path_str)
        .canonicalize()
        .with_context(|| format!("Path not found: {path_str}"))?;

    output::print_header("dev-prune restore");
    output::print_info(&format!(
        "Restoring dependencies in {}",
        output::clean_path(&path)
    ));

    // Read through the user's configured depth, not the built-in default: a repository
    // pruned at a deeper setting has to be restored at that same setting.
    let global_depth = crate::config::Registry::load()
        .map(|r| r.settings.scan_depth)
        .unwrap_or(crate::constants::DEFAULT_SCAN_DEPTH);
    let results = engine::restore_project_to_depth(&path, global_depth)?;

    let mut failed = 0usize;

    for (adapter_name, result) in &results {
        match result {
            Ok(()) => {
                output::print_success(&format!("{adapter_name}: dependencies restored"));
            }
            Err(e) => {
                output::print_error(&format!("{adapter_name}: {e}"));
                failed += 1;
            }
        }
    }

    // A restore that failed has to exit non-zero. `devp prune && devp restore` in a
    // script, or a CI step that runs it, otherwise carries on against a project whose
    // dependencies are half installed — the one situation the exit code exists for.
    if failed > 0 {
        anyhow::bail!(
            "{failed} of {} {} failed to restore — see the errors above",
            results.len(),
            output::plural(results.len(), "adapter", "adapters")
        );
    }

    output::print_success(&format!(
        "Restored {} {}.",
        results.len(),
        output::plural(results.len(), "adapter", "adapters")
    ));

    Ok(())
}

/// Run `restore --last-run`: put back exactly what the most recent prune pass deleted.
///
/// The undo the tool did not have. `devp undo` reverses an `init` or a `link`, which are
/// registry edits; the thing people actually want reversed is the pass that emptied
/// twelve directories across four repositories a minute ago, and reconstructing that list
/// by hand means remembering which repositories were even in it.
///
/// The record is not cleared afterwards. Restoring twice is harmless — the second pass is
/// each manager's own no-op — and a `--last-run` that could only be used once would fail
/// exactly when a partial restore made the user want to re-run it.
pub fn run_last_run() -> Result<()> {
    let registry = Registry::load()?;

    let Some(last) = registry.last_prune.as_ref() else {
        anyhow::bail!(
            "No prune pass has been recorded yet, so there is nothing to put back.\n  \
             `devp restore <path>` restores a project you name."
        );
    };

    let total: u64 = last.dirs.iter().map(|d| d.size_freed).sum();

    output::print_header("dev-prune restore --last-run");
    output::print_info(&format!(
        "Putting back {} {} deleted on {} ({}).",
        last.dirs.len(),
        output::plural(last.dirs.len(), "directory", "directories"),
        last.at.format("%Y-%m-%d %H:%M UTC"),
        output::format_bytes(total)
    ));

    // Grouped by repository so each tree is walked once, in a stable order, however the
    // pass that recorded them happened to interleave.
    let mut by_repo: BTreeMap<PathBuf, Vec<(String, String)>> = BTreeMap::new();
    for dir in &last.dirs {
        by_repo
            .entry(dir.repo_path.clone())
            .or_default()
            .push((dir.bloat_dir.clone(), dir.adapter.clone()));
    }

    let global_depth = registry.settings.scan_depth;
    let mut attempted = 0usize;
    let mut failed = 0usize;

    for (repo_path, deleted) in &by_repo {
        println!();
        output::print_info(&output::clean_path(repo_path));

        // A repository that is gone is reported per directory, not skipped, so the count
        // at the end still adds up to what the prune took.
        if !repo_path.exists() {
            for (label, adapter) in deleted {
                attempted += 1;
                failed += 1;
                output::print_error(&format!(
                    "  {adapter} ({label}): the repository no longer exists at this path"
                ));
            }
            continue;
        }

        for (label, result) in engine::restore_deleted(repo_path, deleted, global_depth) {
            attempted += 1;
            match result {
                Ok(()) => output::print_success(&format!("  {label}: restored")),
                Err(e) => {
                    failed += 1;
                    output::print_error(&format!("  {label}: {e}"));
                }
            }
        }
    }

    println!();
    if failed > 0 {
        anyhow::bail!(
            "{failed} of {attempted} {} failed to restore — see the errors above",
            output::plural(attempted, "directory", "directories")
        );
    }

    output::print_success(&format!(
        "Restored {attempted} {} across {} {}.",
        output::plural(attempted, "directory", "directories"),
        by_repo.len(),
        output::plural(by_repo.len(), "repository", "repositories")
    ));

    Ok(())
}
