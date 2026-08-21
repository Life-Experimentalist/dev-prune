// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Windows Task Scheduler integration.

use crate::constants::{WINDOWS_HIDDEN_BIN, WINDOWS_TASK_NAME as TASK_NAME};
use crate::daemon::{DaemonStatus, get_exe_path};
use crate::spawn;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Builds the schtasks command arguments for installation.
pub fn build_install_command(exe_path: &str, interval_days: u64) -> Vec<String> {
    vec![
        "/Create".to_string(),
        "/SC".to_string(),
        "DAILY".to_string(),
        "/MO".to_string(),
        // schtasks rejects `/SC DAILY /MO` values outside 1–365 outright, so an
        // oversized interval must be capped here or the install fails with a
        // localised error the caller cannot interpret.
        interval_days.clamp(1, 365).to_string(),
        "/TN".to_string(),
        TASK_NAME.to_string(),
        "/TR".to_string(),
        // `--yes` is required: a scheduled task has no console, so a prompt would
        // read EOF and abort the run.
        format!("\"{}\" run --yes --daemon", exe_path),
        "/F".to_string(),
    ]
}

/// Builds the schtasks arguments for a hidden installation.
///
/// `/RU <account> /NP` registers the task to run whether the user is logged on or not,
/// without storing a password (an S4U logon). Such a task runs in a non-interactive
/// session — without it, every firing of the console binary flashes a black window at
/// whoever is logged in, which reads as malware to anyone watching their own screen.
pub fn build_install_command_hidden(
    exe_path: &str,
    interval_days: u64,
    account: &str,
) -> Vec<String> {
    let mut args = build_install_command(exe_path, interval_days);
    args.extend(["/RU".to_string(), account.to_string(), "/NP".to_string()]);
    args
}

/// The account to register the hidden task under, as `DOMAIN\user`.
fn current_account() -> Option<String> {
    let user = std::env::var("USERNAME").ok().filter(|s| !s.is_empty())?;
    Some(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    })
}

/// Marker recording that this machine's scheduler refused the hidden registration.
///
/// Some policies deny S4U logons to non-elevated callers, and a filesystem that cannot
/// hold the windowless twin refuses that route too. Without the marker, every setup
/// pass would re-try the upgrade, fail, and re-register the visible task — churning the
/// scheduler on every single `devp` invocation.
fn hidden_refused_marker() -> Option<PathBuf> {
    crate::config::Registry::config_dir()
        .ok()
        .map(|dir| dir.join(crate::constants::SCHEDULER_HIDDEN_REFUSED_MARKER))
}

/// Set a PE image's subsystem field to GUI, in place.
///
/// The subsystem is a single `u16` in the optional header — `editbin
/// /SUBSYSTEM:WINDOWS` edits exactly this field — and it is the only difference between
/// `python.exe` and `pythonw.exe`-style pairs: a GUI-subsystem process never gets a
/// console, so nothing ever flashes, while pipes and files it writes still work.
/// Everything is bounds-checked and signature-checked because the input is a file from
/// disk, not a value this code produced.
fn patch_subsystem_to_gui(image: &mut [u8]) -> std::result::Result<(), &'static str> {
    const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
    const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;

    if image.len() < 0x40 || &image[..2] != b"MZ" {
        return Err("not a DOS/PE executable");
    }
    let pe_offset =
        u32::from_le_bytes([image[0x3C], image[0x3D], image[0x3E], image[0x3F]]) as usize;
    if image.len() < pe_offset + 4 + 20 + 70 || &image[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Err("PE signature not found");
    }
    // Optional header follows the 4-byte signature and 20-byte COFF header. Its magic
    // distinguishes PE32 from PE32+, but the subsystem sits at offset 68 in both.
    let optional = pe_offset + 4 + 20;
    let magic = u16::from_le_bytes([image[optional], image[optional + 1]]);
    if magic != 0x10B && magic != 0x20B {
        return Err("unrecognised optional-header magic");
    }
    let subsystem = optional + 68;
    let current = u16::from_le_bytes([image[subsystem], image[subsystem + 1]]);
    if current != IMAGE_SUBSYSTEM_WINDOWS_CUI && current != IMAGE_SUBSYSTEM_WINDOWS_GUI {
        return Err("not a console or GUI executable");
    }
    image[subsystem..subsystem + 2].copy_from_slice(&IMAGE_SUBSYSTEM_WINDOWS_GUI.to_le_bytes());
    Ok(())
}

/// The windowless twin's path, beside the binary the scheduler would otherwise run.
fn hidden_twin_path(exe_path: &Path) -> PathBuf {
    exe_path.with_file_name(WINDOWS_HIDDEN_BIN)
}

/// Whether `twin` is already the windowless build of `exe` — same bytes except the one
/// field the patch changes. Same length is a prerequisite the patch guarantees.
fn twin_is_current(exe: &Path, twin: &Path) -> bool {
    let (Ok(source), Ok(existing)) = (std::fs::read(exe), std::fs::read(twin)) else {
        return false;
    };
    let mut expected = source;
    patch_subsystem_to_gui(&mut expected).is_ok() && expected == existing
}

/// Create or refresh `devpw.exe` beside `exe_path`, returning its path on success.
///
/// Generated locally rather than shipped, so every install channel — the installers,
/// npm, PyPI, `cargo install`, a bare unzipped archive — gets it without any of them
/// packaging a second binary. Staged beside and renamed into place like every other
/// managed write; a rename refused because the twin is mid-fire simply keeps the
/// previous build, which the next pass replaces.
fn ensure_hidden_twin(exe_path: &Path) -> Option<PathBuf> {
    let twin = hidden_twin_path(exe_path);
    if twin_is_current(exe_path, &twin) {
        return Some(twin);
    }
    let mut image = std::fs::read(exe_path).ok()?;
    patch_subsystem_to_gui(&mut image).ok()?;
    let staging = twin.with_extension("new");
    if std::fs::write(&staging, &image).is_err() {
        let _ = std::fs::remove_file(&staging);
        return None;
    }
    if std::fs::rename(&staging, &twin).is_err() {
        let _ = std::fs::remove_file(&staging);
        // The rename loses only to the twin being the running task or to a concurrent
        // pass that produced its own build — either way a usable twin is there.
        return twin.is_file().then_some(twin);
    }
    Some(twin)
}

/// Refresh the windowless twin after an upgrade, when one is in use.
///
/// The scheduled task names `devpw.exe`, so replacing the managed binary alone would
/// leave the daemon running the previous release forever. Only refreshes a twin that
/// already exists — creating one is the installer's decision, not a side effect of
/// every settled setup pass.
pub fn refresh_hidden_twin() {
    let exe_path = get_exe_path();
    if hidden_twin_path(&exe_path).is_file() {
        let _ = ensure_hidden_twin(&exe_path);
    }
}

/// Installs the Windows Task Scheduler task.
///
/// Three registrations, in order of preference, so no failure ever costs the machine
/// the daemon itself:
///
/// 1. The windowless twin (`devpw.exe`), as a plain interactive task. Nothing can
///    flash — the binary has no console to show — and the task runs in the user's own
///    session, so mapped network drives and everything else a logon session carries
///    keep working.
/// 2. The console binary under an S4U logon (`/RU <account> /NP`), which runs in a
///    non-interactive session: still no window, but no network credentials either.
/// 3. The console binary as a plain interactive task — visible, but functional.
pub fn install(interval_days: u64) -> Result<()> {
    let exe_path = get_exe_path();

    if let Some(twin) = ensure_hidden_twin(&exe_path) {
        let args = build_install_command(&twin.to_string_lossy(), interval_days);
        if let Ok(output) = spawn::command("schtasks").args(&args).output()
            && output.status.success()
        {
            // A machine that once refused may have been elevated or unlocked since;
            // the marker must not outlive the refusal it records.
            if let Some(marker) = hidden_refused_marker() {
                let _ = std::fs::remove_file(marker);
            }
            return Ok(());
        }
    }

    let exe_str = exe_path.to_string_lossy();
    if let Some(account) = current_account() {
        let args = build_install_command_hidden(&exe_str, interval_days, &account);
        if let Ok(output) = spawn::command("schtasks").args(&args).output()
            && output.status.success()
        {
            // Hidden, though without the interactive session's credentials. The marker
            // still records that the *preferred* route failed, so settled passes stop
            // retrying it; an elevated `devp daemon on` retries regardless.
            if let Some(marker) = hidden_refused_marker() {
                let _ = std::fs::write(marker, "");
            }
            return Ok(());
        }
    }
    if let Some(marker) = hidden_refused_marker() {
        let _ = std::fs::write(marker, "");
    }

    let args = build_install_command(&exe_str, interval_days);
    let output = spawn::command("schtasks")
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
    let output = spawn::command("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .output()
        .context("Failed to execute schtasks")?;

    if output.status.success() {
        return Ok(());
    }
    // A delete that failed because there was nothing to delete is the state the
    // caller asked for. The error text is localised, so instead of matching it,
    // ask the scheduler whether the task exists now — `devp uninstall` used to
    // fail outright on a machine where setup had never managed to register it.
    if matches!(status(), Ok(DaemonStatus::NotInstalled)) {
        return Ok(());
    }
    anyhow::bail!(
        "Failed to uninstall task: {}",
        String::from_utf8_lossy(&output.stderr)
    )
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
    let output = spawn::command("schtasks")
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
        let (pairs, _odd_tail) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
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

/// The registered task's definition, if the task exists and can be read.
fn task_xml() -> Option<String> {
    let output = spawn::command("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/XML", "ONE"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(decode_schtasks_xml(&output.stdout))
}

/// The binary the registered task will run, if the task exists and can be read.
pub fn registered_exe_path() -> Option<std::path::PathBuf> {
    parse_registered_command(&task_xml()?)
}

/// Whether a task definition runs with the interactive token — the logon type whose
/// console window flashes at the logged-in user every time the task fires.
fn parse_is_interactive(xml: &str) -> Option<bool> {
    let start = xml.find("<LogonType>")? + "<LogonType>".len();
    let end = start + xml[start..].find("</LogonType>")?;
    Some(xml[start..end].trim() == "InteractiveToken")
}

/// True when the installed task would flash a console window and this machine has not
/// already refused the hidden registration that fixes it. `ensure_daemon` uses this to
/// upgrade tasks registered by versions that only knew the interactive logon, or that
/// predate the windowless twin.
pub fn wants_hidden_upgrade() -> bool {
    let Some(xml) = task_xml() else {
        // No readable definition means no task to upgrade — re-registering a task
        // that cannot be read would be guessing.
        return false;
    };
    // A task already pointing at `devpw.exe` cannot flash regardless of logon type;
    // there is nothing left to upgrade to.
    if parse_registered_command(&xml).is_some_and(|exe| {
        exe.file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(WINDOWS_HIDDEN_BIN))
    }) {
        return false;
    }
    if hidden_refused_marker().is_some_and(|m| m.exists()) {
        return false;
    }
    // An S4U task registered by an earlier release is hidden but sessionless; the twin
    // route restores mapped drives too, so it is still an upgrade — an interactive
    // console task doubly so.
    parse_is_interactive(&xml).is_some()
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

    #[test]
    fn the_hidden_command_adds_the_password_less_run_as_and_nothing_else() {
        let visible = build_install_command("dev-prune.exe", 2);
        let hidden = build_install_command_hidden("dev-prune.exe", 2, "PC\\krish");
        assert_eq!(hidden[..visible.len()], visible[..]);
        assert_eq!(hidden[visible.len()..], ["/RU", "PC\\krish", "/NP"]);
    }

    #[test]
    fn the_interactive_logon_is_recognised_and_the_hidden_one_is_not() {
        assert_eq!(
            parse_is_interactive("<LogonType>InteractiveToken</LogonType>"),
            Some(true)
        );
        assert_eq!(
            parse_is_interactive("<LogonType>S4U</LogonType>"),
            Some(false)
        );
        // A definition without the element answers nothing — the caller must not
        // re-register a task it cannot read.
        assert_eq!(parse_is_interactive("<Task></Task>"), None);
    }

    #[test]
    fn an_interval_schtasks_would_reject_is_clamped_into_range() {
        // `/SC DAILY /MO` accepts 1–365; outside that, task creation fails outright.
        assert_eq!(build_install_command("dev-prune.exe", 0)[4], "1");
        assert_eq!(build_install_command("dev-prune.exe", 400)[4], "365");
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

    /// A minimal synthetic PE image: DOS stub, signature, COFF header, and an optional
    /// header of the given magic with its subsystem field set to `subsystem`.
    fn synthetic_pe(magic: u16, subsystem: u16) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        let optional = PE_OFFSET + 4 + 20;
        let mut image = vec![0u8; optional + 0xF0];
        image[0] = b'M';
        image[1] = b'Z';
        image[0x3C..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[optional..optional + 2].copy_from_slice(&magic.to_le_bytes());
        image[optional + 68..optional + 70].copy_from_slice(&subsystem.to_le_bytes());
        image
    }

    fn subsystem_of(image: &[u8]) -> u16 {
        let pe = u32::from_le_bytes([image[0x3C], image[0x3D], image[0x3E], image[0x3F]]) as usize;
        let field = pe + 4 + 20 + 68;
        u16::from_le_bytes([image[field], image[field + 1]])
    }

    #[test]
    fn a_console_pe32_plus_image_becomes_gui_and_nothing_else_moves() {
        let mut image = synthetic_pe(0x20B, 3);
        let before = image.clone();
        patch_subsystem_to_gui(&mut image).unwrap();
        assert_eq!(subsystem_of(&image), 2);
        // Only the subsystem field may change — here that is one byte (3 → 2).
        let diffs = before.iter().zip(&image).filter(|(a, b)| a != b).count();
        assert_eq!(diffs, 1);
        assert_eq!(image.len(), before.len());
    }

    #[test]
    fn a_console_pe32_image_is_patched_at_the_same_relative_offset() {
        let mut image = synthetic_pe(0x10B, 3);
        patch_subsystem_to_gui(&mut image).unwrap();
        assert_eq!(subsystem_of(&image), 2);
    }

    #[test]
    fn an_already_gui_image_is_left_valid_and_unchanged() {
        let mut image = synthetic_pe(0x20B, 2);
        let before = image.clone();
        patch_subsystem_to_gui(&mut image).unwrap();
        assert_eq!(image, before);
    }

    #[test]
    fn garbage_and_truncated_inputs_are_refused_not_corrupted() {
        assert!(patch_subsystem_to_gui(&mut []).is_err());
        assert!(patch_subsystem_to_gui(&mut b"not an executable".to_vec()).is_err());
        // Valid DOS header pointing past the end of the file.
        let mut truncated = synthetic_pe(0x20B, 3);
        truncated.truncate(0x82);
        assert!(patch_subsystem_to_gui(&mut truncated).is_err());
        // A driver or other non-console, non-GUI subsystem must not be touched.
        let mut native = synthetic_pe(0x20B, 1);
        assert!(patch_subsystem_to_gui(&mut native).is_err());
        // An unknown optional-header magic means the subsystem offset is a guess.
        let mut bad_magic = synthetic_pe(0x999, 3);
        assert!(patch_subsystem_to_gui(&mut bad_magic).is_err());
    }

    #[test]
    fn the_twin_lives_beside_the_managed_binary_under_the_reserved_name() {
        let twin = hidden_twin_path(Path::new("C:\\cfg\\bin\\dev-prune.exe"));
        assert_eq!(twin, Path::new("C:\\cfg\\bin\\devpw.exe"));
    }

    #[test]
    fn the_twin_is_generated_refreshed_when_stale_and_recognised_when_current() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dev-prune.exe");
        std::fs::write(&exe, synthetic_pe(0x20B, 3)).unwrap();

        let twin = ensure_hidden_twin(&exe).expect("twin should be generated");
        assert_eq!(subsystem_of(&std::fs::read(&twin).unwrap()), 2);
        assert!(twin_is_current(&exe, &twin));

        // An upgrade replaces the source binary; the twin must be rebuilt, not trusted.
        let mut upgraded = synthetic_pe(0x20B, 3);
        upgraded.push(0xAA);
        std::fs::write(&exe, &upgraded).unwrap();
        assert!(!twin_is_current(&exe, &twin));
        let refreshed = ensure_hidden_twin(&exe).unwrap();
        assert_eq!(
            std::fs::read(&refreshed).unwrap().len(),
            upgraded.len(),
            "a stale twin must be replaced by the patched copy of the new binary"
        );
        assert!(twin_is_current(&exe, &refreshed));
    }

    #[test]
    fn a_source_that_is_not_a_pe_produces_no_twin() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dev-prune.exe");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        assert!(ensure_hidden_twin(&exe).is_none());
        assert!(!hidden_twin_path(&exe).exists());
    }

    #[test]
    fn the_upgrade_predicate_stops_once_the_task_names_the_twin() {
        // The XML shape install() produces when the twin registration wins.
        let xml = "<Command>\"C:\\cfg\\bin\\devpw.exe\"</Command>";
        let registered = parse_registered_command(xml).unwrap();
        assert!(
            registered
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(WINDOWS_HIDDEN_BIN))
        );
    }
}
