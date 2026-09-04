// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Cross-platform daemon/scheduler management.
//
// Installs, uninstalls, and checks the status of a background task that runs
// `dev-prune run` on a schedule. Platform-specific implementations:
// - Windows: Task Scheduler (schtasks)
// - macOS: LaunchAgent (.plist)
// - Linux: systemd user timer

// Each platform module is only compiled on its own OS. The alternative — compiling
// all three everywhere under `#![allow(dead_code)]` — silences the lint for genuinely
// dead items too, which is how orphaned helpers accumulate.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use anyhow::Result;
use std::fmt;

/// Status of the background daemon task.
pub enum DaemonStatus {
    Installed,
    NotInstalled,
    Unknown(String),
}

impl fmt::Display for DaemonStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaemonStatus::Installed => write!(f, "Installed"),
            DaemonStatus::NotInstalled => write!(f, "Not Installed"),
            DaemonStatus::Unknown(msg) => write!(f, "Unknown: {}", msg),
        }
    }
}

/// Install the OS-native daemon/scheduled task, firing every `interval_days` days.
///
/// The scheduled command always passes `--yes`: there is no terminal attached to a
/// scheduler-launched process, so a run that stopped to ask for confirmation would
/// simply abort every time. Safety still comes from the idle check and lockfile
/// enforcement, both of which the daemon run performs normally.
pub fn install_daemon(interval_days: u64) -> Result<()> {
    let interval_days = interval_days.max(1);
    #[cfg(target_os = "windows")]
    {
        windows::install(interval_days)
    }
    #[cfg(target_os = "macos")]
    {
        macos::install(interval_days)
    }
    #[cfg(target_os = "linux")]
    {
        linux::install(interval_days)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("Unsupported operating system for daemon installation");
    }
}

/// Uninstall the daemon/scheduled task.
pub fn uninstall_daemon() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::uninstall()
    }
    #[cfg(target_os = "macos")]
    {
        macos::uninstall()
    }
    #[cfg(target_os = "linux")]
    {
        linux::uninstall()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("Unsupported operating system for daemon uninstallation");
    }
}

/// Check if the daemon is installed and its status.
pub fn daemon_status() -> Result<DaemonStatus> {
    #[cfg(target_os = "windows")]
    {
        windows::status()
    }
    #[cfg(target_os = "macos")]
    {
        macos::status()
    }
    #[cfg(target_os = "linux")]
    {
        linux::status()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("Unsupported operating system for daemon status");
    }
}

/// The binary path to register with the scheduler.
///
/// Not `current_exe()`: a scheduled task outlives the process that created it, so the
/// path it records has to outlive it too. See [`crate::setup::stable_exe_path`] for what
/// goes wrong when it does not.
pub fn get_exe_path() -> std::path::PathBuf {
    crate::setup::stable_exe_path()
}

/// Whether the installed scheduler entry should be re-registered to stop it flashing a
/// console window at the logged-in user. Only Windows attaches a console to a scheduled
/// task; the other platforms' schedulers never open a terminal, so there the answer is
/// always no.
pub fn wants_windowless_upgrade() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::wants_windowless_upgrade()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Whether the installed scheduler entry still carries the power gates Windows puts on
/// a bare `schtasks /Create` registration — refuse to start on battery, die on unplug,
/// never catch up a missed trigger. Only Windows has them; launchd and systemd user
/// timers run on battery without being asked.
pub fn wants_power_upgrade() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::wants_power_upgrade()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Lift those power gates off the installed task, keeping its trigger time, logon type
/// and binary as they are. No-op on the other platforms.
pub fn apply_power_settings() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::apply_power_settings()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

/// Replace the windowless scheduler binary after an upgrade, when one is in use.
///
/// Windows-only: the twin (`devpw.exe`) is a separate build target shipped beside the
/// managed binary, so replacing that binary without replacing the twin would leave the
/// daemon running the previous release. The other platforms register the real binary
/// directly and have nothing to refresh.
///
/// It is *placed* from the delivery, never generated here — see the note on
/// `windows::place_windowless_twin` for the release that learnt why.
pub fn refresh_windowless_twin() {
    #[cfg(target_os = "windows")]
    {
        windows::refresh_windowless_twin();
    }
}

/// The binary the installed scheduler entry will actually run, when that can be read.
///
/// `None` means "could not determine", never "nothing is registered" — use
/// [`daemon_status`] for that question. This exists so `devp doctor` can tell a working
/// scheduler apart from one still pointing at a directory that has since been deleted,
/// which is otherwise completely silent: the task keeps reporting itself as `Ready` and
/// fails the instant it fires, every interval, forever.
pub fn registered_exe_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        windows::registered_exe_path()
    }
    #[cfg(target_os = "macos")]
    {
        macos::registered_exe_path()
    }
    #[cfg(target_os = "linux")]
    {
        linux::registered_exe_path()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_status_display() {
        assert_eq!(DaemonStatus::Installed.to_string(), "Installed");
        assert_eq!(DaemonStatus::NotInstalled.to_string(), "Not Installed");
        assert_eq!(
            DaemonStatus::Unknown("error".into()).to_string(),
            "Unknown: error"
        );
    }
}
