// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune trust`.
//
// A tool that deletes directories on a schedule has to answer "what exactly is this
// allowed to do on my machine?", and until this command existed the honest answer was
// "read four documentation pages and three `devp config` keys". This prints it.
//
// Two kinds of row, and the distinction is the whole point:
//
//   - **Guarantees** are structural. They come from the code paths in `engine.rs`, they
//     have no setting and no flag, and a build where one of them did not hold would be a
//     bug rather than a configuration. They read the same on every machine.
//   - **This machine** is read live — the scheduler, the Git hooks, the settings that
//     widen what dev-prune may do. These differ per machine and are the reason the
//     command exists at all.
//
// Nothing here is a self-assessment. Every "this machine" row is a value read back from
// the registry or the OS, and the three rows that can lower the verdict are named
// individually so the report cannot congratulate itself in the abstract.

use anyhow::Result;

use crate::commands::hook::{self, HookState};
use crate::config::Registry;
use crate::constants;
use crate::daemon;
use crate::json;
use crate::output;

/// What one row says about the state it reports.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Structural, with no setting and no flag that changes it.
    Guaranteed,
    /// True on this machine, and the safe answer.
    Safe,
    /// True on this machine, and something the reader should know about. Never a
    /// failure — every one of these is a choice someone made deliberately.
    Widened,
    /// Neither safe nor widened: a fact worth printing, like where the config lives.
    Neutral,
}

impl Verdict {
    /// The glyph in front of the row.
    fn mark(self) -> &'static str {
        match self {
            Verdict::Guaranteed | Verdict::Safe => "+",
            Verdict::Widened => "!",
            Verdict::Neutral => " ",
        }
    }

    /// The word `--json` uses. Separate from the prose so rewording a row never breaks
    /// a script reading the document.
    fn key(self) -> &'static str {
        match self {
            Verdict::Guaranteed => "guaranteed",
            Verdict::Safe => "safe",
            Verdict::Widened => "widened",
            Verdict::Neutral => "neutral",
        }
    }
}

/// One line of the report.
pub struct TrustRow {
    /// Stable identifier for `--json`.
    pub key: &'static str,
    /// What is being reported, for a human.
    pub subject: &'static str,
    /// The state it is in, in the user's terms.
    pub state: String,
    /// How to read that state.
    pub verdict: Verdict,
}

impl TrustRow {
    fn new(
        key: &'static str,
        subject: &'static str,
        state: impl Into<String>,
        verdict: Verdict,
    ) -> Self {
        Self {
            key,
            subject,
            state: state.into(),
            verdict,
        }
    }

    /// The `--json` word for this row's verdict.
    pub fn verdict_key(&self) -> &'static str {
        self.verdict.key()
    }
}

/// One executable dev-prune owns on this machine, and the digest a scanner sees.
///
/// A separate type from [`TrustRow`] because the hash has to survive into `--json` as a
/// field of its own. Somebody comparing their copy against a published `.sha256` should
/// not have to parse it back out of a sentence.
pub struct BinaryIdentity {
    /// Stable identifier for `--json`: what this executable is *for*, not what it is
    /// named, so a reader does not have to know that `devp` is the short name.
    pub role: &'static str,
    /// The file name, for a human.
    pub name: String,
    /// Where it is.
    pub path: String,
    /// Lower-case hex SHA-256, or `None` when the file could not be read.
    pub sha256: Option<String>,
    /// Whether this is the executable answering right now.
    pub running: bool,
    /// Why this one's digest differs from the others, when it does.
    pub note: Option<&'static str>,
}

impl BinaryIdentity {
    /// The scan report for this file's digest, if it could be hashed.
    pub fn scan_url(&self) -> Option<String> {
        self.sha256
            .as_ref()
            .map(|h| format!("{}/{h}", constants::VIRUSTOTAL_FILE_BASE))
    }
}

/// The whole report.
pub struct TrustReport {
    /// Structural guarantees. Identical on every machine.
    pub guarantees: Vec<TrustRow>,
    /// Live state, read from the registry and the OS.
    pub machine: Vec<TrustRow>,
    /// Every executable dev-prune owns here, the one that is running first.
    ///
    /// Filled by [`run`] and not by [`build`]: hashing three executables costs more than
    /// every other row in this report put together, and the first-run configurator opens
    /// on `build` while the user is waiting at a blank terminal.
    pub binaries: Vec<BinaryIdentity>,
}

impl TrustReport {
    /// Every setting on this machine that widens what dev-prune may do without asking.
    ///
    /// Not a score. A list, because "trust level: MEDIUM" tells nobody which switch to
    /// look at, and the only useful version of this answer is the names.
    pub fn widened(&self) -> Vec<&str> {
        self.machine
            .iter()
            .filter(|r| r.verdict == Verdict::Widened)
            .map(|r| r.subject)
            .collect()
    }
}

/// Run the `trust` command.
pub fn run(json_output: bool) -> Result<()> {
    let registry = Registry::load()?;
    let mut report = build(&registry);
    report.binaries = binaries();

    if json_output {
        return json::emit(&json::trust_document(&report));
    }

    print_report(&report);
    Ok(())
}

/// Add every registered repository Git refuses to read to its global `safe.directory`.
///
/// Git will not read a working tree whose owner on disk is not the account running it,
/// and on Windows that state is routine and permanent: a reinstall, a restored backup or
/// a drive carried between machines leaves the old account's identifier on every
/// directory. `devp run` cannot date such a repository, and a repository whose age is
/// unknown is one nothing is ever deleted from — so on an affected machine a large part
/// of the registry silently does nothing until this is resolved.
///
/// Git's own suggestion is one `git config` invocation per repository, printed inside a
/// twelve-line message, once per repository. This is that suggestion, applied to the
/// repositories dev-prune already knows about, after showing which ones and asking.
///
/// It belongs to `trust` rather than to `run` because it widens what Git will open for
/// every tool on the machine, not just this one. That is exactly the kind of change this
/// command exists to make visible.
pub fn fix_ownership(assume_yes: bool) -> Result<()> {
    let registry = Registry::load()?;
    let affected = repositories_git_refuses(&registry);

    if affected.is_empty() {
        output::print_success("Git reads every registered repository. Nothing to fix.");
        return Ok(());
    }

    let n = affected.len();
    output::print_header(&format!(
        "{n} {} Git will not read",
        output::plural(n, "repository", "repositories")
    ));
    for path in &affected {
        println!("    {}", output::styled_path(path));
    }
    println!();
    output::print_info(&format!(
        "This adds {} to git's global `safe.directory` list, which tells Git to open {} despite \
         the owner recorded on disk. It affects every tool on this machine that uses Git, not only \
         dev-prune.",
        output::plural(n, "this path", "these paths"),
        output::plural(n, "it", "them")
    ));
    output::print_info("Undo one with:  git config --global --unset-all safe.directory <path>");

    if !confirm_fix(assume_yes) {
        return Ok(());
    }

    // Read the existing list once rather than per repository: `--add` does not
    // deduplicate, and a machine where this was run twice would accumulate a second copy
    // of every entry in the user's global config forever.
    let existing = configured_safe_directories();
    let mut added = 0usize;
    for path in &affected {
        let value = git_path_value(path);
        if existing.iter().any(|e| e == &value) {
            continue;
        }
        let status = crate::spawn::command("git")
            .args(["config", "--global", "--add", "safe.directory", &value])
            .status();
        match status {
            Ok(s) if s.success() => added += 1,
            _ => output::print_warning(&format!("Could not add `{value}` — skipped.")),
        }
    }

    output::print_success(&format!(
        "Added {added} {}. Run `devp run --dry-run` to see what is now examinable.",
        output::plural(added, "entry", "entries")
    ));
    Ok(())
}

/// Every registered repository whose path exists but which Git refuses on ownership.
///
/// Asks Git directly rather than reusing a prune pass: the question is one `rev-parse`
/// per repository, and a prune pass would also stat every dependency directory on the
/// machine to answer it.
fn repositories_git_refuses(registry: &Registry) -> Vec<std::path::PathBuf> {
    let mut affected: Vec<std::path::PathBuf> = registry
        .repositories
        .keys()
        .filter(|path| path.exists())
        .filter(|path| {
            let output = crate::scanner::git::git_in(path)
                .args(["rev-parse", "--git-dir"])
                .output();
            match output {
                Ok(out) if !out.status.success() => String::from_utf8_lossy(&out.stderr)
                    .to_lowercase()
                    .contains(constants::GIT_DUBIOUS_OWNERSHIP),
                _ => false,
            }
        })
        .cloned()
        .collect();
    // The list is shown to a person and then written to their config; a HashMap's order
    // would put it in a different order every run.
    affected.sort();
    affected
}

/// The values already in the global `safe.directory` list.
fn configured_safe_directories() -> Vec<String> {
    let output = crate::spawn::command("git")
        .args(["config", "--global", "--get-all", "safe.directory"])
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        // An empty list and an unreadable config lead to the same behaviour — write the
        // entry — and `--add` on a duplicate is untidy rather than harmful.
        _ => Vec::new(),
    }
}

/// A path in the spelling Git uses for `safe.directory`.
///
/// Forward slashes even on Windows: that is the form Git prints in its own refusal
/// message and the form it compares against, and a backslash spelling is accepted by
/// `git config` while never matching anything.
fn git_path_value(path: &std::path::Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Ask before writing to the user's global Git configuration.
///
/// Default no, and a non-terminal gets a no with the flag to pass next time: this widens
/// what Git will open for every tool on the machine, which is not something a piped
/// invocation should be able to do by accident.
fn confirm_fix(yes: bool) -> bool {
    use std::io::{IsTerminal, Write};
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        output::print_info("Not running in a terminal — pass `--yes` to write these.");
        return false;
    }
    eprint!("Add them to git's safe.directory list? [y/N]: ");
    if std::io::stderr().flush().is_err() {
        return false;
    }
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Ask the OS its two questions at the same time.
///
/// These are the only two rows that shell out — `schtasks` and `git config` — and
/// together they were most of the second this report took to build. That second was
/// spent before the configurator drew anything at all, which read as a slow tool rather
/// than as two processes being waited on one after the other.
fn machine_answers() -> (String, String) {
    let scheduler = std::thread::spawn(scheduler_state);
    let hooks = hook_state();
    // A panic in the probe must not take the report down with it: the row's whole
    // purpose is to say what is unknown.
    let scheduler = scheduler
        .join()
        .unwrap_or_else(|_| "Unknown (the check did not finish)".to_string());
    (scheduler, hooks)
}

/// Say what is happening while [`machine_answers`] blocks, and erase it afterwards.
///
/// Asking Windows for a scheduled task costs a second on its own, and it happens before
/// the first screen of the configurator can be drawn — so without this the wizard opens
/// on an empty terminal for long enough to look hung. Stderr, so a piped `--json` run is
/// unaffected, and only when stderr is a terminal, so a log file never collects it.
fn with_progress<T>(work: impl FnOnce() -> T) -> T {
    use std::io::{IsTerminal, Write};

    let mut err = std::io::stderr();
    let show = err.is_terminal();
    if show {
        let _ = write!(err, "{}", constants::READING_MACHINE);
        let _ = err.flush();
    }
    let value = work();
    if show {
        // Carriage return and overwrite rather than an erase sequence: this runs before
        // the alternate screen is entered, on terminals that predate it.
        let _ = write!(
            err,
            "\r{:width$}\r",
            "",
            width = constants::READING_MACHINE.chars().count()
        );
        let _ = err.flush();
    }
    value
}

/// Assemble the report from the code's own guarantees and this machine's actual state.
///
/// `pub(crate)` because the first-run configurator opens on this same report: the
/// declaration a new user reads and what `devp trust` prints later must be the same
/// text, or one of them is a marketing claim.
pub(crate) fn build(registry: &Registry) -> TrustReport {
    TrustReport {
        guarantees: guarantees(),
        machine: machine_state(registry),
        binaries: Vec::new(),
    }
}

/// The seven safety invariants plus the three promises that are not invariants but are
/// asked about just as often: no telemetry, build outputs are never touched, and neither
/// is container disk.
///
/// Every string here restates something enforced in `src/engine.rs` or `src/adapters/`.
/// [`docs/SAFETY_INVARIANTS.md`](../../docs/SAFETY_INVARIANTS.md) is the long form.
fn guarantees() -> Vec<TrustRow> {
    use Verdict::Guaranteed as G;
    vec![
        TrustRow::new(
            "filesystem_scope",
            "Filesystem scope",
            "Registered Git repositories only",
            G,
        ),
        TrustRow::new(
            "lockfile_verification",
            "Lockfile verification",
            "Required before every delete",
            G,
        ),
        TrustRow::new("symlinks", "Symlinks and junctions", "Refused", G),
        TrustRow::new(
            "nested_repositories",
            "Nested repositories",
            "Refused — no lockfile rebuilds someone else's history",
            G,
        ),
        TrustRow::new(
            "build_outputs",
            "Build outputs",
            "Never deleted — no dist/, no .next/, no .gitignore rules",
            G,
        ),
        TrustRow::new(
            "container_disk",
            "Container disk",
            "Reported, never deleted — `devp caches docker` prints the commands",
            G,
        ),
        TrustRow::new(
            "deletion_bypass",
            "Deletion bypass",
            "None — no flag disables a safety check",
            G,
        ),
        TrustRow::new(
            "state_writes",
            "State writes",
            "Atomic — temp file, then rename",
            G,
        ),
        TrustRow::new("telemetry", "Telemetry", "None — there is no endpoint", G),
        TrustRow::new(
            "restore",
            "Restore",
            "`devp restore --last-run` rebuilds the last pass",
            G,
        ),
    ]
}

/// Everything read back from this machine rather than asserted.
fn machine_state(registry: &Registry) -> Vec<TrustRow> {
    let s = &registry.settings;
    let (scheduler, hooks) = with_progress(machine_answers);
    let mut rows = vec![
        TrustRow::new(
            "network",
            "Network requests",
            if s.update_check {
                format!(
                    "Release check against GitHub, every {} days",
                    s.update_check_interval_days
                )
            } else {
                "None — the release check is off".to_string()
            },
            Verdict::Safe,
        ),
        TrustRow::new(
            "auto_update",
            "Auto-update",
            // The pin answers this row's question outright, so it answers it here rather
            // than leaving the screen saying a pass will install a release it will not.
            if s.version_lock {
                "Off — `version_lock` pins this copy to the version it is"
            } else if s.auto_update {
                "On (the default) — a newer release installs itself after a pass"
            } else {
                "Off — updates only when you run `devp update --install`"
            },
            // Neutral, not widened, since 1.7.0: `Widened` means someone deliberately
            // switched something on beyond the defaults, and this is now a default. Still
            // its own row, because "replaces its own binary" is a fact anyone reading
            // this screen came here to learn.
            if s.auto_update && !s.version_lock {
                Verdict::Neutral
            } else {
                Verdict::Safe
            },
        ),
        TrustRow::new(
            "confirmation",
            "Confirmation before deleting",
            if s.require_confirmation {
                "Required, except where you pass `--yes`"
            } else {
                "Off — `require_confirmation` is false"
            },
            if s.require_confirmation {
                Verdict::Safe
            } else {
                Verdict::Widened
            },
        ),
        TrustRow::new(
            "lockfile_rewrite",
            "Lockfile rewriting",
            if s.allow_manifest_rewrite {
                "Allowed — a stale lockfile is regenerated instead of refused"
            } else {
                "Refused — verification is read-only"
            },
            if s.allow_manifest_rewrite {
                Verdict::Widened
            } else {
                Verdict::Safe
            },
        ),
        TrustRow::new(
            "scheduler",
            "Background scheduler",
            scheduler,
            Verdict::Neutral,
        ),
        TrustRow::new("git_hooks", "Git hooks", hooks, Verdict::Neutral),
    ];

    // Opt-in adapters delete compiled output, which every other adapter refuses to do.
    // Someone reading this report to find out what is deletable on their machine needs
    // to see that they turned that on.
    let opt_in = opt_in_adapters(registry);
    rows.push(TrustRow::new(
        "opt_in_adapters",
        "Opt-in adapters",
        if opt_in.is_empty() {
            "None — only dependency directories are deletable".to_string()
        } else {
            format!("{} — build trees are deletable too", opt_in.join(", "))
        },
        if opt_in.is_empty() {
            Verdict::Safe
        } else {
            Verdict::Widened
        },
    ));

    rows.push(TrustRow::new(
        "repositories",
        "Registered repositories",
        format!(
            "{} — nothing outside them is ever read or written",
            registry.repositories.len()
        ),
        Verdict::Neutral,
    ));
    rows.push(TrustRow::new(
        "idle_window",
        "Idle window",
        format!(
            "{} days of no commits and no file changes ({} for build trees, before any per-adapter window)",
            s.idle_days,
            s.build_idle_days.max(s.idle_days)
        ),
        Verdict::Neutral,
    ));
    // The managed path, not `current_exe()`: on Windows the scheduler runs `devpw.exe`
    // from the same directory and `devp update` replaces the managed copy, so the path
    // that matters to someone asking what runs on this machine is the one dev-prune
    // owns. The binaries section below hashes them all, this one is running or not.
    rows.push(TrustRow::new(
        "binary",
        "Managed binary",
        output::clean_path(daemon::get_exe_path()),
        Verdict::Neutral,
    ));

    rows
}

/// Every executable dev-prune owns on this machine, hashed, the running one first.
///
/// This exists because of what an antivirus actually looks at. A scanner judges the file
/// on the disk in front of it, so a checksum published on a release page answers nothing
/// on its own — the question is whether *this* copy has that digest. Printing the hash
/// beside the path lets anyone compare the two themselves, and turns "is the thing I
/// installed the thing you published?" from a matter of trust into one line of `diff`.
///
/// Deliberately not [`daemon::get_exe_path`]: that repairs a stale managed copy as a
/// side effect of being asked where it is, and a read-only report must not write to the
/// machine it is describing.
pub(crate) fn binaries() -> Vec<BinaryIdentity> {
    let mut found: Vec<(std::path::PathBuf, &'static str, Option<&'static str>)> = Vec::new();

    // The managed set, derived from the one path that is a constant rather than a
    // reading. `devp` is a hard link or a copy of `dev-prune` and `devpw.exe` is a
    // separate build target, which is exactly the distinction the digests will show.
    if let Ok(managed) = crate::setup::managed_exe_path() {
        let dir = managed.parent().map(|d| d.to_path_buf());
        found.push((managed, "managed", None));
        if let Some(dir) = dir {
            let alias = if cfg!(windows) { "devp.exe" } else { "devp" };
            // Note deliberately left empty here and decided from the digests below. It
            // used to assert the identity unconditionally, which made this report state
            // the one thing it is here to check rather than check it.
            found.push((dir.join(alias), "alias", None));
            if cfg!(windows) {
                found.push((
                    dir.join(constants::WINDOWS_WINDOWLESS_BIN),
                    "windowless",
                    Some(
                        "A different digest by design — the same code linked for \
                         the GUI subsystem, so the scheduler flashes no console window.",
                    ),
                ));
            }
        }
    }

    // Whatever is answering now, which on a `cargo install` machine or inside `npx` is
    // none of the above. Pushed last and promoted first, so it is never listed twice.
    let running = std::env::current_exe().ok();
    if let Some(current) = running.clone() {
        found.push((current, "other", None));
    }

    let same = |a: &std::path::Path, b: &std::path::Path| -> bool {
        // Canonicalised, because on Unix `devp` is a symlink to `dev-prune`: two names
        // for one inode are one binary, and listing two digests for it would invite
        // someone to go looking for a difference that cannot exist.
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => a == b,
        }
    };

    let mut rows: Vec<BinaryIdentity> = Vec::new();
    for (path, role, note) in found {
        if !path.is_file()
            || rows
                .iter()
                .any(|r| same(std::path::Path::new(&r.path), &path))
        {
            continue;
        }
        let is_running = running.as_deref().is_some_and(|c| same(c, &path));
        rows.push(BinaryIdentity {
            role,
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: output::clean_path(&path),
            sha256: sha256_of(&path),
            running: is_running,
            note,
        });
    }

    annotate_alias(&mut rows);

    // The user asked which one they are running; that answer goes at the top, and the
    // rest are context for it.
    rows.sort_by_key(|r| !r.running);
    rows
}

/// Explain the alias row from the two digests, rather than from what ought to be true.
///
/// `devp` is the same bytes as `dev-prune` right up until an upgrade replaces one of them
/// and not the other — which is the normal state between a `cargo install` and the next
/// setup pass, since that pass only runs where somebody can see it report. Two different
/// digests under a caption saying they are the same is the worst outcome available here:
/// the mismatch a reader came to this report to find, explained away in the one sentence
/// they will read instead of comparing the hashes themselves.
///
/// Says nothing when either file could not be hashed. "Same" and "stale" are both claims,
/// and a missing digest supports neither.
fn annotate_alias(rows: &mut [BinaryIdentity]) {
    let managed = rows
        .iter()
        .find(|r| r.role == "managed")
        .and_then(|r| r.sha256.clone());
    let Some(alias) = rows.iter_mut().find(|r| r.role == "alias") else {
        return;
    };
    alias.note = match (&managed, &alias.sha256) {
        (Some(managed), Some(mine)) if managed == mine => Some(
            "The same bytes as dev-prune under a second name, so a scanner \
             builds one reputation record instead of two.",
        ),
        (Some(_), Some(_)) => Some(
            "An earlier release under a second name: an upgrade replaced dev-prune \
             and this has not caught up. The next dev-prune command you run in a \
             terminal restores the pair.",
        ),
        _ => None,
    };
}

/// Lower-case hex SHA-256 of a file, or `None` if it cannot be read.
///
/// Read whole rather than streamed: these are single-digit-megabyte executables, and the
/// alternative is a `Write` bound on the digest type that `sha2` may or may not offer in
/// the next release.
fn sha256_of(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let bytes = std::fs::read(path).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    // Hex-encoded by hand: `sha2` 0.11 returns a `hybrid_array::Array`, which has no
    // `LowerHex`, and every `.sha256` sidecar we publish is lower-case hex.
    Some(h.finalize().iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    }))
}

/// Which opt-in adapters are switched on, in the order the report should name them.
fn opt_in_adapters(registry: &Registry) -> Vec<&'static str> {
    let s = &registry.settings;
    [
        ("cargo", s.enable_cargo),
        ("gradle", s.enable_gradle),
        ("maven", s.enable_maven),
        ("swift", s.enable_swift),
        ("dart", s.enable_dart),
        ("mix_build", s.enable_mix_build),
        ("vcpkg", s.enable_vcpkg),
        ("cmake_build", s.enable_cmake_build),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect()
}

/// Whether anything prunes on its own on this machine.
fn scheduler_state() -> String {
    match daemon::daemon_status() {
        Ok(daemon::DaemonStatus::Installed) => "Installed — prunes on its own".to_string(),
        Ok(daemon::DaemonStatus::NotInstalled) => {
            "Not installed — nothing runs unless you run it".to_string()
        }
        Ok(daemon::DaemonStatus::Unknown(why)) => format!("Unknown ({why})"),
        Err(e) => format!("Unknown ({e})"),
    }
}

/// Whether Git hooks auto-register repositories on this machine.
fn hook_state() -> String {
    if !hook::git_available() {
        return "Not installed — git is not on PATH".to_string();
    }
    match hook::state() {
        Ok(HookState::Active) => "Installed — new repositories register themselves".to_string(),
        Ok(HookState::Absent) => {
            "Not installed — repositories register only when you say so".to_string()
        }
        Ok(HookState::Chained { previous, .. }) => {
            format!("Installed, chained to `{previous}`")
        }
        Ok(HookState::Foreign(p)) => format!("Not ours — `core.hooksPath` belongs to `{p}`"),
        Err(e) => format!("Unknown ({e})"),
    }
}

fn print_report(report: &TrustReport) {
    output::print_header(&format!("What dev-prune {} may do", constants::VERSION));

    println!();
    println!("  Guaranteed by the code, on every machine");
    println!();
    for row in &report.guarantees {
        print_row(row);
    }

    println!();
    println!("  On this machine");
    println!();
    for row in &report.machine {
        print_row(row);
    }

    println!();
    let widened = report.widened();
    if widened.is_empty() {
        output::print_success(
            "Nothing on this machine widens what dev-prune may do without asking.",
        );
    } else {
        output::print_info(&format!(
            "{} {} what dev-prune may do without asking: {}. Each was switched on \
             deliberately; `devp config show` has them.",
            widened.len(),
            if widened.len() == 1 {
                "setting widens"
            } else {
                "settings widen"
            },
            widened.join(", ")
        ));
    }
    output::print_info(
        "The guarantees above are enforced in `src/engine.rs` and described in full at \
         docs/SAFETY_INVARIANTS.md. None of them has a bypass flag.",
    );

    print_binaries(&report.binaries);
}

/// The bottom section: what is actually on this disk, and the digest a scanner reads.
fn print_binaries(binaries: &[BinaryIdentity]) {
    if binaries.is_empty() {
        return;
    }

    println!();
    println!("  Binaries on this machine");
    println!();
    for b in binaries {
        let label = if b.running {
            format!("{} (running)", b.name)
        } else {
            b.name.clone()
        };
        let mark = if b.running { ">" } else { " " };
        println!("  {mark}  {label:<30} {}", b.path);
        match (&b.sha256, b.scan_url()) {
            (Some(hash), Some(url)) => {
                println!("     {:<30} {hash}", "SHA-256");
                println!("     {:<30} {url}", "Scan report");
            }
            // A file that is present and unreadable is worth saying out loud. Dropping
            // the row silently would read as "this one does not have a digest".
            _ => println!("     {:<30} could not be read", "SHA-256"),
        }
        if let Some(note) = b.note {
            println!("     {note}");
        }
        println!();
    }

    output::print_info(
        "Those links are lookups by digest, not uploads — dev-prune sends no file \
         anywhere. A digest the service has never seen comes back `not found`, which \
         means unscanned rather than clean.",
    );
    output::print_info(
        "A copy installed from a release has the same SHA-256 as the asset published \
         beside it, so the two can be compared by hand. One built by `cargo install` was \
         compiled here and matches nothing published.",
    );
}

fn print_row(row: &TrustRow) {
    println!(
        "  {}  {:<30} {}",
        row.verdict.mark(),
        row.subject,
        row.state
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows, `managed` and `alias`, with the digests a test wants to compare.
    fn pair(managed: Option<&str>, alias: Option<&str>) -> Vec<BinaryIdentity> {
        let row = |role: &'static str, sha: Option<&str>| BinaryIdentity {
            role,
            name: String::new(),
            path: String::new(),
            sha256: sha.map(str::to_string),
            running: false,
            note: None,
        };
        vec![row("managed", managed), row("alias", alias)]
    }

    fn alias_note(rows: &[BinaryIdentity]) -> Option<&'static str> {
        rows.iter().find(|r| r.role == "alias").unwrap().note
    }

    #[test]
    fn the_alias_note_follows_the_digests_and_not_the_expectation() {
        // The whole point of printing two hashes is that somebody can compare them. A
        // caption asserting they match, printed above two that do not, is worse than no
        // caption at all.
        let mut same = pair(Some("aa"), Some("aa"));
        annotate_alias(&mut same);
        assert!(alias_note(&same).is_some_and(|n| n.contains("The same bytes")));

        let mut stale = pair(Some("aa"), Some("bb"));
        annotate_alias(&mut stale);
        assert!(alias_note(&stale).is_some_and(|n| n.contains("An earlier release")));

        // Neither claim is supportable without both digests.
        let mut unreadable = pair(Some("aa"), None);
        annotate_alias(&mut unreadable);
        assert!(alias_note(&unreadable).is_none());
    }

    #[test]
    fn safe_directory_values_use_the_spelling_git_compares_against() {
        // A Windows-spelt value is accepted by `git config` and then never matches
        // anything, because Git normalises the directory it is checking to forward
        // slashes before comparing. A repair that silently does nothing is the worst
        // outcome available here.
        let path = std::path::Path::new("V:\\Code\\Project");
        assert_eq!(git_path_value(path), "V:/Code/Project");
    }

    #[test]
    fn the_default_machine_widens_nothing() {
        let registry = Registry::default();
        let report = build(&registry);
        assert!(
            report.widened().is_empty(),
            "a fresh install reports {:?} as widened",
            report.widened()
        );
    }

    #[test]
    fn every_widening_setting_shows_up_by_name() {
        let mut registry = Registry::default();
        registry.settings.require_confirmation = false;
        registry.settings.allow_manifest_rewrite = true;
        registry.settings.enable_gradle = true;

        let report = build(&registry);
        let widened = report.widened();
        assert_eq!(widened.len(), 3, "got {widened:?}");
        // Named, not counted: a report that says "3 settings" and stops is a report
        // nobody can act on.
        assert!(widened.contains(&"Opt-in adapters"));
    }

    #[test]
    fn opt_in_adapters_are_listed_in_a_stable_order() {
        let mut registry = Registry::default();
        registry.settings.enable_swift = true;
        registry.settings.enable_gradle = true;
        assert_eq!(opt_in_adapters(&registry), vec!["gradle", "swift"]);
    }

    #[test]
    fn build_never_hashes_anything() {
        // The first-run configurator opens on `build`, and hashing every managed
        // executable there would put a visible pause in front of the first screen. The
        // command fills this in afterwards; `build` must leave it empty.
        assert!(build(&Registry::default()).binaries.is_empty());
    }

    #[test]
    fn the_running_binary_is_listed_first_and_hashed() {
        let found = binaries();
        let running: Vec<&BinaryIdentity> = found.iter().filter(|b| b.running).collect();
        assert_eq!(running.len(), 1, "expected exactly one running binary");
        assert!(found[0].running, "the running binary must lead the list");

        let hash = found[0]
            .sha256
            .as_deref()
            .expect("the running file is readable");
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_eq!(
            found[0].scan_url().unwrap(),
            format!("{}/{hash}", constants::VIRUSTOTAL_FILE_BASE)
        );
    }

    #[test]
    fn one_file_is_never_listed_twice() {
        // On Unix `devp` is a symlink to `dev-prune`, and on Windows the two are byte
        // identical on purpose. Either way they are one binary, and printing two rows
        // for it would send someone looking for a difference that cannot exist.
        let found = binaries();
        let mut paths: Vec<&str> = found.iter().map(|b| b.path.as_str()).collect();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(
            paths.len(),
            total,
            "duplicate path in {found:?}",
            found = paths
        );
    }

    #[test]
    fn every_row_key_is_unique() {
        // The keys are the `--json` contract; two rows sharing one silently drops a
        // fact from the document.
        let report = build(&Registry::default());
        let mut keys: Vec<&str> = report
            .guarantees
            .iter()
            .chain(report.machine.iter())
            .map(|r| r.key)
            .collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total);
    }

    #[test]
    fn guarantees_never_depend_on_settings() {
        // If a "guarantee" could be turned off it would not be one, and the report would
        // be claiming something the code does not enforce.
        let mut registry = Registry::default();
        registry.settings.allow_manifest_rewrite = true;
        registry.settings.auto_update = true;
        let with = build(&registry);
        let without = build(&Registry::default());

        let states = |r: &TrustReport| -> Vec<String> {
            r.guarantees.iter().map(|g| g.state.clone()).collect()
        };
        assert_eq!(states(&with), states(&without));
    }
}
