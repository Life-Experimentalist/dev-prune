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
/// Every child the CLI spawns goes through here. The one deliberate exception is the
/// uninstaller's deletion helper, which manages its own creation flags because it must
/// outlive this process.
pub fn command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    apply_window_policy(&mut cmd);
    cmd
}

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
