// macOS LaunchAgent integration.
#![allow(dead_code)]

use crate::daemon::{DaemonStatus, get_exe_path};
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.devprune.daemon";

/// Get the path to the LaunchAgent plist file.
pub fn get_plist_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("Could not find HOME directory")?;
    let mut path = PathBuf::from(home);
    path.push("Library");
    path.push("LaunchAgents");
    path.push(format!("{}.plist", LABEL));
    Ok(path)
}

/// Escape the five XML predefined entities so a path containing `&` or `<` cannot
/// produce a plist that `launchctl` refuses to parse.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Generates the plist XML content.
pub fn generate_plist_content(exe_path: &str, interval_days: u64) -> String {
    let interval_secs = interval_days.max(1).saturating_mul(86_400);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>run</string>
        <string>--yes</string>
        <string>--daemon</string>
    </array>
    <key>StartInterval</key>
    <integer>{}</integer>
</dict>
</plist>"#,
        LABEL,
        xml_escape(exe_path),
        interval_secs
    )
}

/// Installs the macOS LaunchAgent.
pub fn install(interval_days: u64) -> Result<()> {
    let exe_path = get_exe_path()?;
    let plist_content = generate_plist_content(&exe_path.to_string_lossy(), interval_days);
    let plist_path = get_plist_path()?;

    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plist_path, plist_content)?;

    let output = Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()
        .context("Failed to execute launchctl load")?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "launchctl load failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

/// Uninstalls the macOS LaunchAgent.
pub fn uninstall() -> Result<()> {
    let plist_path = get_plist_path()?;
    if plist_path.exists() {
        let output = Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .output()
            .context("Failed to execute launchctl unload")?;

        fs::remove_file(&plist_path)?;

        if !output.status.success() {
            anyhow::bail!(
                "launchctl unload failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }
    }
    Ok(())
}

/// Checks the status of the macOS LaunchAgent.
///
/// A plist on disk is not a scheduled job — launchd has to have loaded it. When the file
/// is there but the label is not registered (a load that failed, or a plist restored from
/// a backup), the honest answer is `NotInstalled`: `install` is idempotent, so reporting
/// it that way lets the next setup pass load the agent instead of leaving a file that
/// looks installed and never runs.
pub fn status() -> Result<DaemonStatus> {
    let plist_path = get_plist_path()?;
    if !plist_path.exists() {
        return Ok(DaemonStatus::NotInstalled);
    }

    match Command::new("launchctl").args(["list", LABEL]).output() {
        Ok(output) if output.status.success() => Ok(DaemonStatus::Installed),
        // A non-zero exit from `launchctl list <label>` means exactly one thing: no job
        // by that label is loaded.
        Ok(_) => Ok(DaemonStatus::NotInstalled),
        // `launchctl` itself could not be run. That is genuinely unknown, and must not
        // be read as absent — reinstalling on every command would fail every time.
        Err(e) => Ok(DaemonStatus::Unknown(format!(
            "could not run launchctl: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_plist_content() {
        let content = generate_plist_content("/usr/local/bin/dev-prune", 2);
        assert!(content.contains("<string>com.devprune.daemon</string>"));
        assert!(content.contains("<string>/usr/local/bin/dev-prune</string>"));
        assert!(content.contains("<string>run</string>"));
        assert!(content.contains("<string>--yes</string>"));
        assert!(content.contains("<string>--daemon</string>"));
        assert!(content.contains("<integer>172800</integer>"));
    }

    #[test]
    fn test_generate_plist_honours_interval() {
        assert!(generate_plist_content("/x", 7).contains("<integer>604800</integer>"));
        assert!(generate_plist_content("/x", 0).contains("<integer>86400</integer>"));
    }

    #[test]
    fn test_generate_plist_escapes_xml() {
        let content = generate_plist_content("/home/a&b/dev-prune", 2);
        assert!(content.contains("<string>/home/a&amp;b/dev-prune</string>"));
    }
}
