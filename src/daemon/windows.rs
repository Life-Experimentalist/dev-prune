// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Windows Task Scheduler integration.

use crate::constants::{WINDOWS_TASK_NAME as TASK_NAME, WINDOWS_WINDOWLESS_BIN};
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

/// Builds the schtasks arguments for a sessionless (S4U) installation.
///
/// `/RU <account> /NP` registers the task to run whether the user is logged on or not,
/// without storing a password (an S4U logon). Such a task runs in a non-interactive
/// session — without it, every firing of the console binary flashes a black window at
/// whoever is logged in, which reads as malware to anyone watching their own screen.
pub fn build_install_command_windowless(
    exe_path: &str,
    interval_days: u64,
    account: &str,
) -> Vec<String> {
    let mut args = build_install_command(exe_path, interval_days);
    args.extend(["/RU".to_string(), account.to_string(), "/NP".to_string()]);
    args
}

/// The account to register the S4U task under, as `DOMAIN\user`.
fn current_account() -> Option<String> {
    let user = std::env::var("USERNAME").ok().filter(|s| !s.is_empty())?;
    Some(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    })
}

/// Marker recording that this machine's scheduler refused the S4U registration.
///
/// Some policies deny S4U logons to non-elevated callers, and a filesystem that cannot
/// hold the windowless twin refuses that route too. Without the marker, every setup
/// pass would re-try the upgrade, fail, and re-register the visible task — churning the
/// scheduler on every single `devp` invocation.
fn windowless_refused_marker() -> Option<PathBuf> {
    crate::config::Registry::config_dir()
        .ok()
        .map(|dir| dir.join(crate::constants::SCHEDULER_WINDOWLESS_REFUSED_MARKER))
}

/// Removes every refusal marker from `dir`, whatever it is called.
///
/// Matching the `scheduler-*-refused` family rather than one filename is what retires
/// the spelling an older release used, so a rename cannot strand an empty file in the
/// config directory of every machine that upgrades. Split out from `install` so it can
/// be tested against a real directory without going near the scheduler.
fn sweep_refusal_markers(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("scheduler-") && name.ends_with("-refused") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The windowless twin's path, beside the binary the scheduler would otherwise run.
fn windowless_twin_path(exe_path: &Path) -> PathBuf {
    exe_path.with_file_name(WINDOWS_WINDOWLESS_BIN)
}

/// The `devpw.exe` that shipped with the running binary, if this delivery carries one.
///
/// It is a build target, so it sits beside whatever is executing right now — the
/// unzipped archive, `~/.cargo/bin`, the directory an installer wrote. A delivery that
/// predates it, or one that only unpacks the two console names, simply has no twin here
/// and `install` falls through to its next registration tier.
fn shipped_windowless_binary() -> Option<PathBuf> {
    let shipped = std::env::current_exe()
        .ok()?
        .with_file_name(WINDOWS_WINDOWLESS_BIN);
    shipped.is_file().then_some(shipped)
}

/// Put `devpw.exe` beside `exe_path`, returning its path once it is there.
///
/// The twin is *placed*, never generated. An earlier release built it on the machine by
/// reading its own image, rewriting the PE subsystem field and writing the result out
/// under a new name — a program emitting a modified executable copy of itself and then
/// registering it for persistence, which is what a dropper does and what Sophos
/// quarantined the binary for. The subsystem now comes from the linker instead, so the
/// only thing left to do here is the same hard-link-or-staged-copy every other managed
/// file gets.
fn ensure_windowless_twin(exe_path: &Path) -> Option<PathBuf> {
    // Almost always the managed copy under the config directory, because `get_exe_path`
    // resolves there first. The exception is a machine where that copy could not be made,
    // and then `exe_path` is wherever the package manager put the binary — which for
    // WinGet and Scoop is a directory they version and replace whole on upgrade. A twin
    // written there is orphaned by the next upgrade while the scheduled task still names
    // it, so the daemon runs a release the user thinks they replaced. Refusing costs the
    // windowless route only; `install` falls through to the S4U registration below it.
    if crate::channel::Channel::detect().replaces_its_directory()
        && crate::setup::managed_exe_path().is_ok_and(|managed| managed != exe_path)
    {
        return None;
    }
    place_windowless_twin(
        &shipped_windowless_binary()?,
        &windowless_twin_path(exe_path),
    )
}

/// Copy `shipped` to `twin`, or confirm the copy already there is the same release.
///
/// Split out from `ensure_windowless_twin` so it can be tested against two real paths: the
/// caller's half depends on where this process happens to be running from, which under a
/// test harness is not something a test may assume.
fn place_windowless_twin(shipped: &Path, twin: &Path) -> Option<PathBuf> {
    // Running out of the managed directory already: the shipped twin *is* the twin.
    if shipped == twin {
        return Some(twin.to_path_buf());
    }
    if twin.is_file() {
        if crate::setup::same_contents(twin, shipped) {
            return Some(twin.to_path_buf());
        }
        // An upgrade replaced the shipped binary, so the placed one is a previous
        // release that the scheduled task still names. Replacing a running executable
        // fails on Windows; the next pass that is not itself the twin retries.
        if std::fs::remove_file(twin).is_err() {
            return Some(twin.to_path_buf());
        }
    }
    if std::fs::hard_link(shipped, twin).is_ok() {
        return Some(twin.to_path_buf());
    }
    // Never `fs::copy` onto a name that exists: the usual reason `hard_link` fails is
    // that a concurrent pass created it as a hard link to `shipped`, and `fs::copy`
    // opens its destination with `O_TRUNC` — truncating a hard link empties the shared
    // inode, destroying the very file being copied. `setup::ensure_twin_of` records the
    // CI outage that taught this.
    if twin.is_file() {
        return Some(twin.to_path_buf());
    }
    // Stage beside and rename into place, so a scheduler firing mid-copy never runs a
    // torn binary.
    let staging = twin.with_extension("new");
    if std::fs::copy(shipped, &staging).is_ok() && std::fs::rename(&staging, twin).is_ok() {
        return Some(twin.to_path_buf());
    }
    let _ = std::fs::remove_file(&staging);
    // The rename loses only to a concurrent pass that placed its own copy, which serves
    // exactly as well.
    twin.is_file().then(|| twin.to_path_buf())
}

/// Refresh the windowless twin after an upgrade, when one is in use.
///
/// The scheduled task names `devpw.exe`, so replacing the managed binary alone would
/// leave the daemon running the previous release forever. Only refreshes a twin that
/// already exists — creating one is the installer's decision, not a side effect of
/// every settled setup pass.
pub fn refresh_windowless_twin() {
    let exe_path = get_exe_path();
    if windowless_twin_path(&exe_path).is_file() {
        let _ = ensure_windowless_twin(&exe_path);
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

    // Every path out of this function ends by either writing the refusal marker or
    // clearing it, so start from neither.
    if let Ok(dir) = crate::config::Registry::config_dir() {
        sweep_refusal_markers(&dir);
    }

    if let Some(twin) = ensure_windowless_twin(&exe_path) {
        let args = build_install_command(&twin.to_string_lossy(), interval_days);
        if let Ok(output) = spawn::command("schtasks").args(&args).output()
            && output.status.success()
        {
            // A machine that once refused may have been elevated or unlocked since;
            // the marker must not outlive the refusal it records.
            if let Some(marker) = windowless_refused_marker() {
                let _ = std::fs::remove_file(marker);
            }
            return Ok(());
        }
    }

    let exe_str = exe_path.to_string_lossy();
    if let Some(account) = current_account() {
        let args = build_install_command_windowless(&exe_str, interval_days, &account);
        if let Ok(output) = spawn::command("schtasks").args(&args).output()
            && output.status.success()
        {
            // No window here either, but no logon session's credentials. The marker
            // still records that the *preferred* route failed, so settled passes stop
            // retrying it; an elevated `devp daemon on` retries regardless.
            if let Some(marker) = windowless_refused_marker() {
                let _ = std::fs::write(marker, "");
            }
            return Ok(());
        }
    }
    if let Some(marker) = windowless_refused_marker() {
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
/// already refused the S4U registration that fixes it. `ensure_daemon` uses this to
/// upgrade tasks registered by versions that only knew the interactive logon, or that
/// predate the windowless twin.
pub fn wants_windowless_upgrade() -> bool {
    let Some(xml) = task_xml() else {
        // No readable definition means no task to upgrade — re-registering a task
        // that cannot be read would be guessing.
        return false;
    };
    // A task already pointing at `devpw.exe` cannot flash regardless of logon type;
    // there is nothing left to upgrade to.
    if parse_registered_command(&xml).is_some_and(|exe| {
        exe.file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(WINDOWS_WINDOWLESS_BIN))
    }) {
        return false;
    }
    if windowless_refused_marker().is_some_and(|m| m.exists()) {
        return false;
    }
    // An S4U task registered by an earlier release shows no window but is sessionless;
    // the twin route restores mapped drives too, so it is still an upgrade — an
    // interactive console task doubly so.
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
    fn the_sweep_takes_every_spelling_of_the_marker_and_leaves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let unrelated = dir.path().join("repos.json");
        let current = dir
            .path()
            .join(crate::constants::SCHEDULER_WINDOWLESS_REFUSED_MARKER);
        // What 1.14.0 wrote.
        let legacy = dir.path().join("scheduler-hidden-refused");
        for path in [&unrelated, &current, &legacy] {
            std::fs::write(path, "").unwrap();
        }

        sweep_refusal_markers(dir.path());

        assert!(unrelated.exists(), "the sweep must not touch the registry");
        assert!(!current.exists());
        assert!(
            !legacy.exists(),
            "an upgrade must not strand the old spelling"
        );
    }

    #[test]
    fn the_windowless_command_adds_the_password_less_run_as_and_nothing_else() {
        let visible = build_install_command("dev-prune.exe", 2);
        let windowless = build_install_command_windowless("dev-prune.exe", 2, "PC\\krish");
        assert_eq!(windowless[..visible.len()], visible[..]);
        assert_eq!(windowless[visible.len()..], ["/RU", "PC\\krish", "/NP"]);
    }

    #[test]
    fn the_interactive_logon_is_recognised_and_the_s4u_one_is_not() {
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

    #[test]
    fn the_twin_lives_beside_the_managed_binary_under_the_reserved_name() {
        let twin = windowless_twin_path(Path::new(r"C:\cfg\bin\dev-prune.exe"));
        assert_eq!(twin, Path::new(r"C:\cfg\bin\devpw.exe"));
    }

    /// A delivery that carries no `devpw.exe` must not be answered by inventing one —
    /// `install` has two further tiers that still work without it.
    #[test]
    fn a_missing_shipped_twin_places_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let shipped = dir.path().join("nowhere").join(WINDOWS_WINDOWLESS_BIN);
        let twin = dir.path().join(WINDOWS_WINDOWLESS_BIN);
        assert!(place_windowless_twin(&shipped, &twin).is_none());
        assert!(!twin.exists());
    }

    #[test]
    fn the_shipped_twin_is_placed_verbatim_and_refreshed_when_it_is_a_previous_release() {
        let ship_dir = tempfile::tempdir().unwrap();
        let managed = tempfile::tempdir().unwrap();
        let shipped = ship_dir.path().join(WINDOWS_WINDOWLESS_BIN);
        let twin = managed.path().join(WINDOWS_WINDOWLESS_BIN);
        std::fs::write(&shipped, b"release one").unwrap();

        assert_eq!(place_windowless_twin(&shipped, &twin).unwrap(), twin);
        assert_eq!(std::fs::read(&twin).unwrap(), b"release one");

        // Idempotent: an unchanged release must not rewrite the file.
        assert_eq!(place_windowless_twin(&shipped, &twin).unwrap(), twin);
        assert_eq!(std::fs::read(&twin).unwrap(), b"release one");

        // An upgrade replaces the shipped binary; the placed copy is stale and the
        // scheduled task still names it, so it must be replaced rather than trusted.
        std::fs::write(&shipped, b"release two, which is longer").unwrap();
        assert_eq!(place_windowless_twin(&shipped, &twin).unwrap(), twin);
        assert_eq!(
            std::fs::read(&twin).unwrap(),
            b"release two, which is longer"
        );
        assert!(!twin.with_extension("new").exists(), "no staging debris");
    }

    /// The twin is byte-for-byte what shipped. Nothing here edits an executable, which is
    /// the whole point of making it a build target.
    #[test]
    fn placing_the_twin_never_alters_a_byte_of_it() {
        let ship_dir = tempfile::tempdir().unwrap();
        let managed = tempfile::tempdir().unwrap();
        let shipped = ship_dir.path().join(WINDOWS_WINDOWLESS_BIN);
        let twin = managed.path().join(WINDOWS_WINDOWLESS_BIN);
        let image: Vec<u8> = (0u16..4096).map(|b| (b % 251) as u8).collect();
        std::fs::write(&shipped, &image).unwrap();

        place_windowless_twin(&shipped, &twin).unwrap();
        assert_eq!(std::fs::read(&twin).unwrap(), image);
        assert_eq!(std::fs::read(&shipped).unwrap(), image);
    }

    /// Running out of the managed directory already: there is nothing to place, and in
    /// particular nothing that could truncate the file onto itself.
    #[test]
    fn a_twin_that_is_already_the_shipped_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let twin = dir.path().join(WINDOWS_WINDOWLESS_BIN);
        std::fs::write(&twin, b"the one and only").unwrap();
        assert_eq!(place_windowless_twin(&twin, &twin).unwrap(), twin);
        assert_eq!(std::fs::read(&twin).unwrap(), b"the one and only");
    }

    #[test]
    fn the_upgrade_predicate_stops_once_the_task_names_the_twin() {
        // The XML shape install() produces when the twin registration wins.
        let xml = "<Command>\"C:\\cfg\\bin\\devpw.exe\"</Command>";
        let registered = parse_registered_command(xml).unwrap();
        assert!(
            registered
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(WINDOWS_WINDOWLESS_BIN))
        );
    }
}
