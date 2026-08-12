// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune undo` command.
//
// Reverts the most recent `init` or `link` action by un-registering
// the repositories added in that pass.

use anyhow::Result;

use crate::config::Registry;
use crate::output;

pub fn run() -> Result<()> {
    let mut registry = Registry::load()?;

    if registry.last_added_repos.is_empty() {
        output::print_warning("No recent repository additions to undo.");
        return Ok(());
    }

    let repos_to_undo = registry.last_added_repos.clone();
    let mut removed_count = 0;

    for path in &repos_to_undo {
        if registry.remove_repo(path) {
            removed_count += 1;
            output::print_info(&format!("Unregistered: {}", output::clean_path(path)));
        }
    }

    registry.last_added_repos.clear();
    registry.save()?;

    output::print_header("Undo Operation Complete");
    // The list can be stale — `unlink` removes a repository without touching it — so
    // "unregistered 0 repositories" is a real outcome, and claiming success for it
    // would leave the user believing something was reverted.
    if removed_count == 0 {
        output::print_warning("Those repositories were already unregistered; nothing to undo.");
    } else {
        output::print_success(&format!(
            "Unregistered {removed_count} {}.",
            output::plural(removed_count, "repository", "repositories")
        ));
    }

    Ok(())
}
