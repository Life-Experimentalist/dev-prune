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
    use std::process::Command;

    /// A PowerShell script as `-EncodedCommand` base64 (UTF-16LE). The encoded form
    /// exists for two reasons: no quoting layer between here and the interpreter (the
    /// scripts carry both quote styles), and no codepage — a console in an OEM
    /// codepage would otherwise mangle every non-ASCII character in a PATH entry.
    fn encoded_command(script: &str) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes: Vec<u8> = script
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
                | u32::from(chunk.get(2).copied().unwrap_or(0));
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    fn powershell(script: &str) -> Command {
        // Absolute first: these registry edits are what runs while the user PATH is
        // in flux, so resolving the interpreter *through* `PATH` is the one thing
        // this function must not do. Windows PowerShell has lived at this path since
        // Vista; if it is somehow gone, fall back to whatever lookup finds.
        let exe = crate::spawn::system32(r"WindowsPowerShell\v1.0\powershell.exe");
        let program = if std::path::Path::new(&exe).exists() {
            exe
        } else {
            String::from("powershell")
        };
        let mut cmd = crate::spawn::command(program);
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_command(script),
        ]);
        cmd
    }

    /// Read the *user* PATH — the persisted value under `HKCU\Environment`, not this
    /// process's inherited one, which also carries the machine PATH. Read raw:
    /// `DoNotExpandEnvironmentNames` keeps `%USERPROFILE%`-style entries as their
    /// owner spelled them, so a later write cannot bake them into literal paths.
    fn read_user_path() -> Option<String> {
        let script = "\
            [Console]::OutputEncoding=[System.Text.Encoding]::UTF8\n\
            $k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')\n\
            if($null -eq $k){exit 1}\n\
            $v=$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)\n\
            [Console]::Out.Write([string]$v)";
        let out = powershell(script).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }

    /// Persist a new user PATH, keeping the registry value's kind — flattening
    /// `REG_EXPAND_SZ` to `REG_SZ` would stop every `%VAR%` entry expanding — and
    /// broadcasting `WM_SETTINGCHANGE` so Explorer and new shells pick it up (the raw
    /// registry write does not send it the way the .NET environment API did).
    fn write_user_path(value: &str) -> bool {
        let escaped = value.replace('\'', "''");
        let script = format!(
            "$v='{escaped}'\n\
             $k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$true)\n\
             if($null -eq $k){{exit 1}}\n\
             $kind=[Microsoft.Win32.RegistryValueKind]::ExpandString\n\
             try{{$kind=$k.GetValueKind('Path')}}catch{{}}\n\
             if($kind -ne [Microsoft.Win32.RegistryValueKind]::String){{$kind=[Microsoft.Win32.RegistryValueKind]::ExpandString}}\n\
             $k.SetValue('Path',$v,$kind)\n\
             $sig='[DllImport(\"user32.dll\",SetLastError=true,CharSet=CharSet.Auto)]public static extern IntPtr SendMessageTimeout(IntPtr hWnd,uint Msg,UIntPtr wParam,string lParam,uint fuFlags,uint uTimeout,out UIntPtr lpdwResult);'\n\
             $t=Add-Type -MemberDefinition $sig -Name 'NativeBroadcast' -Namespace DevPrune -PassThru\n\
             $r=[UIntPtr]::Zero\n\
             [void]$t::SendMessageTimeout([IntPtr]0xffff,0x1A,[UIntPtr]::Zero,'Environment',2,5000,[ref]$r)"
        );
        powershell(&script)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
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
