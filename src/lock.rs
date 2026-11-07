// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The run lock: one devp at a time may prune, restore, or rewrite the registry for a
// whole pass on a device.
//
// Before this existed the scheduled task, a manual `devp run`, a git hook and
// `devp restore` could all run at once. Registry writes are atomic, but the last writer
// wins, so one process silently lost the other's updates, and two passes could work on
// the same repository. Two installs of different versions made it worse, because neither
// knew the other existed.
//
// The lock is an operating system advisory lock on a file in the config directory
// (`LockFileEx` on Windows, `flock` elsewhere) rather than a file whose mere existence
// means "busy". The OS drops it when the holder dies, however it dies (a crash, a kill, a
// power cut that is followed by a reboot), so there is no stale lock to detect, no PID
// reuse to be fooled by, and nothing for the next run to clean up. The holder also writes
// its pid, version and start time into the file, but only so a refused run can say who
// it is waiting on; that text is never what decides whether the lock is held.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::constants;

/// How a busy lock is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// The scheduled pass or a git hook: nobody is watching, so it steps aside with one
    /// line and exits `0`. Failing would make the scheduler report an error for what is
    /// the system working as designed.
    Unattended,
    /// A person ran it: they are told who holds the lock and the command exits `1`, so a
    /// script does not mistake "did nothing" for "done".
    Interactive,
}

/// Who holds the lock, as the holder wrote it. Every field is optional because the file
/// can be read in the instant between the holder taking the lock and writing its details.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holder {
    pub pid: Option<u32>,
    pub version: Option<String>,
    pub since: Option<String>,
}

impl Holder {
    fn parse(text: &str) -> Self {
        let mut holder = Holder::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                constants::RUN_LOCK_KEY_PID => holder.pid = value.parse().ok(),
                constants::RUN_LOCK_KEY_VERSION if !value.is_empty() => {
                    holder.version = Some(value.to_string());
                }
                constants::RUN_LOCK_KEY_SINCE if !value.is_empty() => {
                    holder.since = Some(value.to_string());
                }
                _ => {}
            }
        }
        holder
    }

    /// `pid 4242, devp 1.24.0, since 2026-11-08T01:30:00Z`, or what is known of it.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(pid) = self.pid {
            parts.push(format!("pid {pid}"));
        }
        if let Some(version) = &self.version {
            parts.push(format!("devp {version}"));
        }
        if let Some(since) = &self.since {
            parts.push(format!("since {since}"));
        }
        if parts.is_empty() {
            "holder details not written yet".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// The lock, held until this value is dropped.
#[derive(Debug)]
pub struct RunLock {
    file: File,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        // Explicit rather than left to the handle closing: on Windows a lock is released
        // "at some point" after the process or handle goes away, and the next run should
        // not have to guess when. A dead process is still covered by the OS.
        let _ = platform::unlock(&self.file);
    }
}

/// What asking for the lock produced.
#[derive(Debug)]
pub enum Taken {
    Held(RunLock),
    Busy(Holder),
}

/// Where the lock file lives, under the config directory.
pub fn lock_path(config_dir: &Path) -> PathBuf {
    config_dir.join(constants::RUN_LOCK_FILENAME)
}

/// Try to take the lock in `config_dir` without waiting.
pub fn try_take(config_dir: &Path) -> io::Result<Taken> {
    fs::create_dir_all(config_dir)?;
    let path = lock_path(config_dir);
    // No truncate: the file's text belongs to the current holder until we own the lock.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;

    if !platform::lock(&file)? {
        let text = fs::read_to_string(&path).unwrap_or_default();
        return Ok(Taken::Busy(Holder::parse(&text)));
    }

    let details = format!(
        "{}={}\n{}={}\n{}={}\n",
        constants::RUN_LOCK_KEY_PID,
        std::process::id(),
        constants::RUN_LOCK_KEY_VERSION,
        constants::VERSION,
        constants::RUN_LOCK_KEY_SINCE,
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    // A failure here only costs the next refused run its explanation.
    let _ = file.set_len(0);
    let _ = (&file).write_all(details.as_bytes());
    let _ = file.sync_data();
    Ok(Taken::Held(RunLock { file }))
}

/// Run `body` while holding the run lock in `config_dir`.
///
/// If the lock is busy `body` does not run: an [`Pass::Unattended`] caller gets one info
/// line and `Ok`, an [`Pass::Interactive`] one gets an error naming the holder. When the
/// lock itself cannot be used (a config directory that cannot be created, a file system
/// without advisory locks) `body` runs unguarded: a lock that cannot be taken must not
/// stop the scheduler from ever pruning again, and the registry's own atomic writes are
/// what protected it before this existed. `json` moves the unattended line to stderr so
/// it cannot corrupt a document on stdout.
pub fn run_exclusive_in<T>(
    config_dir: &Path,
    pass: Pass,
    json: bool,
    body: impl FnOnce() -> Result<T>,
) -> Result<Option<T>> {
    match try_take(config_dir) {
        Ok(Taken::Held(_guard)) => body().map(Some),
        Ok(Taken::Busy(holder)) => match pass {
            Pass::Unattended => {
                let line = format!(
                    "Skipped: another devp is already running a pass on this device ({}). \
                     Nothing was changed; the next scheduled pass will pick up.",
                    holder.describe()
                );
                if json {
                    eprintln!("{line}");
                } else {
                    crate::output::print_info(&line);
                }
                Ok(None)
            }
            Pass::Interactive => Err(busy_error(&holder)),
        },
        Err(_) => body().map(Some),
    }
}

/// The refusal an interactive caller gets when someone else holds the lock.
fn busy_error(holder: &Holder) -> anyhow::Error {
    anyhow::anyhow!(
        "Another devp is already running a pass on this device ({}). \
         Wait for it to finish, then run this again.",
        holder.describe()
    )
}

/// Take the lock for the rest of the caller's scope, for code whose body is too tangled
/// to wrap in a closure (the dashboard's prune, which returns early from many places).
///
/// `Ok(None)` means the lock could not be used at all and the caller proceeds unguarded,
/// for the same reason [`run_exclusive_in`] does. A busy lock is an error.
pub fn acquire_interactive() -> Result<Option<RunLock>> {
    let Ok(dir) = crate::config::Registry::config_dir() else {
        return Ok(None);
    };
    match try_take(&dir) {
        Ok(Taken::Held(lock)) => Ok(Some(lock)),
        Ok(Taken::Busy(holder)) => Err(busy_error(&holder)),
        Err(_) => Ok(None),
    }
}

/// [`run_exclusive_in`] for the real config directory.
///
/// A skipped unattended run returns `Ok(())`, which is what the scheduler and the git
/// hook need to see.
pub fn run_exclusive(pass: Pass, json: bool, body: impl FnOnce() -> Result<()>) -> Result<()> {
    match crate::config::Registry::config_dir() {
        Ok(dir) => run_exclusive_in(&dir, pass, json, body).map(|_| ()),
        Err(_) => body(),
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    /// `ERROR_IO_PENDING`: some network redirectors answer a refused immediate lock with
    /// this instead of `ERROR_LOCK_VIOLATION`. Both mean "someone else has it".
    const ERROR_IO_PENDING: i32 = 997;

    /// A byte far past the end of the file. Windows byte-range locks also block other
    /// handles from reading the locked bytes, and the whole point of the file's text is
    /// that a refused run can read it while the holder keeps the lock.
    const LOCK_OFFSET_HIGH: u32 = 0x4000;

    fn overlapped() -> OVERLAPPED {
        let mut overlapped = OVERLAPPED::default();
        overlapped.Anonymous.Anonymous.OffsetHigh = LOCK_OFFSET_HIGH;
        overlapped
    }

    pub fn lock(file: &File) -> io::Result<bool> {
        let mut overlapped = overlapped();
        // SAFETY: the handle is valid for the life of `file`, and `overlapped` outlives
        // the call.
        let ok = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        };
        if ok != 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == ERROR_LOCK_VIOLATION as i32 || code == ERROR_IO_PENDING => {
                Ok(false)
            }
            _ => Err(err),
        }
    }

    pub fn unlock(file: &File) -> io::Result<()> {
        let mut overlapped = overlapped();
        // SAFETY: as in `lock`.
        let ok = unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::unix::io::AsRawFd;

    pub fn lock(file: &File) -> io::Result<bool> {
        // SAFETY: the descriptor is valid for the life of `file`.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(err)
        }
    }

    pub fn unlock(file: &File) -> io::Result<()> {
        // SAFETY: as in `lock`.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use std::fs::File;
    use std::io;

    pub fn lock(_file: &File) -> io::Result<bool> {
        Ok(true)
    }

    pub fn unlock(_file: &File) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn held(dir: &Path) -> RunLock {
        match try_take(dir).expect("lock file opens") {
            Taken::Held(lock) => lock,
            Taken::Busy(holder) => panic!("expected to take the lock, busy: {holder:?}"),
        }
    }

    #[test]
    fn a_second_holder_is_refused_and_told_who_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let _first = held(dir.path());

        match try_take(dir.path()).unwrap() {
            Taken::Busy(holder) => {
                assert_eq!(holder.pid, Some(std::process::id()));
                assert_eq!(holder.version.as_deref(), Some(constants::VERSION));
                assert!(holder.since.is_some());
            }
            Taken::Held(_) => panic!("two holders at once"),
        }
    }

    #[test]
    fn the_lock_is_free_again_once_the_holder_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        drop(held(dir.path()));
        drop(held(dir.path()));
    }

    #[test]
    fn a_lock_left_by_a_dead_process_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        // What a crashed holder leaves behind: its text and no OS lock, because the OS
        // released that with the process. A pid that can never be live proves the text
        // is not what decides.
        fs::write(
            lock_path(dir.path()),
            "pid=4294967295\nversion=0.0.1\nsince=2020-01-01T00:00:00Z\n",
        )
        .unwrap();

        let lock = held(dir.path());
        let text = fs::read_to_string(lock_path(dir.path())).unwrap();
        assert!(
            text.contains(&format!("pid={}", std::process::id())),
            "the new holder rewrites the details: {text}"
        );
        assert!(!text.contains("4294967295"));
        drop(lock);
    }

    #[test]
    fn an_unattended_pass_that_finds_the_lock_busy_skips_and_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let _first = held(dir.path());
        let ran = Cell::new(false);

        let out = run_exclusive_in(dir.path(), Pass::Unattended, false, || {
            ran.set(true);
            Ok(())
        });

        assert!(out.expect("a skipped unattended pass is not an error").is_none());
        assert!(!ran.get(), "the body must not run while the lock is held");
    }

    #[test]
    fn an_interactive_pass_that_finds_the_lock_busy_fails_naming_the_holder() {
        let dir = tempfile::tempdir().unwrap();
        let _first = held(dir.path());
        let ran = Cell::new(false);

        let err = run_exclusive_in(dir.path(), Pass::Interactive, false, || {
            ran.set(true);
            Ok(())
        })
        .expect_err("an interactive pass must exit non-zero");

        let message = format!("{err:#}");
        assert!(message.contains(&format!("pid {}", std::process::id())), "{message}");
        assert!(message.contains("Wait for it to finish"), "{message}");
        assert!(!ran.get());
    }

    #[test]
    fn the_body_runs_and_the_lock_is_released_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let out = run_exclusive_in(dir.path(), Pass::Interactive, false, || Ok(7)).unwrap();
        assert_eq!(out, Some(7));
        drop(held(dir.path()));
    }

    #[test]
    fn the_lock_is_released_when_the_body_fails() {
        let dir = tempfile::tempdir().unwrap();
        let out: Result<Option<()>> = run_exclusive_in(dir.path(), Pass::Interactive, false, || {
            anyhow::bail!("the pass broke")
        });
        assert!(out.is_err());
        drop(held(dir.path()));
    }

    #[test]
    fn the_lock_is_released_when_the_body_panics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let outcome = std::panic::catch_unwind(|| {
            let _ = run_exclusive_in::<()>(&path, Pass::Interactive, false, || {
                panic!("the pass panicked")
            });
        });
        assert!(outcome.is_err());
        drop(held(dir.path()));
    }

    #[test]
    fn holder_text_with_missing_or_odd_lines_still_parses() {
        let holder = Holder::parse("pid=12\nnonsense\nversion=\n");
        assert_eq!(holder.pid, Some(12));
        assert_eq!(holder.version, None);
        assert_eq!(Holder::default().describe(), "holder details not written yet");
        assert_eq!(holder.describe(), "pid 12");
    }

    #[test]
    fn a_config_directory_that_cannot_be_made_runs_the_body_unguarded() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the directory should be.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "x").unwrap();
        let out = run_exclusive_in(&blocker.join("cfg"), Pass::Unattended, false, || Ok(1));
        assert_eq!(out.unwrap(), Some(1));
    }
}
