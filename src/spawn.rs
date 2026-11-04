// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Building child processes that stay invisible when this process is.
//
// On Windows a console-subsystem child of a process that has no console — the scheduled
// `devpw` pass, or anything launched from a GUI — gets a brand-new console window of its
// own, visible to whoever is logged in. A background prune that spawns `git` per
// repository would flash a black window per repository, which is exactly the behaviour
// the windowless scheduler binary exists to prevent. `CREATE_NO_WINDOW` suppresses the
// allocation; it is applied only when this process has no console, so interactive runs
// spawn children exactly as they always did.

use std::ffi::OsStr;
use std::process::Command;

/// A `Command` that will not flash a console window when this process has none.
///
/// Every child that can be spawned on Windows is built here. The launchd and systemd
/// calls go straight to `Command::new`, which is the same thing there: the policy
/// below is a no-op off Windows, and neither platform has a console to inherit.
///
/// What holds on every platform is the other half. Every child is waited on, and
/// nothing in this program starts a process that outlives the command that started
/// it -- there is no detached helper anywhere, which is deliberate and is checked by
/// grepping for `.spawn()`.
pub fn command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    apply_window_policy(&mut cmd);
    keep_cwd_out_of_the_search(&mut cmd);
    cmd
}

/// Stop `cmd.exe` looking in the working directory for the programs a child launches.
///
/// `pnpm.cmd` and `yarn.cmd` are batch shims, and a shim that cannot find `node` beside
/// itself runs a bare `node`. `cmd.exe` searches the current directory before `PATH`, and
/// the current directory here is the repository being verified, so a `node.cmd` committed
/// to a project would run the moment dev-prune asked its package manager whether a
/// lockfile is in sync. The variable is the documented switch for exactly this; the other
/// platforms never search the working directory, so it is a no-op there.
#[cfg(windows)]
fn keep_cwd_out_of_the_search(cmd: &mut Command) {
    cmd.env("NoDefaultCurrentDirectoryInExePath", "1");
}

#[cfg(not(windows))]
fn keep_cwd_out_of_the_search(_cmd: &mut Command) {}

#[cfg(windows)]
fn apply_window_policy(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    use std::sync::OnceLock;

    /// Documented value of `CREATE_NO_WINDOW`.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // A process gains or loses its console only through calls this crate never makes,
    // so the answer cannot change over a run.
    static HAS_CONSOLE: OnceLock<bool> = OnceLock::new();
    let has_console = *HAS_CONSOLE.get_or_init(|| {
        // SAFETY: `GetConsoleWindow` reads process state and takes no arguments.
        !unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() }.is_null()
    });
    if !has_console {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(not(windows))]
fn apply_window_policy(_cmd: &mut Command) {}
