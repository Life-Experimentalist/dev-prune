// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Linux systemd user service and timer integration.

use crate::daemon::{DaemonStatus, get_exe_path};
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Get the path to the systemd user config directory.
///
/// systemd reads user units from `$XDG_CONFIG_HOME/systemd/user`, so honour that
/// variable rather than hardcoding `~/.config` — on a machine that sets it elsewhere
/// the units would be written where systemd never looks and the timer would silently
/// never run.
pub fn get_systemd_dir() -> Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = env::var("HOME").context("Could not find HOME directory")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("systemd").join("user"))
}

/// Generates the service unit file content.
///
/// The executable path is quoted so a path containing spaces is passed as one argument,
/// and `--yes` is required because a timer-launched unit has no terminal to prompt on.
pub fn generate_service_unit(exe_path: &str) -> String {
    format!(
        r#"[Unit]
Description=DevPrune Background Runner

[Service]
Type=oneshot
ExecStart="{}" run --yes --daemon
"#,
        exe_path
    )
}

/// Generates the timer unit file content.
///
/// Uses `OnUnitActiveSec` rather than an `OnCalendar` day-of-month expression: the
/// latter (`*-*-1/2`) fires on odd days of the month, which is not the same as every
/// N days and skews at every month boundary. `Persistent=` is deliberately absent: it
/// only applies to `OnCalendar=` timers and would be silently ignored here.
/// `OnBootSec=` covers the machine-was-off case instead.
pub fn generate_timer_unit(interval_days: u64) -> String {
    let interval_days = interval_days.max(1);
    format!(
        r#"[Unit]
Description=Run DevPrune every {interval_days} day(s)

[Timer]
OnBootSec=15min
OnUnitActiveSec={interval_days}d

[Install]
WantedBy=timers.target
"#
    )
}

/// Whether there is a `systemctl` on `PATH` at all.
///
/// Checked before writing anything, so a machine that cannot use these units does not
/// end up with two orphaned files in its config directory.
fn systemctl_available() -> bool {
    !matches!(
        Command::new("systemctl").arg("--version").output(),
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound
    )
}

/// Installs the Linux systemd user timer.
pub fn install(interval_days: u64) -> Result<()> {
    if !systemctl_available() {
        anyhow::bail!("{NO_SYSTEMD_HELP}");
    }

    let exe_path = get_exe_path();
    let service_content = generate_service_unit(&exe_path.to_string_lossy());
    let timer_content = generate_timer_unit(interval_days);

    let systemd_dir = get_systemd_dir()?;
    fs::create_dir_all(&systemd_dir)?;

    let mut service_path = systemd_dir.clone();
    service_path.push("dev-prune.service");
    fs::write(&service_path, service_content)?;

    let mut timer_path = systemd_dir.clone();
    timer_path.push("dev-prune.timer");
    fs::write(&timer_path, timer_content)?;

    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output()?;

    let output = Command::new("systemctl")
        .args(["--user", "enable", "--now", "dev-prune.timer"])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "systemctl enable failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

/// Uninstalls the Linux systemd user timer.
pub fn uninstall() -> Result<()> {
    let systemd_dir = get_systemd_dir()?;
    let mut service_path = systemd_dir.clone();
    service_path.push("dev-prune.service");
    let mut timer_path = systemd_dir.clone();
    timer_path.push("dev-prune.timer");

    // Best-effort: removing the unit files is what actually uninstalls the timer, and
    // that must still happen on a machine where systemd has since gone away.
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", "dev-prune.timer"])
        .output();

    if service_path.exists() {
        fs::remove_file(service_path)?;
    }
    if timer_path.exists() {
        fs::remove_file(timer_path)?;
    }

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    Ok(())
}

/// What to tell a user whose machine has no systemd to talk to.
///
/// Not every Linux runs systemd (Alpine, Void, Devuan, Gentoo/OpenRC) and not every
/// systemd session has a user bus (`ssh` without lingering, most containers). Both are
/// ordinary situations, not faults, so they get an explanation and a manual alternative
/// rather than a raw `os error 2`.
pub const NO_SYSTEMD_HELP: &str = "systemd user units are not available here. \
     Schedule `dev-prune run --yes --daemon` with whatever this system does use \
     (cron, OpenRC, runit), or run `devp run` by hand — everything else works without \
     a scheduler.";

/// Turn `systemctl is-active` output into a state.
///
/// `is-active` answers `inactive` for a unit that exists but is stopped *and* for one
/// that was never installed, and `failed` for one that errored. Only `active` is a
/// running timer; `failed` is a definite answer too, and reporting it as absent lets the
/// next setup pass reinstall over it rather than leaving a dead timer in place forever.
fn classify_is_active(stdout: &str, stderr: &str) -> DaemonStatus {
    match stdout.trim() {
        "active" => DaemonStatus::Installed,
        "inactive" | "failed" | "unknown" => DaemonStatus::NotInstalled,
        other => {
            // No user bus: `is-active` prints nothing and explains itself on stderr.
            let reason = if other.is_empty() {
                stderr.trim()
            } else {
                other
            };
            if reason.is_empty() {
                DaemonStatus::Unknown(NO_SYSTEMD_HELP.to_string())
            } else {
                DaemonStatus::Unknown(reason.to_string())
            }
        }
    }
}

/// Checks the status of the Linux systemd user timer.
pub fn status() -> Result<DaemonStatus> {
    let output = match Command::new("systemctl")
        .args(["--user", "is-active", "dev-prune.timer"])
        .output()
    {
        Ok(output) => output,
        // `systemctl` is absent entirely. Not an error to propagate — it is a fact
        // about the machine, and the caller renders it as a skip with this help text.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DaemonStatus::Unknown(NO_SYSTEMD_HELP.to_string()));
        }
        Err(e) => return Err(e.into()),
    };

    Ok(classify_is_active(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    ))
}

/// Pull the executable out of a service unit's `ExecStart=`.
///
/// [`generate_service_unit`] quotes the path so a directory containing a space stays one
/// argument, so the quoted form is the one that matters — but a unit edited by hand may
/// well not be quoted, and reading that as a path ending at the first space would report
/// a perfectly good scheduler as broken.
fn parse_exec_start(unit: &str) -> Option<PathBuf> {
    let value = unit
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("ExecStart="))?
        .trim();
    let program = match value.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => value.split_whitespace().next()?,
    };
    if program.is_empty() {
        return None;
    }
    Some(PathBuf::from(program))
}

/// The binary the installed timer will run, if there is a unit file to read.
pub fn registered_exe_path() -> Option<PathBuf> {
    let unit = get_systemd_dir().ok()?.join("dev-prune.service");
    parse_exec_start(&fs::read_to_string(unit).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_service_unit() {
        let content = generate_service_unit("/usr/bin/dev-prune");
        assert!(content.contains("ExecStart=\"/usr/bin/dev-prune\" run --yes --daemon"));
    }

    #[test]
    fn test_generate_service_unit_quotes_spaced_path() {
        let content = generate_service_unit("/home/a b/dev-prune");
        assert!(content.contains("ExecStart=\"/home/a b/dev-prune\" run --yes --daemon"));
    }

    #[test]
    fn test_generate_timer_unit() {
        let content = generate_timer_unit(2);
        assert!(content.contains("OnUnitActiveSec=2d"));
        assert!(!content.contains("OnCalendar"));
    }

    #[test]
    fn test_generate_timer_unit_honours_interval() {
        assert!(generate_timer_unit(7).contains("OnUnitActiveSec=7d"));
        // 0 would make systemd fire continuously — clamped to 1.
        assert!(generate_timer_unit(0).contains("OnUnitActiveSec=1d"));
    }

    #[test]
    fn a_running_timer_is_installed() {
        assert!(matches!(
            classify_is_active("active\n", ""),
            DaemonStatus::Installed
        ));
    }

    #[test]
    fn a_stopped_or_dead_timer_reads_as_absent_so_setup_can_replace_it() {
        for answer in ["inactive\n", "failed\n", "unknown\n"] {
            assert!(
                matches!(classify_is_active(answer, ""), DaemonStatus::NotInstalled),
                "{answer:?} should be replaceable"
            );
        }
    }

    #[test]
    fn a_session_without_a_user_bus_explains_itself() {
        // `is-active` prints nothing to stdout in this case; the reason is on stderr,
        // and losing it leaves the user with an empty parenthesis to debug.
        let status = classify_is_active("", "Failed to connect to bus: No such file or directory");
        match status {
            DaemonStatus::Unknown(why) => assert!(why.contains("Failed to connect to bus")),
            other => panic!("expected Unknown, got {other}"),
        }
    }

    #[test]
    fn a_wordless_failure_falls_back_to_the_help_text() {
        match classify_is_active("", "") {
            DaemonStatus::Unknown(why) => assert_eq!(why, NO_SYSTEMD_HELP),
            other => panic!("expected Unknown, got {other}"),
        }
    }

    #[test]
    fn the_registered_binary_is_read_back_out_of_what_we_wrote() {
        let unit = generate_service_unit("/usr/bin/dev-prune");
        assert_eq!(
            parse_exec_start(&unit),
            Some(PathBuf::from("/usr/bin/dev-prune"))
        );
    }

    #[test]
    fn a_quoted_path_with_spaces_is_not_truncated() {
        let unit = generate_service_unit("/home/a b/dev-prune");
        assert_eq!(
            parse_exec_start(&unit),
            Some(PathBuf::from("/home/a b/dev-prune"))
        );
    }

    #[test]
    fn an_unquoted_hand_edited_unit_is_still_readable() {
        let unit = "[Service]\nExecStart=/usr/bin/dev-prune run --yes\n";
        assert_eq!(
            parse_exec_start(unit),
            Some(PathBuf::from("/usr/bin/dev-prune"))
        );
    }

    #[test]
    fn a_unit_without_an_exec_start_answers_nothing() {
        assert!(parse_exec_start("[Unit]\nDescription=x\n").is_none());
        assert!(parse_exec_start("ExecStart=\n").is_none());
    }

    #[test]
    fn the_help_text_names_an_alternative_the_user_can_act_on() {
        assert!(NO_SYSTEMD_HELP.contains("cron"));
        assert!(NO_SYSTEMD_HELP.contains("devp run"));
    }
}
