// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

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
        PruneStatus::ActivityCheckError(_) => "activity_check_error",
        PruneStatus::PathMissing => "path_missing",
        PruneStatus::NoBloat => "no_bloat",
        PruneStatus::Disabled => "disabled",
        PruneStatus::SkippedIgnored => "ignored",
        PruneStatus::DeleteError(_) => "delete_error",
        PruneStatus::ConfigError(_) => "config_error",
        PruneStatus::SkippedSymlink(_) => "skipped_symlink",
    }
}

/// The detail carried by the failure variants, if any.
fn status_message(status: &PruneStatus) -> Option<&str> {
    match status {
        PruneStatus::LockfileError(e)
        | PruneStatus::ActivityCheckError(e)
        | PruneStatus::DeleteError(e)
        | PruneStatus::ConfigError(e)
        | PruneStatus::SkippedSymlink(e) => Some(e.trim()),
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
        "poetry" => "poetry lock",
        "pdm" => "pdm lock",
        "pipenv" => "pipenv lock",
        "cargo" => "cargo generate-lockfile",
        "go" => "go mod tidy",
        "composer" => "composer update --no-install",
        "bundler" => "bundle lock",
        "cocoapods" => "pod install",
        "mix" => "mix deps.get",
        // venv has no lockfile to regenerate — the fix is to write `requirements.txt`,
        // which is authoring work, not a command we can hand over. gradle, maven and
        // swift verify manifest presence, not lockfile sync — a missing manifest has no
        // mechanical fix either.
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
        "shared_bytes": result.shared_bytes,
    });

    if let Some(message) = status_message(&result.status) {
        obj["message"] = json!(message);
    }
    if matches!(result.status, PruneStatus::LockfileError(_))
        && let Some(fix) = lockfile_fix_command(&result.adapter_name)
    {
        obj["fix_command"] = json!(fix);
    }
    obj
}

/// The document emitted by `devp run --json`.
///
/// `summary.errors` counts results whose status is one of the four failure tags; a
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
                    | PruneStatus::ActivityCheckError(_)
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
        "bytes_freed": entry.entry.total_freed_bytes,
        "added_at": entry.entry.added_at.to_rfc3339(),
        "adapters": entry.adapters,
        "reclaimable_bytes": entry.reclaimable_bytes,
        "directories": entry.bloat_dirs.iter().map(|b| json!({
            "name": b.name,
            "path": clean_path(&b.path),
            "bytes": b.size_bytes,
            "shared_bytes": b.shared_bytes,
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
///
/// `top` trims the `repositories` array only. `totals` is always computed over every
/// registered repository, and `top` is echoed back so a consumer can tell a short list
/// from a tidy machine.
pub fn status_document(
    registry: &Registry,
    repos: &[RepoStatusEntry],
    daemon: &str,
    hooks: &str,
    top: Option<usize>,
) -> Value {
    let reclaimable: u64 = repos.iter().map(|r| r.reclaimable_bytes).sum();
    let candidates = repos
        .iter()
        .filter(|r| matches!(r.reason, SkipReason::Candidate))
        .count();
    let listed = crate::engine::take_top(repos, top);

    let mut doc = json!({
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
        "repositories": listed.iter().map(repo_value).collect::<Vec<_>>(),
    });

    // Absent rather than null when the whole list is present, the same rule `message`
    // and `note` follow elsewhere in this contract.
    if let Some(n) = top {
        doc["top"] = json!(n);
    }
    doc
}

/// The document emitted by `devp stats --json`.
///
/// Three different vintages of number live here, and the field names say which is which.
/// `lifetime` has been accumulating since 1.0.0. `recent_passes` and the `bytes_freed`
/// inside `repositories` are only recorded from 1.1.0 onward, so on an upgraded machine
/// they start near zero while `lifetime` does not — `history_starts_at` names the version
/// that changed, so a consumer can say so rather than reporting a regression.
pub fn stats_document(registry: &Registry) -> Value {
    let mut repos: Vec<(&std::path::PathBuf, &crate::config::RepoEntry)> =
        registry.repositories.iter().collect();
    repos.sort_by(|a, b| {
        b.1.total_freed_bytes
            .cmp(&a.1.total_freed_bytes)
            .then_with(|| a.0.cmp(b.0))
    });

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "stats",
        "history_starts_at": constants::HISTORY_STARTS_AT,
        "lifetime": {
            "bytes_freed": registry.total_freed_bytes,
            // Same name and same number as `totals.prune_passes` in the status document.
            // One per pass that deleted something, wherever it was started from.
            "prune_passes": registry.total_pruned_count,
            "repositories": registry.repo_count(),
        },
        "last_prune": registry.last_prune.as_ref().map(|p| json!({
            "at": p.at.to_rfc3339(),
            "bytes_freed": p.dirs.iter().map(|d| d.size_freed).sum::<u64>(),
            "directories": p.dirs.len(),
        })),
        "recent_passes": registry.prune_history.iter().rev().map(|p| json!({
            "at": p.at.to_rfc3339(),
            "bytes_freed": p.bytes_freed,
            "directories": p.dirs_removed,
            "repositories": p.repos_touched,
        })).collect::<Vec<_>>(),
        "repositories": repos.iter().map(|(path, entry)| json!({
            "path": clean_path(path),
            "bytes_freed": entry.total_freed_bytes,
            "last_pruned_at": entry.last_pruned_at.map(|t| t.to_rfc3339()),
        })).collect::<Vec<_>>(),
    })
}

/// The document emitted by `devp caches --json`.
///
/// `clear_command` is the one field an agent can act on, and it is the only place in this
/// contract that carries a command dev-prune will not run itself: these caches are shared
/// by every project on the machine, so clearing one is a human's decision. `note` is
/// present only where there is a cost beyond time.
pub fn caches_document(reports: &[crate::commands::caches::CacheReport]) -> Value {
    let total: u64 = reports.iter().map(|r| r.bytes).sum();

    let caches: Vec<Value> = reports
        .iter()
        .map(|r| {
            let mut obj = json!({
                "manager": r.manager,
                "kind": r.kind,
                "path": clean_path(&r.path),
                "bytes": r.bytes,
                "clear_command": r.clear_command,
            });
            if let Some(note) = r.note {
                obj["note"] = json!(note);
            }
            obj
        })
        .collect();

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "caches",
        "caches": caches,
        "summary": {
            "total_bytes": total,
            "count": reports.len(),
        },
    })
}
/// `caches clear --dry-run --json`: what would be emptied, and nothing touched.
pub fn caches_clear_plan_document(reports: &[crate::commands::caches::CacheReport]) -> Value {
    let total: u64 = reports.iter().map(|r| r.bytes).sum();

    let caches: Vec<Value> = reports
        .iter()
        .map(|r| {
            json!({
                "manager": r.manager,
                "kind": r.kind,
                "path": clean_path(&r.path),
                "bytes": r.bytes,
                "clear_command": r.clear_command,
            })
        })
        .collect();

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "caches clear",
        "dry_run": true,
        "caches": caches,
        "summary": {
            "total_bytes": total,
            "count": reports.len(),
        },
    })
}

/// `caches clear --json`: what actually went.
///
/// `freed_bytes` is measured, not assumed — a `prune` keeps what is still referenced,
/// and a clear that failed half-way still freed part of it.
pub fn caches_clear_document(outcomes: &[crate::commands::caches::ClearOutcome]) -> Value {
    let freed: u64 = outcomes.iter().map(|o| o.freed()).sum();
    let failed = outcomes.iter().filter(|o| o.problem.is_some()).count();

    let caches: Vec<Value> = outcomes
        .iter()
        .map(|o| {
            let mut obj = json!({
                "manager": o.manager,
                "kind": o.kind,
                "path": clean_path(&o.path),
                "bytes_before": o.before,
                "bytes_after": o.after,
                "freed_bytes": o.freed(),
                "cleared": o.problem.is_none(),
            });
            if let Some(problem) = &o.problem {
                obj["error"] = json!(problem);
            }
            obj
        })
        .collect();

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "caches clear",
        "dry_run": false,
        "caches": caches,
        "summary": {
            "freed_bytes": freed,
            "count": outcomes.len(),
            "failed": failed,
        },
    })
}
/// `devp trust --json`: what the tool guarantees, and what this machine has switched on.
///
/// Guarantees and machine state stay in separate arrays because they are different kinds
/// of claim — one is structural and one is a reading — and flattening them would let a
/// consumer treat a setting as a promise.
pub fn trust_document(report: &crate::commands::trust::TrustReport) -> Value {
    let rows = |rows: &[crate::commands::trust::TrustRow]| -> Vec<Value> {
        rows.iter()
            .map(|r| {
                json!({
                    "key": r.key,
                    "subject": r.subject,
                    "state": r.state,
                    "verdict": r.verdict_key(),
                })
            })
            .collect()
    };

    let widened = report.widened();

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "trust",
        "guarantees": rows(&report.guarantees),
        "machine": rows(&report.machine),
        "summary": {
            "widened": widened,
            "widened_count": widened.len(),
        },
    })
}

/// The document emitted by `devp status --drift --json`.
///
/// A separate document from plain `status` because it answers a different question:
/// not "what could a prune reclaim" but "what would a prune refuse, and why". An empty
/// `drift` array means nothing was *detected*, across the adapters that can compare an
/// environment against its lockfile from files alone.
pub fn drift_document(findings: &[crate::commands::status::ProjectDrift]) -> Value {
    let unrecorded_total: usize = findings.iter().map(|f| f.report.unrecorded.len()).sum();

    json!({
        "schema": SCHEMA_VERSION,
        "version": constants::VERSION,
        "command": "status --drift",
        "drift": findings.iter().map(|f| json!({
            "repository": clean_path(&f.repository),
            "project": f.project,
            "adapter": f.adapter,
            "directory": f.report.directory,
            "unrecorded": f.report.unrecorded,
            "record_command": f.report.record_command,
        })).collect::<Vec<_>>(),
        "summary": {
            "projects_with_drift": findings.len(),
            "unrecorded_packages": unrecorded_total,
        },
    })
}

/// Print a document to stdout as pretty JSON with a trailing newline.
///
/// Pretty rather than compact because a human reads this output far more often than a
/// parser does, and `jq` does not care either way.
///
/// When stdout is a terminal, the same document also lands on the clipboard: a pipe or
/// a redirect means a program is consuming the output, but a terminal means a *person*
/// asked for JSON, and the next thing they usually do is paste it somewhere. The
/// notice goes to stderr and the copy is skipped entirely when piped, so the stdout
/// contract — one document, byte-identical either way — holds.
pub fn emit(document: &Value) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    let text = serde_json::to_string_pretty(document)?;
    println!("{text}");
    if std::io::stdout().is_terminal() && copy_to_clipboard(&text) {
        use colored::Colorize;
        eprintln!("{}", "(also copied to your clipboard)".dimmed());
    }
    Ok(())
}

/// Best-effort: put `text` on the system clipboard. Returns whether it worked.
///
/// Spawns the platform's own clipboard tool rather than linking a clipboard crate — a
/// native dependency is a heavy price for a nicety. `clip` on Windows, `pbcopy` on
/// macOS, then `wl-copy`/`xclip`/`xsel` in that order on Linux; a headless box has
/// none of them, and quietly not copying is the right behaviour there.
fn copy_to_clipboard(text: &str) -> bool {
    // `clip.exe` reads its input in the console codepage unless a BOM says otherwise;
    // UTF-16LE with a BOM is the one encoding it always honours, and repository paths
    // are not guaranteed to be ASCII.
    let bytes: Vec<u8> = if cfg!(windows) {
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        utf16
    } else {
        text.as_bytes().to_vec()
    };

    // On Windows the tool is named by full path: `CreateProcess` resolves a bare
    // program name through the *current directory* before PATH, and dev-prune is
    // routinely run from inside checkouts it has no reason to trust — a repository
    // carrying its own `clip.exe` must not become the thing that executes. Unix PATH
    // search never consults the current directory, so the bare names there are fine.
    let windows_clip = std::env::var("SystemRoot")
        .map(|root| format!("{root}\\System32\\clip.exe"))
        .unwrap_or_else(|_| String::from("C:\\Windows\\System32\\clip.exe"));
    let tools: Vec<Vec<&str>> = if cfg!(windows) {
        vec![vec![windows_clip.as_str()]]
    } else if cfg!(target_os = "macos") {
        vec![vec!["pbcopy"]]
    } else {
        vec![
            vec!["wl-copy"],
            vec!["xclip", "-selection", "clipboard"],
            vec!["xsel", "--clipboard", "--input"],
        ]
    };
    tools.iter().any(|tool| pipe_into(tool, &bytes))
}

/// Run `command`, feed `bytes` to its stdin, and report whether it exited cleanly.
fn pipe_into(command: &[&str], bytes: &[u8]) -> bool {
    use std::io::Write;
    use std::process::Stdio;
    let Ok(mut child) = crate::spawn::command(command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(bytes).is_ok());
    let exited_cleanly = child.wait().map(|status| status.success()).unwrap_or(false);
    wrote && exited_cleanly
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
            shared_bytes: 0,
            runtime: None,
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
            PruneStatus::ActivityCheckError("x".into()),
            PruneStatus::PathMissing,
            PruneStatus::NoBloat,
            PruneStatus::Disabled,
            PruneStatus::SkippedIgnored,
            PruneStatus::DeleteError("x".into()),
            PruneStatus::ConfigError("x".into()),
            PruneStatus::SkippedSymlink("x".into()),
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
    fn the_cache_report_totals_what_it_lists() {
        use crate::commands::caches::{CacheReport, Clear};

        let doc = caches_document(&[
            CacheReport {
                manager: "go",
                kind: "module cache",
                path: PathBuf::from("/home/dev/go/pkg/mod"),
                bytes: 4_000,
                clear_command: "go clean -modcache",
                clear: Clear::Command("go", &["clean", "-modcache"]),
                note: None,
            },
            CacheReport {
                manager: "pnpm",
                kind: "store",
                path: PathBuf::from("/home/dev/.pnpm-store"),
                bytes: 1_000,
                clear_command: "pnpm store prune",
                clear: Clear::Command("pnpm", &["store", "prune"]),
                note: Some("hardlinked"),
            },
        ]);

        assert_eq!(doc["command"], "caches");
        assert_eq!(doc["summary"]["total_bytes"], 5_000);
        assert_eq!(doc["summary"]["count"], 2);
        // Absent rather than null where there is nothing to say, matching every other
        // optional field in this contract.
        assert!(doc["caches"][0].get("note").is_none());
        assert_eq!(doc["caches"][1]["note"], "hardlinked");
        assert_eq!(doc["caches"][0]["clear_command"], "go clean -modcache");
    }

    #[test]
    fn an_empty_cache_report_is_still_a_document() {
        // A machine with no package manager installed must produce a parseable zero, not
        // an absent `summary` a consumer would have to special-case.
        let doc = caches_document(&[]);
        assert_eq!(doc["summary"]["total_bytes"], 0);
        assert_eq!(doc["caches"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn every_adapter_with_a_lockfile_has_a_fix_command() {
        for adapter in crate::adapters::get_all_adapters() {
            // venv, gradle, maven and swift verify without a lockfile-sync step — see
            // `lockfile_fix_command` for why each has nothing mechanical to hand over.
            if matches!(adapter.name(), "venv" | "gradle" | "maven" | "swift") {
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
