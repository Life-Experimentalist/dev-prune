// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune stats`.
//
// `devp status` answers "what could I reclaim right now"; this answers "what has this
// thing actually done for me". They are different questions, and folding the second into
// the dashboard would have meant a screen of history above the list people open it for.
//
// Two of the three sections here are only recorded from 1.1.0 onward, because the
// per-repository total and the pass history did not exist before it. The report says so
// rather than letting an upgraded machine look like it has never pruned anything.

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::config::Registry;
use crate::constants::HISTORY_STARTS_AT;
use crate::output;

/// How many passes the text report lists. The registry keeps more; a screen holds fewer.
const PASSES_SHOWN: usize = 10;

/// How many repositories the text report ranks.
const REPOS_SHOWN: usize = 10;

/// Run the `stats` command.
pub fn run(json_output: bool) -> Result<()> {
    let registry = Registry::load()?;

    if json_output {
        return crate::json::emit(&crate::json::stats_document(&registry));
    }

    output::print_header("Lifetime");
    output::print_info(&format!(
        "Space reclaimed:   {}",
        output::format_bytes_styled(registry.total_freed_bytes)
    ));
    // Its own line rather than added to the one above it. Both are space this tool gave
    // back, but they are not interchangeable: the line above cost a reinstall in one
    // repository, this one costs a download in every project on the disk.
    output::print_info(&format!(
        "Caches emptied:    {}",
        output::format_bytes_styled(registry.total_cache_freed_bytes)
    ));
    output::print_info(&format!(
        "Prune passes:      {}",
        registry.total_pruned_count
    ));
    output::print_info(&format!(
        "Repositories:      {} tracked",
        registry.repo_count()
    ));

    print_last_pass(&registry);
    print_recent_passes(&registry);
    print_biggest_repositories(&registry);

    Ok(())
}

/// The pass `devp restore --last-run` would undo.
fn print_last_pass(registry: &Registry) {
    output::print_header("Most recent pass");

    let Some(last) = &registry.last_prune else {
        output::print_info(
            "Nothing recorded yet — `devp run --dry-run` shows what a pass would do.",
        );
        return;
    };

    let bytes: u64 = last.dirs.iter().map(|d| d.size_freed).sum();
    output::print_info(&format!(
        "{} ({}) — {} from {} {}",
        last.at.format("%Y-%m-%d %H:%M UTC"),
        describe_age(last.at),
        output::format_bytes_styled(bytes),
        last.dirs.len(),
        output::plural(last.dirs.len(), "directory", "directories"),
    ));
    output::print_info("Put it back with: devp restore --last-run");
}

fn print_recent_passes(registry: &Registry) {
    if registry.prune_history.is_empty() {
        return;
    }

    output::print_header("Recent passes");
    for summary in registry.prune_history.iter().rev().take(PASSES_SHOWN) {
        use colored::Colorize;
        // Pad before coloring: a `{:>10}` applied to a string carrying ANSI escapes
        // counts the escapes as width and the column drifts.
        println!(
            "  {}   {}   {} {} across {} {}",
            summary.at.format("%Y-%m-%d %H:%M"),
            format!("{:>10}", output::format_bytes(summary.bytes_freed)).green(),
            summary.dirs_removed,
            output::plural(summary.dirs_removed, "directory", "directories"),
            summary.repos_touched,
            output::plural(summary.repos_touched, "repository", "repositories"),
        );
    }

    let total = registry.prune_history.len();
    if total > PASSES_SHOWN {
        output::print_info(&format!(
            "{total} passes recorded; showing the last {PASSES_SHOWN}."
        ));
    }
}

fn print_biggest_repositories(registry: &Registry) {
    let mut ranked: Vec<_> = registry
        .repositories
        .iter()
        .filter(|(_, entry)| entry.total_freed_bytes > 0)
        .collect();

    output::print_header("Biggest reclaims");

    if ranked.is_empty() {
        // The distinction matters on an upgraded machine: `total_freed_bytes` above can
        // be gigabytes while every per-repository figure is still zero, and reading that
        // as "nothing was ever pruned here" would be wrong.
        output::print_info(&format!(
            "No per-repository figures yet — these are recorded from {HISTORY_STARTS_AT} onward."
        ));
        return;
    }

    ranked.sort_by(|a, b| {
        b.1.total_freed_bytes
            .cmp(&a.1.total_freed_bytes)
            .then_with(|| a.0.cmp(b.0))
    });

    for (path, entry) in ranked.iter().take(REPOS_SHOWN) {
        use colored::Colorize;
        let last = entry
            .last_pruned_at
            .map(|at| format!("last pruned {}", describe_age(at)))
            .unwrap_or_else(|| "never pruned by this install".to_string());
        println!(
            "  {}   {}   ({last})",
            format!("{:>10}", output::format_bytes(entry.total_freed_bytes)).green(),
            output::styled_path(path),
        );
    }
}

/// "3 days ago", in the coarsest unit that is not a lie.
fn describe_age(at: DateTime<Utc>) -> String {
    let elapsed = Utc::now().signed_duration_since(at);
    let days = elapsed.num_days();
    if days >= 1 {
        return format!(
            "{days} {} ago",
            output::plural(days as usize, "day", "days")
        );
    }
    let hours = elapsed.num_hours();
    if hours >= 1 {
        return format!(
            "{hours} {} ago",
            output::plural(hours as usize, "hour", "hours")
        );
    }
    let minutes = elapsed.num_minutes().max(0);
    format!(
        "{minutes} {} ago",
        output::plural(minutes as usize, "minute", "minutes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn an_age_is_described_in_the_coarsest_unit_that_fits() {
        assert_eq!(describe_age(Utc::now() - Duration::days(3)), "3 days ago");
        assert_eq!(describe_age(Utc::now() - Duration::days(1)), "1 day ago");
        assert_eq!(describe_age(Utc::now() - Duration::hours(5)), "5 hours ago");
        assert_eq!(
            describe_age(Utc::now() - Duration::minutes(2)),
            "2 minutes ago"
        );
    }

    #[test]
    fn a_timestamp_in_the_future_does_not_render_as_negative() {
        // Clock skew between the daemon and an interactive run is real, and
        // "-1 minutes ago" is worse than rounding it to now.
        assert_eq!(
            describe_age(Utc::now() + Duration::minutes(5)),
            "0 minutes ago"
        );
    }
}
