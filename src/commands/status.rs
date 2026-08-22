// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune status` command.
//
// Displays a rich overview of all registered repositories: status, skip
// reason, last activity, last pruned date, adapters, and reclaimable space.
// Also allows launching a prune pass directly from the status view.

use anyhow::Result;
use std::io::{self, IsTerminal};

use crate::adapters::DriftReport;
use crate::commands::hook::HookState;
use crate::config::Registry;
use crate::engine::{self, PruneStatus};
use crate::output;
use crate::tui::status_view;
use crate::workspace;

/// One project's lockfile drift, located: which repository, which project inside it,
/// which adapter found it, and what it found.
pub struct ProjectDrift {
    /// The registered repository the project lives in.
    pub repository: std::path::PathBuf,
    /// Project path relative to the repository root, `/`-separated; `"."` is the root.
    pub project: String,
    /// The adapter that made the comparison.
    pub adapter: &'static str,
    /// The drifted directory, the unrecorded packages, and the command that records them.
    pub report: DriftReport,
}

/// Run the `status` command.
///
/// `json` replaces the dashboard with one machine-readable document — no banner, no
/// TUI, no prompt to prune. It is a pure read of state, which is what makes it safe to
/// hand to an agent or a monitoring job.
///
/// `top` trims the repository list to the biggest reclaims. It never changes the totals:
/// those are computed over every registered repository, so `--top 5` cannot make a
/// machine look tidier than it is.
///
/// `drift` replaces the dashboard with the lockfile-drift report — the environments
/// holding packages their lockfile never recorded, found before a prune would refuse
/// on them.
pub fn run(top: Option<usize>, drift: bool, json_output: bool) -> Result<()> {
    if drift {
        return run_drift(json_output);
    }
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
            &registry, &repos, &daemon_st, &hook_st, top,
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
        "Historical Space Saved: {} across {} prune {}",
        output::format_bytes_styled(registry.total_freed_bytes),
        registry.total_pruned_count,
        output::plural(registry.total_pruned_count as usize, "pass", "passes")
    ));
    if let Some(n) = top {
        output::print_info(&format!(
            "Showing:                the {n} {} with the most reclaimable space",
            output::plural(n, "repository", "repositories")
        ));
    }
    println!();

    // Nothing registered is the first-run state, not an error — but an empty dashboard
    // with no explanation reads as "the tool is broken", so say how to fill it instead.
    if registry.repositories.is_empty() {
        output::print_info(
            "No repositories are registered yet. `devp init <folder>` scans a folder and \
             registers every Git repository in it; `devp link .` registers just one.",
        );
        return Ok(());
    }

    // Gather full per-repo detail for ALL registered repositories, then trim the list —
    // after the totals above, which are deliberately computed over all of them.
    //
    // Never on the `--json` path: the bar writes to stderr, but a machine-readable mode
    // should produce one document and nothing else, and a progress bar in a log capture
    // is noise a script has to learn to ignore.
    let scan_bar = (!json_output).then(|| {
        output::create_progress_bar("Scanning repositories", registry.repositories.len() as u64)
    });
    let scanned = engine::get_full_status_reporting(&registry, &|done, _total| {
        if let Some(pb) = &scan_bar {
            pb.set_position(done as u64);
        }
    });
    // Over every repository, before `--top` trims the list, for the same reason the
    // totals above are: `--top 5` must not make the machine look cheaper to undo than
    // it is.
    let estimate = restore_estimate_line(&registry, &scanned);
    let repos = engine::take_top(&scanned, top);
    if let Some(pb) = scan_bar {
        // `finish_and_clear`, not `finish`: the dashboard is what the user asked for, and
        // a completed progress bar left above it is scaffolding.
        pb.finish_and_clear();
    }

    if let Some(line) = estimate {
        output::print_info(&line);
        println!();
    }

    // Both ends: the dashboard draws on stdout but reads keys from stdin, and with
    // stdin redirected it would open on a screen no keypress can ever leave.
    if io::stdout().is_terminal() && io::stdin().is_terminal() {
        // Interactive TUI — pass a loader closure so the TUI can reload after
        // the user toggles ignore config in .devprune.json or presence of ignore.devprune.json on any repo.
        // It applies the same trim, so the indices it hands back still address `repos`.
        let registry_ref = &registry;
        match status_view::render_status_tui(&|| {
            engine::take_top(&engine::get_full_status(registry_ref), top)
        }) {
            Ok(Some(candidates)) if !candidates.is_empty() => {
                // User confirmed a prune from within the status view. The TUI hands
                // back paths, not indices — an `i` toggle reloads its list, and
                // indices into the reloaded list do not address `repos` above.
                output::print_header("Pruning Selected Repositories");

                let mut total_freed: u64 = 0;
                let mut pruned_count = 0;
                let mut error_count = 0;
                // A prune is a prune wherever it was started from. Without this the
                // dashboard's own pass left no record, so `devp restore --last-run`
                // silently restored an *older* one.
                let mut pruned_dirs: Vec<crate::config::PrunedDir> = Vec::new();
                let pass_at = chrono::Utc::now();

                // The dashboard offered only repositories its own analysis classed as
                // candidates, so the idle check is settled; everything else follows
                // the user's settings, exactly as `devp run` resolves them. The bare
                // `prune_repo` defaults used here before ignored the configured scan
                // depth, command timeout and manifest-rewrite policy.
                let opts = engine::PruneOptions {
                    idle_days: 0,
                    dry_run: false,
                    force: true,
                    only_dirs: None,
                    adapters: engine::AdapterFilter::default(),
                    min_size_bytes: registry
                        .settings
                        .min_size_mb
                        .saturating_mul(engine::BYTES_PER_MIB),
                    scan_depth: registry.settings.scan_depth,
                    allow_manifest_rewrite: registry.settings.allow_manifest_rewrite,
                    command_timeout_secs: registry.settings.command_timeout_secs,
                    build_idle_days: registry.settings.build_idle_days,
                    adapter_idle_days: registry.settings.adapter_idle_days.clone(),
                };

                for path in &candidates {
                    let recorded_before = pruned_dirs.len();
                    let results = engine::prune_repo_with(path, &opts);
                    for result in results {
                        match &result.status {
                            PruneStatus::Pruned => {
                                total_freed += result.size_freed;
                                pruned_count += 1;
                                registry.mark_pruned(&result.repo_path, result.size_freed);
                                pruned_dirs.push(crate::config::PrunedDir {
                                    repo_path: result.repo_path.clone(),
                                    bloat_dir: result.bloat_dir.clone(),
                                    adapter: result.adapter_name.clone(),
                                    size_freed: result.size_freed,
                                    runtime: result.runtime.clone(),
                                });
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
                                crate::commands::run::report_lockfile_failure(&result, e);
                            }
                            PruneStatus::DeleteError(e) => {
                                error_count += 1;
                                // A non-zero size_freed on a delete error means the
                                // delete got half-way: the directory is corrupt, not
                                // intact. Record it so `devp restore --last-run` can
                                // rebuild it — the error still fails the pass.
                                if result.size_freed > 0 {
                                    pruned_dirs.push(crate::config::PrunedDir {
                                        repo_path: result.repo_path.clone(),
                                        bloat_dir: result.bloat_dir.clone(),
                                        adapter: result.adapter_name.clone(),
                                        size_freed: result.size_freed,
                                        runtime: result.runtime.clone(),
                                    });
                                }
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
                            // A warning, not an error: linked storage is deliberately
                            // left alone and must not fail the pass.
                            PruneStatus::SkippedSymlink(e) => {
                                output::print_warning(&format!(
                                    "{} → {}",
                                    output::clean_path(&result.repo_path),
                                    e.trim(),
                                ));
                            }
                            _ => {}
                        }
                    }

                    // Persisted after every repository, same as `devp run`: a pass
                    // killed half-way through must not leave `--last-run` describing
                    // the previous one. A save failure here is silent — the final
                    // save below reports it.
                    if pruned_dirs.len() > recorded_before {
                        registry.record_prune_progress(pass_at, pruned_dirs.clone());
                        let _ = registry.save();
                    }
                }

                registry.record_prune_progress(pass_at, pruned_dirs);
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

/// The `--drift` mode: every registered repository, checked for installed-but-unrecorded
/// packages.
///
/// This is the same comparison a prune refuses on, run early and as a pure read — no
/// package manager is executed and nothing is written. Only the adapters that can
/// compare an environment against its lockfile from files alone take part (npm, uv,
/// venv); the others have nothing cheap to say and stay silent rather than guessing.
fn run_drift(json_output: bool) -> Result<()> {
    let registry = Registry::load()?;

    let pb = (!json_output)
        .then(|| output::create_spinner("Comparing environments against lockfiles..."));

    let mut findings: Vec<ProjectDrift> = Vec::new();
    for path in registry.repositories.keys() {
        if !path.exists() {
            continue;
        }
        let depth = workspace::resolve_depth(path, registry.settings.scan_depth);
        for project in workspace::discover_to_depth(path, depth) {
            for adapter in &project.adapters {
                for report in adapter.drift(&project.path) {
                    findings.push(ProjectDrift {
                        repository: path.clone(),
                        project: project.relative.clone(),
                        adapter: adapter.name(),
                        report,
                    });
                }
            }
        }
    }
    // The registry is a HashMap; without this the same machine lists its drift in a
    // different order on every run, which reads like the drift itself changed.
    findings.sort_by(|a, b| {
        (&a.repository, &a.project, a.adapter, &a.report.directory).cmp(&(
            &b.repository,
            &b.project,
            b.adapter,
            &b.report.directory,
        ))
    });

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if json_output {
        return crate::json::emit(&crate::json::drift_document(&findings));
    }

    output::print_header("Lockfile drift");
    println!();

    if findings.is_empty() {
        output::print_success(
            "No drift found: nothing is installed that the lockfiles do not record.",
        );
        output::print_info(
            "Checked where a cheap file-level comparison exists: node_modules against \
             package-lock.json (npm), .venv against uv.lock (uv), and every virtual \
             environment against requirements.txt (venv).",
        );
        return Ok(());
    }

    let mut last_repo: Option<&std::path::Path> = None;
    for f in &findings {
        if last_repo != Some(f.repository.as_path()) {
            println!("  {}", output::clean_path(&f.repository));
            last_repo = Some(f.repository.as_path());
        }
        let location = if f.project == "." {
            f.report.directory.clone()
        } else {
            format!("{}/{}", f.project, f.report.directory)
        };
        let shown = f
            .report
            .unrecorded
            .iter()
            .take(10)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if f.report.unrecorded.len() > 10 {
            format!(", … and {} more", f.report.unrecorded.len() - 10)
        } else {
            String::new()
        };
        println!(
            "    {} ({}): {} unrecorded {} — {shown}{suffix}",
            location,
            f.adapter,
            f.report.unrecorded.len(),
            output::plural(f.report.unrecorded.len(), "package", "packages"),
        );
        println!("      record them: {}", f.report.record_command);
        println!();
    }

    output::print_info(
        "A prune refuses to delete these environments as they are — the unrecorded \
         packages would be lost with no way back. Record them with the command shown, \
         or uninstall them, and the refusal goes away.",
    );
    Ok(())
}

/// "How long is this to undo?", answered only when this machine has measured enough to
/// answer it.
///
/// The question `devp status` could not answer before. Space it already reports; what
/// people hesitate over is the reinstall, and every number in this line comes from
/// restores timed on this machine by `devp restore --last-run` — never from a table of
/// typical speeds, which would be a number about somebody else's laptop. An adapter that
/// has never been timed here contributes nothing and is subtracted from the coverage,
/// so a partial answer says it is partial instead of reading as a whole one.
fn restore_estimate_line(registry: &Registry, repos: &[engine::RepoStatusEntry]) -> Option<String> {
    let mut by_adapter: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for repo in repos {
        for (adapter, bytes) in &repo.reclaimable_by_adapter {
            *by_adapter.entry(adapter.clone()).or_default() += bytes;
        }
    }
    let total: u64 = by_adapter.values().sum();
    let tallied: Vec<(String, u64)> = by_adapter.into_iter().collect();
    let (secs, covered) = registry.estimate_restore(&tallied)?;

    let samples: usize = registry
        .restore_rates
        .values()
        .map(|r| r.samples as usize)
        .sum();
    let mut line = format!(
        "Estimated Restore Cost: ~{} to put it all back, from {} timed {} on this machine",
        output::format_seconds(secs.round() as u64),
        samples,
        output::plural(samples, "restore", "restores"),
    );
    if covered < total {
        line.push_str(&format!(
            " (covers {} of {} — the rest has never been restored here)",
            output::format_bytes(covered),
            output::format_bytes(total),
        ));
    }
    Some(line)
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
