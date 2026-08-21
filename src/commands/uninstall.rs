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
// On Windows a running executable cannot delete itself, so whatever is still in use is
// handed to a detached PowerShell (or `cmd.exe`) helper that waits for this process to
// exit and then deletes it. That is scheduled work, not failure — the command
// reports it and exits `0`.

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
    let manager = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(owning_package_manager);
    remove_binaries(
        deep,
        manager.is_some(),
        &mut left_behind,
        &mut pending_files,
        &mut pending_dirs,
    );

    // 7. Every other copy on the machine. A machine that has tried more than one
    // install channel has more than one binary, and the ones not currently first on
    // PATH would quietly *become* the installation the moment the managed pair above
    // is gone.
    let mut manager_hints: Vec<(&'static str, &'static str)> = Vec::new();
    if let Some(hint) = manager {
        manager_hints.push(hint);
    }
    sweep_stray_copies(
        yes,
        &mut manager_hints,
        &mut left_behind,
        &mut pending_files,
    );

    if deep {
        // Per-repo configs, then the config directory itself.
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
    }

    // 8. One detached helper for everything that is in use right now. PowerShell is
    // preferred: its single-quoted string literals are fully literal, so a path
    // carrying `%` survives, where `cmd.exe` would expand it and a `/C` command line
    // has no way to escape one. `cmd.exe` remains the fallback for a machine without
    // PowerShell, and whatever neither could take is listed for manual removal.
    let leftover = spawn_deletion_helper(&pending_files, &pending_dirs);
    if !pending_files.is_empty() || !pending_dirs.is_empty() {
        if leftover.len() < pending_files.len() + pending_dirs.len() {
            output::print_info(
                "The running binary cannot delete itself — the rest is removed \
                 automatically a few seconds after this command exits.",
            );
        }
        if !leftover.is_empty() {
            report_manual_removal(&leftover);
            left_behind.push("the binaries".to_string());
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
    for (name, command) in &manager_hints {
        output::print_info(&format!(
            "{name} still lists dev-prune as installed — finish with `{command}` to clear \
             its records."
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
        // The windowless scheduler twin, generated beside the managed binary on Windows
        // and nowhere else. `WINDOWS_HIDDEN_BIN` already carries its `.exe`.
        #[cfg(windows)]
        candidates.push(bin_dir.join(crate::constants::WINDOWS_HIDDEN_BIN));
    }

    if let Ok(current) = std::env::current_exe() {
        if is_dev_build(&current) {
            output::print_info(
                "This is a development build — leaving the `target/` binaries alone.",
            );
        } else if manager_owned {
            // The caller prints the manager's own uninstall command.
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
struct StrayCopy {
    path: PathBuf,
    manager: Option<(&'static str, &'static str)>,
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
fn sweep_stray_copies(
    yes: bool,
    manager_hints: &mut Vec<(&'static str, &'static str)>,
    left_behind: &mut Vec<String>,
    pending_files: &mut Vec<PathBuf>,
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
        match stray.manager {
            Some((name, _)) => println!(
                "   {}  (installed with {name})",
                output::clean_path(&stray.path)
            ),
            None => println!("   {}", output::clean_path(&stray.path)),
        }
    }

    if !confirm_sweep(yes) {
        output::print_info(
            "Left in place. Remove them yourself, or re-run `devp uninstall` any time.",
        );
        return;
    }

    let mut removed = 0usize;
    for stray in strays {
        if let Some(hint) = stray.manager
            && !manager_hints.iter().any(|(name, _)| *name == hint.0)
        {
            manager_hints.push(hint);
        }
        match fs::remove_file(&stray.path) {
            Ok(()) => removed += 1,
            Err(e) => {
                // The running executable itself is often in this list. Windows keeps
                // it locked; the detached helper finishes the job.
                if cfg!(windows) && is_in_use_error(&e) {
                    pending_files.push(stray.path);
                } else {
                    output::print_error(&format!(
                        "Could not remove {}: {e}",
                        output::clean_path(&stray.path)
                    ));
                    left_behind.push("a stray copy".to_string());
                }
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
fn find_stray_copies() -> Vec<StrayCopy> {
    let managed = setup::managed_bin_dir().ok().map(|d| canon_key(&d));
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
            let manager = owning_package_manager(&candidate);
            found.push(StrayCopy {
                path: candidate,
                manager,
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
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".cargo").join("bin"));
        dirs.push(home.join(".local").join("bin"));
        if !cfg!(windows) {
            dirs.push(home.join(".npm-global").join("bin"));
        }
    }
    if cfg!(windows) {
        // `config_dir` is %APPDATA% — npm's global prefix and pip's per-user scripts
        // both live under it.
        if let Some(appdata) = dirs::config_dir() {
            dirs.push(appdata.join("npm"));
            if let Ok(entries) = fs::read_dir(appdata.join("Python")) {
                for entry in entries.flatten() {
                    let scripts = entry.path().join("Scripts");
                    if scripts.is_dir() {
                        dirs.push(scripts);
                    }
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
    let stems = ["dev-prune", "devp"];
    if cfg!(windows) {
        let mut names: Vec<String> = Vec::new();
        for stem in stems {
            for ext in ["exe", "cmd", "ps1", "bat"] {
                names.push(format!("{stem}.{ext}"));
            }
            names.push(stem.to_string());
        }
        names
    } else {
        stems.iter().map(|s| s.to_string()).collect()
    }
}

/// One canonical string per path, so `C:\X\Bin\` and `c:\x\bin` count once.
fn canon_key(path: &Path) -> String {
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

/// The package manager that owns the running executable's files, if one does, and the
/// command that actually uninstalls through it.
///
/// dev-prune ships through cargo, npm, PyPI and uv as well as the installer scripts.
/// Deleting files out from under one of those managers leaves *its* records pointing at
/// nothing — `pip list` still shows the package, `cargo install` refuses to reinstall —
/// so those copies are left for the manager's own uninstall, which is printed instead.
fn owning_package_manager(exe: &Path) -> Option<(&'static str, &'static str)> {
    let path = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    if path.contains("/.cargo/bin/") {
        return Some(("cargo", "cargo uninstall dev-prune"));
    }
    if path.contains("/node_modules/") || path.contains("/_npx/") {
        return Some(("npm", "npm uninstall -g dev-prune"));
    }
    if path.contains("/uv/tools/") {
        return Some(("uv", "uv tool uninstall dev-prune"));
    }
    if path.contains("/pipx/") {
        return Some(("pipx", "pipx uninstall dev-prune"));
    }
    if let Some(dir) = exe.parent() {
        // npm's global shims sit *beside* its `node_modules`, not inside it.
        if dir.join("node_modules").join("dev-prune").exists() {
            return Some(("npm", "npm uninstall -g dev-prune"));
        }
        // pip puts console scripts beside a Python interpreter — the system
        // `Scripts`/`bin` directory or a virtualenv's.
        for interpreter in ["python.exe", "python", "python3"] {
            if dir.join(interpreter).exists() {
                return Some(("pip", "pip uninstall dev-prune"));
            }
        }
    }
    None
}

/// The install one-liner for this platform, for the goodbye message.
fn reinstall_hint() -> &'static str {
    if cfg!(windows) {
        "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"
    } else {
        "curl -fsSL https://devprune.vkrishna04.me/install.sh | sh"
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
    output::print_error(&format!(
        "{} item(s) are still in use and could not be scheduled for removal.",
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

/// Quote a path as a PowerShell single-quoted string literal.
///
/// Inside single quotes PowerShell expands nothing at all — not `$var`, not a backtick
/// escape, and crucially not `%VAR%`. The only character with meaning is the closing
/// quote, and doubling it is the documented way to write a literal one. That makes this
/// a complete escape rule for an arbitrary path, which is exactly what `cmd /C` could
/// not offer.
#[cfg(windows)]
fn ps_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// Schedule the in-use files for deletion after this process exits.
///
/// Returns the paths that could not be handed over, which is empty in the normal case.
///
/// PowerShell rather than `cmd.exe`, because `cmd` expands `%VAR%` even inside double
/// quotes and a `/C` command line has no escape for a literal `%`. A path carrying one
/// therefore could not be passed at all: it used to be reported and left on disk. The
/// `cmd` route survives only as the fallback for a machine where PowerShell cannot be
/// launched, and there the old restriction still applies.
#[cfg(windows)]
fn spawn_deletion_helper(files: &[PathBuf], dirs: &[PathBuf]) -> Vec<PathBuf> {
    if files.is_empty() && dirs.is_empty() {
        return Vec::new();
    }
    if spawn_powershell_helper(files, dirs) {
        return Vec::new();
    }

    // Fallback. `cmd` cannot be given a literal `%`, so those paths stay behind and are
    // returned for the caller to report.
    let has_percent = |p: &&PathBuf| p.to_string_lossy().contains('%');
    let left_behind: Vec<PathBuf> = files
        .iter()
        .chain(dirs.iter())
        .filter(has_percent)
        .cloned()
        .collect();
    let safe_files: Vec<PathBuf> = files.iter().filter(|p| !has_percent(p)).cloned().collect();
    let safe_dirs: Vec<PathBuf> = dirs.iter().filter(|p| !has_percent(p)).cloned().collect();

    if (safe_files.is_empty() && safe_dirs.is_empty()) || spawn_cmd_helper(&safe_files, &safe_dirs)
    {
        left_behind
    } else {
        files.iter().chain(dirs.iter()).cloned().collect()
    }
}

/// The PowerShell form of the retry loop. `true` if the helper was launched.
#[cfg(windows)]
fn spawn_powershell_helper(files: &[PathBuf], dirs: &[PathBuf]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut attempt = String::new();
    for file in files {
        attempt.push_str(&format!(
            "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue; ",
            ps_quote(file)
        ));
    }
    for dir in dirs {
        attempt.push_str(&format!(
            "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue; ",
            ps_quote(dir)
        ));
    }

    // Three attempts, two seconds apart — the same reasoning as the `cmd` loop below.
    let mut script = String::new();
    for _ in 0..3 {
        script.push_str("Start-Sleep -Seconds 2; ");
        script.push_str(&attempt);
    }

    // Windows PowerShell 5.1 ships with every supported Windows and lives at a fixed
    // place, so it is tried by absolute path first. The rest cover the machines where
    // it does not answer — Nano Server, an image built without the Windows PowerShell
    // feature, or a policy that blocks the inbox copy while permitting PowerShell 7 —
    // and those are found through `PATH`, because 7.x installs beside its own major
    // version rather than into `System32`.
    for program in [
        crate::spawn::system32(r"WindowsPowerShell\v1.0\powershell.exe"),
        String::from("pwsh.exe"),
        String::from("pwsh-preview.exe"),
        String::from("powershell.exe"),
    ] {
        let spawned = std::process::Command::new(&program)
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(&script)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        if spawned {
            return true;
        }
    }
    false
}

/// The original `cmd.exe` form, kept as the fallback. Callers must have filtered out
/// any path containing `%` before calling this.
#[cfg(windows)]
fn spawn_cmd_helper(files: &[PathBuf], dirs: &[PathBuf]) -> bool {
    use std::os::windows::process::CommandExt;
    // Not in windows-sys's prelude of imported constants anywhere else in this crate;
    // documented value of CREATE_NO_WINDOW.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Three attempts, two seconds apart. One would cover the normal case — this
    // process exits the moment the command returns, releasing the image lock — but a
    // slow exit, an antivirus scan hooked on process teardown, or the user running
    // `devp` again inside the first window would otherwise leave the binary behind
    // with nothing ever retrying. `cmd /C` cannot use labels, so the loop is unrolled.
    let mut attempt = String::new();
    for file in files {
        attempt.push_str(&format!(" & del /F /Q \"{}\"", file.display()));
    }
    for dir in dirs {
        attempt.push_str(&format!(" & rmdir /S /Q \"{}\"", dir.display()));
    }
    let mut script = String::new();
    for _ in 0..3 {
        script.push_str("ping -n 3 127.0.0.1 >nul");
        script.push_str(&attempt);
        script.push_str(" & ");
    }
    script.push_str("exit");

    std::process::Command::new(crate::spawn::system32("cmd.exe"))
        // `raw_arg`, because std's quoting would wrap the whole script in quotes and
        // `cmd /C` would then treat it as one file name rather than a command line.
        .raw_arg(format!("/C {script}"))
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// On Unix an open file can be unlinked, so nothing ever needs scheduling; this exists
/// so the call site compiles unconditionally and is unreachable in practice.
#[cfg(not(windows))]
fn spawn_deletion_helper(_files: &[PathBuf], _dirs: &[PathBuf]) -> Vec<PathBuf> {
    Vec::new()
}
