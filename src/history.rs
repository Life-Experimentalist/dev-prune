// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! The per-pass prune log: what each pass deleted, and what asked it to.
//!
//! The registry already counted passes — four integers each, capped at fifty — which
//! answers "how much has this saved me" and nothing else. It could not answer "what did
//! the pass on the 12th actually delete", because the only full directory list it keeps
//! is `last_prune`, and the next pass overwrites it. This file is that missing half.
//!
//! It is a separate file rather than another field on the registry because the two are
//! read on opposite schedules. `registry.json` is parsed by every command and rewritten
//! in full on every save; a growing per-directory record inside it would be a cost paid
//! by `devp status`. This one is opened by `devp history` and appended to by a pass.
//!
//! Nothing here is load-bearing. A pass that cannot write its log entry still deleted the
//! directories and still updated the registry, and `devp restore --last-run` reads the
//! registry, not this. Every write is best-effort and every read tolerates a torn line:
//! a pass killed between the `write_all` and the `sync_all` must cost one log entry, not
//! the command that reads it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{PrunedDir, Registry};
use crate::constants;

/// What started a prune pass.
///
/// The distinction the user is actually asking for when they open the log: an unattended
/// pass that ran while they were asleep reads differently from one they typed. There are
/// exactly three ways a prune starts — `devp run`, the scheduler's `devp run --daemon`,
/// and the `[p]` key in `devp status` — and a Git hook is not one of them: the hook runs
/// `devp link --quiet`, which registers a repository and deletes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Someone typed the command.
    Manual,
    /// The scheduled task, launch agent or systemd timer ran `run --daemon`.
    Scheduled,
    /// Selected with `[p]` from the `devp status` dashboard.
    Dashboard,
}

impl Trigger {
    /// Every trigger, in the order `devp stats` lists them.
    ///
    /// The report prints all three even at zero, because a `scheduled` line reading zero
    /// is the answer to "is the daemon doing anything" — and iterating the variants from
    /// here is what stops a fourth one from being added and silently never printed.
    pub const ALL: [Trigger; 3] = [Trigger::Manual, Trigger::Scheduled, Trigger::Dashboard];

    /// The word the report prints.
    pub fn label(self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::Scheduled => "scheduled",
            Trigger::Dashboard => "dashboard",
        }
    }

    /// The trigger for a `devp run`, given whether the scheduler started it.
    pub fn for_run(daemon: bool) -> Self {
        if daemon {
            Trigger::Scheduled
        } else {
            Trigger::Manual
        }
    }
}

/// One prune pass, in full.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassRecord {
    /// When the pass started. Doubles as its identity — see [`record`].
    pub at: DateTime<Utc>,
    /// What started it.
    pub trigger: Trigger,
    /// The arguments the pass ran under, `argv[1..]`, unjoined.
    ///
    /// Stored as a list and joined only when printed. A repository path with a space in
    /// it comes back out of a joined string as two arguments, and the whole point of
    /// recording the flags is being able to read back what was really asked for.
    #[serde(default)]
    pub argv: Vec<String>,
    /// The version of dev-prune that ran the pass.
    #[serde(default)]
    pub version: String,
    /// Every directory it removed.
    pub dirs: Vec<PrunedDir>,
}

impl PassRecord {
    /// Bytes this pass reclaimed.
    pub fn bytes_freed(&self) -> u64 {
        self.dirs.iter().map(|d| d.size_freed).sum()
    }

    /// How many distinct repositories it touched.
    pub fn repos_touched(&self) -> usize {
        let mut paths: Vec<&PathBuf> = self.dirs.iter().map(|d| &d.repo_path).collect();
        paths.sort();
        paths.dedup();
        paths.len()
    }

    /// The command line as a person would type it, for the report.
    pub fn command_line(&self) -> String {
        let args = self
            .argv
            .iter()
            .map(|a| {
                if a.contains(' ') {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        if args.is_empty() {
            "devp".to_string()
        } else {
            format!("devp {args}")
        }
    }
}

/// Full path to the prune log.
pub fn log_path() -> Result<PathBuf> {
    Ok(Registry::config_dir()?.join(constants::PRUNE_LOG_FILENAME))
}

/// The arguments this process was invoked with, minus the program name.
///
/// Lossy on purpose: an argument that is not valid UTF-8 becomes its replacement form
/// rather than dropping the whole record. A log entry that renders one path oddly is
/// worth more than no entry at all.
fn current_argv() -> Vec<String> {
    std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// Record a pass, superseding this same pass's earlier entry.
///
/// `at` identifies the pass, exactly as it does for
/// [`Registry::record_prune_progress`][crate::config::Registry::record_prune_progress]:
/// a pass persists after every repository so a crash cannot strand it, and each of those
/// calls carries the same timestamp and the growing directory list. Matching on `at`
/// makes the last one win instead of writing one entry per repository.
///
/// Errors are swallowed. The caller has already deleted the directories.
pub fn record(at: DateTime<Utc>, trigger: Trigger, dirs: &[PrunedDir]) {
    if dirs.is_empty() {
        return;
    }
    let record = PassRecord {
        at,
        trigger,
        argv: current_argv(),
        version: constants::VERSION.to_string(),
        dirs: dirs.to_vec(),
    };
    if let Ok(path) = log_path() {
        let _ = append_to(&path, record);
    }
}

/// [`record`], against an explicit path. The testable half.
pub fn append_to(path: &Path, record: PassRecord) -> Result<()> {
    let mut records = load_from(path)?;
    match records.iter().position(|r| r.at == record.at) {
        Some(index) => records[index] = record,
        None => records.push(record),
    }
    // Oldest first, so the overflow comes off the front — the same shape as the
    // registry's own history, and for the same reason.
    if records.len() > constants::PRUNE_LOG_LIMIT {
        let excess = records.len() - constants::PRUNE_LOG_LIMIT;
        records.drain(..excess);
    }
    write_to(path, &records)
}

/// Every recorded pass, oldest first.
pub fn load() -> Result<Vec<PassRecord>> {
    load_from(&log_path()?)
}

/// [`load`], against an explicit path.
///
/// A line that does not parse is skipped rather than failing the read. The last line is
/// the one at risk — a pass killed mid-write leaves a partial one — and one truncated
/// entry must not make every earlier pass unreadable forever.
pub fn load_from(path: &Path) -> Result<Vec<PassRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read prune log at {}", path.display()))?;
    Ok(contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<PassRecord>(line).ok())
        .collect())
}

/// Rewrite the log, atomically, using the same temp-and-rename dance as the registry.
fn write_to(path: &Path, records: &[PassRecord]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
    }
    // Unique per process, for the reason `Registry::save_to` documents: a manual run and
    // a scheduled pass can write at the same moment, and a shared temp name lets one
    // rename the other's half-written file into place.
    let tmp_path = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
    let mut contents = String::new();
    for record in records {
        contents.push_str(&serde_json::to_string(record).context("Failed to serialize a pass")?);
        contents.push('\n');
    }
    {
        use std::io::Write;
        let mut file = fs::File::create(&tmp_path)
            .with_context(|| format!("Failed to write temp prune log {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("Failed to write temp prune log {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to flush temp prune log {}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path)
        .with_context(|| format!("Failed to rename temp prune log to {}", path.display()))
}

/// One pass as `devp history` sees it, from whichever source knows about it.
///
/// The two variants are not a design choice so much as an admission: this log starts at
/// [`constants::PRUNE_LOG_STARTS_AT`], and every pass a machine ran before that upgrade
/// exists only as the registry's four numbers. Showing those as a gap would read as data
/// loss, and leaving them out entirely would make `devp history` disagree with the pass
/// count `devp stats` prints two lines above it.
#[derive(Debug, Clone)]
pub enum Pass {
    /// A pass this log recorded in full.
    Detailed(PassRecord),
    /// A pass known only from the registry's summary.
    Summary {
        at: DateTime<Utc>,
        bytes_freed: u64,
        dirs_removed: usize,
        repos_touched: usize,
        /// The directory list, when this happens to be the pass `last_prune` describes.
        dirs: Option<Vec<PrunedDir>>,
    },
}

impl Pass {
    pub fn at(&self) -> DateTime<Utc> {
        match self {
            Pass::Detailed(r) => r.at,
            Pass::Summary { at, .. } => *at,
        }
    }

    pub fn bytes_freed(&self) -> u64 {
        match self {
            Pass::Detailed(r) => r.bytes_freed(),
            Pass::Summary { bytes_freed, .. } => *bytes_freed,
        }
    }

    pub fn dirs_removed(&self) -> usize {
        match self {
            Pass::Detailed(r) => r.dirs.len(),
            Pass::Summary { dirs_removed, .. } => *dirs_removed,
        }
    }

    pub fn repos_touched(&self) -> usize {
        match self {
            Pass::Detailed(r) => r.repos_touched(),
            Pass::Summary { repos_touched, .. } => *repos_touched,
        }
    }

    /// What started the pass, where that is known.
    ///
    /// `None` for a pass recovered from the registry summary: the summary has never
    /// carried a trigger, so a machine that pruned before 1.17.0 cannot say afterwards
    /// which of those passes it typed and which the scheduler ran.
    pub fn trigger(&self) -> Option<Trigger> {
        match self {
            Pass::Detailed(r) => Some(r.trigger),
            Pass::Summary { .. } => None,
        }
    }

    /// The directory list, where one is known.
    pub fn dirs(&self) -> Option<&[PrunedDir]> {
        match self {
            Pass::Detailed(r) => Some(&r.dirs),
            Pass::Summary { dirs, .. } => dirs.as_deref(),
        }
    }
}

/// Every pass this machine can account for, newest first.
///
/// The log wins wherever both know about a pass: they are keyed by the same timestamp,
/// and the log's copy carries the directory list and the flags.
pub fn merged(records: Vec<PassRecord>, registry: &Registry) -> Vec<Pass> {
    let mut passes: Vec<Pass> = Vec::with_capacity(records.len() + registry.prune_history.len());
    let known: Vec<DateTime<Utc>> = records.iter().map(|r| r.at).collect();
    passes.extend(records.into_iter().map(Pass::Detailed));

    for summary in &registry.prune_history {
        if known.contains(&summary.at) {
            continue;
        }
        // `last_prune` and the newest summary describe the same pass, so on a machine
        // upgrading into 1.17.0 the most recent pre-log pass still has its directories.
        let dirs = registry
            .last_prune
            .as_ref()
            .filter(|last| last.at == summary.at)
            .map(|last| last.dirs.clone());
        passes.push(Pass::Summary {
            at: summary.at,
            bytes_freed: summary.bytes_freed,
            dirs_removed: summary.dirs_removed,
            repos_touched: summary.repos_touched,
            dirs,
        });
    }

    passes.sort_by_key(|pass| std::cmp::Reverse(pass.at()));
    passes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LastPrune, PruneRunSummary};
    use chrono::Duration;
    use tempfile::TempDir;

    fn dir(repo: &str, name: &str, bytes: u64) -> PrunedDir {
        PrunedDir {
            repo_path: PathBuf::from(repo),
            bloat_dir: name.to_string(),
            adapter: "npm".to_string(),
            size_freed: bytes,
            runtime: None,
        }
    }

    fn record_at(at: DateTime<Utc>, dirs: Vec<PrunedDir>) -> PassRecord {
        PassRecord {
            at,
            trigger: Trigger::Manual,
            argv: vec!["run".to_string()],
            version: "1.17.0".to_string(),
            dirs,
        }
    }

    #[test]
    fn a_pass_that_saves_after_every_repository_is_one_entry_not_five() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("prune-log.jsonl");
        let at = Utc::now();

        append_to(&path, record_at(at, vec![dir("/a", "node_modules", 100)])).unwrap();
        append_to(
            &path,
            record_at(
                at,
                vec![
                    dir("/a", "node_modules", 100),
                    dir("/b", "node_modules", 200),
                ],
            ),
        )
        .unwrap();

        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].dirs.len(), 2);
        assert_eq!(loaded[0].bytes_freed(), 300);
        assert_eq!(loaded[0].repos_touched(), 2);
    }

    #[test]
    fn a_torn_last_line_costs_one_pass_and_not_the_whole_log() {
        // The failure this tolerance exists for: a machine powered off between the
        // write and the flush. Every pass before it must still be readable.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("prune-log.jsonl");
        let at = Utc::now();
        append_to(&path, record_at(at, vec![dir("/a", "node_modules", 100)])).unwrap();

        let mut raw = fs::read_to_string(&path).unwrap();
        raw.push_str("{\"at\":\"2026-09-01T00:00:00Z\",\"trig");
        fs::write(&path, raw).unwrap();

        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].at, at);
    }

    #[test]
    fn the_oldest_passes_come_off_the_front_at_the_cap() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("prune-log.jsonl");
        let base = Utc::now();
        for i in 0..(constants::PRUNE_LOG_LIMIT + 5) {
            let at = base + Duration::seconds(i as i64);
            append_to(&path, record_at(at, vec![dir("/a", "node_modules", 1)])).unwrap();
        }
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.len(), constants::PRUNE_LOG_LIMIT);
        assert_eq!(loaded[0].at, base + Duration::seconds(5));
    }

    #[test]
    fn a_missing_log_is_no_passes_and_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let loaded = load_from(&tmp.path().join("absent.jsonl")).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn passes_recorded_before_the_log_existed_still_appear() {
        // The case that made this merge worth writing: an upgraded machine whose ten
        // passes all predate the log. An empty `devp history` beside a `devp stats`
        // reading "10 prune passes" is indistinguishable from a bug.
        let mut registry = Registry::default();
        let old = Utc::now() - Duration::days(30);
        let newest = Utc::now() - Duration::days(1);
        registry.prune_history = vec![
            PruneRunSummary {
                at: old,
                bytes_freed: 500,
                dirs_removed: 2,
                repos_touched: 1,
            },
            PruneRunSummary {
                at: newest,
                bytes_freed: 900,
                dirs_removed: 3,
                repos_touched: 2,
            },
        ];
        registry.last_prune = Some(LastPrune {
            at: newest,
            dirs: vec![dir("/a", "node_modules", 900)],
        });

        let passes = merged(Vec::new(), &registry);
        assert_eq!(passes.len(), 2);
        assert_eq!(passes[0].at(), newest);
        // The newest pre-log pass keeps its directories, because `last_prune` has them.
        assert!(passes[0].dirs().is_some());
        assert!(passes[1].dirs().is_none());
        assert_eq!(passes[1].bytes_freed(), 500);
    }

    #[test]
    fn the_log_supersedes_the_registry_summary_of_the_same_pass() {
        let mut registry = Registry::default();
        let at = Utc::now();
        registry.prune_history = vec![PruneRunSummary {
            at,
            bytes_freed: 300,
            dirs_removed: 2,
            repos_touched: 2,
        }];
        let passes = merged(
            vec![record_at(
                at,
                vec![
                    dir("/a", "node_modules", 100),
                    dir("/b", "node_modules", 200),
                ],
            )],
            &registry,
        );
        assert_eq!(passes.len(), 1);
        assert!(matches!(passes[0], Pass::Detailed(_)));
    }

    #[test]
    fn a_path_with_a_space_survives_the_round_trip_to_a_command_line() {
        // Why `argv` is a list and not a joined string: joined, this reads as two
        // arguments and the log stops being a record of what was asked for.
        let record = PassRecord {
            at: Utc::now(),
            trigger: Trigger::Manual,
            argv: vec!["run".to_string(), "C:\\My Code\\app".to_string()],
            version: "1.17.0".to_string(),
            dirs: vec![],
        };
        assert_eq!(record.command_line(), "devp run \"C:\\My Code\\app\"");
    }

    #[test]
    fn the_scheduler_is_the_only_thing_that_records_a_scheduled_pass() {
        assert_eq!(Trigger::for_run(true), Trigger::Scheduled);
        assert_eq!(Trigger::for_run(false), Trigger::Manual);
    }
}
