// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// `devp installs clean`: leave one devp on the machine.
//
// Every removal here is one the user confirmed, shown with the exact command first, and
// carried out by whatever installed the copy (`cargo uninstall`, `brew uninstall`, ...).
// A package manager keeps records that deleting the file would falsify, so no copy a
// manager owns is ever deleted by hand. The one exception is the install script's own
// copy, which keeps no record beyond the file and goes through the removal code
// `devp uninstall` already uses. A copy nothing claims, or one claimed by a manager devp
// cannot drive, is described and left for the user.
//
// The work is split so the tests can run all of it without touching the machine:
// `choose_keep` and `plan` are pure, and `execute` talks to the world only through
// [`Host`], which the tests replace with a recorder.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::channel::Channel;
use crate::installs::{self, Discovery, Install};
use crate::{UsageError, constants, output, setup};

use super::installs::version_text;

/// What will be done about one copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// Run the package manager's own uninstall command.
    Manager(Vec<String>),
    /// The install script's copy, removed by `devp uninstall`'s own code.
    InstallerCopy,
    /// Nothing is run or deleted: this says what the copy is and how to remove it.
    ByHand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Index into [`Discovery::installs`].
    pub install: usize,
    pub kind: StepKind,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub keep: usize,
    pub steps: Vec<Step>,
    /// Things worth saying that are not a step, such as why the running copy is absent.
    pub notes: Vec<String>,
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Removed,
    /// The user said no.
    Declined,
    /// Confirmed, and it did not work. This is what makes the exit code 1.
    Failed(String),
    /// Described for the user to do; devp did nothing.
    ByHand,
    /// `--dry-run`: shown, not run.
    Planned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repoint {
    Scheduler,
    Hooks,
}

impl Repoint {
    fn name(&self) -> &'static str {
        match self {
            Repoint::Scheduler => "the scheduler",
            Repoint::Hooks => "the git hooks",
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub results: Vec<(usize, Outcome)>,
    /// What was re-registered against the kept copy.
    pub repointed: Vec<&'static str>,
    /// The command to run when devp could not re-point things itself.
    pub hint: Option<String>,
    /// Confirmed steps that did not complete, re-pointing included.
    pub failures: usize,
}

/// Everything `execute` does to the machine, so tests can watch instead.
pub trait Host {
    /// Ask a yes/no question; `false` is the default answer.
    fn confirm(&mut self, question: &str) -> bool;
    /// Run a package manager's command. `Err` is a sentence for the user.
    fn run_manager(&mut self, argv: &[String]) -> std::result::Result<(), String>;
    /// Remove the install script's copy.
    fn remove_installer_copy(&mut self) -> std::result::Result<(), String>;
    /// Whether the file is still there after a removal.
    fn still_exists(&self, path: &Path) -> bool;
    /// Re-register the scheduler or the hook shims against the running binary.
    fn repoint(&mut self, what: &Repoint) -> std::result::Result<(), String>;
}

/// Pick the copy to keep.
///
/// `--keep` must name a copy `devp installs` lists; a path nobody found is a usage error
/// rather than a silent fallback, because the answer decides what gets removed. By
/// default the highest version wins, since the newest build is the one with the fixes;
/// ties go to the copy the scheduler runs (the one already doing background work), then
/// to the one first on PATH (what the user types), then to discovery order.
pub fn choose_keep(installs: &[Install], keep: Option<&str>) -> Result<usize> {
    let candidates: Vec<usize> = installs
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.windowless_only)
        .map(|(idx, _)| idx)
        .collect();
    if candidates.is_empty() {
        anyhow::bail!("No devp binary was found, so there is nothing to keep.");
    }
    if let Some(text) = keep {
        let wanted = std::path::PathBuf::from(crate::config::expand_tilde(text));
        return installs::owner_of_exact(installs, &wanted)
            .filter(|idx| candidates.contains(idx))
            .ok_or_else(|| {
                anyhow::Error::new(UsageError(format!(
                    "`--keep {text}` is not one of the copies `devp installs` lists. Run \
                     `devp installs` and pass one of those paths."
                )))
            });
    }
    Ok(candidates
        .into_iter()
        .max_by_key(|&idx| {
            let i = &installs[idx];
            (
                i.version,
                i.roles.scheduler,
                // Earlier on PATH is better, and off PATH is worst.
                std::cmp::Reverse(i.path_position.unwrap_or(usize::MAX)),
                std::cmp::Reverse(idx),
            )
        })
        .expect("candidates is not empty"))
}

/// Decide what to do about every copy except the kept one.
pub fn plan(installs: &[Install], keep: usize) -> Plan {
    let running = installs.iter().position(|i| i.roles.running);
    let mut steps: Vec<Step> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    for (idx, install) in installs.iter().enumerate() {
        // The windowless launcher goes with the copy it belongs to, never on its own.
        if idx == keep || install.windowless_only {
            continue;
        }
        if install.roles.running {
            notes.push(format!(
                "{} is running this command, so it is not removed here. To remove it, run \
                 `devp installs clean` from the copy you keep.",
                output::clean_path(&install.path)
            ));
            continue;
        }
        let kind = match install.channel {
            Channel::Installer => {
                // The managed pair is removed as a pair, so a protected copy in the same
                // folder (a stale twin of the kept one, say) would go with it.
                let shares_folder = [Some(keep), running]
                    .into_iter()
                    .flatten()
                    .any(|p| same_folder(&installs[p], install));
                if shares_folder {
                    StepKind::ByHand(
                        "It sits in the same folder as a copy that stays, so devp does not \
                         remove it. Delete the file yourself once that copy is moved."
                            .to_string(),
                    )
                } else if steps.iter().any(|s| s.kind == StepKind::InstallerCopy) {
                    // One removal covers every file the install script wrote.
                    continue;
                } else {
                    StepKind::InstallerCopy
                }
            }
            Channel::Unknown => StepKind::ByHand(
                "No package manager owns this file, so devp will not delete it. If you are \
                 sure it is not needed, delete it yourself."
                    .to_string(),
            ),
            Channel::Foreign(name) => StepKind::ByHand(format!(
                "It was installed through {name}, which devp cannot drive. Remove it with \
                 that tool."
            )),
            channel => match channel.uninstall_argv() {
                Some(argv) if channel != installs[keep].channel => StepKind::Manager(argv),
                Some(argv) => StepKind::ByHand(format!(
                    "The copy you keep was installed through {} as well, and `{}` could not \
                     be aimed at only one of them. Remove this one by hand if you want it \
                     gone.",
                    channel.label(),
                    argv.join(" ")
                )),
                None => StepKind::ByHand("Remove it with the tool that installed it.".into()),
            },
        };
        steps.push(Step { install: idx, kind });
    }
    Plan { keep, steps, notes }
}

fn same_folder(a: &Install, b: &Install) -> bool {
    match (a.path.parent(), b.path.parent()) {
        (Some(x), Some(y)) => installs::same_dir(x, y),
        _ => false,
    }
}

/// Walk the plan: show each step, ask, run, and verify.
///
/// A failed or missing manager never stops the walk; the rest still get their turn.
/// Nothing is asked or run in a dry run.
pub fn execute(found: &Discovery, plan: &Plan, dry_run: bool, host: &mut dyn Host) -> Report {
    let installs = &found.installs;
    let kept = &installs[plan.keep];
    let kept_is_running = kept.roles.running;
    let mut report = Report::default();
    let total = plan.steps.len();
    let mut repoint_scheduler = false;
    let mut repoint_hooks = false;

    for (n, step) in plan.steps.iter().enumerate() {
        let install = &installs[step.install];
        println!();
        output::print_header(&format!("{} of {total}", n + 1));
        println!("  Path:     {}", output::clean_path(&install.path));
        println!("  Version:  {}", version_text(install.version));
        println!("  Via:      {}", install.channel.badge());
        if install.roles.scheduler || install.roles.hook {
            let mut who = Vec::new();
            if install.roles.scheduler {
                who.push("the scheduler");
            }
            if install.roles.hook {
                who.push("the git hooks");
            }
            println!(
                "  Note:     {} call{} this copy; after it is removed they are pointed at \
                 the one you keep.",
                who.join(" and "),
                if who.len() == 1 { "s" } else { "" }
            );
        }

        let outcome = match &step.kind {
            StepKind::ByHand(text) => {
                output::print_wrapped("  ", text);
                Outcome::ByHand
            }
            StepKind::Manager(argv) => {
                println!("  Command:  {}", argv.join(" "));
                if dry_run {
                    Outcome::Planned
                } else if !host.confirm("Run it?") {
                    Outcome::Declined
                } else {
                    match host.run_manager(argv) {
                        Err(why) => {
                            output::print_error(&why);
                            Outcome::Failed(why)
                        }
                        Ok(()) => verify_gone(install, host),
                    }
                }
            }
            StepKind::InstallerCopy => {
                println!(
                    "  Action:   remove the install script's copy and its PATH entry (the \
                     same removal `devp uninstall` does, for these files only)"
                );
                if dry_run {
                    Outcome::Planned
                } else if !host.confirm("Remove it?") {
                    Outcome::Declined
                } else {
                    match host.remove_installer_copy() {
                        Err(why) => {
                            output::print_error(&why);
                            Outcome::Failed(why)
                        }
                        Ok(()) => verify_gone(install, host),
                    }
                }
            }
        };

        match &outcome {
            Outcome::Removed => {
                output::print_success("Removed.");
                repoint_scheduler |= install.roles.scheduler;
                repoint_hooks |= install.roles.hook;
            }
            Outcome::Declined => output::print_info("Left in place."),
            Outcome::Failed(_) => report.failures += 1,
            Outcome::ByHand | Outcome::Planned => {}
        }
        report.results.push((step.install, outcome));
    }

    // The scheduler and the hook shims record a path. One that named a copy that is now
    // gone fires into nothing, silently, every time, so they are moved to the kept copy.
    // Only the running binary can do that: the existing setup code registers whatever is
    // running, not whatever it is told. Run from another copy, the honest answer is the
    // command to run from the kept one.
    let wanted: Vec<Repoint> = [
        (repoint_scheduler, Repoint::Scheduler),
        (repoint_hooks, Repoint::Hooks),
    ]
    .into_iter()
    .filter_map(|(needed, what)| needed.then_some(what))
    .collect();
    let mut could_not = Vec::new();
    for what in wanted {
        if kept_is_running {
            match host.repoint(&what) {
                Ok(()) => report.repointed.push(what.name()),
                Err(why) => {
                    output::print_error(&format!(
                        "Could not point {} at the kept copy: {why}",
                        what.name()
                    ));
                    report.failures += 1;
                    could_not.push(what.name());
                }
            }
        } else {
            could_not.push(what.name());
        }
    }
    if !could_not.is_empty() {
        report.hint = Some(format!(
            "Run `{} setup` to point {} at the copy you kept.",
            quote_if_spaced(&kept.path),
            could_not.join(" and ")
        ));
    }
    report
}

/// A manager that exits 0 while the file is still there did not do what was asked: it
/// answered for a different environment, or the copy was never its own.
fn verify_gone(install: &Install, host: &dyn Host) -> Outcome {
    if host.still_exists(&install.path) || host.still_exists(&install.canonical) {
        Outcome::Failed(format!(
            "the command finished, but {} is still there. It may have removed a different \
             copy; run `devp installs` to see what is left.",
            output::clean_path(&install.path)
        ))
    } else {
        Outcome::Removed
    }
}

fn quote_if_spaced(path: &Path) -> String {
    let text = output::clean_path(path);
    if text.contains(' ') {
        format!("\"{text}\"")
    } else {
        text
    }
}

/// `devp installs clean [--keep PATH] [--dry-run] [--yes]`.
pub fn run(keep: Option<&str>, yes: bool, dry_run: bool) -> Result<()> {
    use std::io::IsTerminal;
    // Checked before discovery so the refusal is instant, and a dry run is exempt: it asks
    // nothing.
    if !yes && !dry_run && !std::io::stdin().is_terminal() {
        return Err(anyhow::Error::new(UsageError(
            "`devp installs clean` asks before each removal and there is no terminal to ask \
             on. Run it in a terminal, add `--dry-run` to see the plan, or pass `--yes` to \
             confirm every step."
                .to_string(),
        )));
    }
    let found = installs::discover_machine();
    let mut host = MachineHost { yes };
    clean(&found, keep, dry_run, &mut host)
}

/// The whole command against any [`Host`].
pub fn clean(
    found: &Discovery,
    keep: Option<&str>,
    dry_run: bool,
    host: &mut dyn Host,
) -> Result<()> {
    let real = found.installs.iter().filter(|i| !i.windowless_only).count();
    let keep_idx = choose_keep(&found.installs, keep)?;
    let kept = &found.installs[keep_idx];

    output::print_header("Cleaning up copies of devp");
    output::print_info(&format!(
        "Keeping version {} at {} (installed with {}).",
        version_text(kept.version),
        output::clean_path(&kept.path),
        kept.channel.badge()
    ));
    if keep.is_none() {
        output::print_dimmed(
            "  Chosen because it has the highest version; ties go to the copy the scheduler \
             runs, then the first on PATH. `--keep <path>` overrides.",
        );
    }

    let plan = plan(&found.installs, keep_idx);
    for note in &plan.notes {
        output::print_info(note);
    }
    if real <= 1 || (plan.steps.is_empty() && plan.notes.is_empty()) {
        output::print_success("Only one copy of devp is installed. Nothing to clean up.");
        return Ok(());
    }
    if plan.steps.is_empty() {
        return Ok(());
    }

    let report = execute(found, &plan, dry_run, host);

    println!();
    if dry_run {
        output::print_info("Dry run: nothing was run or removed.");
        return Ok(());
    }
    let count =
        |wanted: fn(&Outcome) -> bool| report.results.iter().filter(|(_, o)| wanted(o)).count();
    let removed = count(|o| matches!(o, Outcome::Removed));
    let declined = count(|o| matches!(o, Outcome::Declined));
    let by_hand = count(|o| matches!(o, Outcome::ByHand));
    let failed = count(|o| matches!(o, Outcome::Failed(_)));
    output::print_info(&format!(
        "Removed {removed}, left {declined} in place at your request, {by_hand} for you to \
         remove by hand, {failed} failed."
    ));
    for what in &report.repointed {
        output::print_success(&format!("Pointed {what} at the copy you kept."));
    }
    if let Some(hint) = &report.hint {
        output::print_info(hint);
    }
    if report.failures > 0 {
        anyhow::bail!(
            "{} step{} you confirmed did not complete.",
            report.failures,
            if report.failures == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

/// The real machine: real prompts, real package managers.
struct MachineHost {
    yes: bool,
}

impl Host for MachineHost {
    fn confirm(&mut self, question: &str) -> bool {
        use std::io::Write;
        if self.yes {
            return true;
        }
        // Default no, and on stderr so a piped stdout cannot swallow the question.
        eprint!("{question} [y/N]: ");
        if std::io::stderr().flush().is_err() {
            return false;
        }
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            return false;
        }
        matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
    }

    fn run_manager(&mut self, argv: &[String]) -> std::result::Result<(), String> {
        let program = crate::adapters::resolve_program(&argv[0]);
        let mut cmd = crate::spawn::command(&program);
        cmd.args(&argv[1..]);
        let ceiling = Duration::from_secs(constants::MANAGER_UNINSTALL_CEILING_SECS);
        match crate::spawn::output_within(cmd, ceiling) {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(failure_text(&argv[0], &out)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!(
                "`{}` is not installed or not on PATH, so this copy was not removed.",
                argv[0]
            )),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Err(format!(
                "`{}` did not finish within {}s and was stopped, so this copy may still be \
                 installed.",
                argv[0],
                constants::MANAGER_UNINSTALL_CEILING_SECS
            )),
            Err(e) => Err(format!("could not start `{}`: {e}", argv[0])),
        }
    }

    fn remove_installer_copy(&mut self) -> std::result::Result<(), String> {
        super::uninstall::remove_installer_copy()
    }

    fn still_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn repoint(&mut self, what: &Repoint) -> std::result::Result<(), String> {
        // The same rule `devp doctor --fix` follows: this variable means devp registers
        // nothing on this machine.
        if setup::no_auto_setup_requested() {
            return Err(format!("{} is set", setup::ENV_NO_AUTO_SETUP));
        }
        let registry = crate::config::Registry::load().map_err(|e| format!("{e:#}"))?;
        let outcome = match what {
            Repoint::Scheduler => setup::ensure_daemon(registry.settings.check_interval_days),
            Repoint::Hooks => setup::ensure_hooks(registry.settings.auto_hooks_chain),
        };
        match outcome {
            setup::Outcome::Installed | setup::Outcome::AlreadyPresent => Ok(()),
            setup::Outcome::Skipped(why) | setup::Outcome::Failed(why) => Err(why),
        }
    }
}

/// A failed manager, in a sentence: its exit status, the last words it said, and, when
/// they point at missing rights, that devp never asks for them.
fn failure_text(program: &str, out: &std::process::Output) -> String {
    let said =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    let tail: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .rev()
        .take(3)
        .collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    let mut text = format!("`{program}` exited with {}", out.status);
    if !tail.is_empty() {
        text.push_str(&format!(": {}", tail.join(" / ")));
    }
    text.push('.');
    let lower = said.to_lowercase();
    if [
        "administrator",
        "access is denied",
        "permission denied",
        "eacces",
        "elevat",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        text.push_str(
            " This needs administrator rights, which devp never asks for. Run the command \
             above from an elevated shell if you want this copy gone.",
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installs::{Roles, Version};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn install(path: &str, channel: Channel, version: Option<Version>) -> Install {
        Install {
            path: PathBuf::from(path),
            canonical: PathBuf::from(path),
            channel,
            version,
            path_position: None,
            roles: Roles::default(),
            windowless_only: false,
        }
    }

    fn found(installs: Vec<Install>) -> Discovery {
        Discovery {
            installs,
            scheduler: None,
            hook: None,
        }
    }

    /// Records everything, performs nothing.
    #[derive(Default)]
    struct FakeHost {
        /// Answers to successive questions; once empty, the answer is `default_answer`.
        answers: Vec<bool>,
        default_answer: bool,
        asked: usize,
        ran: Vec<String>,
        /// Program name to the failure it reports.
        failing: HashMap<String, String>,
        installer_removals: usize,
        installer_failure: Option<String>,
        /// Paths that survive a "successful" removal.
        survivors: HashSet<PathBuf>,
        repointed: Vec<Repoint>,
        repoint_failure: Option<String>,
    }

    impl Host for FakeHost {
        fn confirm(&mut self, _question: &str) -> bool {
            self.asked += 1;
            if self.answers.is_empty() {
                self.default_answer
            } else {
                self.answers.remove(0)
            }
        }
        fn run_manager(&mut self, argv: &[String]) -> std::result::Result<(), String> {
            self.ran.push(argv.join(" "));
            match self.failing.get(&argv[0]) {
                Some(why) => Err(why.clone()),
                None => Ok(()),
            }
        }
        fn remove_installer_copy(&mut self) -> std::result::Result<(), String> {
            self.installer_removals += 1;
            match &self.installer_failure {
                Some(why) => Err(why.clone()),
                None => Ok(()),
            }
        }
        fn still_exists(&self, path: &Path) -> bool {
            self.survivors.contains(path)
        }
        fn repoint(&mut self, what: &Repoint) -> std::result::Result<(), String> {
            self.repointed.push(what.clone());
            match &self.repoint_failure {
                Some(why) => Err(why.clone()),
                None => Ok(()),
            }
        }
    }

    fn yes_host() -> FakeHost {
        FakeHost {
            default_answer: true,
            ..FakeHost::default()
        }
    }

    // --- choosing the copy to keep -------------------------------------------------

    #[test]
    fn the_highest_version_is_kept_by_default() {
        let installs = vec![
            install("/a/devp", Channel::Cargo, Some((1, 22, 0))),
            install("/b/devp", Channel::Npm, Some((1, 24, 0))),
            install("/c/devp", Channel::Homebrew, Some((1, 23, 9))),
        ];
        assert_eq!(choose_keep(&installs, None).unwrap(), 1);
    }

    #[test]
    fn a_tie_goes_to_the_scheduler_then_to_the_first_on_path() {
        let mut installs = vec![
            install("/a/devp", Channel::Cargo, Some((1, 23, 0))),
            install("/b/devp", Channel::Npm, Some((1, 23, 0))),
            install("/c/devp", Channel::Homebrew, Some((1, 23, 0))),
        ];
        installs[0].path_position = Some(3);
        installs[1].path_position = Some(1);
        // No scheduler role: first on PATH wins.
        assert_eq!(choose_keep(&installs, None).unwrap(), 1);
        // The scheduler's copy beats PATH position.
        installs[2].roles.scheduler = true;
        assert_eq!(choose_keep(&installs, None).unwrap(), 2);
    }

    #[test]
    fn a_copy_that_states_no_version_is_never_preferred_over_one_that_does() {
        let installs = vec![
            install("/a/devp", Channel::Cargo, None),
            install("/b/devp", Channel::Npm, Some((0, 1, 0))),
        ];
        assert_eq!(choose_keep(&installs, None).unwrap(), 1);
    }

    #[test]
    fn keep_names_a_listed_copy_and_overrides_the_default() {
        let installs = vec![
            install("/a/devp", Channel::Cargo, Some((1, 22, 0))),
            install("/b/devp", Channel::Npm, Some((1, 24, 0))),
        ];
        assert_eq!(choose_keep(&installs, Some("/a/devp")).unwrap(), 0);
    }

    #[test]
    fn keep_naming_an_unlisted_path_is_a_usage_error() {
        let installs = vec![install("/a/devp", Channel::Cargo, Some((1, 22, 0)))];
        let err = choose_keep(&installs, Some("/nowhere/devp")).unwrap_err();
        assert!(err.downcast_ref::<UsageError>().is_some());
    }

    // --- what gets proposed --------------------------------------------------------

    #[test]
    fn the_running_and_the_kept_copy_are_never_proposed() {
        let mut installs = vec![
            install("/kept/devp", Channel::Cargo, Some((1, 24, 0))),
            install("/running/devp", Channel::Npm, Some((1, 23, 0))),
            install("/other/devp", Channel::Homebrew, Some((1, 22, 0))),
        ];
        installs[1].roles.running = true;
        let plan = plan(&installs, 0);
        let touched: Vec<usize> = plan.steps.iter().map(|s| s.install).collect();
        assert_eq!(touched, vec![2]);
        assert_eq!(plan.notes.len(), 1);
        assert!(plan.notes[0].contains("/running/devp"));
    }

    #[test]
    fn the_right_command_is_built_for_each_channel() {
        let expected = [
            (Channel::Cargo, "cargo uninstall dev-prune"),
            (Channel::Homebrew, "brew uninstall dev-prune"),
            (Channel::Npm, "npm uninstall -g dev-prune"),
            (Channel::Pnpm, "pnpm remove -g dev-prune"),
            (Channel::Yarn, "yarn global remove dev-prune"),
            (Channel::Bun, "bun remove -g dev-prune"),
            (Channel::Pip, "pip uninstall -y dev-prune"),
            (Channel::Pipx, "pipx uninstall dev-prune"),
            (Channel::UvTool, "uv tool uninstall dev-prune"),
            (Channel::Scoop, "scoop uninstall dev-prune"),
            (
                Channel::WinGet,
                "winget uninstall --id VKrishna04.dev-prune",
            ),
        ];
        for (channel, command) in expected {
            let installs = vec![
                install("/kept/devp", Channel::Installer, Some((1, 24, 0))),
                install("/old/devp", channel, Some((1, 20, 0))),
            ];
            let plan = plan(&installs, 0);
            match &plan.steps[0].kind {
                StepKind::Manager(argv) => assert_eq!(argv.join(" "), command),
                other => panic!("{channel:?} planned {other:?}"),
            }
        }
    }

    #[test]
    fn a_manager_that_also_owns_the_kept_copy_is_not_run() {
        // `npm uninstall -g` cannot be told which of two npm copies to take.
        let installs = vec![
            install("/kept/devp", Channel::Npm, Some((1, 24, 0))),
            install("/old/devp", Channel::Npm, Some((1, 20, 0))),
        ];
        let plan = plan(&installs, 0);
        assert!(matches!(plan.steps[0].kind, StepKind::ByHand(_)));
    }

    #[test]
    fn the_install_scripts_copy_is_removed_unless_a_protected_copy_shares_its_folder() {
        let installs = vec![
            install("/kept/devp", Channel::Cargo, Some((1, 24, 0))),
            install("/managed/bin/devp", Channel::Installer, Some((1, 20, 0))),
        ];
        assert_eq!(plan(&installs, 0).steps[0].kind, StepKind::InstallerCopy);

        // A stale twin in the folder of the copy being kept would go with the pair.
        let installs = vec![
            install(
                "/managed/bin/dev-prune",
                Channel::Installer,
                Some((1, 24, 0)),
            ),
            install("/managed/bin/devp", Channel::Installer, Some((1, 20, 0))),
        ];
        assert!(matches!(
            plan(&installs, 0).steps[0].kind,
            StepKind::ByHand(_)
        ));
    }

    // --- running it ----------------------------------------------------------------

    fn two_with_a_manager() -> Discovery {
        found(vec![
            install("/kept/devp", Channel::Installer, Some((1, 24, 0))),
            install("/old/devp", Channel::Cargo, Some((1, 20, 0))),
        ])
    }

    #[test]
    fn nothing_runs_without_a_yes() {
        let found = two_with_a_manager();
        let plan = plan(&found.installs, 0);
        let mut host = FakeHost::default();
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(host.asked, 1);
        assert!(host.ran.is_empty());
        assert_eq!(report.results[0].1, Outcome::Declined);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn a_yes_runs_exactly_the_shown_command() {
        let found = two_with_a_manager();
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(host.ran, vec!["cargo uninstall dev-prune".to_string()]);
        assert_eq!(report.results[0].1, Outcome::Removed);
    }

    #[test]
    fn a_dry_run_asks_nothing_and_runs_nothing() {
        let mut installs = two_with_a_manager().installs;
        installs.push(install("/m/bin/devp", Channel::Installer, Some((1, 19, 0))));
        let found = found(installs);
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        let report = execute(&found, &plan, true, &mut host);
        assert_eq!(host.asked, 0);
        assert!(host.ran.is_empty());
        assert_eq!(host.installer_removals, 0);
        assert!(report.results.iter().all(|(_, o)| *o == Outcome::Planned));
    }

    #[test]
    fn a_failed_manager_is_counted_and_the_rest_continue() {
        let found = found(vec![
            install("/kept/devp", Channel::Installer, Some((1, 24, 0))),
            install("/a/devp", Channel::Cargo, Some((1, 20, 0))),
            install("/b/devp", Channel::Homebrew, Some((1, 21, 0))),
        ]);
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        host.failing.insert(
            "cargo".into(),
            "`cargo` is not installed or not on PATH.".into(),
        );
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(host.ran.len(), 2, "brew still ran after cargo failed");
        assert!(matches!(report.results[0].1, Outcome::Failed(_)));
        assert_eq!(report.results[1].1, Outcome::Removed);
        assert_eq!(report.failures, 1);
    }

    #[test]
    fn a_manager_that_succeeds_but_leaves_the_file_is_a_failure() {
        let found = two_with_a_manager();
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        host.survivors.insert(PathBuf::from("/old/devp"));
        let report = execute(&found, &plan, false, &mut host);
        assert!(matches!(report.results[0].1, Outcome::Failed(_)));
        assert_eq!(report.failures, 1);
    }

    #[test]
    fn unknown_and_foreign_copies_are_described_and_never_touched() {
        let found = found(vec![
            install("/kept/devp", Channel::Cargo, Some((1, 24, 0))),
            install("/hand/devp", Channel::Unknown, Some((1, 20, 0))),
            install("/pkg/devp", Channel::Foreign("Nix"), Some((1, 19, 0))),
        ]);
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(host.asked, 0, "there is nothing to confirm");
        assert!(host.ran.is_empty());
        assert_eq!(host.installer_removals, 0);
        assert!(report.results.iter().all(|(_, o)| *o == Outcome::ByHand));
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn the_install_scripts_copy_goes_through_the_host_after_a_yes() {
        let found = found(vec![
            install("/kept/devp", Channel::Cargo, Some((1, 24, 0))),
            install("/managed/bin/devp", Channel::Installer, Some((1, 20, 0))),
        ]);
        let plan = plan(&found.installs, 0);
        let mut declined = FakeHost::default();
        execute(&found, &plan, false, &mut declined);
        assert_eq!(declined.installer_removals, 0);
        let mut host = yes_host();
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(host.installer_removals, 1);
        assert_eq!(report.results[0].1, Outcome::Removed);
    }

    // --- pointing the scheduler and hooks at the kept copy --------------------------

    fn scheduler_on_the_old_copy(kept_runs: bool) -> Discovery {
        let mut kept = install("/kept/devp", Channel::Installer, Some((1, 24, 0)));
        kept.roles.running = kept_runs;
        let mut old = install("/old/devp", Channel::Cargo, Some((1, 20, 0)));
        old.roles.scheduler = true;
        found(vec![kept, old])
    }

    #[test]
    fn the_scheduler_is_repointed_when_the_kept_copy_is_the_running_one() {
        let found = scheduler_on_the_old_copy(true);
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(host.repointed, vec![Repoint::Scheduler]);
        assert_eq!(report.repointed, vec!["the scheduler"]);
        assert!(report.hint.is_none());
    }

    #[test]
    fn from_another_copy_the_command_to_run_is_printed_instead() {
        let found = scheduler_on_the_old_copy(false);
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        let report = execute(&found, &plan, false, &mut host);
        assert!(host.repointed.is_empty());
        let hint = report.hint.unwrap();
        assert!(hint.contains("/kept/devp setup"), "{hint}");
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn nothing_is_repointed_when_the_removal_was_declined() {
        let found = scheduler_on_the_old_copy(true);
        let plan = plan(&found.installs, 0);
        let mut host = FakeHost::default();
        let report = execute(&found, &plan, false, &mut host);
        assert!(host.repointed.is_empty());
        assert!(report.hint.is_none());
    }

    #[test]
    fn a_repoint_that_fails_is_reported_with_the_command_and_counted() {
        let found = scheduler_on_the_old_copy(true);
        let plan = plan(&found.installs, 0);
        let mut host = yes_host();
        host.repoint_failure = Some("refused".into());
        let report = execute(&found, &plan, false, &mut host);
        assert_eq!(report.failures, 1);
        assert!(report.hint.unwrap().contains("setup"));
    }

    // --- the whole command ---------------------------------------------------------

    #[test]
    fn one_copy_means_nothing_to_do() {
        let found = found(vec![install(
            "/only/devp",
            Channel::Cargo,
            Some((1, 24, 0)),
        )]);
        let mut host = yes_host();
        clean(&found, None, false, &mut host).unwrap();
        assert_eq!(host.asked, 0);
        assert!(host.ran.is_empty());
    }

    #[test]
    fn a_confirmed_failure_makes_the_command_fail() {
        let found = two_with_a_manager();
        let mut host = yes_host();
        host.failing.insert("cargo".into(), "boom".into());
        assert!(clean(&found, None, false, &mut host).is_err());
        // The same failure in a dry run was never attempted.
        let mut host = yes_host();
        host.failing.insert("cargo".into(), "boom".into());
        assert!(clean(&found, None, true, &mut host).is_ok());
    }

    #[test]
    fn a_failure_text_names_the_admin_requirement() {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1)
        };
        #[cfg(not(windows))]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1 << 8)
        };
        let out = std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: b"Access is denied.\n".to_vec(),
        };
        let text = failure_text("winget", &out);
        assert!(text.contains("administrator rights"), "{text}");
        assert!(text.contains("Access is denied."), "{text}");
    }
}
