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
//! On Linux the identity is `linux:<uuid>:<root>`: the filesystem UUID that udev publishes
//! under `/dev/disk/by-uuid`, found by matching the mount's source device in
//! `/proc/self/mountinfo`, plus the `root` field of that mount (the subtree of the
//! filesystem that was mounted). The root is part of the identity because two btrfs
//! subvolumes share one UUID, and a bind mount of `/data` is not the disk's top level.
//! On macOS it is `mac:<uuid>`, the `VolumeUUID` that `diskutil info -plist` reports.
//! Both are read without administrator rights.
//!
//! A filesystem that has no UUID (some FAT volumes without a serial, a network mount,
//! tmpfs, overlay) has no identity, and a registered repository on one still just goes
//! inactive when its mount is absent. Other Unix systems have no identity at all.
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

/// The Unix filesystem root. Every path that is not under a mount point lives on it, so
/// stamping it on each registered repository would record nothing useful and cost a
/// lookup per repository; the system disk is not a drive that comes back elsewhere.
fn is_unix_system_root(root: &Path) -> bool {
    cfg!(unix) && root == Path::new("/")
}

impl VolumeView for SystemVolumes {
    fn identity_at(&self, root: &Path) -> Option<String> {
        if is_unix_system_root(root) {
            return None;
        }
        sys::volume_info(root).map(|v| v.identity)
    }

    fn label_at(&self, root: &Path) -> Option<String> {
        if is_unix_system_root(root) {
            return None;
        }
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

#[derive(Clone)]
struct VolumeInfo {
    identity: String,
    label: String,
}

/// The text parsers behind the Linux and macOS identities.
///
/// They take text and tables, never the machine, so the tests run on every OS. Each
/// platform's `sys` module uses its own half, hence the blanket allow.
#[allow(dead_code)]
mod parse {
    use super::VolumeInfo;
    use std::path::{Path, PathBuf};

    /// One line of `/proc/self/mountinfo`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MountEntry {
        /// The subtree of the filesystem that is mounted here: `/` for a whole filesystem,
        /// `/@home` for a btrfs subvolume, `/data` for a bind mount of a directory.
        pub root: String,
        pub mount_point: PathBuf,
        pub fstype: String,
        pub source: String,
    }

    /// The octal escapes the kernel writes for a space, tab, newline or backslash in a
    /// path (`\040` for a space).
    fn unescape_octal(field: &str) -> String {
        let bytes = field.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\'
                && bytes
                    .get(i + 1..i + 4)
                    .is_some_and(|d| d.iter().all(|b| (b'0'..=b'7').contains(b)))
            {
                let value = u32::from(bytes[i + 1] - b'0') * 64
                    + u32::from(bytes[i + 2] - b'0') * 8
                    + u32::from(bytes[i + 3] - b'0');
                if let Ok(byte) = u8::try_from(value) {
                    out.push(byte);
                    i += 4;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Every mount in `/proc/self/mountinfo` text, in file order. A line that does not
    /// have the documented shape is skipped, never guessed at.
    pub fn parse_mountinfo(text: &str) -> Vec<MountEntry> {
        text.lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                // id parent major:minor root mount-point options [optional...] - fstype source super
                let dash = fields.iter().position(|f| *f == "-")?;
                if dash < 6 || fields.len() < dash + 3 {
                    return None;
                }
                Some(MountEntry {
                    root: unescape_octal(fields[3]),
                    mount_point: PathBuf::from(unescape_octal(fields[4])),
                    fstype: fields[dash + 1].to_string(),
                    source: unescape_octal(fields[dash + 2]),
                })
            })
            .collect()
    }

    /// The mount whose mount point is exactly `at`. A directory that is merely inside a
    /// mount does not count: an unmounted `/mnt/usb` is a plain directory on the system
    /// disk, and must not borrow the system disk's identity. When mounts are stacked on
    /// one point the last one is what the path shows.
    pub fn mount_at<'a>(mounts: &'a [MountEntry], at: &Path) -> Option<&'a MountEntry> {
        mounts.iter().rev().find(|m| m.mount_point == at)
    }

    /// The UUID whose by-uuid link leads to `device`. Both sides are canonical paths, so
    /// `/dev/mapper/vg-root` and `/dev/dm-0` meet.
    pub fn device_entry<'a>(device: &Path, table: &'a [(String, PathBuf)]) -> Option<&'a str> {
        table
            .iter()
            .find(|(_, target)| target == device)
            .map(|(name, _)| name.as_str())
    }

    /// `linux:<uuid>:<root>`. The UUID is lower-cased so a FAT serial reads the same
    /// however udev capitalises it.
    pub fn linux_identity(mount: &MountEntry, uuid: &str) -> String {
        format!("linux:{}:{}", uuid.to_ascii_lowercase(), mount.root)
    }

    /// The identity of one mount, or `None` when it has no UUID to follow.
    ///
    /// Only a mount whose source is a block device can have one. tmpfs, overlay, proc and
    /// every network filesystem put a name or `host:/path` there instead, so they fall
    /// out here without a list of filesystem types to keep up to date.
    pub fn identity_of_mount(
        mount: &MountEntry,
        canonical_source: Option<&Path>,
        by_uuid: &[(String, PathBuf)],
    ) -> Option<String> {
        if !mount.source.starts_with("/dev/") {
            return None;
        }
        let uuid = device_entry(canonical_source?, by_uuid)?;
        Some(linux_identity(mount, uuid))
    }

    /// Every mount that has an identity, as (mount point, identity), except the system
    /// root. A disk that is not the system disk is what can come back elsewhere.
    pub fn mounted_volumes(
        mounts: &[MountEntry],
        by_uuid: &[(String, PathBuf)],
        canonical: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> Vec<(PathBuf, String)> {
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        for (i, mount) in mounts.iter().enumerate() {
            if mount.mount_point == Path::new("/") {
                continue;
            }
            // A mount buried under a later one on the same point is not reachable.
            if mounts[i + 1..]
                .iter()
                .any(|later| later.mount_point == mount.mount_point)
            {
                continue;
            }
            let canonical_source = canonical(&mount.source);
            if let Some(id) = identity_of_mount(mount, canonical_source.as_deref(), by_uuid) {
                out.push((mount.mount_point.clone(), id));
            }
        }
        out
    }

    /// udev writes a space or other awkward character in a `/dev/disk/by-label` name as
    /// `\x20`.
    pub fn unescape_udev(name: &str) -> String {
        let bytes = name.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\'
                && bytes.get(i + 1) == Some(&b'x')
                && let Some(hex) = name.get(i + 2..i + 4)
                && let Ok(byte) = u8::from_str_radix(hex, 16)
            {
                out.push(byte);
                i += 4;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// What `diskutil info -plist` says about one volume.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct DiskutilInfo {
        pub volume_uuid: Option<String>,
        pub volume_name: Option<String>,
        pub mount_point: Option<String>,
    }

    fn unescape_xml(text: &str) -> String {
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    }

    /// The string value of the first `<key>name</key>` in a plist, scanning the text
    /// rather than building a tree: the three keys wanted are top-level strings, and a
    /// plist crate would be a new dependency for that.
    fn plist_string(text: &str, name: &str) -> Option<String> {
        let marker = format!("<key>{name}</key>");
        let after = &text[text.find(&marker)? + marker.len()..];
        let value = after.trim_start().strip_prefix("<string>")?;
        let end = value.find("</string>")?;
        Some(unescape_xml(&value[..end]))
    }

    pub fn parse_diskutil_plist(text: &str) -> DiskutilInfo {
        DiskutilInfo {
            volume_uuid: plist_string(text, "VolumeUUID").filter(|v| !v.is_empty()),
            volume_name: plist_string(text, "VolumeName").filter(|v| !v.is_empty()),
            mount_point: plist_string(text, "MountPoint").filter(|v| !v.is_empty()),
        }
    }

    /// `mac:<uuid>` for the volume diskutil described, provided it is the volume mounted
    /// at `root` itself. diskutil answers for the volume that holds a path it is given,
    /// so a plain directory left behind in `/Volumes` would otherwise be reported as the
    /// system disk. A reply that names no mount point is not trusted.
    pub fn mac_volume(info: &DiskutilInfo, root: &Path) -> Option<VolumeInfo> {
        let mount_point = info.mount_point.as_deref()?;
        if Path::new(mount_point) != root {
            return None;
        }
        Some(VolumeInfo {
            identity: format!("mac:{}", info.volume_uuid.as_deref()?.to_ascii_lowercase()),
            label: info.volume_name.clone().unwrap_or_default(),
        })
    }
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

#[cfg(target_os = "linux")]
mod sys {
    use super::VolumeInfo;
    use super::parse::{
        MountEntry, identity_of_mount, mount_at, mounted_volumes, parse_mountinfo, unescape_udev,
    };
    use crate::constants;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn mounts() -> Option<Vec<MountEntry>> {
        Some(parse_mountinfo(
            &fs::read_to_string(constants::LINUX_MOUNTINFO_PATH).ok()?,
        ))
    }

    /// (name, canonical device) for every link in a udev directory. Canonical on both
    /// sides, because a mount source such as `/dev/mapper/vg-root` is itself a link.
    fn link_table(dir: &str) -> Vec<(String, PathBuf)> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let target = fs::canonicalize(e.path()).ok()?;
                Some((unescape_udev(&e.file_name().to_string_lossy()), target))
            })
            .collect()
    }

    fn canonical(source: &str) -> Option<PathBuf> {
        fs::canonicalize(source).ok()
    }

    pub fn volume_info(root: &Path) -> Option<VolumeInfo> {
        let mounts = mounts()?;
        let mount = mount_at(&mounts, root)?;
        let source = canonical(&mount.source);
        let identity = identity_of_mount(
            mount,
            source.as_deref(),
            &link_table(constants::LINUX_BY_UUID_DIR),
        )?;
        // The label is only shown, so a missing one falls back to the folder's name
        // (udisks mounts a labelled disk at a folder of the same name).
        let label = source
            .as_deref()
            .and_then(|s| {
                link_table(constants::LINUX_BY_LABEL_DIR)
                    .into_iter()
                    .find(|(_, target)| target == s)
                    .map(|(name, _)| name)
            })
            .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        Some(VolumeInfo { identity, label })
    }

    pub fn mounted() -> Vec<(PathBuf, String)> {
        let Some(mounts) = mounts() else {
            return Vec::new();
        };
        mounted_volumes(
            &mounts,
            &link_table(constants::LINUX_BY_UUID_DIR),
            &|source| canonical(source),
        )
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::VolumeInfo;
    use super::parse::{mac_volume, parse_diskutil_plist};
    use crate::constants;
    use std::collections::HashMap;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant, SystemTime};

    /// What a mount root looked like when `diskutil` was last asked about it.
    struct Cached {
        device: u64,
        created: SystemTime,
        asked: Instant,
        info: Option<VolumeInfo>,
    }

    static CACHE: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);

    /// The device number and creation time of `root`, provided it is itself a mount
    /// point (its device differs from its parent's). Two `stat` calls.
    ///
    /// Neither alone is enough to notice a swapped disk: the device number is handed out
    /// again to the next disk that appears as the same `diskN`, and a creation time can
    /// repeat across a copy. Together they change whenever a different volume is mounted
    /// at the same path.
    fn mount_key(root: &Path) -> Option<(u64, SystemTime)> {
        let meta = std::fs::metadata(root).ok()?;
        let parent = std::fs::metadata(root.parent()?).ok()?;
        if meta.dev() == parent.dev() {
            return None;
        }
        Some((meta.dev(), meta.created().ok()?))
    }

    fn ask_diskutil(root: &Path) -> Option<VolumeInfo> {
        let mut cmd = crate::spawn::command(constants::MACOS_DISKUTIL_PATH);
        cmd.args(["info", "-plist"]).arg(root);
        let out = crate::spawn::output_within(
            cmd,
            Duration::from_secs(constants::VOLUME_QUERY_CEILING_SECS),
        )
        .ok()?;
        if !out.status.success() {
            return None;
        }
        mac_volume(
            &parse_diskutil_plist(&String::from_utf8_lossy(&out.stdout)),
            root,
        )
    }

    /// `diskutil` is spawned at most once per volume root per [`constants::MACOS_VOLUME_CACHE_SECS`],
    /// not once per repository: the check before each prune calls this for every
    /// repository on the disk, and the `stat` key above is what makes reusing the answer
    /// safe.
    pub fn volume_info(root: &Path) -> Option<VolumeInfo> {
        let (device, created) = mount_key(root)?;
        let ttl = Duration::from_secs(constants::MACOS_VOLUME_CACHE_SECS);
        if let Ok(guard) = CACHE.lock()
            && let Some(hit) = guard.as_ref().and_then(|c| c.get(root))
            && hit.device == device
            && hit.created == created
            && hit.asked.elapsed() < ttl
        {
            return hit.info.clone();
        }
        // The lock is not held across the spawn, so a slow `diskutil` never blocks the
        // other scan threads' cache hits.
        let info = ask_diskutil(root);
        if let Ok(mut guard) = CACHE.lock() {
            guard.get_or_insert_with(HashMap::new).insert(
                root.to_path_buf(),
                Cached {
                    device,
                    created,
                    asked: Instant::now(),
                    info: info.clone(),
                },
            );
        }
        info
    }

    pub fn mounted() -> Vec<(PathBuf, String)> {
        let Ok(entries) = std::fs::read_dir(constants::MACOS_VOLUMES_DIR) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            // `/Volumes/Macintosh HD` is a link to the system disk, which is not a
            // volume that can come back somewhere else.
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| {
                let path = e.path();
                let info = volume_info(&path)?;
                Some((path, info.identity))
            })
            .collect()
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_a_mount_has_no_identity_and_the_identities_read_are_well_formed() {
        let view = SystemVolumes;
        // A folder that exists but is not a mount point must not borrow the system
        // disk's identity, or an unmounted /mnt/usb would look like a healthy disk.
        let here = std::env::current_dir().unwrap();
        assert_eq!(view.identity_at(&here.join("no-such-mount")), None);
        for (root, id) in view.mounted() {
            assert!(
                id.starts_with("linux:") || id.starts_with("mac:"),
                "{root:?}: {id}"
            );
            assert_ne!(root, Path::new("/"));
        }
        if let Some(id) = view.identity_at(Path::new("/")) {
            assert_eq!(view.identity_at(Path::new("/")), Some(id), "stable");
        }
    }

    use super::parse::*;

    /// A btrfs disk mounted three ways (top level, a subvolume, a bind of a folder), a
    /// vfat stick whose label has a space, an LVM volume, a tmpfs and an NFS share.
    const MOUNTINFO: &str = "\
22 1 0:20 / /run rw,nosuid,nodev - tmpfs tmpfs rw,mode=755
25 1 253:0 / / rw,relatime shared:1 - ext4 /dev/mapper/vg-root rw
30 25 8:17 / /mnt/data rw,relatime shared:5 - btrfs /dev/sdb1 rw,subvolid=5,subvol=/
31 25 8:17 /@home /home rw,relatime shared:6 - btrfs /dev/sdb1 rw,subvolid=256,subvol=/@home
32 25 8:17 /projects /srv/projects rw,relatime shared:7 master:3 - btrfs /dev/sdb1 rw,subvolid=5,subvol=/
33 25 8:33 / /media/me/My\\040Stick rw,nosuid - vfat /dev/sdc1 rw,fmask=0022
34 25 0:45 / /mnt/ram rw - tmpfs tmpfs rw,size=1g
35 25 0:50 / /mnt/share rw - nfs4 server:/export rw,vers=4.2
not a mount line
";

    fn by_uuid() -> Vec<(String, PathBuf)> {
        vec![
            (
                "5a3c2d7e-0001-4a64-9b9f-0123456789ab".into(),
                "/dev/dm-0".into(),
            ),
            ("9F8E7D6C-btrfs-0002".into(), "/dev/sdb1".into()),
            ("ABCD-1234".into(), "/dev/sdc1".into()),
        ]
    }

    fn identity_at_mount(mount_point: &str) -> Option<String> {
        let mounts = parse_mountinfo(MOUNTINFO);
        let mount = mount_at(&mounts, Path::new(mount_point))?;
        identity_of_mount(mount, Some(Path::new(&mount.source)), &by_uuid())
    }

    #[test]
    fn mountinfo_lines_are_read_with_escaped_paths_and_optional_fields() {
        let mounts = parse_mountinfo(MOUNTINFO);
        assert_eq!(mounts.len(), 8, "the junk line is skipped");
        let stick = mount_at(&mounts, Path::new("/media/me/My Stick")).expect("octal escape");
        assert_eq!(stick.fstype, "vfat");
        assert_eq!(stick.source, "/dev/sdc1");
        let bind = mount_at(&mounts, Path::new("/srv/projects")).unwrap();
        assert_eq!(
            bind.root, "/projects",
            "two optional fields before the dash"
        );
        assert_eq!(bind.fstype, "btrfs");
    }

    #[test]
    fn a_btrfs_subvolume_a_bind_mount_and_the_top_level_are_three_identities_and_tmpfs_has_none() {
        let top = identity_at_mount("/mnt/data").unwrap();
        let subvolume = identity_at_mount("/home").unwrap();
        let bind = identity_at_mount("/srv/projects").unwrap();
        assert_eq!(top, "linux:9f8e7d6c-btrfs-0002:/");
        assert_eq!(subvolume, "linux:9f8e7d6c-btrfs-0002:/@home");
        assert_eq!(bind, "linux:9f8e7d6c-btrfs-0002:/projects");
        assert_ne!(top, subvolume);
        assert_ne!(top, bind);
        assert_ne!(subvolume, bind);
        assert_eq!(identity_at_mount("/mnt/ram"), None, "tmpfs");
        assert_eq!(identity_at_mount("/run"), None, "tmpfs");
        assert_eq!(identity_at_mount("/mnt/share"), None, "network filesystem");
    }

    #[test]
    fn a_device_mapper_source_meets_its_by_uuid_link_and_a_fat_serial_is_lower_cased() {
        // The by-uuid link is canonicalised to /dev/dm-0 while the mount names
        // /dev/mapper/vg-root, so the caller hands in the canonical form.
        let mounts = parse_mountinfo(MOUNTINFO);
        let root = mount_at(&mounts, Path::new("/")).unwrap();
        assert_eq!(
            identity_of_mount(root, Some(Path::new("/dev/dm-0")), &by_uuid()).as_deref(),
            Some("linux:5a3c2d7e-0001-4a64-9b9f-0123456789ab:/")
        );
        assert_eq!(
            identity_at_mount("/media/me/My Stick").as_deref(),
            Some("linux:abcd-1234:/")
        );
        // A source that cannot be resolved is "cannot prove", not a guess.
        assert_eq!(identity_of_mount(root, None, &by_uuid()), None);
    }

    #[test]
    fn a_folder_inside_a_mount_is_not_the_mount() {
        let mounts = parse_mountinfo(MOUNTINFO);
        assert!(mount_at(&mounts, Path::new("/mnt/data/projects")).is_none());
        assert!(mount_at(&mounts, Path::new("/mnt/unmounted")).is_none());
    }

    #[test]
    fn the_mounted_list_skips_the_system_root_and_mounts_without_a_uuid() {
        let mounts = parse_mountinfo(MOUNTINFO);
        let listed = mounted_volumes(&mounts, &by_uuid(), &|s| Some(PathBuf::from(s)));
        let points: Vec<&Path> = listed.iter().map(|(p, _)| p.as_path()).collect();
        assert_eq!(
            points,
            [
                Path::new("/mnt/data"),
                Path::new("/home"),
                Path::new("/srv/projects"),
                Path::new("/media/me/My Stick"),
            ]
        );
    }

    #[test]
    fn a_mount_hidden_under_a_later_one_is_not_listed_and_the_later_one_wins() {
        let text = "\
30 25 8:17 / /mnt/usb rw - ext4 /dev/sdb1 rw
31 25 8:33 / /mnt/usb rw - ext4 /dev/sdc1 rw
";
        let mounts = parse_mountinfo(text);
        let table = vec![
            ("aaaa".to_string(), PathBuf::from("/dev/sdb1")),
            ("bbbb".to_string(), PathBuf::from("/dev/sdc1")),
        ];
        let listed = mounted_volumes(&mounts, &table, &|s| Some(PathBuf::from(s)));
        assert_eq!(
            listed,
            [(PathBuf::from("/mnt/usb"), "linux:bbbb:/".to_string())]
        );
        let top = mount_at(&mounts, Path::new("/mnt/usb")).unwrap();
        assert_eq!(top.source, "/dev/sdc1");
    }

    #[test]
    fn udev_label_escapes_are_decoded() {
        assert_eq!(unescape_udev(r"My\x20Stick"), "My Stick");
        assert_eq!(unescape_udev("plain"), "plain");
        assert_eq!(unescape_udev(r"odd\x"), r"odd\x");
    }

    const DISKUTIL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>DeviceIdentifier</key>
	<string>disk4s2</string>
	<key>FilesystemName</key>
	<string>APFS</string>
	<key>MountPoint</key>
	<string>/Volumes/Backup &amp; Work</string>
	<key>VolumeName</key>
	<string>Backup &amp; Work</string>
	<key>VolumeUUID</key>
	<string>6C1F4E0A-3B2D-4F55-9C0E-7A1D2B3C4D5E</string>
	<key>Writable</key>
	<true/>
</dict>
</plist>
"#;

    #[test]
    fn a_diskutil_plist_yields_its_volume_uuid_name_and_mount_point() {
        let info = parse_diskutil_plist(DISKUTIL);
        assert_eq!(
            info.volume_uuid.as_deref(),
            Some("6C1F4E0A-3B2D-4F55-9C0E-7A1D2B3C4D5E")
        );
        assert_eq!(info.volume_name.as_deref(), Some("Backup & Work"));
        let volume = mac_volume(&info, Path::new("/Volumes/Backup & Work")).unwrap();
        assert_eq!(volume.identity, "mac:6c1f4e0a-3b2d-4f55-9c0e-7a1d2b3c4d5e");
        assert_eq!(volume.label, "Backup & Work");
    }

    #[test]
    fn a_diskutil_answer_for_another_mount_or_without_a_uuid_is_not_an_identity() {
        let info = parse_diskutil_plist(DISKUTIL);
        // A plain folder in /Volumes: diskutil answers with the volume that holds it.
        assert!(mac_volume(&info, Path::new("/Volumes/Backup & Work 1")).is_none());
        assert!(mac_volume(&info, Path::new("/")).is_none());
        let no_uuid = DISKUTIL.replace("VolumeUUID", "SomethingElse");
        assert!(
            mac_volume(
                &parse_diskutil_plist(&no_uuid),
                Path::new("/Volumes/Backup & Work")
            )
            .is_none()
        );
        assert_eq!(parse_diskutil_plist("garbage"), DiskutilInfo::default());
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
