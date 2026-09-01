// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Making the managed binaries reachable from a fresh shell, and undoing it.
//
// pip in a virtualenv, `npx`, `uv tool run` — every one of those puts the binary
// somewhere that stops existing, or stops being on PATH, the moment the environment
// closes. The managed pair under `<config>/bin` already outlives them (the scheduler
// and the git hooks are registered against it for exactly that reason); this module
// closes the last gap by making the *user's own shell* find that copy too.
//
// On Windows that means one entry in the user PATH (`HKCU\Environment`), read and
// written as raw registry data. The obvious .NET call —
// `[Environment]::GetEnvironmentVariable('Path','User')` — hands back the *expanded*
// value, and writing that back bakes `%USERPROFILE%`-style entries into literal paths
// for good; going through the registry API with `DoNotExpandEnvironmentNames`, and
// preserving the value's `REG_EXPAND_SZ`/`REG_SZ` kind, leaves every entry exactly as
// its owner spelled it.
// Everywhere else it means symlinks in `~/.local/bin`, the XDG-conventional user
// executable directory — no shell profile is ever edited, because a profile has no
// safe "remove exactly what I added" operation and an uninstall that leaves edits
// behind is worse than an install that asks the user to add one line.

use std::path::Path;

use crate::output;
use crate::setup::Outcome;

/// Whether one PATH entry names the same directory as another.
///
/// Windows treats `C:\x\bin` and `C:\x\bin\` as the same entry and compares without
/// case; Unix does neither. Trailing-separator trimming is safe on both.
pub(crate) fn entries_equal(a: &str, b: &str) -> bool {
    let a = a.trim().trim_end_matches(['\\', '/']);
    let b = b.trim().trim_end_matches(['\\', '/']);
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Whether `path_value` (a `;`- or `:`-joined PATH string) already contains `dir`.
fn path_value_contains(path_value: &str, dir: &str) -> bool {
    let sep = if cfg!(windows) { ';' } else { ':' };
    path_value.split(sep).any(|entry| entries_equal(entry, dir))
}

/// `path_value` with every entry naming `dir` removed, or `None` when nothing matched.
///
/// Empty entries are dropped too — on Windows an empty PATH entry means "search the
/// current directory", which nobody wants and which a naive join could introduce.
// Only the Windows uninstall path rewrites a PATH string; on Unix removal is deleting
// symlinks. The function still compiles (and is unit-tested) on both.
#[cfg_attr(unix, allow(dead_code))]
fn path_value_without(path_value: &str, dir: &str) -> Option<String> {
    let sep = if cfg!(windows) { ";" } else { ":" };
    if !path_value_contains(path_value, dir) {
        return None;
    }
    Some(
        path_value
            .split(sep)
            .filter(|entry| !entry.trim().is_empty() && !entries_equal(entry, dir))
            .collect::<Vec<_>>()
            .join(sep),
    )
}

#[cfg(windows)]
mod imp {
    use super::*;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    // This module talks to the registry directly rather than driving `powershell.exe`.
    // The script it used to run was passed as `-EncodedCommand` base64 and reached
    // `SendMessageTimeout` through `Add-Type`/`DllImport` — an encoded PowerShell
    // command that compiles code at runtime to P/Invoke into user32 is, feature for
    // feature, what commodity loaders do, and every behavioural scanner scores it that
    // way. Sophos quarantined the binary on that profile before it could run once.
    // Nothing here needs an interpreter: these are three `advapi32` calls and one
    // `user32` broadcast.

    /// A NUL-terminated UTF-16 string, as every `*W` entry point wants one.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// An open registry key that closes itself.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful `RegOpenKeyExW`, and a `Key` is
            // only ever built from one, so this closes a live handle exactly once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    /// Open `HKCU\Environment`, the key holding the *user* PATH.
    fn open_environment(access: u32) -> Option<Key> {
        let subkey = wide("Environment");
        let mut hkey: HKEY = std::ptr::null_mut();
        // SAFETY: `subkey` is NUL-terminated and outlives the call, and `hkey` is a
        // valid out-pointer.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut hkey) };
        // Built only on success: an unopened handle must not reach `Key`, whose `Drop`
        // would close it.
        if rc == ERROR_SUCCESS {
            Some(Key(hkey))
        } else {
            None
        }
    }

    /// Read `Path` from an open key, as `(value, kind)`.
    ///
    /// `RegQueryValueExW` hands back the value *raw*. The .NET call this replaced —
    /// `[Environment]::GetEnvironmentVariable('Path','User')` — expands it first, and
    /// writing that back bakes `%USERPROFILE%`-style entries into literal paths for
    /// good; reading through the registry API leaves every entry as its owner spelled
    /// it. A missing `Path` value is an empty PATH, not a failure: a profile that never
    /// had one is a normal state.
    fn query_path(key: &Key) -> Option<(String, REG_VALUE_TYPE)> {
        let name = wide("Path");
        let mut kind: REG_VALUE_TYPE = 0;
        let mut len: u32 = 0;
        // SAFETY: a null data pointer with a zero length asks for the size only.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if rc == ERROR_FILE_NOT_FOUND {
            return Some((String::new(), REG_EXPAND_SZ));
        }
        if rc != ERROR_SUCCESS {
            return None;
        }

        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` holds exactly the `len` bytes the sizing call asked for, and
        // `len` is updated in place with the count actually written.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        buf.truncate(len as usize);
        Some((decode_utf16_value(&buf), kind))
    }

    /// Decode a `REG_SZ`/`REG_EXPAND_SZ` payload.
    ///
    /// The registry stores UTF-16 and promises neither a terminator nor only one, so
    /// the value ends at the first NUL if there is one and at the end of the buffer if
    /// there is not. A trailing odd byte cannot begin a code unit and is dropped.
    fn decode_utf16_value(buf: &[u8]) -> String {
        let units: Vec<u16> = buf
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|&unit| unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    fn read_user_path() -> Option<String> {
        let key = open_environment(KEY_READ)?;
        query_path(&key).map(|(value, _)| value)
    }

    /// Persist a new user PATH, keeping the registry value's kind — flattening
    /// `REG_EXPAND_SZ` to `REG_SZ` would stop every `%VAR%` entry expanding — and
    /// broadcasting `WM_SETTINGCHANGE` so Explorer and new shells pick it up, which the
    /// registry write does not do on its own.
    fn write_user_path(value: &str) -> bool {
        let Some(key) = open_environment(KEY_READ | KEY_SET_VALUE) else {
            return false;
        };
        // Anything not already a plain string is written back as expandable: that is
        // what Windows itself creates `Path` as, and it is the safe way to guess.
        let kind = match query_path(&key) {
            Some((_, REG_SZ)) => REG_SZ,
            _ => REG_EXPAND_SZ,
        };

        let data = wide(value);
        let bytes = std::mem::size_of_val(data.as_slice()) as u32;
        let name = wide("Path");
        // SAFETY: `data` is NUL-terminated UTF-16 and `bytes` is its exact length in
        // bytes, terminator included, which is what `RegSetValueExW` wants for a string.
        let rc = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                kind,
                data.as_ptr().cast::<u8>(),
                bytes,
            )
        };
        if rc != ERROR_SUCCESS {
            return false;
        }

        let environment = wide("Environment");
        let mut delivered: usize = 0;
        // SAFETY: `environment` is NUL-terminated and outlives the call. The PATH is
        // already written by this point, so a failed broadcast costs a stale Explorer
        // environment until the next sign-in, never the edit itself; `SMTO_ABORTIFHUNG`
        // keeps one wedged top-level window from stalling setup behind it.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5000,
                &mut delivered,
            )
        };
        true
    }

    pub fn ensure_reachable(bin_dir: &Path) -> Outcome {
        let dir = bin_dir.display().to_string();
        let Some(current) = read_user_path() else {
            return Outcome::Failed("could not read the user PATH".to_string());
        };
        if path_value_contains(&current, &dir) {
            return Outcome::AlreadyPresent;
        }
        let new_value = if current.trim().is_empty() {
            dir.clone()
        } else {
            format!("{};{}", current.trim_end_matches(';'), dir)
        };
        if write_user_path(&new_value) {
            output::print_notice(&format!(
                "`{}` was added to your user PATH — terminals opened from now on will find `devp`.",
                output::clean_path(bin_dir)
            ));
            Outcome::Installed
        } else {
            Outcome::Failed("could not write the user PATH".to_string())
        }
    }

    /// Read-only: whether `bin_dir` is on the persisted user PATH.
    pub fn is_reachable(bin_dir: &Path) -> bool {
        read_user_path()
            .is_some_and(|current| path_value_contains(&current, &bin_dir.display().to_string()))
    }

    /// Take the managed directory back out of the user PATH. `Ok(true)` when an entry
    /// was actually removed.
    pub fn remove_reachability(bin_dir: &Path) -> anyhow::Result<bool> {
        let dir = bin_dir.display().to_string();
        let Some(current) = read_user_path() else {
            anyhow::bail!("could not read the user PATH");
        };
        let Some(new_value) = path_value_without(&current, &dir) else {
            return Ok(false);
        };
        if write_user_path(&new_value) {
            Ok(true)
        } else {
            anyhow::bail!("could not write the user PATH")
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn utf16(s: &str) -> Vec<u8> {
            s.encode_utf16().flat_map(u16::to_le_bytes).collect()
        }

        #[test]
        fn a_registry_string_ends_at_its_first_terminator_if_it_has_one() {
            // No terminator at all: the whole buffer is the value.
            assert_eq!(decode_utf16_value(&utf16(r"C:\a;C:\b")), r"C:\a;C:\b");

            // One terminator, and the doubled form Windows sometimes stores.
            let mut one = utf16(r"C:\a");
            one.extend_from_slice(&[0, 0]);
            assert_eq!(decode_utf16_value(&one), r"C:\a");
            let mut two = utf16(r"C:\a");
            two.extend_from_slice(&[0, 0, 0, 0]);
            assert_eq!(decode_utf16_value(&two), r"C:\a");

            // Anything past the terminator is not part of the value.
            let mut trailing = utf16(r"C:\a");
            trailing.extend_from_slice(&[0, 0]);
            trailing.extend_from_slice(&utf16("junk"));
            assert_eq!(decode_utf16_value(&trailing), r"C:\a");

            // A dangling odd byte cannot begin a code unit.
            let mut odd = utf16(r"C:\a");
            odd.push(b'x');
            assert_eq!(decode_utf16_value(&odd), r"C:\a");

            assert_eq!(decode_utf16_value(&[]), "");
        }

        /// A `%VAR%` entry has to survive the round trip unexpanded: expanding it and
        /// writing it back is what bakes one user's home directory into another's PATH.
        #[test]
        fn an_unexpanded_entry_is_read_back_verbatim() {
            let raw = r"%USERPROFILE%\bin;C:\Windows\System32";
            assert_eq!(decode_utf16_value(&utf16(raw)), raw);
        }

        /// Non-ASCII is why this is decoded as UTF-16 rather than taken as bytes: a
        /// console codepage would have mangled it, which is what the old PowerShell
        /// reader had to work around.
        #[test]
        fn a_non_ascii_entry_survives_decoding() {
            let raw = r"C:\Users\Müller\bin;C:\Users\日本\bin";
            assert_eq!(decode_utf16_value(&utf16(raw)), raw);
        }

        /// The registry plumbing end to end: open, size, read, decode. Read-only, so it
        /// is safe on a real machine — it asserts nothing about that machine's own PATH,
        /// only that reading it succeeds.
        #[test]
        fn the_user_path_can_be_read_from_the_registry() {
            assert!(read_user_path().is_some(), "could not read the user PATH");
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::fs;

    fn local_bin() -> Option<std::path::PathBuf> {
        Some(dirs::home_dir()?.join(".local").join("bin"))
    }

    pub fn ensure_reachable(bin_dir: &Path) -> Outcome {
        let Some(local_bin) = local_bin() else {
            return Outcome::Skipped("could not determine the home directory".to_string());
        };
        if fs::create_dir_all(&local_bin).is_err() {
            return Outcome::Failed(format!(
                "could not create {}",
                output::clean_path(&local_bin)
            ));
        }

        let mut created_any = false;
        for name in ["dev-prune", "devp"] {
            let link = local_bin.join(name);
            let target = bin_dir.join(name);
            match fs::read_link(&link) {
                Ok(existing) if existing == target => continue,
                Ok(existing) if existing.starts_with(bin_dir) => {
                    // Our own link, pointing at a name that moved. Repoint it.
                    let _ = fs::remove_file(&link);
                }
                Ok(_) => continue, // someone else's link — leave it, it resolves
                Err(_) if link.exists() => continue, // a real file the user put there
                Err(_) => {}
            }
            if std::os::unix::fs::symlink(&target, &link).is_ok() {
                created_any = true;
            }
        }

        let on_path = std::env::var("PATH")
            .map(|p| path_value_contains(&p, &local_bin.display().to_string()))
            .unwrap_or(false);
        if !on_path {
            return Outcome::Skipped(format!(
                "linked into `{}`, which is not on your PATH — add it in your shell profile",
                output::clean_path(&local_bin)
            ));
        }
        if created_any {
            Outcome::Installed
        } else {
            Outcome::AlreadyPresent
        }
    }

    /// Read-only: whether the `~/.local/bin` links exist and point into `bin_dir`.
    pub fn is_reachable(bin_dir: &Path) -> bool {
        local_bin().is_some_and(|local_bin| {
            fs::read_link(local_bin.join("devp")).is_ok_and(|target| target.starts_with(bin_dir))
        })
    }

    /// Remove the `~/.local/bin` links, but only the ones that point into `bin_dir` —
    /// a binary the user placed there themselves is theirs.
    pub fn remove_reachability(bin_dir: &Path) -> anyhow::Result<bool> {
        let Some(local_bin) = local_bin() else {
            return Ok(false);
        };
        let mut removed_any = false;
        for name in ["dev-prune", "devp"] {
            let link = local_bin.join(name);
            if let Ok(target) = fs::read_link(&link)
                && target.starts_with(bin_dir)
            {
                fs::remove_file(&link)?;
                removed_any = true;
            }
        }
        Ok(removed_any)
    }
}

pub use imp::{ensure_reachable, is_reachable, remove_reachability};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_entry_matches_with_and_without_a_trailing_separator() {
        if cfg!(windows) {
            assert!(path_value_contains(r"C:\a;C:\x\bin\;C:\b", r"C:\x\bin"));
            assert!(path_value_contains(r"c:\X\BIN", r"C:\x\bin"));
            assert!(!path_value_contains(r"C:\x\binx", r"C:\x\bin"));
        } else {
            assert!(path_value_contains("/a:/x/bin/:/b", "/x/bin"));
            assert!(!path_value_contains("/x/BIN", "/x/bin"));
            assert!(!path_value_contains("/x/binx", "/x/bin"));
        }
    }

    #[test]
    fn removal_strips_the_entry_and_reports_no_change_when_absent() {
        if cfg!(windows) {
            assert_eq!(
                path_value_without(r"C:\a;C:\x\bin;C:\b", r"C:\x\bin"),
                Some(r"C:\a;C:\b".to_string())
            );
            assert_eq!(path_value_without(r"C:\a;C:\b", r"C:\x\bin"), None);
            // Empty entries mean "search the current directory" on Windows; a removal
            // must never leave one behind.
            assert_eq!(
                path_value_without(r"C:\a;;C:\x\bin", r"C:\x\bin"),
                Some(r"C:\a".to_string())
            );
        } else {
            assert_eq!(
                path_value_without("/a:/x/bin:/b", "/x/bin"),
                Some("/a:/b".to_string())
            );
            assert_eq!(path_value_without("/a:/b", "/x/bin"), None);
        }
    }
}
