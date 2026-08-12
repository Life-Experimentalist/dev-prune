// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune status` command.
//
// Displays a rich overview of all registered repositories: status, skip
// reason, last activity, last pruned date, adapters, and reclaimable space.
// Also allows launching a prune pass directly from the status view.

use anyhow::Result;
use std::io::{self, IsTerminal};

use crate::commands::hook::HookState;
use crate::config::Registry;
use crate::engine::{self, PruneStatus};
use crate::output;
use crate::tui::status_view;

/// Run the `status` command.
///
/// `json` replaces the dashboard with one machine-readable document — no banner, no
/// TUI, no prompt to prune. It is a pure read of state, which is what makes it safe to
/// hand to an agent or a monitoring job.
pub fn run(json_output: bool) -> Result<()> {
    let mut registry = Registry::load()?;

    let daemon_st = crate::daemon::daemon_status()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "Unknown".to_string());
    // Both halves of the hook installation, not just the files. Hook scripts on disk with
    // `core.hooksPath` pointing at another tool never run, and reporting that as "Active"
    // is the difference between "my repos register themselves" and silently not.
    let hook_st = match crate::commands::hook::state() {
        Ok(HookState::Active) => "Active (post-commit, post-checkout, post-merge)".to_string(),
        Ok(HookState::Chained { previous, drifted }) if drifted.is_empty() => {
            format!("Active, chained to {previous}")
        }
        Ok(HookState::Chained { previous, drifted }) => format!(
            "Active, chained to {previous} ({} hook(s) not forwarded)",
            drifted.len()
        ),
        Ok(HookState::Foreign(path)) => format!("Inactive (core.hooksPath belongs to {path})"),
        Ok(HookState::Absent) | Err(_) => "Inactive".to_string(),
    };

    if json_output {
        let repos = engine::get_full_status(&registry);
        return crate::json::emit(&crate::json::status_document(
            &registry, &repos, &daemon_st, &hook_st,
        ));
    }

    output::print_banner();

    // Only on the human path: JSON output is a contract, and a version notice printed
    // into it would corrupt the document.
    if crate::commands::update::notify_if_outdated(&mut registry) {
        let _ = registry.save();
    }

    let reg_path = Registry::registry_path()
        .map(|p| output::clean_path(&p))
        .unwrap_or_else(|_| "unknown".to_string());

    output::print_info(&format!("Global Config Location: {}", reg_path));
    output::print_info(&format!("Background OS Daemon:   {}", daemon_st));
    output::print_info(&format!("Background Git Hooks:   {}", hook_st));
    // The minutes are derived, not a hardcoded "(10m)" — that read as the default even
    // after `devp config set command_timeout_secs 60`.
    let timeout = registry.settings.command_timeout_secs;
    output::print_info(&format!(
        "Global Command Timeout: {timeout}s ({})",
        format_duration(timeout)
    ));
    if registry.settings.min_size_mb > 0 {
        output::print_info(&format!(
            "Minimum Directory Size: {} MiB (smaller ones are left alone)",
            registry.settings.min_size_mb
        ));
    }
    output::print_info(&format!(
        "Tracked Repositories:   {}",
        registry.repo_count()
    ));
    output::print_info(&format!(
        "Historical Space Saved: {} across {} prune passes",
        output::format_bytes(registry.total_freed_bytes),
        registry.total_pruned_count
    ));
    println!();

    // Gather full per-repo detail for ALL registered repositories
    let repos = engine::get_full_status(&registry);

    if io::stdout().is_terminal() {
        // Interactive TUI — pass a loader closure so the TUI can reload after
        // the user toggles ignore config in .devprune.json or presence of ignore.devprune.json on any repo.
        let registry_ref = &registry;
        match status_view::render_status_tui(&|| engine::get_full_status(registry_ref)) {
            Ok(Some(selected_indices)) if !selected_indices.is_empty() => {
                // User confirmed a prune from within the status view
                let candidates: Vec<_> = selected_indices
                    .iter()
                    .map(|&i| repos[i].path.clone())
                    .collect();

                output::print_header("Pruning Selected Repositories");

                let mut total_freed: u64 = 0;
                let mut pruned_count = 0;
                let mut error_count = 0;

                for path in &candidates {
                    let results = engine::prune_repo(path, 0, false, true);
                    for result in results {
                        match &result.status {
                            PruneStatus::Pruned => {
                                total_freed += result.size_freed;
                                pruned_count += 1;
                                registry.mark_pruned(&result.repo_path, result.size_freed);
                                output::print_success(&format!(
                                    "{} → {} ({}) — {}",
                                    output::clean_path(&result.repo_path),
                                    result.bloat_dir,
                                    output::format_bytes(result.size_freed),
                                    result.adapter_name,
                                ));
                            }
                            PruneStatus::LockfileError(e) => {
                                error_count += 1;
                                output::print_error(&format!(
                                    "{} lockfile sync failed: {}",
                                    output::clean_path(&result.repo_path),
                                    e,
                                ));
                            }
                            PruneStatus::DeleteError(e) => {
                                error_count += 1;
                                output::print_error(&format!(
                                    "{} delete failed: {}",
                                    output::clean_path(&result.repo_path),
                                    e,
                                ));
                            }
                            PruneStatus::ConfigError(e) => {
                                error_count += 1;
                                output::print_error(&format!(
                                    "{} skipped — unreadable .devprune.json: {}",
                                    output::clean_path(&result.repo_path),
                                    e,
                                ));
                            }
                            _ => {}
                        }
                    }
                }

                registry.save()?;

                output::print_header("Summary");
                output::print_success(&format!(
                    "Freed: {} across {pruned_count} directories",
                    output::format_bytes(total_freed)
                ));
                // Same contract as `devp run`: a prune that failed exits non-zero,
                // whether it was started from the dashboard or from the command line.
                if error_count > 0 {
                    anyhow::bail!("{error_count} directories could not be pruned.");
                }
            }
            Ok(_) => {
                // User quit without pruning — nothing to do
            }
            Err(e) => {
                // Not necessarily a terminal that cannot do raw mode: toggling ignore
                // with `i` also ends the view if the config write fails. `{e}` carries
                // the real reason, so this line does not guess at one.
                output::print_warning(&format!("Interactive view ended: {e:#}"));
                status_view::render_status_plain(&repos);
            }
        }
    } else {
        // Non-TTY: plain text table
        status_view::render_status_plain(&repos);
    }

    Ok(())
}

/// A seconds count as the unit a human would have typed it in.
fn format_duration(secs: u64) -> String {
    match secs {
        s if s > 0 && s % 3600 == 0 => format!("{}h", s / 3600),
        s if s > 0 && s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timeout_is_described_in_whatever_unit_fits_it() {
        // The old line hardcoded "(10m)", so every value looked like the default.
        assert_eq!(format_duration(600), "10m");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(90), "90s");
        assert_eq!(format_duration(0), "0s");
    }
}
