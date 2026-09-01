// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune install --channel`, which moves an existing install from one
// package manager to another.
//
// `devp update` upgrades the copy that is running, through whichever channel installed
// it, and that is deliberately the only thing it does. Changing *which* channel owns the
// binary was the gap: somebody who ran `cargo install dev-prune` and later wanted WinGet
// had to know to remove the old copy first, and if they did not, two binaries sat on
// PATH and which one won was an accident of ordering.
//
// So this command does the two halves in the order that leaves a working `devp` at every
// point in between: install through the new manager first, then remove the old copy
// through the manager that owns it. An install that fails leaves the old copy exactly
// where it was, which is why it is not removed first.
//
// Nothing has to be migrated. Configuration, the repository registry and the undo state
// all live in the config directory, which no channel owns and none of them touch.
//
// It spawns another package manager to uninstall something — the one category of action
// this tool keeps behind an explicit request — so it is a command you type, it prints the
// whole plan before running any of it, and it asks first unless `--yes` is passed.

use anyhow::{Context, Result};
use clap::ValueEnum;

use crate::channel::Channel;
use crate::config::Registry;
use crate::output;

/// The channels a user can move *to*, as `--channel` accepts them.
///
/// Deliberately not every [`Channel`]: `Unknown` is not a destination, and `Pip` is
/// omitted because a bare `pip install` of a CLI puts the console script wherever the
/// active interpreter happens to be, which is exactly the ambiguity `uv tool` and `pipx`
/// exist to remove.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum TargetChannel {
    /// The install script, into the managed `<config>/bin` directory.
    Installer,
    /// `cargo install dev-prune` (or `cargo binstall`, when it is available).
    Cargo,
    /// `npm install -g dev-prune`.
    Npm,
    /// `bun add -g dev-prune`.
    Bun,
    /// `pnpm add -g dev-prune`.
    Pnpm,
    /// `yarn global add dev-prune` — Yarn 1.x only.
    Yarn,
    /// `uv tool install dev-prune`.
    Uv,
    /// `pipx install dev-prune`.
    Pipx,
    /// `winget install` — Windows only.
    Winget,
    /// `scoop install` from the project's bucket — Windows only.
    Scoop,
    /// `brew install` from the project's tap — macOS and Linux.
    Homebrew,
}

impl TargetChannel {
    fn channel(self) -> Channel {
        match self {
            TargetChannel::Installer => Channel::Installer,
            TargetChannel::Cargo => Channel::Cargo,
            TargetChannel::Npm => Channel::Npm,
            TargetChannel::Bun => Channel::Bun,
            TargetChannel::Pnpm => Channel::Pnpm,
            TargetChannel::Yarn => Channel::Yarn,
            TargetChannel::Uv => Channel::UvTool,
            TargetChannel::Pipx => Channel::Pipx,
            TargetChannel::Winget => Channel::WinGet,
            TargetChannel::Scoop => Channel::Scoop,
            TargetChannel::Homebrew => Channel::Homebrew,
        }
    }
}

pub fn run(channel: Option<TargetChannel>, dry_run: bool, yes: bool) -> Result<()> {
    let exe = std::env::current_exe().context("could not locate the running binary")?;
    let managed = crate::setup::managed_exe_path().ok();
    let current = Channel::detect_at(&exe, managed.as_deref());

    let Some(target) = channel.map(TargetChannel::channel) else {
        return report(current, &exe);
    };

    output::print_header("dev-prune install channel");

    if target == current {
        output::print_success(&format!(
            "This copy already came from {} — nothing to move.",
            current.label()
        ));
        if let Some(cmd) = current.upgrade_command() {
            output::print_info(&format!("Upgrade it in place with: {cmd}"));
        }
        return converge(&exe, dry_run, yes);
    }

    // A channel move installs the latest release through the new manager, so under a
    // pin it is an update wearing a different name. Refused here rather than moved and
    // then reported, so that one rule stays true without exception: while
    // `version_lock` is on, nothing dev-prune does changes which version is installed.
    if Registry::load().is_ok_and(|r| r.settings.version_lock) {
        anyhow::bail!(
            "Moving to {} would install the latest release through it. {}",
            target.label(),
            super::update::locked_notice(None)
        );
    }

    let sources = target.install_sources();
    let install = target.install_argv();
    let uninstall = current.uninstall_argv();

    println!();
    println!("  From:  {} ({})", current.label(), exe.display());
    println!("  To:    {}", target.label());
    println!();
    let mut step = 0;
    for argv in sources.iter().chain(install.iter()) {
        step += 1;
        println!("  {step}. {}", argv.join(" "));
    }
    step += 1;
    match &uninstall {
        Some(argv) => println!("  {step}. {}", argv.join(" ")),
        None => println!(
            "  {step}. nothing to uninstall — {}",
            match current {
                // The managed copy is what the scheduler and the Git hook run, and it
                // refreshes itself from whichever binary is newest on the next healthy
                // pass, so removing it here would break both to no purpose.
                Channel::Installer =>
                    "the managed copy stays, and refreshes itself from the new binary",
                _ =>
                    "this copy was not installed by a package manager, so remove the \
                      file yourself if you want it gone",
            }
        ),
    }
    println!();

    if dry_run {
        output::print_info("`--dry-run`: nothing was run.");
        return Ok(());
    }

    if !confirm(yes) {
        output::print_info("Nothing was changed.");
        return Ok(());
    }

    for argv in &sources {
        // A tap or bucket that is already added reports failure, and that is not a
        // reason to stop — the install below is what actually has to succeed.
        if let Err(e) = spawn(argv) {
            output::print_dimmed(&format!("  ({e:#} — continuing.)"));
        }
    }

    // The install goes first on purpose: if it fails, the copy on PATH is still the one
    // that was there before, and the machine is exactly as it was.
    if let Some(argv) = &install {
        spawn(argv)?;
    }
    output::print_success(&format!("Installed through {}.", target.label()));

    if let Some(argv) = uninstall {
        // Removing the binary that is executing right now. Windows will not let a
        // package manager delete a running image: `cargo uninstall` fails with `Access
        // is denied` and keeps its ledger entry, and renaming the file aside first only
        // trades that for `corrupt metadata, ... does not exist when it should`, which
        // keeps the entry too. Exiting first is the only order that clears the record,
        // and the only way to run something after this process exits is to leave a
        // detached shell behind — the exact shape anti-virus heuristics quarantine, and
        // one dev-prune has been quarantined for. So on Windows this command is the
        // user's to run, and saying so plainly beats doing it invisibly.
        #[cfg(windows)]
        {
            output::print_warning(&format!(
                "The {} copy is still here: Windows cannot uninstall a binary while it \
                 is running. Run this once, in a new shell:\n  {}",
                current.label(),
                argv.join(" ")
            ));
        }
        #[cfg(not(windows))]
        match spawn(&argv) {
            Ok(()) => output::print_success(&format!("Removed the {} copy.", current.label())),
            Err(e) => output::print_warning(&format!(
                "The new copy is installed, but removing the old one failed ({e:#}).\n\
                 Run it yourself when convenient: {}",
                argv.join(" ")
            )),
        }
    }

    println!();
    output::print_info(
        "Your configuration, repository registry and undo history are unchanged — they \
         live in the config directory, which no channel owns.",
    );
    output::print_info("Open a new shell, then `devp update` to confirm which copy it finds.");
    Ok(())
}

/// Finish the job when the channel asked for is the one this copy already came from.
///
/// Nobody types `devp install --channel installer` on an installer copy to be told about
/// provenance. They type it because a second copy is in the way, and they want one left.
/// That used to be the end of the road: the message said there was nothing to move, and
/// the installer script, knowing it, handed the same command to the *older* binary
/// instead — the one copy least able to run it, because nothing before 1.8.0 has an
/// `install` subcommand at all. The offer dead-ended on precisely the machines that
/// needed it. This copy is new by definition, so the work happens here.
///
/// Only copies outside this binary's own directory count. The alias sitting beside it is
/// the same install under a second name, and removing it would break `devp`.
fn converge(exe: &std::path::Path, dry_run: bool, yes: bool) -> Result<()> {
    use crate::commands::uninstall::{canon_key, find_stray_copies, group_by_channel};

    let here = exe.parent().map(canon_key);
    let others: Vec<_> = find_stray_copies()
        .into_iter()
        .filter(|s| s.path.parent().map(canon_key) != here)
        .collect();

    println!();
    if others.is_empty() {
        output::print_info("No other copy of dev-prune is on this machine.");
        return Ok(());
    }

    output::print_warning(&format!(
        "{} other cop{} of dev-prune {} on this machine:",
        others.len(),
        if others.len() == 1 { "y" } else { "ies" },
        if others.len() == 1 { "is" } else { "are" }
    ));
    println!();
    for stray in &others {
        println!("  {}", output::clean_path(&stray.path));
        match stray.channel.uninstall_argv() {
            Some(argv) => println!("      {}: {}", stray.channel.label(), argv.join(" ")),
            // No manager holds a record of it, so there is no command to run and the
            // file itself is the whole install.
            None => println!("      {}: delete the file", stray.channel.label()),
        }
    }
    println!();

    if dry_run {
        output::print_info("`--dry-run`: nothing was run.");
        return Ok(());
    }

    // The same rule the channel move above is refused by, on the path that reaches this
    // one without passing it: whichever of these copies answers on PATH is the version
    // that runs, so removing it changes that version just as surely as an upgrade would.
    if Registry::load().is_ok_and(|r| r.settings.version_lock) {
        anyhow::bail!(
            "Removing another copy would change which version answers on PATH. {}",
            super::update::locked_notice(None)
        );
    }

    if !confirm(yes) {
        output::print_info("Left in place. Nothing was changed.");
        return Ok(());
    }

    let mut removed = 0usize;
    let mut failed: Vec<(std::path::PathBuf, String)> = Vec::new();
    // Grouped, because one manager is told once however many of its files turned up.
    // `~/.cargo/bin` holds both names, and a second `cargo uninstall dev-prune` exits
    // 101 — reporting that would say the removal failed when it had just worked.
    for (channel, paths) in group_by_channel(others) {
        let Some(argv) = channel.uninstall_argv() else {
            // No manager holds a record, so the file is the whole install.
            for path in paths {
                match std::fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(e) => failed.push((path, e.to_string())),
                }
            }
            continue;
        };
        match spawn(&argv) {
            Ok(()) => removed += paths.len(),
            Err(e) => {
                for path in paths {
                    failed.push((path, format!("{e:#}")));
                }
            }
        }
    }

    println!();
    if removed > 0 {
        output::print_success(&format!(
            "Removed {removed} other cop{}.",
            if removed == 1 { "y" } else { "ies" }
        ));
    }
    for (path, why) in &failed {
        output::print_warning(&format!(
            "{} is still there: {why}",
            output::clean_path(path)
        ));
    }
    output::print_info("Open a new shell, then `devp update` to confirm which copy it finds.");
    Ok(())
}

/// What `devp install` prints on its own: which channel owns this copy, and the names
/// `--channel` accepts.
fn report(current: Channel, exe: &std::path::Path) -> Result<()> {
    output::print_header("dev-prune install channel");
    println!();
    println!("  Installed by:  {}", current.label());
    println!("  Binary:        {}", exe.display());
    // The receipt describes the managed copy, so it is only true of this one when this
    // one *is* the managed copy.
    if current == Channel::Installer
        && let Some(receipt) = crate::receipt::load()
    {
        println!("  Receipt:       {}", crate::receipt::summary(&receipt));
    }
    if let Some(cmd) = current.upgrade_command() {
        println!("  Upgrade:       {cmd}");
    }
    println!();
    // Read off the enum clap itself parses, so the list cannot name a channel
    // `--channel` rejects, or omit one it accepts.
    let names = TargetChannel::value_variants()
        .iter()
        .filter_map(|t| t.to_possible_value())
        .map(|v| v.get_name().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    output::print_info(&format!(
        "Move it to another package manager with `devp install --channel <name>`:\n  \
         {names}."
    ));
    output::print_info("`--dry-run` prints the whole plan without running any of it.");
    Ok(())
}

/// Run one of the two commands, wired to the terminal so the manager's own progress and
/// prompts reach the user directly.
fn spawn(argv: &[String]) -> Result<()> {
    output::print_info(&format!("Running: {}", argv.join(" ")));
    let status = crate::spawn::command(crate::adapters::resolve_program(&argv[0]))
        .args(&argv[1..])
        // Installing through the `installer` channel re-runs `install.sh` or
        // `install.ps1`, and those scripts offer to migrate a copy another manager owns.
        // That is the offer whose answer brought us here — and the old copy is still on
        // PATH, because it is removed after this command, not before — so without this
        // the child would ask the same question again, and its answer would run this
        // command again.
        .env(crate::constants::ENV_NO_MIGRATE_PROMPT, "1")
        .status()
        .with_context(|| format!("could not start `{}`", argv[0]))?;
    if !status.success() {
        anyhow::bail!("`{}` exited with {status}", argv.join(" "));
    }
    Ok(())
}

/// Ask before anything runs. Default no, like every other prompt that removes something:
/// this one runs two package managers back to back.
fn confirm(yes: bool) -> bool {
    use std::io::{IsTerminal, Write};
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        output::print_info("Not running in a terminal — pass `--yes` to go ahead.");
        return false;
    }
    eprint!("Run this plan? [y/N]: ");
    if std::io::stderr().flush().is_err() {
        return false;
    }
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_destination_has_an_install_command() {
        // A `--channel` value with no install command would silently do nothing.
        // The value list and the command table have to stay in step.
        for target in TargetChannel::value_variants() {
            assert!(
                target.channel().install_argv().is_some(),
                "`--channel {target:?}` has no install command"
            );
        }
    }

    #[test]
    fn the_old_copy_is_removed_through_the_manager_that_owns_it() {
        // Not a restatement: the rule is that a channel with bookkeeping must be told,
        // rather than having its file deleted behind its back — otherwise the manager
        // goes on believing dev-prune is installed and reinstalls the old binary.
        for channel in [
            Channel::Cargo,
            Channel::Npm,
            Channel::Bun,
            Channel::Pnpm,
            Channel::Yarn,
            Channel::UvTool,
            Channel::Pipx,
            Channel::Pip,
            Channel::WinGet,
            Channel::Scoop,
            Channel::Homebrew,
        ] {
            assert!(channel.owns_its_files());
            assert!(
                channel.uninstall_argv().is_some(),
                "{channel:?} keeps a record but has no uninstall command"
            );
        }
        assert!(Channel::Installer.uninstall_argv().is_none());
        assert!(Channel::Unknown.uninstall_argv().is_none());
    }
}
