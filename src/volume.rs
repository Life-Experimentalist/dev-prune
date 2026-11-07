// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! Which physical volume a path lives on, independent of the letter it is mounted under.
//!
//! A removable drive can come back as `F:` after it was `E:`, and a letter can be handed to
//! a different drive altogether. A registry that only remembers `E:\work\api` cannot tell
//! "my drive is unplugged" from "the letter now belongs to another disk", and the second
//! case is the dangerous one: the same relative path may exist there and look healthy.
//! A volume identity is a string that stays with the filesystem, so the registry can
//! recognise its drive wherever it is mounted.
//!
//! On Windows the identity is `win:XXXX-XXXX:FS`: the volume serial number from
//! `GetVolumeInformationW` and the filesystem name. Everything is read through the
//! ordinary drive-root APIs, so no raw `\\.\E:` handle is opened and no administrator
//! rights are involved.
//!
//! Elsewhere there is no identity and no list of volumes. Linux and macOS mount points
//! (`/mnt/usb`, `/Volumes/Backup`) keep their names across a replug, which is the
//! problem letters have and mount points do not, so the mount-based logic in
//! [`crate::config`] already covers them.
//!
//! The registry never calls the system directly. It takes a [`VolumeView`], so a test can
//! stand in drives that appear, vanish and swap letters without any hardware.

use std::path::{Path, PathBuf};

/// What the registry needs to know about the machine's volumes.
///
/// `Sync` because the liveness scan asks it from several worker threads at once.
pub trait VolumeView: Sync {
    /// The stable identity of the volume mounted at `root`, or `None` when it has none or
    /// cannot be read (not connected, a network share, serial number 0, an API failure).
    /// `None` always means "cannot prove", never "a different volume".
    fn identity_at(&self, root: &Path) -> Option<String>;

    /// The volume's label, for display only. Never used to decide anything.
    fn label_at(&self, root: &Path) -> Option<String>;

    /// Every mounted local volume as (root, identity). Only fixed and removable drives
    /// that have an identity are listed.
    fn mounted(&self) -> Vec<(PathBuf, String)>;
}

/// The machine's real volumes.
pub struct SystemVolumes;

impl VolumeView for SystemVolumes {
    fn identity_at(&self, root: &Path) -> Option<String> {
        sys::volume_info(root).map(|v| v.identity)
    }

    fn label_at(&self, root: &Path) -> Option<String> {
        sys::volume_info(root)
            .map(|v| v.label)
            .filter(|l| !l.is_empty())
    }

    fn mounted(&self) -> Vec<(PathBuf, String)> {
        sys::mounted()
    }
}

/// `win:1A2B-3C4D:NTFS`, or `None` for serial 0 or an empty filesystem name.
///
/// Serial 0 is what virtual and unformatted devices report, and every one of them would
/// share the same "identity". Treating it as unknown keeps two unrelated drives from ever
/// matching each other.
pub fn format_identity(serial: u32, filesystem: &str) -> Option<String> {
    let filesystem = filesystem.trim();
    if serial == 0 || filesystem.is_empty() {
        return None;
    }
    Some(format!(
        "win:{:04X}-{:04X}:{}",
        serial >> 16,
        serial & 0xFFFF,
        filesystem
    ))
}

/// A volume root as a person would write it: `E:` for `\\?\E:\`, and the path itself
/// where it has no drive letter.
pub fn display_root(root: &Path) -> String {
    let text = root.display().to_string();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let trimmed = text.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        text.to_string()
    } else {
        trimmed.to_string()
    }
}

struct VolumeInfo {
    identity: String,
    label: String,
}

#[cfg(windows)]
mod sys {
    use super::{VolumeInfo, format_identity};
    use std::path::{Component, Path, PathBuf, Prefix};

    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, QueryDosDeviceW,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SetThreadErrorMode,
    };

    /// `GetDriveTypeW` results for removable media and fixed disks, from `fileapi.h`.
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    /// The drive letter a root names, from `E:\` or `\\?\E:\`. UNC shares and anything
    /// else have no letter and so no identity.
    fn drive_letter(root: &Path) -> Option<u8> {
        match root.components().next()? {
            Component::Prefix(p) => match p.kind() {
                Prefix::Disk(b) | Prefix::VerbatimDisk(b) => Some(b.to_ascii_uppercase()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Runs `f` with the "no disk in drive" dialog suppressed on this thread, then puts the
    /// previous mode back. The mode is per thread, so nothing outside `f` is affected.
    fn quietly<R>(f: impl FnOnce() -> R) -> R {
        let mut old = 0u32;
        // SAFETY: `old` is a valid out-pointer for the duration of the call.
        let changed = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut old) } != 0;
        let out = f();
        if changed {
            // SAFETY: a null old-mode pointer is allowed and `old` came from the call above.
            unsafe { SetThreadErrorMode(old, std::ptr::null_mut()) };
        }
        out
    }

    fn wide_root(letter: u8) -> [u16; 4] {
        [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0]
    }

    fn is_local_disk(letter: u8) -> bool {
        let root = wide_root(letter);
        // SAFETY: `root` is NUL-terminated and outlives the call.
        let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
        kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE
    }

    /// A `subst X: C:\folder` drive reports as a fixed disk and reads the serial number of
    /// the volume underneath, which would give two letters one identity.
    fn is_subst(letter: u8) -> bool {
        let device = [u16::from(letter), u16::from(b':'), 0];
        let mut target = [0u16; 512];
        // SAFETY: `device` is NUL-terminated, `target` is writable for its stated length.
        let written =
            unsafe { QueryDosDeviceW(device.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
        let prefix: Vec<u16> = r"\??\".encode_utf16().collect();
        written > 0 && target.starts_with(&prefix)
    }

    fn text(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    pub fn volume_info(root: &Path) -> Option<VolumeInfo> {
        let letter = drive_letter(root)?;
        if !is_local_disk(letter) || is_subst(letter) {
            return None;
        }
        let wide = wide_root(letter);
        let mut label = [0u16; 261];
        let mut filesystem = [0u16; 261];
        let mut serial = 0u32;
        let ok = quietly(|| {
            // SAFETY: `wide` is NUL-terminated; every buffer is writable for the length
            // passed beside it; the three optional out-pointers are null, which the API
            // documents as "not wanted".
            unsafe {
                GetVolumeInformationW(
                    wide.as_ptr(),
                    label.as_mut_ptr(),
                    label.len() as u32,
                    &mut serial,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    filesystem.as_mut_ptr(),
                    filesystem.len() as u32,
                )
            }
        });
        if ok == 0 {
            return None;
        }
        Some(VolumeInfo {
            identity: format_identity(serial, &text(&filesystem))?,
            label: text(&label),
        })
    }

    pub fn mounted() -> Vec<(PathBuf, String)> {
        // SAFETY: takes no arguments and only reads the logical-drive bitmask.
        let mask = unsafe { GetLogicalDrives() };
        (0u8..26)
            .filter(|bit| mask & (1u32 << bit) != 0)
            .filter_map(|bit| {
                let letter = b'A' + bit;
                let root = PathBuf::from(format!("{}:\\", letter as char));
                let info = volume_info(&root)?;
                Some((root, info.identity))
            })
            .collect()
    }
}

#[cfg(not(windows))]
mod sys {
    use super::VolumeInfo;
    use std::path::{Path, PathBuf};

    pub fn volume_info(_root: &Path) -> Option<VolumeInfo> {
        None
    }

    pub fn mounted() -> Vec<(PathBuf, String)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_carries_the_serial_in_two_halves_and_the_filesystem() {
        assert_eq!(
            format_identity(0x1A2B_3C4D, "NTFS").as_deref(),
            Some("win:1A2B-3C4D:NTFS")
        );
        assert_eq!(
            format_identity(0x0000_00FF, "exFAT").as_deref(),
            Some("win:0000-00FF:exFAT")
        );
    }

    #[test]
    fn serial_zero_and_a_missing_filesystem_name_give_no_identity() {
        assert_eq!(format_identity(0, "NTFS"), None);
        assert_eq!(format_identity(0x1234_5678, ""), None);
        assert_eq!(format_identity(0x1234_5678, "  "), None);
    }

    #[test]
    fn a_root_is_shown_the_way_a_person_writes_it() {
        assert_eq!(display_root(Path::new(r"\\?\E:\")), "E:");
        assert_eq!(display_root(Path::new(r"E:\")), "E:");
        assert_eq!(display_root(Path::new("/mnt/usb")), "/mnt/usb");
        assert_eq!(display_root(Path::new("/")), "/");
    }

    #[cfg(not(windows))]
    #[test]
    fn other_platforms_report_no_identity_and_no_volumes() {
        let view = SystemVolumes;
        assert_eq!(view.identity_at(Path::new("/")), None);
        assert_eq!(view.label_at(Path::new("/")), None);
        assert!(view.mounted().is_empty());
    }

    #[cfg(windows)]
    fn current_drive_root() -> PathBuf {
        let here = std::env::current_dir().unwrap();
        match here.components().next() {
            Some(std::path::Component::Prefix(p)) => {
                PathBuf::from(format!("{}\\", p.as_os_str().to_string_lossy()))
            }
            _ => panic!("the working directory has no drive prefix: {here:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_current_drive_has_a_well_formed_stable_identity_when_it_has_one() {
        let root = current_drive_root();
        let first = SystemVolumes.identity_at(&root);
        // Some virtual drives report serial 0 and so no identity; that is allowed, and
        // the rest of the contract only applies when there is one.
        if let Some(id) = &first {
            let rest = id.strip_prefix("win:").expect("win: prefix");
            let (serial, fs) = rest.split_once(':').expect("serial:filesystem");
            assert_eq!(serial.len(), 9, "{id}");
            assert_eq!(serial.as_bytes()[4], b'-', "{id}");
            assert!(
                serial
                    .chars()
                    .all(|c| c == '-' || c.is_ascii_digit() || ('A'..='F').contains(&c)),
                "{id}"
            );
            assert!(!fs.is_empty(), "{id}");
        }
        assert_eq!(
            SystemVolumes.identity_at(&root),
            first,
            "stable across calls"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_plain_and_verbatim_spellings_of_a_root_name_the_same_volume() {
        let root = current_drive_root();
        let verbatim = PathBuf::from(format!(r"\\?\{}", root.display()));
        assert_eq!(
            SystemVolumes.identity_at(&root),
            SystemVolumes.identity_at(&verbatim)
        );
    }

    #[cfg(windows)]
    #[test]
    fn every_mounted_volume_is_a_drive_root_with_a_well_formed_identity() {
        for (root, id) in SystemVolumes.mounted() {
            assert!(id.starts_with("win:"), "{id}");
            assert_eq!(root.as_os_str().len(), 3, "{root:?}");
            assert_eq!(
                SystemVolumes.identity_at(&root).as_deref(),
                Some(id.as_str())
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_letter_with_no_drive_and_a_network_share_have_no_identity() {
        let unused = ('D'..='Z')
            .rev()
            .map(|c| PathBuf::from(format!("{c}:\\")))
            .find(|root| !root.exists());
        if let Some(root) = unused {
            assert_eq!(SystemVolumes.identity_at(&root), None, "{root:?}");
            assert!(SystemVolumes.mounted().iter().all(|(r, _)| *r != root));
        }
        assert_eq!(
            SystemVolumes.identity_at(Path::new(r"\\server\share\")),
            None
        );
        assert_eq!(
            SystemVolumes.identity_at(Path::new(r"\\?\UNC\server\share\")),
            None
        );
    }
}
