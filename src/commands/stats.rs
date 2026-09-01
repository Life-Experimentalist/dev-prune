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

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::config::Registry;
use crate::constants::{HISTORY_STARTS_AT, PRUNE_LOG_STARTS_AT};
use crate::history::{self, Pass, Trigger};
use crate::output;

/// How many passes the text report lists. The registry keeps more; a screen holds fewer.
const PASSES_SHOWN: usize = 10;

/// How many repositories the text report ranks.
const REPOS_SHOWN: usize = 10;

/// How many package managers the text report ranks. There are two dozen adapters and a
/// machine that has used more than ten of them is not the one this line is written for.
const MANAGERS_SHOWN: usize = 10;

/// Run the `stats` command.
pub fn run(json_output: bool) -> Result<()> {
    let registry = Registry::load()?;
    // Best-effort, like every other read of it: a machine with no log yet still gets the
    // totals, and the two sections that need one say so themselves.
    let passes = history::merged(history::load().unwrap_or_default(), &registry);

    if json_output {
        return crate::json::emit(&crate::json::stats_document(&registry, &passes));
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
    // A third line for the same reason there is a second: an image costs a pull of the
    // whole layer stack to put back, which is neither of the two bills above.
    output::print_info(&format!(
        "Containers cleared: {}",
        output::format_bytes_styled(registry.total_container_freed_bytes)
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
    print_by_manager(&passes);
    print_by_trigger(&passes);

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

/// One package manager's lifetime contribution to pruning.
pub(crate) struct ManagerTotal {
    pub(crate) manager: String,
    pub(crate) bytes: u64,
    pub(crate) dirs: usize,
}

/// Rank package managers by what they gave back, biggest first.
///
/// The second return is how many passes could not be counted. A pass recovered from the
/// registry summary knows its total and nothing else — no directory list, so no adapter
/// — and quietly leaving those out would make the ranking under-report without saying so.
pub(crate) fn rank_managers(passes: &[Pass]) -> (Vec<ManagerTotal>, usize) {
    let mut totals: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
    let mut unaccounted = 0;

    for pass in passes {
        let Some(dirs) = pass.dirs() else {
            unaccounted += 1;
            continue;
        };
        for dir in dirs {
            let entry = totals.entry(dir.adapter.as_str()).or_default();
            entry.0 += dir.size_freed;
            entry.1 += 1;
        }
    }

    let mut ranked: Vec<ManagerTotal> = totals
        .into_iter()
        .map(|(manager, (bytes, dirs))| ManagerTotal {
            manager: manager.to_string(),
            bytes,
            dirs,
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.manager.cmp(&b.manager))
    });
    (ranked, unaccounted)
}

/// What one trigger has reclaimed, across every pass it started.
pub(crate) struct TriggerTotal {
    pub(crate) trigger: Trigger,
    pub(crate) bytes: u64,
    pub(crate) passes: usize,
}

/// Split the passes by what started them, in [`Trigger::ALL`] order.
///
/// Every trigger is returned even at zero — see [`Trigger::ALL`]. The second return is
/// the passes with no trigger recorded, which is every pass from before the log existed.
pub(crate) fn split_by_trigger(passes: &[Pass]) -> (Vec<TriggerTotal>, usize) {
    let mut totals: Vec<TriggerTotal> = Trigger::ALL
        .iter()
        .map(|&trigger| TriggerTotal {
            trigger,
            bytes: 0,
            passes: 0,
        })
        .collect();
    let mut unaccounted = 0;

    for pass in passes {
        let Some(trigger) = pass.trigger() else {
            unaccounted += 1;
            continue;
        };
        if let Some(slot) = totals.iter_mut().find(|t| t.trigger == trigger) {
            slot.bytes += pass.bytes_freed();
            slot.passes += 1;
        }
    }

    (totals, unaccounted)
}

/// Which package manager is actually earning its keep.
fn print_by_manager(passes: &[Pass]) {
    let (ranked, unaccounted) = rank_managers(passes);

    output::print_header("By package manager");

    if ranked.is_empty() {
        output::print_info(&format!(
            "No per-manager figures yet — these are recorded from {PRUNE_LOG_STARTS_AT} onward."
        ));
        return;
    }

    let width = ranked
        .iter()
        .take(MANAGERS_SHOWN)
        .map(|m| m.manager.len())
        .max()
        .unwrap_or(0);

    for total in ranked.iter().take(MANAGERS_SHOWN) {
        use colored::Colorize;
        // Pad before coloring, for the reason `print_recent_passes` documents.
        println!(
            "  {}   {:width$}   {} {}",
            format!("{:>10}", output::format_bytes(total.bytes)).green(),
            total.manager,
            total.dirs,
            output::plural(total.dirs, "directory", "directories"),
        );
    }

    if ranked.len() > MANAGERS_SHOWN {
        output::print_info(&format!(
            "{} managers in total; showing the top {MANAGERS_SHOWN}.",
            ranked.len()
        ));
    }
    // Without this the section reads as a breakdown of the lifetime total above, and it
    // is not one: emptying a cache or clearing an image is not a prune pass and never
    // enters the log these figures are summed from.
    output::print_info("Pruned project directories only — not the caches or containers above.");
    if unaccounted > 0 {
        output::print_info(&format!(
            "{unaccounted} earlier {} not counted here: only totals were kept before {PRUNE_LOG_STARTS_AT}.",
            output::plural(unaccounted, "pass is", "passes are"),
        ));
    }
}

/// Whether the scheduler is pulling its weight, or you are doing it all by hand.
fn print_by_trigger(passes: &[Pass]) {
    let (totals, unaccounted) = split_by_trigger(passes);
    let counted: usize = totals.iter().map(|t| t.passes).sum();

    output::print_header("How passes start");

    if counted == 0 {
        output::print_info(&format!(
            "No triggers recorded yet — these are recorded from {PRUNE_LOG_STARTS_AT} onward."
        ));
        return;
    }

    let bytes_total: u64 = totals.iter().map(|t| t.bytes).sum();
    let width = Trigger::ALL
        .iter()
        .map(|t| t.label().len())
        .max()
        .unwrap_or(0);

    for total in &totals {
        use colored::Colorize;
        // u128 so the multiply cannot wrap. The figure it guards is unreachable; the
        // cast is cheaper than deciding that for certain.
        let share = if bytes_total == 0 {
            0
        } else {
            u128::from(total.bytes) * 100 / u128::from(bytes_total)
        };
        println!(
            "  {}   {:width$}   {} {}   ({share}%)",
            format!("{:>10}", output::format_bytes(total.bytes)).green(),
            total.trigger.label(),
            total.passes,
            output::plural(total.passes, "pass", "passes"),
        );
    }

    // A zero on the scheduled line is the whole point of the section, and it is also the
    // one line nobody reads as a call to action unless it says so.
    if totals
        .iter()
        .any(|t| t.trigger == Trigger::Scheduled && t.passes == 0)
    {
        output::print_info("Nothing has run unattended yet — `devp status daemon` says why.");
    }
    if unaccounted > 0 {
        output::print_info(&format!(
            "{unaccounted} earlier {} no trigger recorded.",
            output::plural(unaccounted, "pass has", "passes have"),
        ));
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
    use crate::config::PrunedDir;
    use crate::history::PassRecord;
    use chrono::Duration;

    /// A logged pass: `(adapter, bytes)` per directory, all in one repository.
    fn logged(trigger: Trigger, dirs: &[(&str, u64)]) -> Pass {
        Pass::Detailed(PassRecord {
            at: Utc::now(),
            trigger,
            argv: vec!["run".to_string()],
            version: "1.17.0".to_string(),
            dirs: dirs
                .iter()
                .map(|(adapter, size)| PrunedDir {
                    repo_path: std::path::PathBuf::from("/tmp/repo"),
                    bloat_dir: "node_modules".to_string(),
                    adapter: (*adapter).to_string(),
                    size_freed: *size,
                    runtime: None,
                })
                .collect(),
        })
    }

    /// A pass from before the log, as `history::merged` reconstructs one.
    fn recovered(bytes: u64) -> Pass {
        Pass::Summary {
            at: Utc::now() - Duration::days(2),
            bytes_freed: bytes,
            dirs_removed: 1,
            repos_touched: 1,
            dirs: None,
        }
    }

    #[test]
    fn managers_rank_by_what_they_gave_back() {
        let passes = [
            logged(Trigger::Manual, &[("npm", 500), ("cargo", 800)]),
            logged(Trigger::Scheduled, &[("npm", 900)]),
        ];

        let (ranked, unaccounted) = rank_managers(&passes);

        // npm's 1400 is spread over two passes; the ranking is by the sum, not by the
        // biggest single directory, or cargo's 800 would lead.
        assert_eq!(ranked[0].manager, "npm");
        assert_eq!(ranked[0].bytes, 1400);
        assert_eq!(ranked[0].dirs, 2);
        assert_eq!(ranked[1].manager, "cargo");
        assert_eq!(unaccounted, 0);
    }

    #[test]
    fn a_pass_with_no_directories_is_declared_rather_than_dropped() {
        // The upgraded-machine case. Its bytes cannot be attributed to any manager, and
        // the section has to say so — silently omitting it would make the breakdown look
        // like a complete account of the lifetime total, which it is not.
        let passes = [logged(Trigger::Manual, &[("npm", 500)]), recovered(9_000)];

        let (ranked, unaccounted) = rank_managers(&passes);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].bytes, 500);
        assert_eq!(unaccounted, 1);
    }

    #[test]
    fn every_trigger_is_listed_even_at_zero() {
        // A scheduled line reading zero is the answer to "is the daemon doing anything",
        // and dropping empty rows would delete exactly that answer.
        let passes = [logged(Trigger::Manual, &[("npm", 700)])];

        let (totals, unaccounted) = split_by_trigger(&passes);

        assert_eq!(totals.len(), Trigger::ALL.len());
        let scheduled = totals
            .iter()
            .find(|t| t.trigger == Trigger::Scheduled)
            .expect("scheduled is one of the three");
        assert_eq!(scheduled.passes, 0);
        assert_eq!(scheduled.bytes, 0);

        let manual = totals
            .iter()
            .find(|t| t.trigger == Trigger::Manual)
            .expect("manual is one of the three");
        assert_eq!(manual.passes, 1);
        assert_eq!(manual.bytes, 700);
        assert_eq!(unaccounted, 0);
    }

    #[test]
    fn a_pass_from_before_the_log_has_no_trigger_to_split_by() {
        let passes = [recovered(9_000)];

        let (totals, unaccounted) = split_by_trigger(&passes);

        assert!(totals.iter().all(|t| t.passes == 0));
        assert_eq!(unaccounted, 1);
    }

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
