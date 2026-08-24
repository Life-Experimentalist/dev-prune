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

/// The whole report.
pub struct TrustReport {
    /// Structural guarantees. Identical on every machine.
    pub guarantees: Vec<TrustRow>,
    /// Live state, read from the registry and the OS.
    pub machine: Vec<TrustRow>,
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
    let report = build(&registry);

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
    // The managed path, not `current_exe()`: on Windows the scheduler runs a patched
    // twin and `devp update` replaces the managed copy, so the path that matters to
    // someone asking what runs on this machine is the one dev-prune owns.
    rows.push(TrustRow::new(
        "binary",
        "Managed binary",
        output::clean_path(daemon::get_exe_path()),
        Verdict::Neutral,
    ));

    rows
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
