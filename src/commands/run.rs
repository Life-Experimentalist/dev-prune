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
use crate::i18n;
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
    /// Explain every decision and touch nothing.
    pub explain: bool,
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
        return Err(anyhow::Error::new(crate::UsageError(
            "`--json` cannot ask for confirmation. Pass `--dry-run` to analyse, or `--yes` to delete."
                .to_string(),
        )));
    }

    if !args.json {
        output::print_banner();
        if args.force {
            print_ignore_idle_notice();
        }
    }

    if args.explain {
        return run_explain(&args, &filter);
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
        build_idle_days: resolve_build_idle_days(registry.as_ref()),
        adapter_idle_days: resolve_adapter_idle_days(registry.as_ref()),
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
                    | PruneStatus::ActivityCheckError(_)
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

    output::print_header(&i18n::tf(
        "run.header.targeted",
        &[("path", clean.as_str())],
    ));
    if let Some(desc) = filter.describe() {
        output::print_info(&format!("Adapter filter: {desc}"));
    }

    if results.is_empty() {
        output::print_info(&i18n::tf(
            "run.nothing.bloat.targeted",
            &[("path", clean.as_str())],
        ));
        return Ok(());
    }

    let mut total_freed = 0;
    for result in results {
        match &result.status {
            PruneStatus::Pruned => {
                total_freed += result.size_freed;
                output::print_success(&format!(
                    "{} → {} ({}) — {}{}",
                    output::clean_path(&result.repo_path),
                    result.bloat_dir,
                    output::format_bytes(result.size_freed),
                    result.adapter_name,
                    output::shared_note(result.shared_bytes, &result.adapter_name)
                ));
            }
            PruneStatus::SkippedDryRun => {
                output::print_info(&format!(
                    "  • {} → {} ({}) [{}] (Dry Run){}",
                    output::clean_path(&result.repo_path),
                    result.bloat_dir,
                    output::format_bytes(result.size_freed),
                    result.adapter_name,
                    output::shared_note(result.shared_bytes, &result.adapter_name)
                ));
            }
            PruneStatus::SkippedActive => {
                output::print_info(&format!(
                    "{clean} is currently active (not idle). Use `devp --ignore-idle run` to override."
                ));
            }
            PruneStatus::LockfileError(e) => report_lockfile_failure(&result, e),
            PruneStatus::ActivityCheckError(e) => {
                output::print_error(&format!(
                    "{clean} skipped — its activity could not be determined:\n    {}",
                    e.trim()
                ));
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
            PruneStatus::SkippedSymlink(e) => {
                output::print_warning(&format!("{clean} → {}", e.trim()));
            }
            PruneStatus::SkippedDeclaration(e) => {
                output::print_warning(&format!("{clean} → {}", e.trim()));
            }
            _ => {}
        }
    }

    if !args.dry_run && total_freed > 0 {
        output::print_success(&i18n::tf(
            "run.freed.targeted",
            &[
                ("size", &output::format_bytes(total_freed)),
                ("path", clean.as_str()),
            ],
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

    // A DeleteError with a non-zero size_freed is a delete that got half-way: the
    // directory is corrupt, not intact, so `restore --last-run` must know to rebuild it.
    let pruned: Vec<crate::config::PrunedDir> = results
        .iter()
        .filter(|r| {
            matches!(r.status, PruneStatus::Pruned)
                || (matches!(r.status, PruneStatus::DeleteError(_)) && r.size_freed > 0)
        })
        .map(|r| crate::config::PrunedDir {
            repo_path: r.repo_path.clone(),
            bloat_dir: r.bloat_dir.clone(),
            adapter: r.adapter_name.clone(),
            size_freed: r.size_freed,
            runtime: r.runtime.clone(),
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

/// The idle window for adapters holding compiler output, in days.
fn resolve_build_idle_days(registry: Option<&Registry>) -> u64 {
    registry
        .map(|r| r.settings.build_idle_days)
        .unwrap_or(constants::DEFAULT_BUILD_IDLE_DAYS)
}

/// The user's per-adapter idle windows, empty when there is no registry yet.
fn resolve_adapter_idle_days(
    registry: Option<&Registry>,
) -> std::collections::BTreeMap<String, u64> {
    registry
        .map(|r| r.settings.adapter_idle_days.clone())
        .unwrap_or_default()
}

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
            output::print_header(i18n::t("run.header.dry"));
        } else {
            output::print_header(i18n::t("run.header"));
        }
    }

    let mut registry = Registry::load()?;

    // A repository `git init` created fires no Git hook and so never registered itself.
    // Picking it up here is what keeps `devp run` from reporting "No repositories
    // registered" while standing inside one. See `link::adopt_enclosing_repo`.
    let adopted = crate::commands::link::adopt_enclosing_repo(&mut registry);
    if adopted.is_some() {
        registry.save()?;
    }
    if let Some(path) = &adopted
        && !args.json
    {
        crate::commands::link::report_cwd_adoption(path);
        println!();
    }

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
        build_idle_days: resolve_build_idle_days(Some(&registry)),
        adapter_idle_days: resolve_adapter_idle_days(Some(&registry)),
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
    let mut left_alone: Vec<PruneResult> = Vec::new();
    let mut missing: Vec<PruneResult> = Vec::new();
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
            | PruneStatus::ActivityCheckError(_)
            | PruneStatus::DeleteError(_) => blocked.push(result),
            // Reported, never failed on: the link is permanent and deliberate, and a
            // "failure" here made every scheduled pass over the repo exit 1 forever.
            // A refused declaration joins it for the same reason — it is a standing
            // state of the repository's own config, not something this pass did wrong.
            PruneStatus::SkippedSymlink(_) | PruneStatus::SkippedDeclaration(_) => {
                left_alone.push(result)
            }
            // Same reasoning: a deleted clone stays deleted, and failing on it would
            // keep every scheduled pass red until the entry is unlinked.
            PruneStatus::PathMissing => missing.push(result),
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
            json::emit(&json::run_document(
                &[candidates, blocked, left_alone, missing].concat(),
                true,
            ))?;
            return Ok(());
        }
        if candidates.is_empty()
            && blocked.is_empty()
            && left_alone.is_empty()
            && missing.is_empty()
        {
            output::print_info(i18n::t("run.nothing"));
            return Ok(());
        }
        if !candidates.is_empty() {
            report_candidates(&candidates);
        }
        let total: u64 = candidates.iter().map(|c| c.size_freed).sum();
        output::print_header(i18n::t("run.summary.dry"));
        output::print_info(&i18n::tf(
            "run.would_free",
            &[
                ("size", &output::format_bytes(total)),
                ("count", &candidates.len().to_string()),
            ],
        ));
        // Reported, but not an error: a dry run's job is to say what it found, and it
        // found this too.
        report_blocked(&blocked);
        report_left_alone(&left_alone);
        report_missing(&missing);
        return Ok(());
    }

    if candidates.is_empty() {
        if args.json {
            json::emit(&json::run_document(
                &[blocked.clone(), left_alone, missing].concat(),
                false,
            ))?;
            return fail_if_blocked(&blocked);
        }
        if blocked.is_empty() && left_alone.is_empty() && missing.is_empty() {
            output::print_info(i18n::t("run.nothing"));
            return Ok(());
        }
        output::print_info(i18n::t("run.nothing.bloat"));
        report_blocked(&blocked);
        report_left_alone(&left_alone);
        report_missing(&missing);
        return fail_if_blocked(&blocked);
    }

    let total_reclaimable: u64 = candidates.iter().map(|c| c.size_freed).sum();

    if !args.json {
        report_binaries(&candidates);
        report_candidates(&candidates);
        output::print_info(&i18n::tf(
            "run.reclaimable",
            &[("size", &output::format_bytes_styled(total_reclaimable))],
        ));
        report_blocked(&blocked);
        report_left_alone(&left_alone);
        report_missing(&missing);
    }

    // Determine target candidates to prune (either interactive TUI selection or all).
    // `--json` short-circuits both: it was already required to carry `--yes`.
    let target_candidates: Vec<PruneResult> = if args.json
        || args.yes
        || !registry.settings.require_confirmation
    {
        candidates
    } else if io::stdout().is_terminal() && io::stdin().is_terminal() {
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
        // Reaching here means stdout is piped. If stdin is too, there is nobody to
        // answer: the read hits EOF at once, and the old code then reported "aborted by
        // user" about a user who was never asked. Failing with the fix beats that.
        if !io::stdin().is_terminal() {
            anyhow::bail!(
                "Deleting {} directories ({}) needs confirmation, and there is no \
                 terminal to ask on. Re-run with `--yes` to confirm, or `--dry-run` \
                 to only analyse.",
                candidates.len(),
                output::format_bytes(total_reclaimable)
            );
        }
        println!();
        output::print_warning("CAUTION: Deleting bloat directories cannot be undone directly.");
        output::print_info(
            "Note: You can re-install missing dependencies anytime using `dev-prune restore`.",
        );
        // The question goes to stderr: stdout is a pipe here, and a prompt written into
        // it is invisible on the terminal — the command just appears to hang.
        eprint!(
            "Proceed with deletion of {} directories ({})? [y/N]: ",
            candidates.len(),
            output::format_bytes(total_reclaimable)
        );
        io::stderr().flush()?;

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
        output::print_header(&i18n::tf(
            "run.header.deleting",
            &[
                ("repos", &target_candidates.len().to_string()),
                ("size", &output::format_bytes(selected_total_bytes)),
            ],
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
    // Left-alone directories ride along for the document only — they are not errors.
    let mut error_count = blocked.len();
    let mut all_results: Vec<PruneResult> = blocked;
    all_results.extend(left_alone);
    all_results.extend(missing);
    let mut total_freed: u64 = 0;
    let mut pruned_count = 0;
    let mut pruned_dirs: Vec<crate::config::PrunedDir> = Vec::new();
    // One timestamp identifies the whole pass, so every incremental save below
    // supersedes the previous one instead of counting as its own pass.
    let pass_at = chrono::Utc::now();

    for (repo_path, dirs) in &selection {
        let recorded_before = pruned_dirs.len();
        // The idle check runs again here, not just at analysis: the selector can sit
        // open for hours, and a repository someone started working in between analysis
        // and Enter must not be pruned on the strength of a stale answer. Only
        // `--ignore-idle` skips it, exactly as it skipped the first check.
        let idle_days = registry
            .repositories
            .get(repo_path)
            .and_then(|e| e.override_idle_days)
            .unwrap_or(registry.settings.idle_days);
        let single_results = engine::prune_repo_with(
            repo_path,
            &PruneOptions {
                idle_days,
                dry_run: false,
                force: args.force,
                only_dirs: Some(dirs.clone()),
                adapters: filter.clone(),
                min_size_bytes: 0,
                scan_depth: analysis.scan_depth,
                allow_manifest_rewrite: analysis.allow_manifest_rewrite,
                command_timeout_secs: analysis.command_timeout_secs,
                build_idle_days: analysis.build_idle_days,
                adapter_idle_days: analysis.adapter_idle_days.clone(),
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
                        runtime: result.runtime.clone(),
                    });
                    if !args.json {
                        output::print_success(&format!(
                            "{} → {} ({}) — {}{}",
                            output::clean_path(&result.repo_path),
                            result.bloat_dir,
                            output::format_bytes(result.size_freed),
                            result.adapter_name,
                            output::shared_note(result.shared_bytes, &result.adapter_name)
                        ));
                    }
                }
                PruneStatus::LockfileError(e) => {
                    error_count += 1;
                    if !args.json {
                        report_lockfile_failure(&result, e);
                    }
                }
                PruneStatus::ActivityCheckError(e) => {
                    error_count += 1;
                    if !args.json {
                        output::print_error(&format!(
                            "{} skipped — its activity could not be determined:\n    {}",
                            output::clean_path(&result.repo_path),
                            e.trim()
                        ));
                    }
                }
                PruneStatus::DeleteError(e) => {
                    error_count += 1;
                    // A non-zero size_freed on a delete error means the delete got
                    // half-way: the directory is corrupt, not intact. Record it so
                    // `devp restore --last-run` knows to rebuild it — while the error
                    // above still fails the pass.
                    if result.size_freed > 0 {
                        pruned_dirs.push(crate::config::PrunedDir {
                            repo_path: result.repo_path.clone(),
                            bloat_dir: result.bloat_dir.clone(),
                            adapter: result.adapter_name.clone(),
                            size_freed: result.size_freed,
                            runtime: result.runtime.clone(),
                        });
                    }
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
                // The repo saw activity between analysis and execution — the re-check
                // above caught it. A protective skip, not a failure.
                PruneStatus::SkippedActive if !args.json => {
                    output::print_info(&format!(
                        "{} became active since the analysis — left alone. \
                         Use `--ignore-idle` to prune it anyway.",
                        output::clean_path(&result.repo_path)
                    ));
                }
                _ => {}
            }
            all_results.push(result);
        }

        // Persisted after every repository, not once at the end. A pass killed
        // half-way through used to leave the registry describing the *previous*
        // pass, so `devp restore --last-run` offered to reinstall directories that
        // were never deleted and said nothing about the ones that were. A save
        // failure here is silent — the final save below reports it.
        if pruned_dirs.len() > recorded_before {
            registry.record_prune_progress(pass_at, pruned_dirs.clone());
            let _ = registry.save();
        }
    }

    registry.record_prune_progress(pass_at, pruned_dirs);
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

    output::print_header(i18n::t("run.summary"));
    output::print_success(&i18n::tf(
        "run.freed",
        &[
            ("size", &output::format_bytes_styled(total_freed)),
            ("count", &pruned_count.to_string()),
        ],
    ));

    if error_count > 0 {
        output::print_warning(&i18n::tf(
            "run.not_pruned",
            &[("count", &error_count.to_string())],
        ));

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

    // After the pass, never before it: an upgrade mid-run would swap the binary out
    // from under the work the user actually asked for.
    crate::commands::update::maybe_auto_update(&registry);

    Ok(())
}

/// Why a repository's activity could not be read, when the reason is one that every
/// affected repository shares.
///
/// Git prints its "dubious ownership" refusal as twelve lines, ten of which are word for
/// word identical for every repository it refuses — the same explanation, the same two
/// account identifiers, the same `git config` invitation. On a machine where one Windows
/// reinstall left twenty-one repositories with a stale owner, printing that per
/// repository buries the only line that differs (the path) in two hundred that do not.
/// One cause with one fix should read as one paragraph, however many repositories it
/// covers.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ActivityFailure {
    /// Git refuses the working tree because it is owned by another account.
    UntrustedOwner,
    /// The registered path is no longer a working tree.
    NotARepository,
    /// Anything else: reported individually, with git's own words.
    Individual,
}

impl ActivityFailure {
    /// Classify one activity-check failure from Git's own stderr.
    ///
    /// Deliberately a substring match on Git's wording rather than a parse. The
    /// alternative is asking Git a second question per repository, and the cost of a
    /// wrong guess here is a message that reads slightly less well — never a wrong
    /// deletion, because a repository in this list is one nothing was done to.
    fn classify(message: &str) -> Self {
        let lower = message.to_lowercase();
        if lower.contains(constants::GIT_DUBIOUS_OWNERSHIP) {
            Self::UntrustedOwner
        } else if lower.contains(constants::GIT_NOT_A_REPOSITORY) {
            Self::NotARepository
        } else {
            Self::Individual
        }
    }
}

/// How many paths a grouped cause lists before it stops and says how many are left.
///
/// Eight is enough to recognise a pattern — one directory tree, one old drive — without
/// the list becoming the thing that has to be scrolled past.
const GROUPED_PATHS_SHOWN: usize = 8;

/// Report the repositories the analysis pass could not get past, with the fix for each.
///
/// Silent for an empty list, so callers do not have to guard it.
fn report_blocked(blocked: &[PruneResult]) {
    if blocked.is_empty() {
        return;
    }
    output::print_header(&i18n::tf(
        "run.header.blocked",
        &[("count", &blocked.len().to_string())],
    ));

    let grouped = |failure: ActivityFailure| -> Vec<&PruneResult> {
        blocked
            .iter()
            .filter(|r| match &r.status {
                PruneStatus::ActivityCheckError(e) => ActivityFailure::classify(e) == failure,
                _ => false,
            })
            .collect()
    };

    let untrusted = grouped(ActivityFailure::UntrustedOwner);
    if !untrusted.is_empty() {
        let n = untrusted.len();
        output::print_error(&format!(
            "{n} {} owned by a different account — Git will not read {}.",
            output::plural(n, "repository is", "repositories are"),
            output::plural(n, "it", "them")
        ));
        list_paths(&untrusted);
        output::print_wrapped(
            "    ",
            "Nothing is wrong with the repositories themselves. The owner recorded on \
             disk is usually one a Windows reinstall, a restored backup or a drive moved \
             between machines left behind.",
        );
        output::print_wrapped(
            "    ",
            "dev-prune dates a repository by its last commit, so one Git will not open \
             has no known age — and nothing is ever deleted from a repository whose age is \
             unknown.",
        );
        output::print_info(&format!(
            "  Fix all {n} at once:  devp trust --fix-ownership"
        ));
    }

    let orphaned = grouped(ActivityFailure::NotARepository);
    if !orphaned.is_empty() {
        if !untrusted.is_empty() {
            println!();
        }
        let n = orphaned.len();
        output::print_error(&format!(
            "{n} registered {} not {} git {} any more.",
            output::plural(n, "path is", "paths are"),
            output::plural(n, "a", ""),
            output::plural(n, "repository", "repositories")
        ));
        list_paths(&orphaned);
        output::print_wrapped(
            "    ",
            "The directory is still there; its `.git` is not — a clone deleted and \
             recreated by hand, or a worktree `git worktree prune` has since removed. The \
             registry entry outlived what it pointed at.",
        );
        // Not `--missing`: that clears entries whose *directory* has gone, and these
        // directories are still on disk. Naming the wrong repair here would have the
        // user run a command that reports it removed nothing.
        output::print_info(&format!(
            "  Drop {} from the registry:  devp unlink <path>",
            output::plural(n, "it", "them")
        ));
    }

    for result in blocked {
        let clean_p = output::clean_path(&result.repo_path);
        match &result.status {
            PruneStatus::ConfigError(e) => {
                output::print_error(&format!(
                    "{clean_p} skipped — its .devprune.json could not be read:
    {}",
                    e.trim()
                ));
                output::print_info(&format!(
                    "  Fix command:       devp config {clean_p} --update"
                ));
            }
            PruneStatus::LockfileError(e) => report_lockfile_failure(result, e),
            PruneStatus::ActivityCheckError(e)
                if ActivityFailure::classify(e) == ActivityFailure::Individual =>
            {
                output::print_error(&format!(
                    "{clean_p} skipped — its activity could not be determined:
    {}",
                    output::condense_tool_output(e, 4)
                ));
            }
            PruneStatus::DeleteError(e) => {
                output::print_error(&format!("{clean_p} → delete failed: {e}"));
            }
            // Everything else was covered by one of the grouped causes above.
            _ => {}
        }
    }
}

/// Print the paths of one grouped cause, indented, stopping at [`GROUPED_PATHS_SHOWN`].
///
/// `--json` is named as the way to see the rest rather than a `--verbose` flag, because
/// it already lists every result and is the output a script would be reading anyway.
fn list_paths(results: &[&PruneResult]) {
    for result in results.iter().take(GROUPED_PATHS_SHOWN) {
        println!("    {}", output::styled_path(&result.repo_path));
    }
    if let Some(rest) = results
        .len()
        .checked_sub(GROUPED_PATHS_SHOWN)
        .filter(|n| *n > 0)
    {
        output::print_dimmed(&format!(
            "    … and {rest} more — `devp run --dry-run --json` lists every one."
        ));
    }
}

/// Report directories that were deliberately left alone: symlinks, and declarations
/// that did not pass their checks.
///
/// Informational only, never part of the exit code. The storage a link points at is not
/// this repository's to delete; a declaration dev-prune refuses is a standing fact about
/// the repository's own config. Both are permanent until somebody changes something, and
/// failing on either would turn every scheduled pass over such a repo red forever.
fn report_left_alone(left_alone: &[PruneResult]) {
    for result in left_alone {
        if let PruneStatus::SkippedSymlink(e) | PruneStatus::SkippedDeclaration(e) = &result.status
        {
            output::print_warning(&format!(
                "{} → {}",
                output::clean_path(&result.repo_path),
                e.trim()
            ));
        }
    }
}

/// Report registered paths that no longer exist on disk.
///
/// Informational only, never part of the exit code: the clone is already gone, the state
/// does not fix itself, and failing on it would keep every scheduled pass red until the
/// user notices. The fix is one command, so name it.
fn report_missing(missing: &[PruneResult]) {
    if missing.is_empty() {
        return;
    }
    println!();
    let n = missing.len();
    output::print_warning(&format!(
        "{n} registered {} no longer {} on disk.",
        output::plural(n, "path", "paths"),
        output::plural(n, "exists", "exist")
    ));
    // One line per path was fine for the one or two a person deletes by hand. It stopped
    // being fine the first time a tool that clones into a temporary directory registered
    // thirty of them: the report ended in thirty near-identical lines carrying one
    // instruction, repeated thirty times.
    for result in missing.iter().take(GROUPED_PATHS_SHOWN) {
        println!("    {}", output::styled_path(&result.repo_path));
    }
    if let Some(rest) = missing
        .len()
        .checked_sub(GROUPED_PATHS_SHOWN)
        .filter(|n| *n > 0)
    {
        output::print_dimmed(&format!(
            "    … and {rest} more — `devp run --dry-run --json` lists every one."
        ));
    }
    output::print_info(&format!(
        "  Clear {} from the registry:  devp unlink --missing",
        output::plural(n, "it", "them all")
    ));
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
    output::print_header(i18n::t("run.header.binaries"));
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
    output::print_header(i18n::t("run.header.candidates"));
    for candidate in candidates {
        output::print_info(&format!(
            "  • {} → {} ({}) [{}]{}",
            output::styled_path(&candidate.repo_path),
            candidate.bloat_dir,
            output::format_bytes_styled(candidate.size_freed),
            output::styled_adapter(&candidate.adapter_name),
            output::shared_note(candidate.shared_bytes, &candidate.adapter_name)
        ));
    }
}

pub(crate) fn report_lockfile_failure(result: &PruneResult, error: &str) {
    // The project directory, not the repository root: a monorepo reports
    // `backend/.venv`, and `uv lock` at the root would not fix it.
    let project = output::clean_path(result.project_dir());

    output::print_error(&format!(
        "{} → {} lockfile sync failed:\n    {}",
        project,
        result.adapter_name,
        error.trim(),
    ));
    match json::lockfile_fix_command(&result.adapter_name) {
        Some(sync_cmd) => {
            // `;` on PowerShell, `&&` on a POSIX shell — pasted, either has to work as
            // typed or the `cd` is decoration.
            #[cfg(windows)]
            let manual_cmd = format!("cd \"{project}\"; {sync_cmd}");
            #[cfg(not(windows))]
            let manual_cmd = format!("cd \"{project}\" && {sync_cmd}");
            output::print_info(&format!("  Fix command:       {manual_cmd}"));
        }
        // venv, gradle, maven and swift have no mechanical fix — saying where still
        // beats sending someone to the repository root to go looking.
        None => output::print_info(&format!("  Fix it in:         {project}")),
    }
    output::print_info(&format!(
        "  Troubleshooting:   {}",
        constants::TROUBLESHOOTING_URL
    ));
}

/// `devp run --explain` — the decision for every repository and directory, with
/// nothing done.
///
/// The prune pass keeps quiet about the states that are not its job to fix — a
/// repository still active, one opted out, a directory under the size floor — which is
/// exactly what someone staring at "no candidates found" needs to hear about. This mode
/// runs the same analysis and reports every verdict instead of only the actionable
/// ones. Read-only by construction: the engine runs in dry-run mode, and the size floor
/// is applied here in the report rather than in the engine, so a too-small directory is
/// named as too small instead of silently missing.
fn run_explain(args: &RunArgs<'_>, filter: &AdapterFilter) -> Result<()> {
    output::print_header(i18n::t("run.header.reasons"));
    if let Some(desc) = filter.describe() {
        output::print_info(&format!("Adapter filter: {desc}"));
    }

    if let Some(target_str) = args.target_path {
        let raw = Path::new(target_str);
        let path = if raw.exists() {
            raw.canonicalize().unwrap_or_else(|_| raw.to_path_buf())
        } else {
            raw.to_path_buf()
        };
        if !crate::scanner::is_git_repo(&path) {
            anyhow::bail!(
                "{} is not a Git repository — dev-prune only prunes Git repos.",
                output::clean_path(&path)
            );
        }
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
        let floor = resolve_min_size(args, registry.as_ref());
        let results = engine::prune_repo_with(
            &path,
            &PruneOptions {
                idle_days,
                dry_run: true,
                force: args.force,
                only_dirs: None,
                adapters: filter.clone(),
                min_size_bytes: 0,
                scan_depth: resolve_scan_depth(registry.as_ref()),
                allow_manifest_rewrite: resolve_manifest_rewrite(registry.as_ref()),
                command_timeout_secs: resolve_command_timeout(registry.as_ref()),
                build_idle_days: resolve_build_idle_days(registry.as_ref()),
                adapter_idle_days: resolve_adapter_idle_days(registry.as_ref()),
            },
        );
        let refs: Vec<&PruneResult> = results.iter().collect();
        explain_repo(&path, &refs, floor, idle_days);
        print_explain_footer();
        return Ok(());
    }

    let mut registry = Registry::load()?;
    if registry.repo_count() == 0 {
        output::print_warning("No repositories registered. Run `dev-prune init` first.");
        return Ok(());
    }

    let except = parse_except(args.except);
    let global_floor = resolve_min_size(args, Some(&registry));
    let analysis = PruneOptions {
        idle_days: 0, // replaced per repository from the registry
        dry_run: true,
        force: args.force,
        only_dirs: None,
        adapters: filter.clone(),
        min_size_bytes: 0,
        scan_depth: resolve_scan_depth(Some(&registry)),
        allow_manifest_rewrite: resolve_manifest_rewrite(Some(&registry)),
        command_timeout_secs: resolve_command_timeout(Some(&registry)),
        build_idle_days: resolve_build_idle_days(Some(&registry)),
        adapter_idle_days: resolve_adapter_idle_days(Some(&registry)),
    };
    let results = engine::prune_all_with(&mut registry, &analysis);

    let mut by_repo: std::collections::HashMap<&Path, Vec<&PruneResult>> =
        std::collections::HashMap::new();
    for r in &results {
        by_repo.entry(r.repo_path.as_path()).or_default().push(r);
    }

    let mut repos: Vec<&std::path::PathBuf> = registry.repositories.keys().collect();
    repos.sort();
    for path in repos {
        if is_excepted(path, &except) {
            println!();
            output::print_info(&output::clean_path(path));
            println!("  • left completely alone this pass (`--except`)");
            continue;
        }
        let idle_days = registry
            .repositories
            .get(path)
            .and_then(|e| e.override_idle_days)
            .unwrap_or(registry.settings.idle_days);
        let empty = Vec::new();
        let repo_results = by_repo.get(path.as_path()).unwrap_or(&empty);
        explain_repo(path, repo_results, global_floor, idle_days);
    }
    print_explain_footer();
    Ok(())
}

/// One repository's verdicts, one line per decision.
fn explain_repo(path: &Path, results: &[&PruneResult], floor: u64, idle_days: u64) {
    println!();
    output::print_info(&output::clean_path(path));

    if results.is_empty() {
        println!(
            "  • idle, but no known bloat directories were found. A project deeper than \
             `scan_depth` is not examined — `devp status` shows what dev-prune can see."
        );
        return;
    }

    for r in results {
        match &r.status {
            PruneStatus::SkippedDryRun => {
                if r.size_freed >= floor {
                    output::print_success(&format!(
                        "would prune {} ({}) [{}]{}",
                        r.bloat_dir,
                        output::format_bytes(r.size_freed),
                        r.adapter_name,
                        output::shared_note(r.shared_bytes, &r.adapter_name)
                    ));
                } else {
                    println!(
                        "  • {} ({}) is under the size floor of {} — the reinstall would \
                         cost more than the space is worth. `--min-size 0` includes it.",
                        r.bloat_dir,
                        output::format_bytes(r.size_freed),
                        output::format_bytes(floor)
                    );
                }
            }
            PruneStatus::SkippedActive => {
                let age = crate::scanner::git::get_last_activity(path)
                    .ok()
                    .flatten()
                    .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                    .map(|d| d.as_secs() / 86_400);
                match age {
                    Some(0) => println!(
                        "  • active — there was activity today, and the idle \
                         threshold is {idle_days} days. `--ignore-idle` overrides."
                    ),
                    Some(days) => println!(
                        "  • active — last activity {days} day{} ago, and the idle \
                         threshold is {idle_days} days. `--ignore-idle` overrides.",
                        if days == 1 { "" } else { "s" }
                    ),
                    None => println!(
                        "  • active (not idle for {idle_days} days yet). \
                         `--ignore-idle` overrides."
                    ),
                }
            }
            other => println!("  • {other}"),
        }
    }
}

/// The one-line contract of `--explain`, printed after the verdicts.
fn print_explain_footer() {
    println!();
    output::print_info(
        "Nothing was verified or deleted. `devp run --dry-run` verifies candidates; \
         `devp run` prunes.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gits_ownership_refusal_is_recognised_whatever_the_path() {
        // The whole grouped report hangs off this substring. If Git ever reworded the
        // message, twenty-one repositories would silently go back to printing twelve
        // lines each, and nothing else in the suite would notice.
        let message = "git could not read `V:/x`: fatal: detected dubious ownership in repository \
                       at 'V:/x'";
        assert_eq!(
            ActivityFailure::classify(message),
            ActivityFailure::UntrustedOwner
        );
    }

    #[test]
    fn a_path_that_lost_its_git_directory_is_its_own_cause() {
        // Deliberately distinct from UntrustedOwner: the two have different fixes, and
        // pointing the user at `devp unlink --missing` for a directory that still exists
        // is a command that reports it removed nothing.
        let message = "fatal: not a git repository (or any of the parent directories): .git";
        assert_eq!(
            ActivityFailure::classify(message),
            ActivityFailure::NotARepository
        );
    }

    #[test]
    fn an_unfamiliar_failure_is_still_printed_in_full() {
        assert_eq!(
            ActivityFailure::classify("fatal: unable to read tree"),
            ActivityFailure::Individual
        );
    }
}
