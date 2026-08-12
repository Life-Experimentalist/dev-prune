// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune run` command.
//
// Executes a full prune pass across all registered repositories.
// Supports pre-deletion analysis, optimized ecosystem binary pre-checks,
// interactive TUI multi-selection, progressive deletion, shell-specific
// troubleshooting, and interactive error fallback.

use anyhow::Result;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use crate::adapters;
use crate::config::Registry;
use crate::constants;
use crate::engine::{self, AdapterFilter, PruneOptions, PruneResult, PruneStatus};
use crate::json;
use crate::output;
use crate::tui;

/// Everything `devp run` was asked to do.
///
/// A struct rather than nine positional parameters: the call site in `run_cli` reads as
/// a list of names, and adding a flag does not silently shift an argument.
pub struct RunArgs<'a> {
    /// Optional single workspace to act on instead of the whole registry.
    pub target_path: Option<&'a str>,
    /// Report sizes and stop.
    pub dry_run: bool,
    /// Bypass the idle threshold. Lockfile verification still applies.
    pub force: bool,
    /// Skip the confirmation prompt.
    pub yes: bool,
    /// This is the scheduled background pass.
    pub daemon: bool,
    /// Comma-separated adapters to act on exclusively.
    pub only: Option<&'a str>,
    /// Comma-separated adapters to leave alone.
    pub skip: Option<&'a str>,
    /// Size floor in MiB, overriding the configured `min_size_mb`.
    pub min_size_mb: Option<u64>,
    /// Comma-separated repositories to leave completely alone this pass.
    pub except: Option<&'a str>,
    /// Emit one JSON document instead of the human report.
    pub json: bool,
}

/// Run the `run` command — prune all registered repos or a specific target directory (`devp run .`).
///
/// `daemon` marks the scheduled background pass; repositories that set `disable_daemon`
/// in `.devprune.json` are excluded from it but remain pruneable by hand.
pub fn run(args: RunArgs<'_>) -> Result<()> {
    let filter = AdapterFilter::new(args.only, args.skip)?;

    // In JSON mode there is no one to answer a prompt and no terminal to draw a selector
    // in, so deletion has to have been authorised on the command line. Failing loudly
    // beats either silently deleting or silently doing nothing.
    if args.json && !args.dry_run && !args.yes {
        anyhow::bail!(
            "`--json` cannot ask for confirmation. Pass `--dry-run` to analyse, or `--yes` to delete."
        );
    }

    if !args.json {
        output::print_banner();
        if args.force {
            print_ignore_idle_notice();
        }
    }

    if let Some(target_str) = args.target_path {
        return run_targeted(&args, &filter, target_str);
    }
    run_registry(&args, &filter)
}

/// What `--ignore-idle` does and, more usefully, what it does not.
///
/// Printed whenever the idle check is bypassed, because that is the moment someone is
/// most likely to be working around a problem rather than solving it — and the problem
/// they hit is almost always one of the three below. Suppressed in JSON mode, where the
/// document is the contract and prose on stdout would corrupt it.
fn print_ignore_idle_notice() {
    output::print_warning(
        "Idle check bypassed — repositories you are working in right now are fair game.",
    );
    println!(
        "  Still enforced: lockfile verification, `ignore.devprune.json`, `\"ignore\": true`,"
    );
    println!(
        "  symlinked directories, and nested repositories. This flag does not turn those off."
    );
    println!();
    println!("  If you reached for this because something would not prune, it is usually:");
    println!("    • \"lockfile verification failed\"  → run the fix command printed next to it;");
    println!("      it regenerates the lockfile so the reinstall is guaranteed to work.");
    println!("    • nothing listed at all            → the project is deeper than `scan_depth`,");
    println!("      or under `min_size_mb`. Try `devp status` to see what dev-prune can see.");
    println!("    • \"could not be examined\"          → `.devprune.json` has a syntax error.");
    println!();
    println!("  Still stuck? Point your AI assistant at the bundled skill — `devp skill`");
    println!("  exports a SKILL.md that teaches it this tool, exit codes and all. It has");
    println!("  read the manual more recently than either of us.");
    println!();
}

/// `devp run <PATH>` — one workspace, no registry, no selector.
fn run_targeted(args: &RunArgs<'_>, filter: &AdapterFilter, target_str: &str) -> Result<()> {
    let raw = std::path::Path::new(target_str);
    let path = if raw.exists() {
        raw.canonicalize().unwrap_or_else(|_| raw.to_path_buf())
    } else {
        raw.to_path_buf()
    };

    let clean = output::clean_path(&path);
    if !crate::scanner::is_git_repo(&path) {
        // Returning Ok here made `devp run <path>` exit 0 on a path it refused to
        // touch, which is invisible to any script or CI step checking the status.
        anyhow::bail!("{clean} is not a Git repository — dev-prune only prunes Git repos.");
    }

    // A targeted run still respects the configured idle threshold. Passing 0 here
    // would make every repo look idle and silently defeat the guard — `--ignore-idle` is
    // the documented way to prune a repo you are actively working in.
    let registry = Registry::load().ok();
    let idle_days = registry
        .as_ref()
        .map(|r| {
            r.repositories
                .get(&path)
                .and_then(|e| e.override_idle_days)
                .unwrap_or(r.settings.idle_days)
        })
        .unwrap_or(constants::DEFAULT_IDLE_DAYS);

    let opts = PruneOptions {
        idle_days,
        dry_run: args.dry_run,
        force: args.force,
        only_dirs: None,
        adapters: filter.clone(),
        min_size_bytes: resolve_min_size(args, registry.as_ref()),
        scan_depth: resolve_scan_depth(registry.as_ref()),
        allow_manifest_rewrite: resolve_manifest_rewrite(registry.as_ref()),
        command_timeout_secs: resolve_command_timeout(registry.as_ref()),
    };

    let results = engine::prune_repo_with(&path, &opts);

    // A directory that could not be verified or deleted is a failure of the command,
    // whichever output mode asked for it. `devp run <path>` used to exit 0 after a
    // lockfile or delete error, which a script or CI step has no way to notice.
    let error_count = results
        .iter()
        .filter(|r| {
            matches!(
                r.status,
                PruneStatus::LockfileError(_)
                    | PruneStatus::DeleteError(_)
                    | PruneStatus::ConfigError(_)
            )
        })
        .count();

    // Recorded before the output branches, so `--json` and the human report leave the
    // same registry behind. A targeted run used to update neither the lifetime totals nor
    // anything `restore` could read: `devp run .` freed two gigabytes and `devp status`
    // still said nothing had ever been pruned.
    record_targeted_prune(&path, &results, args.dry_run);

    if args.json {
        json::emit(&json::run_document(&results, args.dry_run))?;
        if error_count > 0 {
            anyhow::bail!("{error_count} directories in {clean} could not be pruned.");
        }
        return Ok(());
    }

    output::print_header(&format!("dev-prune Targeted Run ({clean})"));
    if let Some(desc) = filter.describe() {
        output::print_info(&format!("Adapter filter: {desc}"));
    }

    if results.is_empty() {
        output::print_info(&format!("No pruneable bloat directories found in {clean}."));
        return Ok(());
    }

    let mut total_freed = 0;
    for result in results {
        match &result.status {
            PruneStatus::Pruned => {
                total_freed += result.size_freed;
                output::print_success(&format!(
                    "{} → {} ({}) — {}",
                    output::clean_path(&result.repo_path),
                    result.bloat_dir,
                    output::format_bytes(result.size_freed),
                    result.adapter_name
                ));
            }
            PruneStatus::SkippedDryRun => {
                output::print_info(&format!(
                    "  • {} → {} ({}) [{}] (Dry Run)",
                    output::clean_path(&result.repo_path),
                    result.bloat_dir,
                    output::format_bytes(result.size_freed),
                    result.adapter_name
                ));
            }
            PruneStatus::SkippedActive => {
                output::print_info(&format!(
                    "{clean} is currently active (not idle). Use `devp --ignore-idle run` to override."
                ));
            }
            PruneStatus::LockfileError(e) => {
                output::print_error(&format!("{clean} lockfile sync failed:\n    {}", e.trim()));
            }
            PruneStatus::DeleteError(e) => {
                output::print_error(&format!("{clean} delete error: {e}"));
            }
            PruneStatus::ConfigError(e) => {
                output::print_error(&format!(
                    "{clean} skipped — its .devprune.json could not be read:\n    {}\n    \
                     Fix it, or run `devp config {clean} --update` to reset it.",
                    e.trim()
                ));
            }
            _ => {}
        }
    }

    if !args.dry_run && total_freed > 0 {
        output::print_success(&format!(
            "Freed: {} in {clean}",
            output::format_bytes(total_freed)
        ));
    }

    if error_count > 0 {
        anyhow::bail!("{error_count} directories in {clean} could not be pruned.");
    }

    Ok(())
}

/// Persist what a targeted run deleted: the lifetime totals and the `--last-run` record.
///
/// Silent on every failure. The directories are already gone by the time this is called,
/// and a registry that could not be written is not a reason to report the prune itself as
/// failed — it only costs the user `devp restore --last-run` for this one pass.
fn record_targeted_prune(path: &std::path::Path, results: &[PruneResult], dry_run: bool) {
    if dry_run {
        return;
    }

    let pruned: Vec<crate::config::PrunedDir> = results
        .iter()
        .filter(|r| matches!(r.status, PruneStatus::Pruned))
        .map(|r| crate::config::PrunedDir {
            repo_path: r.repo_path.clone(),
            bloat_dir: r.bloat_dir.clone(),
            adapter: r.adapter_name.clone(),
            size_freed: r.size_freed,
        })
        .collect();

    if pruned.is_empty() {
        return;
    }

    let freed: u64 = pruned.iter().map(|d| d.size_freed).sum();
    if let Ok(mut registry) = Registry::load() {
        registry.mark_pruned(path, freed);
        registry.record_prune(pruned);
        let _ = registry.save();
    }
}

/// The size floor for this pass: `--min-size` if given, otherwise the global setting.
///
/// A per-repository `min_size_mb` still wins over both — that decision belongs to the
/// repository and is applied inside the engine.
fn resolve_min_size(args: &RunArgs<'_>, registry: Option<&Registry>) -> u64 {
    let mb = args
        .min_size_mb
        .or_else(|| registry.map(|r| r.settings.min_size_mb))
        .unwrap_or(constants::DEFAULT_MIN_SIZE_MB);
    mb.saturating_mul(engine::BYTES_PER_MIB)
}

/// Repositories named by `--except`, as a set of lowercased names and path fragments.
///
/// Empty when the flag was not passed.
///
/// Each entry is tilde-expanded first, because a comma-separated list arrives as one
/// argument and no shell expands a `~` sitting in the middle of it — not even bash.
fn parse_except(spec: Option<&str>) -> Vec<String> {
    spec.map(|s| {
        s.split(',')
            .map(|part| {
                crate::config::expand_tilde(part.trim())
                    .trim_end_matches(['/', '\\'])
                    .to_lowercase()
            })
            .filter(|part| !part.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Whether `--except` names this repository.
///
/// Matched three ways because there are three things a user reasonably types: the folder
/// name (`api`), a path fragment (`work/api`), or the full path they see in `devp status`.
/// Case-insensitive, and `/` and `\` are treated as the same separator, so the flag
/// behaves the same in PowerShell and in bash.
fn is_excepted(repo_path: &Path, except: &[String]) -> bool {
    if except.is_empty() {
        return false;
    }
    let full = output::clean_path(repo_path)
        .to_lowercase()
        .replace('\\', "/");
    let name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    except.iter().any(|want| {
        let want = want.replace('\\', "/");
        name == want || full == want || full.ends_with(&format!("/{want}"))
    })
}

/// The global scan depth, falling back to the default when there is no registry yet.
fn resolve_scan_depth(registry: Option<&Registry>) -> usize {
    registry
        .map(|r| r.settings.scan_depth)
        .unwrap_or(constants::DEFAULT_SCAN_DEPTH)
}

/// The ceiling on any one package-manager command, in seconds.
fn resolve_command_timeout(registry: Option<&Registry>) -> u64 {
    registry
        .map(|r| r.settings.command_timeout_secs)
        .unwrap_or(constants::DEFAULT_COMMAND_TIMEOUT_SECS)
}

/// Whether an adapter may run its lockfile-rewriting sync command.
fn resolve_manifest_rewrite(registry: Option<&Registry>) -> bool {
    registry
        .map(|r| r.settings.allow_manifest_rewrite)
        .unwrap_or(constants::DEFAULT_ALLOW_MANIFEST_REWRITE)
}

/// `devp run` — the full pass over every registered repository.
fn run_registry(args: &RunArgs<'_>, filter: &AdapterFilter) -> Result<()> {
    if !args.json {
        if args.dry_run {
            output::print_header("dev-prune run (DRY RUN)");
        } else {
            output::print_header("dev-prune run");
        }
    }

    let mut registry = Registry::load()?;

    // Suppressed in JSON mode: the document is a contract, and a version notice printed
    // into it would corrupt the output.
    if !args.json && crate::commands::update::notify_if_outdated(&mut registry) {
        let _ = registry.save();
    }

    if registry.repo_count() == 0 {
        if args.json {
            return json::emit(&json::run_document(&[], args.dry_run));
        }
        output::print_warning("No repositories registered. Run `dev-prune init` first.");
        return Ok(());
    }

    // Validated against the registry *before* anything is analysed. A name that matches
    // nothing is a typo, and the cost of a silent typo here is the one repository the
    // user was trying to protect getting pruned — so it is an error, not a no-op.
    let except = parse_except(args.except);
    if !except.is_empty() {
        let unmatched: Vec<&String> = except
            .iter()
            .filter(|want| {
                !registry
                    .repositories
                    .keys()
                    .any(|p| is_excepted(p, std::slice::from_ref(*want)))
            })
            .collect();
        if !unmatched.is_empty() {
            anyhow::bail!(
                "`--except` names no registered repository: {}\n  \
                 Run `devp status` to see the registered names.",
                unmatched
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    let min_size_bytes = resolve_min_size(args, Some(&registry));
    let analysis = PruneOptions {
        idle_days: 0, // replaced per repository from the registry
        dry_run: true,
        force: args.force,
        only_dirs: None,
        adapters: filter.clone(),
        min_size_bytes,
        scan_depth: resolve_scan_depth(Some(&registry)),
        allow_manifest_rewrite: resolve_manifest_rewrite(Some(&registry)),
        command_timeout_secs: resolve_command_timeout(Some(&registry)),
    };

    if !args.json {
        output::print_info(&format!(
            "Scanning {} registered repositories for prune candidates...",
            registry.repo_count()
        ));
        if let Some(desc) = filter.describe() {
            output::print_info(&format!("Adapter filter: {desc}"));
        }
        if min_size_bytes > 0 {
            output::print_info(&format!(
                "Size floor: ignoring directories under {}",
                output::format_bytes(min_size_bytes)
            ));
        }
    }

    // Pre-run analysis (dry-run mode first to compute exact savings)
    //
    // Two lists come out of it, and both are reported. A repository the analysis refused
    // to examine — an unreadable `.devprune.json`, most often — used to be dropped here
    // along with every other non-candidate state, so a pass that had quietly skipped it
    // still ended on "No idle repositories or pruneable bloat directories found." and
    // exit 0. The execution loop further down knows how to report these states, but it
    // only ever sees selected candidates, so it never got the chance.
    let mut candidates: Vec<PruneResult> = Vec::new();
    let mut blocked: Vec<PruneResult> = Vec::new();
    for result in engine::prune_all_with(&mut registry, &analysis) {
        // An excepted repository leaves the pass entirely — including its failures. The
        // user said not to touch it, so a broken config in there is not this run's
        // problem and must not fail an otherwise clean exit code.
        if is_excepted(&result.repo_path, &except) {
            continue;
        }
        match result.status {
            PruneStatus::SkippedDryRun => candidates.push(result),
            PruneStatus::ConfigError(_)
            | PruneStatus::LockfileError(_)
            | PruneStatus::DeleteError(_) => blocked.push(result),
            _ => {}
        }
    }

    if !args.json && !except.is_empty() {
        output::print_info(&format!("Leaving alone: {}", except.join(", ")));
    }

    if args.daemon {
        let before = candidates.len();
        candidates.retain(|c| {
            // An unreadable config drops the candidate. The engine already refuses such a
            // repository outright, so this cannot fire today; if that ever changes, the
            // unattended pass must not be the code path that guesses.
            match crate::config::PerRepoConfig::load_with_diagnostics(&c.repo_path) {
                Ok(Some(cfg)) => !cfg.disable_daemon,
                Ok(None) => true,
                Err(_) => false,
            }
        });
        let skipped = before - candidates.len();
        if skipped > 0 && !args.json {
            output::print_info(&format!(
                "Skipped {skipped} bloat directories in repositories that set `disable_daemon`."
            ));
        }
    }

    // A dry run stops here in both output modes: sizes are known, nothing was verified.
    if args.dry_run {
        if args.json {
            json::emit(&json::run_document(&[candidates, blocked].concat(), true))?;
            return Ok(());
        }
        if candidates.is_empty() && blocked.is_empty() {
            output::print_info("No idle repositories or pruneable bloat directories found.");
            return Ok(());
        }
        if !candidates.is_empty() {
            report_candidates(&candidates);
        }
        let total: u64 = candidates.iter().map(|c| c.size_freed).sum();
        output::print_header("Summary (Dry Run)");
        output::print_info(&format!(
            "Would free {} across {} bloat directories.",
            output::format_bytes(total),
            candidates.len()
        ));
        // Reported, but not an error: a dry run's job is to say what it found, and it
        // found this too.
        report_blocked(&blocked);
        return Ok(());
    }

    if candidates.is_empty() {
        if args.json {
            json::emit(&json::run_document(&blocked, false))?;
            return fail_if_blocked(&blocked);
        }
        if blocked.is_empty() {
            output::print_info("No idle repositories or pruneable bloat directories found.");
            return Ok(());
        }
        output::print_info("No pruneable bloat directories found.");
        report_blocked(&blocked);
        return fail_if_blocked(&blocked);
    }

    let total_reclaimable: u64 = candidates.iter().map(|c| c.size_freed).sum();

    if !args.json {
        report_binaries(&candidates);
        report_candidates(&candidates);
        output::print_info(&format!(
            "Total Reclaimable Space: {}",
            output::format_bytes(total_reclaimable)
        ));
        report_blocked(&blocked);
    }

    // Determine target candidates to prune (either interactive TUI selection or all).
    // `--json` short-circuits both: it was already required to carry `--yes`.
    let target_candidates: Vec<PruneResult> = if args.json
        || args.yes
        || !registry.settings.require_confirmation
    {
        candidates
    } else if io::stdout().is_terminal() {
        eprintln!();
        eprintln!(
            "  Loading interactive selector... (↑↓ navigate, Space toggle, Enter confirm, q cancel)"
        );
        eprintln!();
        let selected = tui::selection_view::select_candidates_tui(&candidates)?;
        if selected.is_empty() {
            output::print_info("Prune pass cancelled by user (0 candidates selected).");
            return Ok(());
        }
        selected
    } else {
        println!();
        output::print_warning("CAUTION: Deleting bloat directories cannot be undone directly.");
        output::print_info(
            "Note: You can re-install missing dependencies anytime using `dev-prune restore`.",
        );
        print!(
            "Proceed with deletion of {} directories ({})? [y/N]: ",
            candidates.len(),
            output::format_bytes(total_reclaimable)
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            output::print_info("Prune pass aborted by user.");
            return Ok(());
        }
        candidates
    };

    if !args.json {
        let selected_total_bytes: u64 = target_candidates.iter().map(|c| c.size_freed).sum();
        output::print_header(&format!(
            "Executing Progressive Deletion ({} repos, {})",
            target_candidates.len(),
            output::format_bytes(selected_total_bytes)
        ));
    }

    // Execute deletion ONLY on the selected bloat directories.
    //
    // The selector works per bloat directory, so group the selection by repo and pass
    // the chosen directory names down — pruning the whole repo would delete dirs the
    // user explicitly unticked.
    let mut selection: Vec<(std::path::PathBuf, Vec<String>)> = Vec::new();
    for candidate in &target_candidates {
        match selection
            .iter_mut()
            .find(|(p, _)| *p == candidate.repo_path)
        {
            Some((_, dirs)) => dirs.push(candidate.bloat_dir.clone()),
            None => selection.push((
                candidate.repo_path.clone(),
                vec![candidate.bloat_dir.clone()],
            )),
        }
    }

    // Seeded with what the analysis pass could not get past. Those repositories belong in
    // the document and in the exit code exactly as much as a failure from the loop below.
    let mut error_count = blocked.len();
    let mut all_results: Vec<PruneResult> = blocked;
    let mut total_freed: u64 = 0;
    let mut pruned_count = 0;
    let mut pruned_dirs: Vec<crate::config::PrunedDir> = Vec::new();

    for (repo_path, dirs) in &selection {
        // Candidates were already filtered through the idle check during the dry-run
        // analysis pass, so re-checking here would only re-walk the tree.
        let single_results = engine::prune_repo_with(
            repo_path,
            &PruneOptions {
                idle_days: 0,
                dry_run: false,
                force: true,
                only_dirs: Some(dirs.clone()),
                adapters: filter.clone(),
                min_size_bytes: 0,
                scan_depth: analysis.scan_depth,
                allow_manifest_rewrite: analysis.allow_manifest_rewrite,
                command_timeout_secs: analysis.command_timeout_secs,
            },
        );
        for result in single_results {
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
                    });
                    if !args.json {
                        output::print_success(&format!(
                            "{} → {} ({}) — {}",
                            output::clean_path(&result.repo_path),
                            result.bloat_dir,
                            output::format_bytes(result.size_freed),
                            result.adapter_name,
                        ));
                    }
                }
                PruneStatus::LockfileError(e) => {
                    error_count += 1;
                    if !args.json {
                        report_lockfile_failure(&result, e);
                    }
                }
                PruneStatus::DeleteError(e) => {
                    error_count += 1;
                    if !args.json {
                        output::print_error(&format!(
                            "{} → delete failed: {}",
                            output::clean_path(&result.repo_path),
                            e,
                        ));
                    }
                }
                PruneStatus::ConfigError(e) => {
                    error_count += 1;
                    if !args.json {
                        let clean_p = output::clean_path(&result.repo_path);
                        output::print_error(&format!(
                            "{clean_p} skipped — its .devprune.json could not be read:\n    {}",
                            e.trim()
                        ));
                        output::print_info(&format!(
                            "  Fix command:       devp config {clean_p} --update"
                        ));
                    }
                }
                _ => {}
            }
            all_results.push(result);
        }
    }

    registry.record_prune(pruned_dirs);
    registry.save()?;

    if args.json {
        json::emit(&json::run_document(&all_results, false))?;
        // The document already carries `summary.errors`; a non-zero exit keeps the
        // shell contract identical in both output modes.
        if error_count > 0 {
            anyhow::bail!("{error_count} repositories could not be pruned.");
        }
        return Ok(());
    }

    output::print_header("Summary");
    output::print_success(&format!(
        "Freed: {} across {pruned_count} directories",
        output::format_bytes(total_freed)
    ));

    if error_count > 0 {
        output::print_warning(&format!("{error_count} repos were not pruned."));

        // Only when a lockfile was actually the problem. `error_count` also counts
        // unreadable configs and failed deletions, and a lecture about lockfiles in front
        // of a JSON syntax error sends the user to the wrong file.
        if all_results
            .iter()
            .any(|r| matches!(r.status, PruneStatus::LockfileError(_)))
        {
            // Lockfile enforcement is not overridable — `--ignore-idle` only bypasses the idle
            // check. Without a lockfile a deleted dependency tree cannot be rebuilt, so
            // point at the fix instead of offering an override that does not exist.
            output::print_info(
                "Lockfile verification cannot be bypassed: without a lockfile the deleted \
                 dependencies could not be reinstalled. Run the fix command shown above for \
                 each repo, then re-run `devp run`.",
            );
        }
        // Exit non-zero so a scheduled or scripted run surfaces the failure.
        anyhow::bail!("{error_count} repositories could not be pruned.");
    }

    Ok(())
}

/// Report the repositories the analysis pass could not get past, with the fix for each.
///
/// Silent for an empty list, so callers do not have to guard it.
fn report_blocked(blocked: &[PruneResult]) {
    if blocked.is_empty() {
        return;
    }
    output::print_header("Repositories That Could Not Be Examined");
    for result in blocked {
        let clean_p = output::clean_path(&result.repo_path);
        match &result.status {
            PruneStatus::ConfigError(e) => {
                output::print_error(&format!(
                    "{clean_p} skipped — its .devprune.json could not be read:\n    {}",
                    e.trim()
                ));
                output::print_info(&format!(
                    "  Fix command:       devp config {clean_p} --update"
                ));
            }
            PruneStatus::LockfileError(e) => report_lockfile_failure(result, e),
            PruneStatus::DeleteError(e) => {
                output::print_error(&format!("{clean_p} → delete failed: {e}"));
            }
            // `blocked` is built from exactly the three arms above.
            _ => {}
        }
    }
}

/// Turn a non-empty blocked list into the process's failure exit.
///
/// A pass that skipped a repository the user asked it to handle has not succeeded, and a
/// scheduled or scripted run has to be able to see that.
fn fail_if_blocked(blocked: &[PruneResult]) -> Result<()> {
    if blocked.is_empty() {
        return Ok(());
    }
    anyhow::bail!("{} repositories could not be examined.", blocked.len());
}

/// Report which ecosystem binaries the pass will need and whether they are present.
fn report_binaries(candidates: &[PruneResult]) {
    let adapter_names: Vec<String> = candidates.iter().map(|c| c.adapter_name.clone()).collect();
    let binary_statuses = adapters::scan_required_binaries(&adapter_names);
    if binary_statuses.is_empty() {
        return;
    }
    output::print_header("Required Ecosystem Binaries Pre-Check");
    for b in &binary_statuses {
        if b.available {
            output::print_success(&format!(
                "  {} — available ({})",
                b.name,
                b.version.as_deref().unwrap_or("detected")
            ));
        } else {
            output::print_warning(&format!(
                "  {} — missing (lockfile fallback active)",
                b.name
            ));
        }
    }
}

fn report_candidates(candidates: &[PruneResult]) {
    output::print_header("Prune Candidates & Space Savings Calculation");
    for candidate in candidates {
        output::print_info(&format!(
            "  • {} → {} ({}) [{}]",
            output::clean_path(&candidate.repo_path),
            candidate.bloat_dir,
            output::format_bytes(candidate.size_freed),
            candidate.adapter_name
        ));
    }
}

fn report_lockfile_failure(result: &PruneResult, error: &str) {
    let clean_p = output::clean_path(&result.repo_path);
    let sync_cmd_help =
        json::lockfile_fix_command(&result.adapter_name).unwrap_or("check the adapter's docs");

    #[cfg(windows)]
    let manual_cmd = format!("cd \"{}\"; {}", clean_p, sync_cmd_help);
    #[cfg(not(windows))]
    let manual_cmd = format!("cd \"{}\" && {}", clean_p, sync_cmd_help);

    output::print_error(&format!(
        "{} → {} lockfile sync failed:\n    {}",
        clean_p,
        result.adapter_name,
        error.trim(),
    ));
    output::print_info(&format!("  Fix command:       {}", manual_cmd));
    output::print_info(&format!(
        "  Troubleshooting:   {}",
        constants::TROUBLESHOOTING_URL
    ));
}
