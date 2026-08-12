// Windows Task Scheduler integration.
#![allow(dead_code)]

use crate::daemon::{DaemonStatus, get_exe_path};
use anyhow::{Context, Result};
use std::process::Command;

const TASK_NAME: &str = "DevPrune";

/// Builds the schtasks command arguments for installation.
pub fn build_install_command(exe_path: &str, interval_days: u64) -> Vec<String> {
    vec![
        "/Create".to_string(),
        "/SC".to_string(),
        "DAILY".to_string(),
        "/MO".to_string(),
        interval_days.to_string(),
        "/TN".to_string(),
        TASK_NAME.to_string(),
        "/TR".to_string(),
        // `--yes` is required: a scheduled task has no console, so a prompt would
        // read EOF and abort the run.
        format!("\"{}\" run --yes --daemon", exe_path),
        "/F".to_string(),
    ]
}

/// Installs the Windows Task Scheduler task.
pub fn install(interval_days: u64) -> Result<()> {
    let exe_path = get_exe_path();
    let exe_str = exe_path.to_string_lossy();
    let args = build_install_command(&exe_str, interval_days);

    let output = Command::new("schtasks")
        .args(&args)
        .output()
        .context("Failed to execute schtasks")?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "Failed to install task: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

/// Uninstalls the Windows Task Scheduler task.
pub fn uninstall() -> Result<()> {
    let output = Command::new("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .output()
        .context("Failed to execute schtasks")?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "Failed to uninstall task: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

/// Decide the task's state from the output of a task-list query.
///
/// Split out from [`status`] so the parsing is testable without a scheduler, and kept
/// deliberately free of any English text: `schtasks` localises its messages, so matching
/// on them reports `Unknown` on every non-English Windows — which in turn makes setup
/// skip the scheduler forever on those machines.
///
/// The task *name* is not localised, so that is what this looks for. Rows are CSV with
/// the name first, e.g. `"\DevPrune","14/02/2026 09:00:00","Ready"`.
fn classify_query(succeeded: bool, stdout: &str, stderr: &str) -> DaemonStatus {
    if !succeeded {
        // Enumerating every task should not fail. If it did, the answer is genuinely
        // unknown — an access-denied here must not be read as "absent", or every run
        // would try to reinstall a task that is already there.
        let reason = stderr.trim();
        return DaemonStatus::Unknown(if reason.is_empty() {
            "schtasks query failed without a message".to_string()
        } else {
            reason.to_string()
        });
    }

    let found = stdout.lines().any(|line| {
        line.split(',')
            .next()
            .map(|name| name.trim().trim_matches('"'))
            .is_some_and(|name| name.eq_ignore_ascii_case(&format!("\\{TASK_NAME}")))
    });

    if found {
        DaemonStatus::Installed
    } else {
        DaemonStatus::NotInstalled
    }
}

/// Checks the status of the Windows Task Scheduler task.
pub fn status() -> Result<DaemonStatus> {
    // Enumerate rather than query by name: a by-name query that fails cannot be told
    // apart from a task that is absent without reading a localised error string.
    let output = Command::new("schtasks")
        .args(["/Query", "/FO", "CSV", "/NH"])
        .output()
        .context("Failed to execute schtasks")?;

    Ok(classify_query(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    ))
}

/// Decode the bytes `schtasks /XML` writes.
///
/// It emits UTF-16LE with a byte-order mark, not the console codepage every other
/// `schtasks` query uses. Read as UTF-8 the whole document comes back as text separated
/// by NUL bytes, which no `find` on an element name will ever match — a silent empty
/// answer rather than a visible failure.
fn decode_schtasks_xml(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Pull the executable out of a task definition.
///
/// `<Actions><Exec><Command>` holds the program on its own — `schtasks` splits the `/TR`
/// string into `Command` and `Arguments` when it registers the task — and element names
/// are not localised, unlike the field labels `/V /FO LIST` prints.
fn parse_registered_command(xml: &str) -> Option<std::path::PathBuf> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = start + xml[start..].find("</Command>")?;
    let command = xml[start..end].trim().trim_matches('"').trim();
    if command.is_empty() {
        return None;
    }
    // `&` is the only one of the five predefined entities that can appear in a Windows
    // path, but decoding all of them costs nothing and `&amp;` must come last or it would
    // re-expand the ampersands the others just produced.
    let unescaped = command
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    Some(std::path::PathBuf::from(unescaped))
}

/// The binary the registered task will run, if the task exists and can be read.
pub fn registered_exe_path() -> Option<std::path::PathBuf> {
    let output = Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/XML", "ONE"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_registered_command(&decode_schtasks_xml(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_install_command() {
        let cmd = build_install_command("C:\\test\\dev-prune.exe", 2);
        assert_eq!(
            cmd,
            vec![
                "/Create",
                "/SC",
                "DAILY",
                "/MO",
                "2",
                "/TN",
                "DevPrune",
                "/TR",
                "\"C:\\test\\dev-prune.exe\" run --yes --daemon",
                "/F"
            ]
        );
    }

    #[test]
    fn test_build_install_command_honours_interval() {
        let cmd = build_install_command("dev-prune.exe", 7);
        assert_eq!(cmd[4], "7");
    }

    /// One CSV row as `schtasks /FO CSV /NH` emits it.
    fn row(name: &str) -> String {
        format!("\"{name}\",\"14/02/2026 09:00:00\",\"Ready\"\n")
    }

    #[test]
    fn the_task_is_found_by_name_in_the_listing() {
        let listing = format!("{}{}", row("\\SomeOtherTask"), row("\\DevPrune"));
        assert!(matches!(
            classify_query(true, &listing, ""),
            DaemonStatus::Installed
        ));
    }

    #[test]
    fn an_absent_task_reads_as_not_installed_not_unknown() {
        let listing = row("\\SomeOtherTask");
        assert!(matches!(
            classify_query(true, &listing, ""),
            DaemonStatus::NotInstalled
        ));
    }

    #[test]
    fn a_localised_scheduler_still_answers_correctly() {
        // The whole point of enumerating: nothing here is English, and the task name
        // is the one column Windows does not translate.
        let listing = "\"\\DevPrune\",\"14.02.2026 09:00:00\",\"Bereit\"\n";
        assert!(matches!(
            classify_query(true, listing, ""),
            DaemonStatus::Installed
        ));
    }

    #[test]
    fn a_similarly_named_task_is_not_mistaken_for_ours() {
        let listing = format!("{}{}", row("\\DevPruneOld"), row("\\Foo\\DevPrune"));
        assert!(matches!(
            classify_query(true, &listing, ""),
            DaemonStatus::NotInstalled
        ));
    }

    #[test]
    fn a_failed_query_is_unknown_rather_than_absent() {
        // Reading access-denied as "absent" would make every run reinstall a task that
        // is already there, and fail every time.
        let status = classify_query(false, "", "ERROR: Access is denied.");
        match status {
            DaemonStatus::Unknown(why) => assert!(why.contains("Access is denied")),
            other => panic!("expected Unknown, got {other}"),
        }
    }

    #[test]
    fn a_silent_failure_still_explains_itself() {
        match classify_query(false, "", "   ") {
            DaemonStatus::Unknown(why) => assert!(!why.is_empty()),
            other => panic!("expected Unknown, got {other}"),
        }
    }

    /// The shape `schtasks /XML ONE` actually returns, trimmed to the part that matters.
    const TASK_XML: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Actions Context="Author">
    <Exec>
      <Command>"C:\Users\a\AppData\Roaming\dev-prune\bin\dev-prune.exe"</Command>
      <Arguments>run --yes --daemon</Arguments>
    </Exec>
  </Actions>
</Task>"#;

    #[test]
    fn the_registered_binary_is_read_out_of_the_task_definition() {
        assert_eq!(
            parse_registered_command(TASK_XML),
            Some(std::path::PathBuf::from(
                "C:\\Users\\a\\AppData\\Roaming\\dev-prune\\bin\\dev-prune.exe"
            ))
        );
    }

    #[test]
    fn the_arguments_are_not_mistaken_for_part_of_the_path() {
        let path = parse_registered_command(TASK_XML).unwrap();
        assert!(!path.to_string_lossy().contains("--daemon"));
    }

    #[test]
    fn an_escaped_ampersand_in_the_path_survives() {
        let xml = "<Command>\"C:\\a &amp; b\\dev-prune.exe\"</Command>";
        assert_eq!(
            parse_registered_command(xml),
            Some(std::path::PathBuf::from("C:\\a & b\\dev-prune.exe"))
        );
    }

    #[test]
    fn a_document_without_an_exec_action_answers_nothing() {
        assert!(parse_registered_command("<Task><Actions /></Task>").is_none());
        assert!(parse_registered_command("<Command></Command>").is_none());
    }

    #[test]
    fn utf16_output_is_decoded_rather_than_read_as_nul_separated_bytes() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "<Command>\"C:\\x.exe\"</Command>".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let decoded = decode_schtasks_xml(&bytes);
        assert!(!decoded.contains('\0'));
        assert_eq!(
            parse_registered_command(&decoded),
            Some(std::path::PathBuf::from("C:\\x.exe"))
        );
    }

    #[test]
    fn output_without_a_bom_is_still_readable() {
        assert_eq!(
            decode_schtasks_xml(b"<Command>x</Command>"),
            "<Command>x</Command>"
        );
    }
}
