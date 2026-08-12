// Machine-readable output for `--json`.
//
// This module is the whole contract. Every field an AI agent, CI step or script can rely
// on is built here, so there is exactly one place to look when asking "what does
// dev-prune emit?" and exactly one place to change when the answer moves.
//
// ## Stability
//
// `schema` is an integer that increases when a consumer would have to change to keep
// working: a field removed, renamed, or given a different meaning. *Adding* a field does
// not bump it, so parse permissively and ignore what you do not recognise.
//
// Paths are emitted through `output::clean_path`, which is what the human output shows,
// so the two never disagree about what a repository is called.

use serde_json::{Value, json};

use crate::config::{Registry, Settings};
use crate::constants;
use crate::engine::{PruneResult, PruneStatus, RepoStatusEntry, SkipReason};
use crate::output::clean_path;

/// Current output schema version. See the module docs before changing it.
pub const SCHEMA_VERSION: u32 = 1;

/// The stable machine name for a prune outcome.
///
/// Deliberately not the `Display` string: the human text is free to be reworded, these
/// are not. Keep them lowercase snake_case and never reuse a retired one.
fn status_tag(status: &PruneStatus) -> &'static str {
    match status {
        PruneStatus::Pruned => "pruned",
        PruneStatus::SkippedActive => "skipped_active",
        PruneStatus::SkippedDryRun => "skipped_dry_run",
        PruneStatus::LockfileError(_) => "lockfile_error",
        PruneStatus::NoBloat => "no_bloat",
        PruneStatus::Disabled => "disabled",
        PruneStatus::SkippedIgnored => "ignored",
        PruneStatus::DeleteError(_) => "delete_error",
        PruneStatus::ConfigError(_) => "config_error",
    }
}

/// The detail carried by the failure variants, if any.
fn status_message(status: &PruneStatus) -> Option<&str> {
    match status {
        PruneStatus::LockfileError(e)
        | PruneStatus::DeleteError(e)
        | PruneStatus::ConfigError(e) => Some(e.trim()),
        _ => None,
    }
}

/// The command an agent should run to fix a failed lockfile check, or `None` when the
/// failure is not of that kind.
///
/// This is the single reason an agent can act on a `lockfile_error` without a human:
/// the fix is mechanical and the same one the human report prints.
///
/// Each of these is the *writing* form of that adapter's verification — the one
/// [`crate::adapters::enforce_two_tier`] refuses to run on the user's behalf unless
/// they set `allow_manifest_rewrite`. It resyncs the lockfile with the manifest, which
/// is exactly what a failed read-only verification is complaining about.
pub fn lockfile_fix_command(adapter: &str) -> Option<&'static str> {
    Some(match adapter {
        "npm" => "npm install --package-lock-only --ignore-scripts",
        "pnpm" => "pnpm install --lockfile-only",
        "yarn" => "yarn install --mode update-lockfile",
        // bun has no resolve-only write mode; a plain install is what refreshes
        // `bun.lock`, and unlike the others it also populates `node_modules`.
        "bun" => "bun install",
        "uv" => "uv lock",
        "cargo" => "cargo generate-lockfile",
        "go" => "go mod tidy",
        // venv has no lockfile to regenerate — the fix is to write `requirements.txt`,
        // which is authoring work, not a command we can hand over.
        _ => return None,
    })
}

fn result_value(result: &PruneResult) -> Value {
    let mut obj = json!({
        "repository": clean_path(&result.repo_path),
        "adapter": result.adapter_name,
        "directory": result.bloat_dir,
        "status": status_tag(&result.status),
        "bytes": result.size_freed,
    });

    if let Some(message) = status_message(&result.status) {
        obj["message"] = json!(message);
    }
    if matches!(result.status, PruneStatus::LockfileError(_)) {
        if let Some(fix) = lockfile_fix_command(&result.adapter_name) {
            obj["fix_command"] = json!(fix);
        }
    }
    obj
}

/// The document emitted by `devp run --json`.
///
/// `summary.errors` counts results whose status is one of the three failure tags; a
/// consumer that only wants to know "did anything go wrong" can read that alone.
pub fn run_document(results: &[PruneResult], dry_run: bool) -> Value {
    let bytes_freed: u64 = results
        .iter()
        .filter(|r| matches!(r.status, PruneStatus::Pruned))
        .map(|r| r.size_freed)
        .sum();
    let directories_pruned = results
        .iter()
        .filter(|r| matches!(r.status, PruneStatus::Pruned))
        .count();
    let bytes_reclaimable: u64 = results
        .iter()
        .filter(|r| matches!(r.status, PruneStatus::SkippedDryRun))
        .map(|r| r.size_freed)
        .sum();
    let errors = results
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

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "run",
        "dry_run": dry_run,
        "results": results.iter().map(result_value).collect::<Vec<_>>(),
        "summary": {
            "bytes_freed": bytes_freed,
            "bytes_reclaimable": bytes_reclaimable,
            "directories_pruned": directories_pruned,
            "errors": errors,
        },
    })
}

/// The stable machine name for why a repository is or is not a candidate.
fn reason_tag(reason: &SkipReason) -> &'static str {
    match reason {
        SkipReason::Candidate => "candidate",
        SkipReason::Active => "active",
        SkipReason::Ignored => "ignored",
        SkipReason::NoBloat => "no_bloat",
        SkipReason::PathMissing => "path_missing",
        SkipReason::ConfigError(_) => "config_error",
    }
}

fn settings_value(settings: &Settings) -> Value {
    json!({
        "idle_days": settings.idle_days,
        "check_interval_days": settings.check_interval_days,
        "auto_setup": settings.auto_setup,
        "auto_hooks": settings.auto_hooks,
        "auto_daemon": settings.auto_daemon,
        "require_confirmation": settings.require_confirmation,
        "command_timeout_secs": settings.command_timeout_secs,
        "min_size_mb": settings.min_size_mb,
        "update_check": settings.update_check,
    })
}

fn repo_value(entry: &RepoStatusEntry) -> Value {
    let mut obj = json!({
        "path": clean_path(&entry.path),
        "state": reason_tag(&entry.reason),
        "enabled": entry.entry.enabled,
        "idle_days": entry.idle_days,
        "last_activity": entry.last_activity.map(|t| t.to_rfc3339()),
        "last_pruned_at": entry.entry.last_pruned_at.map(|t| t.to_rfc3339()),
        "added_at": entry.entry.added_at.to_rfc3339(),
        "adapters": entry.adapters,
        "reclaimable_bytes": entry.reclaimable_bytes,
        "directories": entry.bloat_dirs.iter().map(|b| json!({
            "name": b.name,
            "path": clean_path(&b.path),
            "bytes": b.size_bytes,
        })).collect::<Vec<_>>(),
    });

    // Present only on `config_error`, and absent rather than null everywhere else — the
    // same rule `result_value` follows for `message`, so one parser handles both
    // documents. It carries the actual parse failure, so an agent can report what is
    // wrong with the file instead of only the state word.
    if let SkipReason::ConfigError(e) = &entry.reason {
        obj["error"] = json!(e);
    }
    obj
}

/// The document emitted by `devp status --json`.
///
/// `daemon` and `hooks` are the same strings the dashboard shows; they describe the
/// state of the machine's integrations, which is what an agent needs to decide whether
/// to suggest `devp setup`.
pub fn status_document(
    registry: &Registry,
    repos: &[RepoStatusEntry],
    daemon: &str,
    hooks: &str,
) -> Value {
    let reclaimable: u64 = repos.iter().map(|r| r.reclaimable_bytes).sum();
    let candidates = repos
        .iter()
        .filter(|r| matches!(r.reason, SkipReason::Candidate))
        .count();

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "status",
        "config_path": Registry::registry_path().map(|p| clean_path(&p)).ok(),
        "integrations": { "daemon": daemon, "git_hooks": hooks },
        "settings": settings_value(&registry.settings),
        "totals": {
            "repositories": registry.repo_count(),
            "candidates": candidates,
            "reclaimable_bytes": reclaimable,
            "historical_bytes_freed": registry.total_freed_bytes,
            "prune_passes": registry.total_pruned_count,
        },
        "repositories": repos.iter().map(repo_value).collect::<Vec<_>>(),
    })
}

/// Print a document to stdout as pretty JSON with a trailing newline.
///
/// Pretty rather than compact because a human reads this output far more often than a
/// parser does, and `jq` does not care either way.
pub fn emit(document: &Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(document)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn result(status: PruneStatus, bytes: u64) -> PruneResult {
        PruneResult {
            repo_path: PathBuf::from("/tmp/repo"),
            adapter_name: "pnpm".to_string(),
            bloat_dir: "node_modules".to_string(),
            size_freed: bytes,
            status,
        }
    }

    #[test]
    fn every_status_has_a_distinct_stable_tag() {
        let all = [
            PruneStatus::Pruned,
            PruneStatus::SkippedActive,
            PruneStatus::SkippedDryRun,
            PruneStatus::LockfileError("x".into()),
            PruneStatus::NoBloat,
            PruneStatus::Disabled,
            PruneStatus::SkippedIgnored,
            PruneStatus::DeleteError("x".into()),
            PruneStatus::ConfigError("x".into()),
        ];
        let mut tags: Vec<&str> = all.iter().map(status_tag).collect();
        let count = tags.len();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), count, "two statuses share a JSON tag");
    }

    #[test]
    fn every_repository_state_has_a_distinct_stable_tag() {
        let all = [
            SkipReason::Candidate,
            SkipReason::Active,
            SkipReason::Ignored,
            SkipReason::NoBloat,
            SkipReason::PathMissing,
            SkipReason::ConfigError("x".into()),
        ];
        let mut tags: Vec<&str> = all.iter().map(reason_tag).collect();
        let count = tags.len();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), count, "two repository states share a JSON tag");
    }

    #[test]
    fn only_an_unreadable_config_carries_an_error_field() {
        let entry = |reason| RepoStatusEntry {
            path: PathBuf::from("/tmp/repo"),
            entry: crate::config::RepoEntry::new(),
            reason,
            adapters: Vec::new(),
            bloat_dirs: Vec::new(),
            reclaimable_bytes: 0,
            last_activity: None,
            idle_days: 15,
        };

        let broken = repo_value(&entry(SkipReason::ConfigError("bad json".into())));
        assert_eq!(broken["state"], "config_error");
        assert_eq!(broken["error"], "bad json");

        // Absent, not null — the same shape rule `message` follows in the run document.
        let healthy = repo_value(&entry(SkipReason::Candidate));
        assert!(healthy.get("error").is_none());
    }

    #[test]
    fn run_summary_counts_only_real_deletions() {
        let doc = run_document(
            &[
                result(PruneStatus::Pruned, 100),
                result(PruneStatus::Pruned, 50),
                result(PruneStatus::SkippedActive, 0),
                result(PruneStatus::LockfileError("nope".into()), 0),
            ],
            false,
        );
        assert_eq!(doc["summary"]["bytes_freed"], 150);
        assert_eq!(doc["summary"]["directories_pruned"], 2);
        assert_eq!(doc["summary"]["errors"], 1);
    }

    #[test]
    fn dry_run_bytes_land_in_reclaimable_not_freed() {
        // A dry run must never claim to have freed anything — a CI step that adds up
        // `bytes_freed` across runs would otherwise report space that still exists.
        let doc = run_document(&[result(PruneStatus::SkippedDryRun, 4096)], true);
        assert_eq!(doc["summary"]["bytes_freed"], 0);
        assert_eq!(doc["summary"]["bytes_reclaimable"], 4096);
        assert_eq!(doc["dry_run"], true);
    }

    #[test]
    fn lockfile_errors_carry_the_fix_command() {
        let doc = run_document(
            &[result(PruneStatus::LockfileError("boom".into()), 0)],
            false,
        );
        assert_eq!(doc["results"][0]["message"], "boom");
        assert_eq!(
            doc["results"][0]["fix_command"],
            "pnpm install --lockfile-only"
        );
    }

    #[test]
    fn a_successful_result_carries_no_message_or_fix() {
        let doc = run_document(&[result(PruneStatus::Pruned, 1)], false);
        assert!(doc["results"][0].get("message").is_none());
        assert!(doc["results"][0].get("fix_command").is_none());
    }

    #[test]
    fn venv_has_no_mechanical_lockfile_fix() {
        // There is no command that writes a requirements.txt, so offering one would be
        // a lie an agent would then run.
        assert!(lockfile_fix_command("venv").is_none());
        assert!(lockfile_fix_command("nonsense").is_none());
    }

    #[test]
    fn every_adapter_with_a_lockfile_has_a_fix_command() {
        for adapter in crate::adapters::get_all_adapters() {
            if adapter.name() == "venv" {
                continue;
            }
            assert!(
                lockfile_fix_command(adapter.name()).is_some(),
                "{} has no fix command",
                adapter.name()
            );
        }
    }
}
