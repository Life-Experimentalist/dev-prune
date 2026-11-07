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

/// Run `cmd` to completion and capture its output, killing it if it outlives `deadline`.
///
/// For the read-only questions put to `git`. A `git` that blocks on a stalled network
/// drive or a locked index never exits, and `Command::output` waits for it forever; on
/// Windows the scheduled task's `IgnoreNew` policy then skips every later run behind the
/// hung one. This is a ceiling on one stalled child, not a limit on how long a scan may
/// take: a scan that keeps making progress is made of many calls that each finish.
///
/// The pipe readers are left to finish on their own after a kill rather than joined. A
/// grandchild that inherited the pipe keeps it open past the kill, and joining would
/// bring the hang back one step later. Expiry is reported as `ErrorKind::TimedOut`.
pub fn output_within(
    mut cmd: Command,
    deadline: std::time::Duration,
) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let start = Instant::now();
    // Short naps at first, because a healthy `git rev-parse` is done in a few
    // milliseconds and a fixed 100 ms poll would multiply a 188-repository scan's wall
    // time by the poll interval.
    let mut nap = Duration::from_millis(1);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("did not finish within {}s", deadline.as_secs()),
            ));
        }
        std::thread::sleep(nap);
        nap = (nap * 2).min(Duration::from_millis(50));
    };

    Ok(std::process::Output {
        status,
        stdout: stdout.recv().unwrap_or_default(),
        stderr: stderr.recv().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A child that would run for half a minute if nobody stopped it.
    fn sleeper() -> Command {
        #[cfg(windows)]
        {
            let mut cmd = command("powershell");
            cmd.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = command("sleep");
            cmd.arg("30");
            cmd
        }
    }

    #[test]
    fn a_child_that_outlives_the_deadline_is_killed_and_reported() {
        let started = Instant::now();
        let err = output_within(sleeper(), Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the stalled child must be cut off, not waited for"
        );
    }

    #[test]
    fn a_child_that_finishes_in_time_returns_its_output() {
        let mut cmd = command("git");
        cmd.arg("--version");
        let out = output_within(cmd, Duration::from_secs(60)).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("git version"));
    }
}
