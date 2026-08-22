// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune restore` command.
//
// Detects package managers in a project and restores dependencies from lockfiles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{PrunedDir, Registry};
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

    // Read through the user's configured depth and timeout, not the built-in defaults:
    // a repository pruned at a deeper setting has to be restored at that same setting,
    // and a reinstall is the longest command this tool runs — the raised
    // `command_timeout_secs` was almost certainly raised *for* it.
    let (global_depth, timeout_secs) = crate::config::Registry::load()
        .map(|r| (r.settings.scan_depth, r.settings.command_timeout_secs))
        .unwrap_or((
            crate::constants::DEFAULT_SCAN_DEPTH,
            crate::constants::DEFAULT_COMMAND_TIMEOUT_SECS,
        ));
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let results = engine::restore_project_to_depth(&path, global_depth, timeout)?;

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

    // Settled once, before anything is rebuilt: asking per directory would put the same
    // question in front of the user forty times in a pass that touched forty projects.
    let dropped = settle_runtimes(&last.dirs)?;

    // Grouped by repository so each tree is walked once, in a stable order, however the
    // pass that recorded them happened to interleave.
    let mut by_repo: BTreeMap<PathBuf, Vec<PrunedDir>> = BTreeMap::new();
    for dir in &last.dirs {
        let mut dir = dir.clone();
        if dir
            .runtime
            .as_deref()
            .is_some_and(|t| dropped.iter().any(|d| d == t))
        {
            dir.runtime = None;
        }
        by_repo.entry(dir.repo_path.clone()).or_default().push(dir);
    }

    let global_depth = registry.settings.scan_depth;
    let timeout = std::time::Duration::from_secs(registry.settings.command_timeout_secs);
    let mut attempted = 0usize;
    let mut failed = 0usize;

    for (repo_path, deleted) in &by_repo {
        println!();
        output::print_info(&output::clean_path(repo_path));

        // A repository that is gone is reported per directory, not skipped, so the count
        // at the end still adds up to what the prune took.
        if !repo_path.exists() {
            for dir in deleted {
                attempted += 1;
                failed += 1;
                output::print_error(&format!(
                    "  {} ({}): the repository no longer exists at this path",
                    dir.adapter, dir.bloat_dir
                ));
            }
            continue;
        }

        for (label, result) in engine::restore_deleted(repo_path, deleted, global_depth, timeout) {
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

/// What a `--last-run` restore intends to do about the interpreters it recorded.
///
/// Split out from the command so the decision is testable without a terminal: the
/// question of *whether* to ask is a pure function of what was recorded and what is
/// installed, and only the asking needs stdin.
#[derive(Debug, PartialEq, Eq)]
struct RuntimePlan {
    /// Recorded versions this machine has, which will be used as recorded.
    honoured: Vec<String>,
    /// Recorded versions this machine does not have. Rebuilding those directories means
    /// rebuilding them on a different interpreter, which is the thing worth asking about.
    missing: Vec<String>,
}

impl RuntimePlan {
    /// `available` is asked once per distinct version rather than once per directory —
    /// a prune of forty Python projects would otherwise spawn forty identical probes.
    fn build(dirs: &[PrunedDir], available: impl Fn(&str) -> bool) -> Self {
        let mut honoured = Vec::new();
        let mut missing = Vec::new();
        for tag in dirs.iter().filter_map(|d| d.runtime.as_deref()) {
            if honoured.iter().any(|t| t == tag) || missing.iter().any(|t| t == tag) {
                continue;
            }
            if available(tag) {
                honoured.push(tag.to_string());
            } else {
                missing.push(tag.to_string());
            }
        }
        honoured.sort();
        missing.sort();
        Self { honoured, missing }
    }
}

/// Decide, and say out loud, which interpreter each recorded directory is rebuilt on.
///
/// Returns the versions to drop — the ones this machine cannot provide and the user has
/// agreed to rebuild on whatever `python` is. Bails instead when they say no, because a
/// restore onto the wrong interpreter is not something to do by default: it is the
/// failure this recording exists to prevent, and it surfaces much later as an import
/// error nobody connects back to here.
fn settle_runtimes(dirs: &[PrunedDir]) -> Result<Vec<String>> {
    let plan = RuntimePlan::build(dirs, crate::adapters::python_runtime_available);

    for tag in &plan.honoured {
        output::print_info(&format!(
            "Python {tag} environments will be rebuilt on Python {tag}, as recorded."
        ));
    }
    if plan.missing.is_empty() {
        return Ok(Vec::new());
    }

    let versions = plan.missing.join(", ");
    output::print_warning(&format!(
        "Python {versions} {} recorded for some of these environments, but not installed \
         here. Rebuilding them means rebuilding on whatever `python` resolves to, and \
         pinned wheels may not exist for it.",
        output::plural(plan.missing.len(), "was", "were"),
    ));
    for tag in &plan.missing {
        output::print_info(&format!("  Install it first:  uv python install {tag}"));
    }

    if !confirm_other_interpreter() {
        anyhow::bail!(
            "Nothing was restored. Install the recorded {} and run `devp restore \
             --last-run` again, or answer yes to rebuild on the interpreter you have.",
            output::plural(plan.missing.len(), "interpreter", "interpreters"),
        );
    }
    Ok(plan.missing)
}

/// Default no. Everything else this command does puts back exactly what was taken; this
/// is the one step that knowingly puts back something slightly different, so a reflexive
/// Enter should not be what agrees to it. The question goes to stderr so a piped stdout
/// cannot eat it.
fn confirm_other_interpreter() -> bool {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        output::print_info(
            "Not running in a terminal, so this is not being answered for you — install \
             the recorded interpreter, or re-run this where the question can be asked.",
        );
        return false;
    }
    eprint!("Rebuild them on the interpreter you have? [y/N]: ");
    if std::io::stderr().flush().is_err() {
        return false;
    }
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(runtime: Option<&str>) -> PrunedDir {
        PrunedDir {
            repo_path: PathBuf::from("/repo"),
            bloat_dir: ".venv".to_string(),
            adapter: "venv".to_string(),
            size_freed: 1,
            runtime: runtime.map(str::to_string),
        }
    }

    #[test]
    fn a_recorded_interpreter_that_is_installed_is_used_without_asking() {
        let dirs = [dir_with(Some("3.12")), dir_with(None)];
        let plan = RuntimePlan::build(&dirs, |_| true);
        assert_eq!(plan.honoured, vec!["3.12".to_string()]);
        assert!(plan.missing.is_empty(), "nothing to ask about");
    }

    #[test]
    fn each_version_is_probed_once_however_many_directories_recorded_it() {
        // Forty Python projects in one pass must not mean forty identical probes.
        let dirs: Vec<PrunedDir> = (0..40).map(|_| dir_with(Some("3.12"))).collect();
        let probes = std::cell::Cell::new(0);
        let plan = RuntimePlan::build(&dirs, |_| {
            probes.set(probes.get() + 1);
            true
        });
        assert_eq!(probes.get(), 1);
        assert_eq!(plan.honoured.len(), 1);
    }

    #[test]
    fn an_interpreter_this_machine_does_not_have_is_what_gets_asked_about() {
        let dirs = [dir_with(Some("3.12")), dir_with(Some("3.9"))];
        let plan = RuntimePlan::build(&dirs, |tag| tag == "3.12");
        assert_eq!(plan.honoured, vec!["3.12".to_string()]);
        assert_eq!(plan.missing, vec!["3.9".to_string()]);
    }

    #[test]
    fn a_pass_that_recorded_nothing_asks_nothing() {
        // Everything pruned before 1.4.0 lands here, and so does a pass that only
        // touched node_modules. Neither should produce a question.
        let dirs = [dir_with(None), dir_with(None)];
        let plan = RuntimePlan::build(&dirs, |_| unreachable!("nothing to probe"));
        assert!(plan.honoured.is_empty() && plan.missing.is_empty());
    }
}
