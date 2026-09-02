// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune uninstall`.
//
// Two modes, and both of them remove the program itself — an uninstall that leaves a
// fully working binary on PATH is not an uninstall, it is a settings change:
//
// - Light (default): removes the scheduler, the Git hooks, the file-type icons, the
//   agent skill, the PATH entry and the binaries. The config directory — registry,
//   prune history, settings — is kept, so a later reinstall picks up where it left off.
// - Deep (`--deep`): all of the above, plus `.devprune.json` in every registered
//   repository and the config directory itself.
//
// Both modes also sweep for *other* copies of the pair — a machine that has tried
// `pip install`, `cargo install` and the shell installer over time has binaries and
// shims in `~/.cargo/bin`, `~/.local/bin`, npm's global directory, a venv's `Scripts`
// — and offers to delete every one it finds, so "uninstall" means the command stops
// resolving everywhere, not just in the managed directory.
//
// A copy a package manager installed is removed by *that manager's* uninstall, never
// by deleting the file: with the file gone, `cargo uninstall dev-prune` exits 101 with
// `corrupt metadata, ... does not exist when it should` and leaves its ledger entry
// standing, so the command printed as the remedy can no longer succeed.
//
// On Windows a running executable cannot delete itself. dev-prune does not work around
// that by leaving a shell behind to finish the job after it exits: that shape is the
// textbook self-delete, and it is what gets an unsigned binary quarantined. It renames
// the locked image aside instead — which Windows does allow — hands the residue to the
// session manager for the next boot, and prints whatever survives both.
//
// A package manager's own uninstall cannot be rescued the same way. While its binary is
// executing, `cargo uninstall` fails with `Access is denied` and keeps its ledger entry,
// and renaming the file aside first only trades that failure for the `corrupt metadata`
// one, which keeps the entry too. Exit first, then uninstall, is the only order that
// clears the record, so that command is printed for the user rather than run.
//
// Neither is a failure: the command reports both and exits `0`.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::channel::Channel;
use crate::commands::hook;
use crate::config::Registry;
use crate::output;
use crate::setup;

pub fn run(deep: bool, yes: bool) -> Result<()> {
    output::print_header(if deep {
        "dev-prune Deep Uninstaller (Full Purge)"
    } else {
        "dev-prune Uninstaller"
    });

    let registry = Registry::load().ok();

    // A deep uninstall deletes files inside the user's own repositories and destroys
    // the prune history. That is not something to do on a mistyped flag.
    if deep && !yes {
        use std::io::{IsTerminal, Write};
        let repo_count = registry.as_ref().map(|r| r.repo_count()).unwrap_or(0);
        output::print_warning(&format!(
            "This deletes the global config directory (including prune history) and \
             removes `.devprune.json` from {repo_count} registered repositories."
        ));
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Refusing to deep-uninstall without confirmation. Re-run with `--yes`.");
        }
        // stderr, like every other confirmation: with stdout piped the question would
        // vanish into the pipe and the command would appear to hang.
        eprint!("Continue? [y/N]: ");
        std::io::stderr().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
            output::print_info("Deep uninstall cancelled.");
            return Ok(());
        }
    }

    // Each step keeps going when another fails — a scheduler that refuses to uninstall
    // must not stop the hooks being removed — but none of them is silent about it. What
    // could not be removed is reported, and its presence makes the exit code `1`.
    let mut left_behind: Vec<String> = Vec::new();
    // Files and directories that are in use right now (Windows keeps a running image
    // locked, under every one of its hard-linked names). Deleted by a detached helper
    // the moment this process exits.
    let mut pending_files: Vec<PathBuf> = Vec::new();
    let mut pending_dirs: Vec<PathBuf> = Vec::new();

    // `DEV_PRUNE_NO_AUTO_SETUP` means "dev-prune manages nothing on this machine" — and
    // that has to cut both ways. If the variable stopped setup from registering a
    // scheduler or writing into agent skill directories, then uninstall must not reach
    // for them either: whatever is there was put there by hand (or by another install
    // this process knows nothing about), and hands-off means hands-off. It is also what
    // lets the test suite run this command against a real machine.
    let hands_off = setup::no_auto_setup_requested();

    // 1. Background scheduler.
    if hands_off {
        output::print_info(&format!(
            "{} is set — leaving the scheduler and agent skills alone.",
            setup::ENV_NO_AUTO_SETUP
        ));
    } else {
        output::print_info("Removing background daemon scheduler...");
        if let Err(e) = crate::daemon::uninstall_daemon() {
            output::print_error(&format!("Background scheduler: {e:#}"));
            left_behind.push("the background scheduler".to_string());
        }
    }

    // 2. Global Git hooks.
    output::print_info("Removing global Git auto-registration hooks...");
    if let Err(e) = hook::run_uninstall() {
        output::print_error(&format!("Git hooks: {e:#}"));
        left_behind.push("the global Git hooks".to_string());
    }

    // 3. The `*.devprune.json` file type, out of the desktop database.
    crate::commands::icon::unregister_file_type();

    // 4. The skill installed into AI agents' own directories. Only directories named
    // for this tool are touched — `~/.claude/skills/dev-prune/`, never a sibling — and
    // none at all under hands-off, for the same reason as the scheduler above.
    let skill_roots = if hands_off {
        Vec::new()
    } else {
        setup::agent_skill_roots()
    };
    for root in skill_roots {
        if !root.exists() {
            continue;
        }
        match fs::remove_dir_all(&root) {
            Ok(()) => output::print_info(&format!(
                "Removed the agent skill at {}.",
                output::clean_path(&root)
            )),
            Err(e) => {
                output::print_error(&format!(
                    "Could not remove {}: {e}",
                    output::clean_path(&root)
                ));
                left_behind.push("the AI agent skill".to_string());
            }
        }
    }

    // 5. Reachability: the user-PATH entry on Windows, the `~/.local/bin` links
    // elsewhere. Before the binaries go, so no window exists where PATH names a
    // directory whose contents are gone.
    if let Ok(bin_dir) = setup::managed_bin_dir() {
        match crate::pathenv::remove_reachability(&bin_dir) {
            Ok(true) => output::print_info("Removed dev-prune from your PATH."),
            Ok(false) => {}
            Err(e) => {
                output::print_error(&format!("Could not update your PATH: {e:#}"));
                left_behind.push("the PATH entry".to_string());
            }
        }
    }

    // 6. The binaries themselves.
    let channel = Channel::detect();
    remove_binaries(
        deep,
        channel.owns_its_files(),
        &mut left_behind,
        &mut pending_files,
        &mut pending_dirs,
    );

    // 7. Every other copy on the machine. A machine that has tried more than one
    // install channel has more than one binary, and the ones not currently first on
    // PATH would quietly *become* the installation the moment the managed pair above
    // is gone.
    //
    // The channel this binary came from needs no separate handling: the sweep looks in
    // the running executable's own directory, so a manager-owned copy is found there
    // like any other and removed the same way.
    let mut manager_hints: Vec<Channel> = Vec::new();
    let mut pending_commands: Vec<Channel> = Vec::new();
    sweep_stray_copies(
        yes,
        &mut manager_hints,
        &mut left_behind,
        &mut pending_files,
        &mut pending_commands,
    );

    if deep {
        // Per-repo configs, then the config directory itself.
        //
        // Only the personal file. `project.devprune.json` is a tracked file somebody
        // committed, and uninstalling a tool from one machine is not a mandate to delete
        // a file from a shared repository -- the deletion would show up in `git status`
        // on a branch the user never meant to touch, and reach their colleagues on the
        // next push.
        if let Some(reg) = registry {
            for repo_path in reg.repositories.keys() {
                let cfg_file = repo_path.join(crate::constants::PER_REPO_CONFIG_FILE);
                if cfg_file.exists() {
                    let _ = fs::remove_file(cfg_file);
                }
            }
        }

        if let Ok(config_dir) = Registry::config_dir()
            && config_dir.exists()
        {
            match fs::remove_dir_all(&config_dir) {
                Ok(()) => output::print_info("Removed global configuration directory."),
                Err(e) => {
                    // Routine on Windows: the managed copy under `<config>/bin` is
                    // often the very binary running this command, and a running
                    // executable cannot be deleted. That case is finished by the
                    // helper; anything else really is left behind.
                    let running_inside =
                        std::env::current_exe().is_ok_and(|exe| exe.starts_with(&config_dir));
                    if cfg!(windows) && running_inside {
                        pending_dirs.push(config_dir);
                    } else {
                        output::print_error(&format!(
                            "Could not remove {}: {e}",
                            output::clean_path(&config_dir)
                        ));
                        left_behind.push("the global configuration directory".to_string());
                    }
                }
            }
        }
    } else {
        // Stamp the current version so a surviving copy (a package-manager install, a
        // dev build) does not reinstall, on its very next command, everything this
        // command was run to remove.
        setup::suppress_next_auto_setup();
        // And put the first-run question back the way a fresh machine has it: a
        // "granted" that outlives the things it granted would make the first upgrade
        // after this quietly reinstall all of them.
        setup::clear_setup_consent();
    }

    // 8. Everything above that Windows refused because this very process holds the file
    // open. It is finished here, in this process, or it is reported -- never handed to
    // a background shell. `finish_locked_removals` has the reasoning.
    let (residue, still_installed) = finish_locked_removals(&pending_files, &pending_dirs);
    if !residue.is_empty() {
        report_manual_removal(&residue);
    } else if !pending_files.is_empty() || !pending_dirs.is_empty() {
        output::print_info(
            "The binary running this command cannot delete itself. Its name is free \
             again, and Windows removes what is left of the file at the next restart.",
        );
    }
    if still_installed {
        left_behind.push("the binaries".to_string());
    }
    // A package manager cannot uninstall a binary that is executing either, and no
    // ordering inside this process changes that -- see the note at the top of the file.
    // Its command goes back onto the list printed below, for the user to run once this
    // command has exited.
    for channel in &pending_commands {
        if !manager_hints.contains(channel) {
            manager_hints.push(*channel);
        }
    }

    println!();
    if left_behind.is_empty() {
        output::print_success(if deep {
            "Deep uninstall complete: program, integrations, configuration and registry removed."
        } else {
            "Uninstall complete: program and integrations removed. Configuration and \
             prune history preserved for a future reinstall."
        });
    }
    for hint in &manager_hints {
        let Some(command) = hint.uninstall_command() else {
            continue;
        };
        output::print_info(&format!(
            "{} still lists dev-prune as installed — finish with `{command}` to clear \
             its records.",
            hint.label()
        ));
    }
    output::print_info(&format!("Reinstall any time with: {}", reinstall_hint()));

    if !left_behind.is_empty() {
        anyhow::bail!("Uninstall finished, but {} is still installed.", {
            left_behind.join(" and ")
        });
    }

    Ok(())
}

/// Delete the managed pair, and the pair beside the running executable.
///
/// Skips a development build outright — deleting `target/debug/dev-prune` because a
/// test or a contributor ran `uninstall` would destroy the build being worked on — and
/// skips the copy beside a package-manager-owned executable, which the sweep offers to
/// delete *with confirmation* rather than silently, because pulling files out from
/// under a manager leaves its records dangling until its own uninstall command runs.
fn remove_binaries(
    deep: bool,
    manager_owned: bool,
    left_behind: &mut Vec<String>,
    pending_files: &mut Vec<PathBuf>,
    pending_dirs: &mut Vec<PathBuf>,
) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let managed_bin_dir = setup::managed_bin_dir().ok();

    if let Some(bin_dir) = &managed_bin_dir {
        for stem in ["dev-prune", "devp"] {
            candidates.push(bin_dir.join(exe_name(stem)));
        }
        // The windowless scheduler twin, which ships in the Windows archives as its own
        // build target. `WINDOWS_WINDOWLESS_BIN` already carries its `.exe`.
        #[cfg(windows)]
        candidates.push(bin_dir.join(crate::constants::WINDOWS_WINDOWLESS_BIN));
    }

    if let Ok(current) = std::env::current_exe() {
        if is_dev_build(&current) {
            output::print_info(
                "This is a development build — leaving the `target/` binaries alone.",
            );
        } else if manager_owned {
            // The sweep below removes it, by running the manager's own uninstall.
            // Deleting it here would destroy that command's precondition.
        } else if let Some(parent) = current.parent() {
            for stem in ["dev-prune", "devp"] {
                let twin = parent.join(exe_name(stem));
                if !candidates.contains(&twin) {
                    candidates.push(twin);
                }
            }
        }
    }

    let mut removed_any = false;
    for exe in candidates {
        if !exe.is_file() {
            continue;
        }
        match fs::remove_file(&exe) {
            Ok(()) => removed_any = true,
            Err(e) => {
                if cfg!(windows) && is_in_use_error(&e) {
                    pending_files.push(exe);
                } else {
                    output::print_error(&format!(
                        "Could not remove {}: {e}",
                        output::clean_path(&exe)
                    ));
                    left_behind.push("the binaries".to_string());
                }
            }
        }
    }
    if removed_any {
        output::print_info("Removed the dev-prune binaries.");
    }

    // The install receipt describes the binary in this directory and nothing else, so it
    // goes when that binary goes. Left behind it would outlive its subject, and it would
    // also keep the directory below from ever being empty.
    if let Some(bin_dir) = &managed_bin_dir {
        let _ = fs::remove_file(bin_dir.join(crate::constants::INSTALL_RECEIPT_FILE));
    }

    // The managed `bin` directory should not outlive its contents. On a deep uninstall
    // the whole config directory goes anyway; on a light one, remove it once empty, or
    // let the helper do it after the pending deletions.
    if !deep
        && let Some(bin_dir) = managed_bin_dir
        && bin_dir.is_dir()
        && fs::remove_dir(&bin_dir).is_err()
        && !pending_files.is_empty()
    {
        pending_dirs.push(bin_dir);
    }
}

/// One copy of dev-prune found somewhere other than the managed directory.
pub(crate) struct StrayCopy {
    pub(crate) path: PathBuf,
    pub(crate) channel: Channel,
}

/// Find every other copy of the pair, show the list, and — with the user's yes —
/// delete them all.
///
/// Discovery covers every directory on this process's PATH plus the well-known install
/// directories that are often *not* on it any more: `~/.cargo/bin`, `~/.local/bin`
/// (uv, pipx and the XDG convention), npm's global directory, pip's per-user `Scripts`
/// directories, and whatever directory the running executable lives in. Only files
/// carrying the pair's own names are ever considered, so nothing else in those
/// directories can be touched.
///
/// Deletion is opt-in: the list is printed and confirmed first (`--yes` counts as
/// confirmation; a non-terminal without it leaves everything in place). A declined
/// prompt is a decision, not a failure — it does not change the exit code.
///
/// What removal *means* depends on who owns the file, and there are three answers, not
/// two. A copy in a location nothing claims is deleted outright, because the file is the
/// whole install. A copy a manager installed is removed by that manager's own uninstall
/// — see [`Channel::uninstall_argv`] for why deleting it directly is worse than leaving
/// it. And a copy inside a manager dev-prune knows by name but cannot drive is named and
/// left, which is the same reasoning with no command at the end of it.
fn sweep_stray_copies(
    yes: bool,
    manager_hints: &mut Vec<Channel>,
    left_behind: &mut Vec<String>,
    pending_files: &mut Vec<PathBuf>,
    pending_commands: &mut Vec<Channel>,
) {
    // Anything already queued for the deletion helper still exists on disk right now;
    // finding it again here would list it as a stray and queue it twice.
    let already_pending: HashSet<String> = pending_files.iter().map(|p| canon_key(p)).collect();
    let strays: Vec<StrayCopy> = find_stray_copies()
        .into_iter()
        .filter(|s| !already_pending.contains(&canon_key(&s.path)))
        .collect();
    if strays.is_empty() {
        return;
    }

    println!();
    output::print_warning(&format!(
        "Found {} more cop{} of dev-prune, from other install channels:",
        strays.len(),
        if strays.len() == 1 { "y" } else { "ies" }
    ));
    for stray in &strays {
        if stray.channel.owns_its_files() {
            println!(
                "   {}  (installed with {})",
                output::clean_path(&stray.path),
                stray.channel.label()
            );
        } else {
            println!("   {}", output::clean_path(&stray.path));
        }
    }

    // Every manager whose copy turned up. Entries come off this list as each
    // manager's own uninstall runs or is scheduled; whatever is left over is printed
    // at the end for the user to finish by hand.
    for stray in &strays {
        if stray.channel.owns_its_files() && !manager_hints.contains(&stray.channel) {
            manager_hints.push(stray.channel);
        }
    }

    if !confirm_sweep(yes) {
        output::print_info(
            "Left in place. Remove them yourself, or re-run `devp uninstall` any time.",
        );
        return;
    }

    let running = std::env::current_exe().ok().map(|p| canon_key(&p));
    let mut removed = 0usize;
    for (channel, paths) in group_by_channel(strays) {
        let Some(argv) = channel.uninstall_argv() else {
            // Two channels have no command, and only one of them may be deleted. A
            // `Foreign` copy is inside somebody's package tree; removing the file would
            // leave that manager listing a binary that is gone, with no `uninstall_argv`
            // to repair it afterwards.
            if !channel.may_delete_directly() {
                let who = channel.label();
                output::print_warning(&format!(
                    "{who} installed {} of the copies above, and dev-prune does not know \
                     {who}'s uninstall command. Left in place: deleting the file would \
                     leave {who} listing a binary that is gone.",
                    paths.len()
                ));
                continue;
            }
            // Nobody holds a record of these, so the file is the whole install.
            for path in paths {
                match fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(e) => {
                        // The running executable itself is often in this list. Windows
                        // keeps it locked; the detached helper finishes the job.
                        if cfg!(windows) && is_in_use_error(&e) {
                            pending_files.push(path);
                        } else {
                            output::print_error(&format!(
                                "Could not remove {}: {e}",
                                output::clean_path(&path)
                            ));
                            left_behind.push("a stray copy".to_string());
                        }
                    }
                }
            }
            continue;
        };

        // Without the manager on PATH there is no way to clear its record, and deleting
        // the file would make the record unclearable. Leaving it is the only move that
        // keeps the machine recoverable.
        if !crate::adapters::binary_available(&argv[0]) {
            output::print_warning(&format!(
                "{} is not on PATH, so its copy was left in place.",
                channel.label()
            ));
            continue;
        }

        // Windows will not let a manager delete a binary that is executing, and there
        // is no way to get around it from inside that binary — see the note at the top
        // of this file. Scheduled, not skipped.
        if cfg!(windows) && paths.iter().any(|p| Some(canon_key(p)) == running) {
            pending_commands.push(channel);
            manager_hints.retain(|c| *c != channel);
            continue;
        }

        match run_manager_uninstall(&argv) {
            Ok(()) => {
                manager_hints.retain(|c| *c != channel);
                removed += paths.len();
                // Whatever the manager did not take is safe to delete now: its record
                // is clear, so the file is no longer part of an install.
                for path in paths {
                    if fs::symlink_metadata(&path).is_ok() {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
            Err(e) => {
                output::print_error(&format!("`{}` failed: {e:#}", argv.join(" ")));
                left_behind.push(format!("the {} copy", channel.label()));
            }
        }
    }
    if removed > 0 {
        output::print_info(&format!(
            "Removed {removed} stray cop{}.",
            if removed == 1 { "y" } else { "ies" }
        ));
    }
}

/// Group the strays by the manager that owns them, keeping discovery order.
///
/// One manager is told once, however many of its files turned up. `~/.cargo/bin` holds
/// both `dev-prune` and `devp`, and a second `cargo uninstall dev-prune` exits 101 with
/// "package ID specification did not match any packages" — a failure to report, from a
/// command that had in fact already worked.
pub(crate) fn group_by_channel(strays: Vec<StrayCopy>) -> Vec<(Channel, Vec<PathBuf>)> {
    let mut groups: Vec<(Channel, Vec<PathBuf>)> = Vec::new();
    for stray in strays {
        match groups.iter_mut().find(|(c, _)| *c == stray.channel) {
            Some((_, paths)) => paths.push(stray.path),
            None => groups.push((stray.channel, vec![stray.path])),
        }
    }
    groups
}

/// Run a package manager's own uninstall, wired to this terminal so its progress and
/// its errors are the user's to read.
fn run_manager_uninstall(argv: &[String]) -> Result<()> {
    output::print_info(&format!("Running: {}", argv.join(" ")));
    let status = crate::spawn::command(crate::adapters::resolve_program(&argv[0]))
        .args(&argv[1..])
        .status()
        .with_context(|| format!("could not start `{}`", argv[0]))?;
    if !status.success() {
        anyhow::bail!("exited with {status}");
    }
    Ok(())
}

/// Ask before the sweep deletes anything. `--yes` answers for the user; a pipe or a
/// script without it gets a "no" plus the flag to pass next time.
fn confirm_sweep(yes: bool) -> bool {
    use std::io::{IsTerminal, Write};
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        output::print_info("Not running in a terminal — pass `--yes` to remove these too.");
        return false;
    }
    // Default no, like every other deletion prompt in this tool: these files live in
    // directories dev-prune does not manage, and a reflexive Enter should never be
    // what deletes them. The question goes to stderr so a piped stdout cannot eat it.
    eprint!("Remove them all? [y/N]: ");
    if std::io::stderr().flush().is_err() {
        return false;
    }
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Every dev-prune/devp file in the sweep directories, except the managed pair (the
/// caller already removed it), development builds, and directories, deduplicated.
///
/// A dangling symlink still counts — it is exactly the kind of leftover the sweep
/// exists to clean up — which is why this checks `symlink_metadata`, not `is_file`.
pub(crate) fn find_stray_copies() -> Vec<StrayCopy> {
    let managed = setup::managed_bin_dir().ok().map(|d| canon_key(&d));
    let managed_exe = setup::managed_exe_path().ok();
    let names = sweep_names();
    let mut seen_dirs: HashSet<String> = HashSet::new();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut found = Vec::new();

    for dir in sweep_dirs() {
        let dir_key = canon_key(&dir);
        if !seen_dirs.insert(dir_key.clone()) {
            continue;
        }
        if managed.as_deref() == Some(dir_key.as_str()) {
            continue;
        }
        for name in &names {
            let candidate = dir.join(name);
            let Ok(meta) = fs::symlink_metadata(&candidate) else {
                continue;
            };
            if meta.is_dir() || is_dev_build(&candidate) {
                continue;
            }
            if !seen_files.insert(canon_key(&candidate)) {
                continue;
            }
            let channel = Channel::detect_at(&candidate, managed_exe.as_deref());
            found.push(StrayCopy {
                path: candidate,
                channel,
            });
        }
    }
    found
}

/// The directories worth looking in: everything on PATH, plus the install directories
/// each supported channel writes to — which stop being on PATH the moment a venv
/// deactivates or a profile line is removed, without the files going anywhere.
fn sweep_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(path_var) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path_var));
    }
    // Under hands-off the sweep stays inside directories the caller's own environment
    // names. `PATH` is the caller's to shape; the home-derived extras below are this
    // code guessing at install locations, which is exactly the reaching-around that
    // `DEV_PRUNE_NO_AUTO_SETUP` turns off — and what keeps the test suite out of the
    // developer's real `~/.cargo/bin`.
    if setup::no_auto_setup_requested() {
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            dirs.push(parent.to_path_buf());
        }
        return dirs;
    }
    dirs.extend(crate::channel::install_dirs(dirs::home_dir().as_deref()));
    if cfg!(windows) {
        // `config_dir` is %APPDATA% — pip's per-user scripts live under it, one
        // directory per interpreter version, so they have to be enumerated rather than
        // named.
        if let Some(appdata) = dirs::config_dir()
            && let Ok(entries) = fs::read_dir(appdata.join("Python"))
        {
            for entry in entries.flatten() {
                let scripts = entry.path().join("Scripts");
                if scripts.is_dir() {
                    dirs.push(scripts);
                }
            }
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        dirs.push(parent.to_path_buf());
    }
    dirs
}

/// The file names one of the pair can appear under. On Windows that is more than the
/// two `.exe`s: npm writes `.cmd` and `.ps1` shims plus an extensionless sh shim for
/// Git Bash, and each is a separate file to delete.
fn sweep_names() -> Vec<String> {
    // `devpw` is the windowless scheduler twin. It is a build target like the other
    // two, so `cargo install` puts it on PATH beside them and an uninstall that did not
    // look for it would leave a working copy of the whole CLI behind under a name
    // nobody thinks to check.
    let stems = ["dev-prune", "devp", "devpw"];
    if cfg!(windows) {
        let mut names: Vec<String> = Vec::new();
        for stem in stems {
            // `exe.old` is dev-prune's own debris: an update renames the running binary
            // aside so the channel can write a fresh one at the real name, and the
            // delete that follows is best-effort because the file is still the running
            // image. The next update sweeps it -- but somebody who updates once and then
            // uninstalls never has a next update, and the orphan outlives the install.
            // `.new` likewise: an update stages the downloaded bytes beside the target
            // and renames them in, and a stage that failed to rename is debris nothing
            // else ever looks at.
            for ext in ["exe", "cmd", "ps1", "bat", "exe.old", "new"] {
                names.push(format!("{stem}.{ext}"));
            }
            names.push(stem.to_string());
        }
        names
    } else {
        stems
            .iter()
            .flat_map(|s| [s.to_string(), format!("{s}.new")])
            .collect()
    }
}

/// One canonical string per path, so `C:\X\Bin\` and `c:\x\bin` count once.
pub(crate) fn canon_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    let key = key.trim_end_matches('/').to_string();
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

/// The on-disk file name for one of the pair, on this platform.
fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Whether a deletion failure means "in use right now" — the one case the detached
/// helper can finish. 5 is ERROR_ACCESS_DENIED, which is what deleting the running
/// image reports; 32 is ERROR_SHARING_VIOLATION. Anything else (read-only media, a
/// policy block) the helper would only inherit, so it is reported instead of queued.
fn is_in_use_error(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32))
}

/// Whether this executable is running out of a Cargo build directory.
fn is_dev_build(exe: &Path) -> bool {
    let path = exe.to_string_lossy().replace('\\', "/");
    path.contains("/target/debug/") || path.contains("/target/release/")
}

/// The install one-liner for this platform, for the goodbye message.
fn reinstall_hint() -> String {
    if cfg!(windows) {
        format!("iwr -useb {} | iex", crate::constants::INSTALL_PS1_URL)
    } else {
        format!("curl -fsSL {} | sh", crate::constants::INSTALL_SH_URL)
    }
}

/// List what could not be scheduled, with enough detail to act on, plus the command
/// that removes it.
///
/// The stray-copy sweep lists every path before it deletes anything; this is the same
/// courtesy for the residue. "Some files could not be removed" leaves someone hunting
/// through Program Files for a name they were never told, so each line carries the
/// name, the directory it sits in, what kind of thing it is and how big it is.
fn report_manual_removal(paths: &[PathBuf]) {
    output::print_warning(&format!(
        "{} item(s) were open in this process and could not be removed from inside it. \
         Delete them once this command has exited, or restart — from an elevated shell \
         Windows would have taken them at the next boot on its own.",
        paths.len()
    ));
    for path in paths {
        let meta = fs::symlink_metadata(path).ok();
        let kind = match meta.as_ref() {
            Some(m) if m.is_dir() => "directory".to_string(),
            Some(m) => format!("file, {}", output::format_bytes(m.len())),
            None => "already gone".to_string(),
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| output::clean_path(path));
        let parent = path
            .parent()
            .map(output::clean_path)
            .unwrap_or_else(|| "—".to_string());
        println!("    {name}  ({kind})");
        println!("      in {parent}");
    }
    println!("\n  Remove them yourself with:");
    for path in paths {
        // `-LiteralPath` and single quotes, because these are exactly the paths whose
        // `%` the fallback could not survive — the command printed here has to be one
        // that can be pasted verbatim.
        println!(
            "    Remove-Item -LiteralPath '{}' -Recurse -Force",
            path.display().to_string().replace('\'', "''")
        );
    }
}

/// What became of one file that Windows would not let this process delete.
#[cfg(windows)]
enum Retired {
    /// Gone, or queued with the session manager for the next boot.
    Handled,
    /// Renamed out of the way. Still on disk, but nothing resolves to it any more.
    Aside(PathBuf),
    /// Still sitting at the name it was installed under.
    Stuck(PathBuf),
}

/// Finish, from inside this process, the removals Windows refused while the binary was
/// executing -- and report, rather than defer, whatever is left.
///
/// Returns the paths still on disk, and whether any of them is still at the name it was
/// installed under. That second value is the difference between an uninstall that
/// worked and one that did not: a file renamed aside resolves to nothing, is on the
/// sweep list, and is cleared by the next install; a file still at `devp.exe` is a
/// working install that was asked to go away and did not.
///
/// Nothing here starts a child process, and that is deliberate. The obvious way to
/// delete a running executable on Windows is to leave a shell behind to do it once you
/// have exited: a `cmd /C` line that pings itself to pass the time and then deletes the
/// file is the canonical form, and a hidden PowerShell running `Start-Sleep` before
/// `Remove-Item -Recurse -Force` is the same idea in better clothes. Both are also the
/// canonical *malware* self-delete. The literal strings are enough for a static scanner
/// to score on, and spawning either with `CREATE_NO_WINDOW` out of an unsigned binary
/// is enough for a behavioural one. dev-prune has been quarantined for less, so it does
/// neither, and what is left is what Windows itself offers for a file that is in use:
///
/// 1. Rename the locked image aside. A running executable cannot be deleted, but it
///    can be renamed -- the handle follows the file, not the name -- so the name is
///    free at once and a reinstall is never blocked by the copy it replaces.
/// 2. Ask the session manager to delete the residue at the next boot, through
///    `MoveFileEx` with `MOVEFILE_DELAY_UNTIL_REBOOT`.
/// 3. Report what survived both, with the command that finishes it by hand.
#[cfg(windows)]
fn finish_locked_removals(files: &[PathBuf], dirs: &[PathBuf]) -> (Vec<PathBuf>, bool) {
    let mut residue: Vec<PathBuf> = Vec::new();
    let mut stuck = false;
    for file in files {
        match retire_file(file) {
            Retired::Handled => {}
            Retired::Aside(path) => residue.push(path),
            Retired::Stuck(path) => {
                residue.push(path);
                stuck = true;
            }
        }
    }
    for dir in dirs {
        stuck |= purge_tree(dir, &mut residue);
    }
    (residue, stuck)
}

/// The name a locked file is renamed to: `devp.exe` becomes `devp.exe.old`.
///
/// That name is already on the sweep list, so the residue is cleared by the next
/// install or the next uninstall without anyone having to be told about it.
#[cfg(windows)]
fn aside_name(path: &Path) -> PathBuf {
    match path.extension() {
        Some(ext) => path.with_extension(format!("{}.old", ext.to_string_lossy())),
        None => path.with_extension("old"),
    }
}

/// Delete one file; failing that, get its name out of the way and queue the rest.
#[cfg(windows)]
fn retire_file(path: &Path) -> Retired {
    if fs::remove_file(path).is_ok() || fs::symlink_metadata(path).is_err() {
        return Retired::Handled;
    }
    let aside = aside_name(path);
    let Ok(()) = fs::rename(path, &aside) else {
        // The rename is the part that matters, so its failure is the real one: the
        // install is still resolvable at this name.
        return if delete_on_reboot(path) {
            Retired::Handled
        } else {
            Retired::Stuck(path.to_path_buf())
        };
    };
    if delete_on_reboot(&aside) {
        Retired::Handled
    } else {
        Retired::Aside(aside)
    }
}

/// Remove a directory tree, carrying on past the entries that are locked.
///
/// `fs::remove_dir_all` stops at the first error, which here is reliably the running
/// binary -- so everything beside it would be left standing for no reason. Returns
/// whether any file inside is still at its installed name; the directory itself
/// surviving is residue to report, not an install that refused to go.
#[cfg(windows)]
fn purge_tree(dir: &Path, residue: &mut Vec<PathBuf>) -> bool {
    if fs::remove_dir_all(dir).is_ok() || fs::symlink_metadata(dir).is_err() {
        return false;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        residue.push(dir.to_path_buf());
        return false;
    };
    let mut stuck = false;
    for entry in entries.flatten() {
        let path = entry.path();
        // The entry's own type, never the target's: a link here is removed, not
        // followed. A directory link needs `remove_dir` where a file link needs
        // `remove_file`, and neither touches whatever it points at.
        match entry.file_type() {
            Ok(t) if t.is_symlink() => {
                if fs::remove_file(&path).is_err() && fs::remove_dir(&path).is_err() {
                    residue.push(path);
                }
            }
            Ok(t) if t.is_dir() => stuck |= purge_tree(&path, residue),
            _ => match retire_file(&path) {
                Retired::Handled => {}
                Retired::Aside(left) => residue.push(left),
                Retired::Stuck(left) => {
                    residue.push(left);
                    stuck = true;
                }
            },
        }
    }
    if fs::remove_dir(dir).is_ok() {
        return stuck;
    }
    // Something inside is still open. The reboot queue is replayed in the order it was
    // written and every file above was queued first, so this directory is empty by the
    // time its own entry is read.
    if !delete_on_reboot(dir) {
        residue.push(dir.to_path_buf());
    }
    stuck
}

/// Queue a path with the session manager for deletion at the next boot.
///
/// `MoveFileEx` with a null destination and `MOVEFILE_DELAY_UNTIL_REBOOT` is what
/// Windows offers for a file that is in use, and what an installer replacing one is
/// expected to use. It appends to `PendingFileRenameOperations` under `HKLM`, so it
/// needs administrator rights: from an elevated shell the residue is gone after the
/// next restart, and from an ordinary one this fails and the caller reports the path
/// instead. Nothing is left running either way.
#[cfg(windows)]
fn delete_on_reboot(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call, and a null destination is
    // the documented spelling of "delete this" for this function.
    unsafe { MoveFileExW(wide.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) != 0 }
}

/// On Unix an open file can be unlinked, so nothing is ever left locked; this exists so
/// the call site compiles unconditionally and is unreachable in practice.
#[cfg(not(windows))]
fn finish_locked_removals(_files: &[PathBuf], _dirs: &[PathBuf]) -> (Vec<PathBuf>, bool) {
    (Vec::new(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep has to know the name the update leaves behind, or dev-prune's own
    /// debris outlives the uninstall that reported it gone.
    #[test]
    #[cfg(windows)]
    fn the_sweep_looks_for_the_file_an_update_renames_aside() {
        let names = sweep_names();
        assert!(names.contains(&"devp.exe.old".to_string()), "{names:?}");
        assert!(
            names.contains(&"dev-prune.exe.old".to_string()),
            "{names:?}"
        );
    }

    /// Only Windows needs the rename-aside; elsewhere the sweep is the three stems plus
    /// the `.new` staging name an interrupted update can leave beside any of them.
    #[test]
    #[cfg(not(windows))]
    fn elsewhere_the_sweep_is_the_stems_and_their_staging_names() {
        assert_eq!(
            sweep_names(),
            [
                "dev-prune",
                "dev-prune.new",
                "devp",
                "devp.new",
                "devpw",
                "devpw.new"
            ]
        );
    }

    /// The residue has to land on a name the sweep already knows, or an uninstall that
    /// could not delete the running image leaves a file nothing will ever clear.
    #[test]
    #[cfg(windows)]
    fn the_name_a_locked_binary_is_renamed_to_is_one_the_sweep_looks_for() {
        let aside = aside_name(Path::new(r"C:\bin\devp.exe"));
        assert_eq!(aside.file_name().unwrap(), "devp.exe.old");
        assert!(sweep_names().contains(&"devp.exe.old".to_string()));
        // A name with no extension at all still gets one, rather than being left alone
        // under the name the install resolves to.
        assert_eq!(
            aside_name(Path::new(r"C:\bin\devp")).file_name().unwrap(),
            "devp.old"
        );
    }

    /// Nothing is locked in a test, so every path here is the one that just deletes the
    /// file — which is the case that has to stay cheap and silent.
    #[test]
    #[cfg(windows)]
    fn an_unlocked_file_is_simply_deleted_and_no_residue_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("devp.exe");
        std::fs::write(&file, b"binary").unwrap();

        let (residue, still_installed) = finish_locked_removals(std::slice::from_ref(&file), &[]);

        assert!(residue.is_empty(), "{residue:?}");
        assert!(!still_installed);
        assert!(!file.exists());
        assert!(
            !aside_name(&file).exists(),
            "nothing should be renamed aside"
        );
    }

    /// `remove_dir_all` stops at the first entry it cannot take; the tree walk must not,
    /// or one locked binary strands every unrelated file beside it.
    #[test]
    #[cfg(windows)]
    fn a_tree_with_nothing_locked_in_it_goes_completely() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("dev-prune");
        std::fs::create_dir_all(config.join("bin")).unwrap();
        std::fs::write(config.join("bin").join("devp.exe"), b"binary").unwrap();
        std::fs::write(config.join("registry.json"), b"{}").unwrap();

        let (residue, still_installed) = finish_locked_removals(&[], std::slice::from_ref(&config));

        assert!(residue.is_empty(), "{residue:?}");
        assert!(!still_installed);
        assert!(!config.exists());
    }

    /// A directory that is already gone is the normal case after a light uninstall, and
    /// must not be reported as something the user has to clean up.
    #[test]
    #[cfg(windows)]
    fn a_path_that_is_already_gone_is_not_reported() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("never-existed");

        let (residue, still_installed) =
            finish_locked_removals(&[missing.join("devp.exe")], &[missing]);

        assert!(residue.is_empty(), "{residue:?}");
        assert!(!still_installed);
    }
}
