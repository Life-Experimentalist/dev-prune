// Cross-platform daemon/scheduler management.
//
// Installs, uninstalls, and checks the status of a background task that runs
// `dev-prune run` on a schedule. Platform-specific implementations:
// - Windows: Task Scheduler (schtasks)
// - macOS: LaunchAgent (.plist)
// - Linux: systemd user timer

mod linux;
mod macos;
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

/// Get the absolute path to the current binary.
pub fn get_exe_path() -> Result<std::path::PathBuf> {
    std::env::current_exe().map_err(Into::into)
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
