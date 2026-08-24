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
        return Ok(());
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

    let (sources, install) = install_plan(target);
    let uninstall = uninstall_argv(current);

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
    for argv in &install {
        spawn(argv)?;
    }
    output::print_success(&format!("Installed through {}.", target.label()));

    if let Some(argv) = uninstall {
        // Removing the binary that is executing right now. Windows locks a running
        // image against deletion but not against rename, so it steps aside first and
        // the manager's own uninstall is not blocked by it; the `.old` is swept by the
        // next `devp update --install`, when nothing holds it open.
        #[cfg(windows)]
        let aside = {
            let aside = exe.with_extension("exe.old");
            let _ = std::fs::remove_file(&aside);
            std::fs::rename(&exe, &aside).ok().map(|_| aside)
        };

        let removed = spawn(&argv);

        #[cfg(windows)]
        if let Some(aside) = aside
            && removed.is_err()
            && !exe.exists()
        {
            // The uninstall failed and the old copy is now nameless. Put it back rather
            // than leaving a PATH entry pointing at nothing.
            let _ = std::fs::rename(&aside, &exe);
        }

        match removed {
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
    output::print_info(
        "Move it to another package manager with `devp install --channel <name>`:\n  \
         installer, cargo, npm, uv, pipx, winget, scoop, homebrew.",
    );
    output::print_info("`--dry-run` prints the whole plan without running any of it.");
    Ok(())
}

/// How to install dev-prune fresh through `channel`: the sources to add first, then the
/// install itself.
///
/// Homebrew and Scoop are the reason for the first half. The formula and the manifest
/// live in this project's own tap and bucket rather than the default index, and `brew
/// install dev-prune` without the tap resolves against homebrew-core, where dev-prune is
/// not published. Adding a source that is already added is not an error worth stopping
/// for, so those steps are best-effort; the install itself is not.
fn install_plan(channel: Channel) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let sources = match channel {
        Channel::Scoop => vec![owned(&[
            "scoop",
            "bucket",
            "add",
            crate::constants::SCOOP_BUCKET_NAME,
            crate::constants::SCOOP_BUCKET_URL,
        ])],
        Channel::Homebrew => vec![owned(&["brew", "tap", crate::constants::HOMEBREW_TAP])],
        _ => Vec::new(),
    };
    (sources, install_argv(channel))
}

/// The command that installs dev-prune through `channel`, once its source exists.
fn install_argv(channel: Channel) -> Vec<Vec<String>> {
    let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match channel {
        // Same preference as `devp update --install`: binstall fetches the prebuilt
        // release, a plain `cargo install` compiles for minutes.
        Channel::Cargo => {
            if crate::adapters::binary_available("cargo-binstall") {
                vec![owned(&["cargo", "binstall", "dev-prune", "-y"])]
            } else {
                vec![owned(&["cargo", "install", "dev-prune"])]
            }
        }
        Channel::Npm => vec![owned(&["npm", "install", "-g", "dev-prune"])],
        Channel::UvTool => vec![owned(&["uv", "tool", "install", "dev-prune"])],
        Channel::Pipx => vec![owned(&["pipx", "install", "dev-prune"])],
        Channel::WinGet => vec![vec![
            "winget".to_string(),
            "install".to_string(),
            "--id".to_string(),
            crate::constants::WINGET_PACKAGE_ID.to_string(),
            "--accept-package-agreements".to_string(),
            "--accept-source-agreements".to_string(),
        ]],
        Channel::Scoop => vec![owned(&["scoop", "install", "dev-prune"])],
        Channel::Homebrew => vec![owned(&["brew", "install", "dev-prune"])],
        Channel::Installer => {
            if cfg!(windows) {
                vec![vec![
                    "powershell".to_string(),
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    format!("iwr -useb {} | iex", crate::constants::INSTALL_PS1_URL),
                ]]
            } else {
                vec![vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("curl -fsSL {} | sh", crate::constants::INSTALL_SH_URL),
                ]]
            }
        }
        // Not offered as a destination; see `TargetChannel`.
        Channel::Pip | Channel::Unknown => Vec::new(),
    }
}

/// The command that removes the copy `channel` installed, or `None` when there is no
/// manager holding a record of it.
fn uninstall_argv(channel: Channel) -> Option<Vec<String>> {
    let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Some(match channel {
        Channel::Cargo => owned(&["cargo", "uninstall", "dev-prune"]),
        Channel::Npm => owned(&["npm", "uninstall", "-g", "dev-prune"]),
        Channel::UvTool => owned(&["uv", "tool", "uninstall", "dev-prune"]),
        Channel::Pipx => owned(&["pipx", "uninstall", "dev-prune"]),
        Channel::Pip => owned(&["pip", "uninstall", "-y", "dev-prune"]),
        Channel::WinGet => vec![
            "winget".to_string(),
            "uninstall".to_string(),
            "--id".to_string(),
            crate::constants::WINGET_PACKAGE_ID.to_string(),
        ],
        Channel::Scoop => owned(&["scoop", "uninstall", "dev-prune"]),
        Channel::Homebrew => owned(&["brew", "uninstall", "dev-prune"]),
        Channel::Installer | Channel::Unknown => return None,
    })
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
        // A `--channel` value that maps to an empty argv would panic in `spawn`. The
        // value list and the command table have to stay in step.
        for target in TargetChannel::value_variants() {
            let argv = install_argv(target.channel());
            assert!(
                !argv.is_empty(),
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
            Channel::UvTool,
            Channel::Pipx,
            Channel::Pip,
            Channel::WinGet,
            Channel::Scoop,
            Channel::Homebrew,
        ] {
            assert!(channel.owns_its_files());
            assert!(
                uninstall_argv(channel).is_some(),
                "{channel:?} keeps a record but has no uninstall command"
            );
        }
        assert!(uninstall_argv(Channel::Installer).is_none());
        assert!(uninstall_argv(Channel::Unknown).is_none());
    }
}
