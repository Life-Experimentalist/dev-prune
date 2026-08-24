// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! What this project's own installer did, written down beside the binary it installed.
//!
//! Three programs used to derive the same facts independently — `install.sh`,
//! `install.ps1` and this binary each worked out which copy is the managed one, what
//! the last install actually wrote, and whether the `devp` twin and the PATH entry came
//! from us or were already there. Three derivations of one truth is how they drift, and
//! the drift is invisible until an uninstall removes a PATH entry it never added.
//!
//! So the installer writes it down. `<bindir>/install.json` sits next to the binary, is
//! created by whichever installer performed the install, and survives the shell that ran
//! it — which is the whole point, because a shell variable does not.
//!
//! It is deliberately **not** a source of truth about the machine. [`Channel::detect`]
//! stays the classifier: a receipt cannot describe a copy that arrived through `cargo
//! install`, and a receipt that is missing means only that no installer of ours wrote
//! one — never that the binary is unmanaged. Everything here is therefore advisory, read
//! with `Option`, and reported rather than acted on.
//!
//! [`Channel::detect`]: crate::channel::Channel::detect

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The current on-disk shape.
///
/// Bumped only when a field changes meaning. A reader that finds a higher number than it
/// knows ignores the file rather than guessing — the receipt is advisory, so "ignore it"
/// is always a safe answer, and it is the only answer that cannot corrupt anything.
pub const SCHEMA: u32 = 1;

/// One install, as the installer that performed it saw it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// On-disk shape. See [`SCHEMA`].
    pub schema: u32,
    /// The version that was written, exactly as the installer resolved it.
    pub version: String,
    /// Which channel performed it — `installer` for both scripts, or the label of the
    /// channel a later in-place upgrade came through.
    pub channel: String,
    /// Which program wrote this file: `install.sh`, `install.ps1`, or `devp`.
    pub installed_by: String,
    /// When, in UTC, RFC 3339. Absolute — never "today".
    pub installed_at: String,
    /// Absolute path of the binary that was installed.
    pub exe: String,
    /// Whether the installer also wrote the `devp` twin beside it.
    pub alias: bool,
    /// Whether the directory is on PATH because one of our installers put it there, on
    /// this run or an earlier one. False when PATH was left alone on request, and false
    /// when whatever makes `devp` resolve is something neither script wrote.
    pub path_entry: bool,
}

/// Where the receipt lives: beside the managed binary, in the one directory no package
/// manager owns.
pub fn path() -> Result<PathBuf> {
    Ok(crate::setup::managed_bin_dir()?.join(crate::constants::INSTALL_RECEIPT_FILE))
}

/// Read the receipt, or `None` if there is not one worth trusting.
///
/// Every failure is a `None`: absent, unreadable, malformed, or written by a future
/// version that means something different by these fields. Nothing here is load-bearing
/// enough to be worth an error path, and a caller forced to handle one would only end up
/// ignoring it.
pub fn load() -> Option<Receipt> {
    load_from(&path().ok()?)
}

fn load_from(file: &Path) -> Option<Receipt> {
    let text = std::fs::read_to_string(file).ok()?;
    let receipt: Receipt = serde_json::from_str(&text).ok()?;
    (receipt.schema <= SCHEMA).then_some(receipt)
}

/// Write the receipt, replacing any previous one.
///
/// Atomic, like every other state file this program writes: staged beside the target and
/// renamed over it, so a write that dies half-way leaves the old receipt intact rather
/// than a truncated one that parses as nothing.
pub fn write(receipt: &Receipt) -> Result<()> {
    write_to(&path()?, receipt)
}

fn write_to(file: &Path, receipt: &Receipt) -> Result<()> {
    let body = serde_json::to_string_pretty(receipt)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let staged = file.with_extension("json.new");
    std::fs::write(&staged, format!("{body}\n"))
        .with_context(|| format!("could not write {}", staged.display()))?;
    std::fs::rename(&staged, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })?;
    Ok(())
}

/// Move an existing receipt forward after an in-place upgrade.
///
/// Only ever an update: a missing receipt stays missing. `devp update --install` can
/// replace a managed copy that some *other* manager installed, and inventing a receipt
/// there would claim an installer ran when none did — exactly the drift this file exists
/// to prevent. Best-effort by design; an upgrade does not fail because a note about it
/// could not be written.
pub fn refresh_after_upgrade(version: &str) {
    let Ok(file) = path() else { return };
    let Some(mut receipt) = load_from(&file) else {
        return;
    };
    receipt.version = version.to_string();
    receipt.installed_at = chrono::Utc::now().to_rfc3339();
    receipt.installed_by = "devp".to_string();
    let _ = write_to(&file, &receipt);
}

/// One line for `devp doctor` and `devp install`: what the installer wrote, and when.
///
/// The date is the useful half. "Install channel: install script" is already on the
/// screen from [`Channel::detect`]; what nothing else can answer is whether that copy
/// arrived last week or two years ago.
///
/// [`Channel::detect`]: crate::channel::Channel::detect
pub fn summary(receipt: &Receipt) -> String {
    let when = chrono::DateTime::parse_from_rfc3339(&receipt.installed_at)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| receipt.installed_at.clone());
    format!("v{} by {} on {when}", receipt.version, receipt.installed_by)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Receipt {
        Receipt {
            schema: SCHEMA,
            version: "1.9.0".to_string(),
            channel: "installer".to_string(),
            installed_by: "install.sh".to_string(),
            installed_at: "2026-08-25T09:14:02Z".to_string(),
            exe: "/home/k/.config/dev-prune/bin/dev-prune".to_string(),
            alias: true,
            path_entry: true,
        }
    }

    #[test]
    fn a_receipt_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("install.json");
        write_to(&file, &sample()).unwrap();
        let back = load_from(&file).unwrap();
        assert_eq!(back.version, "1.9.0");
        assert_eq!(back.installed_by, "install.sh");
        assert!(back.alias);
        assert!(back.path_entry);
    }

    #[test]
    fn a_newer_schema_is_ignored_rather_than_guessed_at() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("install.json");
        let mut future = sample();
        future.schema = SCHEMA + 1;
        write_to(&file, &future).unwrap();
        assert!(load_from(&file).is_none());
    }

    #[test]
    fn nothing_and_nonsense_both_read_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_from(&dir.path().join("install.json")).is_none());
        let junk = dir.path().join("junk.json");
        std::fs::write(&junk, "not json at all").unwrap();
        assert!(load_from(&junk).is_none());
    }

    #[test]
    fn the_summary_shortens_the_timestamp_to_a_date() {
        assert_eq!(summary(&sample()), "v1.9.0 by install.sh on 2026-08-25");
    }

    #[test]
    fn an_unparseable_timestamp_is_printed_as_it_was_written() {
        let mut odd = sample();
        odd.installed_at = "sometime".to_string();
        assert_eq!(summary(&odd), "v1.9.0 by install.sh on sometime");
    }

    // The two installers write this file with `printf` and `Set-Content`, not with serde,
    // so the field names are a contract between three programs in three languages. A
    // rename on this side would be invisible until someone's receipt stopped parsing.
    #[test]
    fn the_field_names_are_what_the_shell_installers_write() {
        let json = serde_json::to_string(&sample()).unwrap();
        for key in [
            "schema",
            "version",
            "channel",
            "installed_by",
            "installed_at",
            "exe",
            "alias",
            "path_entry",
        ] {
            assert!(json.contains(&format!("\"{key}\"")), "missing {key}");
        }
    }
}
