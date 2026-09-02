// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune init` command.
//
// Scans provided paths for Git repositories, registers them in the registry,
// auto-configures background daemon & hooks, and self-heals project configuration schemas.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{PerRepoConfig, Registry};
use crate::output;
use crate::{constants, discovery, scanner};

/// Run the `init` command.
///
/// Scans each provided path for Git repositories and adds them to the registry. With
/// `auto`, the paths are worked out rather than given — see [`crate::discovery`].
pub fn run(paths: &[String], dry_run: bool, auto: bool) -> Result<()> {
    output::print_banner();
    output::print_header("dev-prune init");

    let mut registry = Registry::load()?;
    let mut total_found = 0;
    let mut skipped_throwaway = 0;
    let mut skipped_opted_out = 0;
    let mut newly_added_repos: Vec<PathBuf> = Vec::new();

    // In `--auto` the candidates are already filtered — discovery has dropped the
    // registered, the disposable and the opted-out — so the scan loop below is fed a
    // single synthetic "path" list and does no scanning of its own.
    let auto_found = if auto {
        let found = discovery::discover(&registry)?;
        output::print_info(&format!(
            "Looked for unregistered repositories in {} {}:",
            found.roots.len(),
            output::plural(found.roots.len(), "location", "locations")
        ));
        for root in &found.roots {
            output::print_info(&format!("  {}", output::clean_path(root)));
        }
        total_found = found.found.len();
        skipped_throwaway = found.throwaway;
        skipped_opted_out = found.opted_out;
        Some(found.found)
    } else {
        None
    };

    let scans: Vec<(PathBuf, Vec<PathBuf>)> = match auto_found {
        Some(repos) => vec![(PathBuf::new(), repos)],
        None => paths
            .iter()
            .map(|path_str| {
                let path = Path::new(path_str)
                    .canonicalize()
                    .with_context(|| format!("Path not found: {path_str}"))?;
                output::print_info(&format!(
                    "Scanning {} for Git repositories...",
                    output::clean_path(&path)
                ));
                let repos = scanner::scan_for_repos(&path)?;
                total_found += repos.len();
                Ok((path, repos))
            })
            .collect::<Result<_>>()?,
    };

    for (path, repos) in scans {
        for repo in repos {
            // A plugin manager's throwaway clone is not a workspace. Skipped quietly and
            // counted, rather than listed: on the scan that motivated this there were
            // twenty-eight of them, and twenty-eight lines of explanation would have
            // buried the repositories the user actually wanted registered.
            if !auto && super::link::is_throwaway_checkout(&path, &repo) {
                skipped_throwaway += 1;
                continue;
            }
            // Honoured before registration, not just before deletion. `devp init` is a
            // bulk scan of somebody else's directory tree, and a repository carrying the
            // opt-out has already said no to being one of dev-prune's. `devp link <path>`
            // still registers it, because naming a repository is not a bulk scan.
            if !auto && repo.join(constants::DEVPRUNE_IGNORE_FILE).exists() {
                skipped_opted_out += 1;
                continue;
            }
            if registry.add_repo(repo.clone()) {
                newly_added_repos.push(repo.clone());
                let verb = if dry_run {
                    "Would register"
                } else {
                    "Registered"
                };
                output::print_success(&format!("{verb}: {}", output::clean_path(&repo)));
                let adoption =
                    registry.adopt_moved_entry(&repo, scanner::git::repo_identity(&repo));
                super::link::report_adoption(&adoption);
            } else {
                output::print_info(&format!(
                    "Already registered: {}",
                    output::clean_path(&repo)
                ));
                // Backfill, so one `devp init ~/code` teaches the whole registry to
                // recognise a move later. Only when it is missing: this scan visits
                // every repository under the path, every time it is run.
                if registry.needs_identity(&repo) {
                    let adoption =
                        registry.adopt_moved_entry(&repo, scanner::git::repo_identity(&repo));
                    super::link::report_adoption(&adoption);
                }
            }
        }
    }

    if !dry_run {
        registry.last_added_repos = newly_added_repos.clone();
        registry.save()?;
    }

    output::print_header("Summary");
    // The registry was mutated in memory either way; only the save is skipped. Saying
    // "added" after a `--dry-run` would describe a file that was never written.
    output::print_info(&format!(
        "Found {total_found} {}{}, {} {} new",
        if auto { "unregistered " } else { "Git " },
        output::plural(total_found, "repo", "repos"),
        if dry_run { "would add" } else { "added" },
        newly_added_repos.len()
    ));
    if skipped_opted_out > 0 {
        output::print_info(&format!(
            "Skipped {skipped_opted_out} {} carrying `{}`.",
            output::plural(skipped_opted_out, "repository", "repositories"),
            constants::DEVPRUNE_IGNORE_FILE,
        ));
    }
    if skipped_throwaway > 0 {
        // Named, not silent. A scan that quietly drops repositories is one the user
        // cannot debug when it drops one they wanted — and `devp link` is the way back.
        output::print_info(&format!(
            "Skipped {skipped_throwaway} disposable {} (plugin-manager clones, temp directories). \
             `devp link <path>` registers one anyway.",
            output::plural(skipped_throwaway, "checkout", "checkouts"),
        ));
    }
    // After a dry run the in-memory count includes repositories that were never
    // written; "would be tracked" is the honest phrasing for it.
    output::print_info(&format!(
        "{}: {}",
        if dry_run {
            "Would be tracked"
        } else {
            "Total tracked"
        },
        registry.repo_count()
    ));

    if !dry_run {
        // Install anything missing. Idempotent, and identical to what `devp setup` and
        // the post-upgrade pass do, so there is one code path and one set of rules —
        // but only on a machine that has said yes to it. `devp init` was typed to
        // register repositories, which is not yet a yes to a scheduled task, and
        // stamping while the question is open would swallow the question itself.
        if crate::setup::consent_state() == crate::setup::SetupConsent::Granted {
            if let Some(report) = crate::setup::ensure_integrations_if_enabled(&registry)
                && (report.changed_anything() || report.needs_attention())
            {
                output::print_header("Integrations");
                report.print(false);
            }
            crate::setup::suppress_next_auto_setup();
        }

        if registry.settings.auto_config {
            for repo in &newly_added_repos {
                crate::commands::link::ensure_default_repo_config(repo);
            }
        }

        // Validate (do NOT rewrite) existing per-repo configs.
        //
        // The previous behaviour re-serialised `unwrap_or_default()` into every repo,
        // which silently replaced a malformed `.devprune.json` with defaults and created
        // a config plus an exclude entry in repos that never had one. Report instead.
        for repo in registry.repositories.keys() {
            if let Err(e) = PerRepoConfig::load_with_diagnostics(repo) {
                output::print_warning(&format!(
                    "{}: `.devprune.json` could not be parsed and was left untouched — {e}",
                    output::clean_path(repo)
                ));
            }
        }
    }

    // Setting a machine up is exactly the moment to find out the binary is a version
    // behind, so this asks now rather than waiting for the weekly interval that governs
    // `devp run`. `--dry-run` included: the check writes nothing to disk of its own, and
    // knowing before you commit to the real run is the point.
    output::print_header("Version");
    output::print_info(&format!("Installed: v{}", crate::constants::VERSION));
    if registry.settings.update_check {
        let mut checked = registry;
        if crate::commands::update::check_now(&mut checked) && !dry_run {
            let _ = checked.save();
        }
        registry = checked;
    } else {
        output::print_info(
            "The release check is off (`devp config set update_check true` re-enables it).",
        );
    }

    if dry_run {
        output::print_header("Dry Run Complete");
        output::print_info("Nothing was written. Re-run without `--dry-run` to register.");
        return Ok(());
    }

    output::print_header("Initialization Complete");
    output::print_success(&format!(
        "dev-prune initialization complete! All {} tracked {} registered & verified.",
        registry.repo_count(),
        output::plural(registry.repo_count(), "repository", "repositories")
    ));

    output::print_info("Review or undo the integrations with `devp setup --status`.");

    Ok(())
}
