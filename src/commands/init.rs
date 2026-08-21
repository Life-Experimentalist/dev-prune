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
use crate::scanner;

/// Run the `init` command.
///
/// Scans each provided path for Git repositories and adds them to the registry.
pub fn run(paths: &[String], dry_run: bool) -> Result<()> {
    output::print_banner();
    output::print_header("dev-prune init");

    let mut registry = Registry::load()?;
    let mut total_found = 0;
    let mut newly_added_repos: Vec<PathBuf> = Vec::new();

    for path_str in paths {
        let path = Path::new(path_str)
            .canonicalize()
            .with_context(|| format!("Path not found: {path_str}"))?;

        output::print_info(&format!(
            "Scanning {} for Git repositories...",
            output::clean_path(&path)
        ));

        let repos = scanner::scan_for_repos(&path)?;
        total_found += repos.len();

        for repo in repos {
            if registry.add_repo(repo.clone()) {
                newly_added_repos.push(repo.clone());
                let verb = if dry_run {
                    "Would register"
                } else {
                    "Registered"
                };
                output::print_success(&format!("{verb}: {}", output::clean_path(&repo)));
            } else {
                output::print_info(&format!(
                    "Already registered: {}",
                    output::clean_path(&repo)
                ));
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
        "Found {total_found} Git {}, {} {} new",
        output::plural(total_found, "repo", "repos"),
        if dry_run { "would add" } else { "added" },
        newly_added_repos.len()
    ));
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
        // the post-upgrade pass do, so there is one code path and one set of rules.
        if let Some(report) = crate::setup::ensure_integrations_if_enabled(&registry)
            && (report.changed_anything() || report.needs_attention())
        {
            output::print_header("Integrations");
            report.print(false);
        }
        crate::setup::suppress_next_auto_setup();

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
