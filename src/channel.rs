// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! Which package manager delivered the binary that is running, and what that implies.
//!
//! Every lifecycle command needs this answer and each one used to work it out for
//! itself. `update` had a private `Channel` enum, `uninstall` had a substring match
//! returning an uninstall command, and `doctor` had a hand-written list of directories
//! to search. Three classifiers, three sets of markers, and only one of them had ever
//! heard of WinGet — so `devp update --install` overwrote a file WinGet owns, `devp
//! uninstall` offered to delete it, and `devp doctor` never looked there at all.
//!
//! This module is the one answer. A channel knows its own name, its upgrade and
//! uninstall commands, whether it owns the files it installed, and — the distinction
//! that matters most — whether it *replaces its whole directory* on upgrade.
//!
//! The markers are path fragments rather than probes on purpose: classification happens
//! on the startup path of every lifecycle command, so it must not spawn a process, touch
//! the network, or depend on a manager being installed to recognise what it installed.

use std::path::{Path, PathBuf};

/// Path fragments that identify a channel, matched against the executable's path with
/// separators normalised to `/` and folded to lower case.
///
/// Kept here rather than in `constants` because nothing outside this module refers to
/// them — they are this classifier's private fingerprints, not names shared with the
/// install scripts.
mod marker {
    pub const WINGET: &[&str] = &["/microsoft/winget/packages/", "/winget/links/"];
    pub const SCOOP: &[&str] = &["/scoop/apps/", "/scoop/shims/"];
    pub const HOMEBREW: &[&str] = &["/cellar/", "/homebrew/", "/linuxbrew/"];
    pub const CARGO: &[&str] = &["/.cargo/"];
    pub const NPM: &[&str] = &["/node_modules/", "/_npx/"];
    pub const UV_TOOL: &[&str] = &["/uv/tools/", "/uv-tool/"];
    pub const PIPX: &[&str] = &["/pipx/"];
}

/// The package manager that owns the running binary.
///
/// One channel owns one binary. A copy installed through uv is upgraded through uv,
/// never through npm, because two managers writing the same PATH entry would fight over
/// it forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// `install.sh` / `install.ps1` put it under the managed `<config>/bin`.
    Installer,
    /// `cargo install` / `cargo binstall` put it under `~/.cargo/bin`.
    Cargo,
    /// `npm install -g` — the binary lives under a `node_modules` tree.
    Npm,
    /// `uv tool install` — under uv's tool environments.
    UvTool,
    /// `pipx install` — under a `pipx` venv.
    Pipx,
    /// `pip install` — a console script beside a Python interpreter, in the system
    /// scripts directory or a virtualenv's.
    Pip,
    /// `winget install` — under `%LOCALAPPDATA%\Microsoft\WinGet\Packages`.
    WinGet,
    /// `scoop install` — under `~/scoop/apps`, shimmed from `~/scoop/shims`.
    Scoop,
    /// `brew install` — under the Cellar, symlinked into the prefix's `bin`.
    Homebrew,
    /// Anywhere else: a dev build, a hand-copied binary, a distro package.
    Unknown,
}

impl Channel {
    /// Classify the running executable.
    pub fn detect() -> Self {
        let Ok(exe) = std::env::current_exe() else {
            return Channel::Unknown;
        };
        let managed = crate::setup::managed_exe_path().ok();
        Self::detect_at(&exe, managed.as_deref())
    }

    /// Classify `exe` by the directories in its path.
    ///
    /// Purely lexical: this must not touch the network or spawn anything, and each
    /// channel's layout is stable enough that its marker directory is a reliable
    /// fingerprint. `managed` is passed in rather than resolved here so tests can probe
    /// the classification without a config directory on disk.
    ///
    /// The managed path is checked first and the three directory-owning managers next.
    /// Order is load-bearing at least once already: a Scoop install of a Rust toolchain
    /// can put `.cargo` inside `~/scoop`, and misreading that as `Cargo` would send
    /// `devp update` to run `cargo install` against a directory Scoop replaces wholesale.
    pub fn detect_at(exe: &Path, managed: Option<&Path>) -> Self {
        if let Some(managed) = managed
            && exe == managed
        {
            return Channel::Installer;
        }
        let path = exe.to_string_lossy().replace('\\', "/").to_lowercase();
        let any = |markers: &[&str]| markers.iter().any(|m| path.contains(m));

        if any(marker::WINGET) {
            Channel::WinGet
        } else if any(marker::SCOOP) {
            Channel::Scoop
        } else if any(marker::HOMEBREW) {
            Channel::Homebrew
        } else if any(marker::CARGO) {
            Channel::Cargo
        } else if any(marker::NPM) || npm_shim_beside(exe) {
            Channel::Npm
        } else if any(marker::UV_TOOL) {
            Channel::UvTool
        } else if any(marker::PIPX) {
            Channel::Pipx
        } else if pip_script_beside(exe) {
            Channel::Pip
        } else {
            Channel::Unknown
        }
    }

    /// How to name this channel in a sentence addressed to the user.
    pub fn label(self) -> &'static str {
        match self {
            Channel::Installer => "the install script",
            Channel::Cargo => "cargo",
            Channel::Npm => "npm",
            Channel::UvTool => "uv",
            Channel::Pipx => "pipx",
            Channel::Pip => "pip",
            Channel::WinGet => "WinGet",
            Channel::Scoop => "Scoop",
            Channel::Homebrew => "Homebrew",
            Channel::Unknown => "an unrecognised location",
        }
    }

    /// The command that upgrades through this channel, as the user would type it.
    ///
    /// `None` for [`Channel::Unknown`] only: there is no command to name for a binary
    /// somebody copied into place by hand.
    pub fn upgrade_command(self) -> Option<String> {
        Some(
            match self {
                // The one channel whose command is not a fixed string: it is the install
                // one-liner, and its URL has a single source of truth in `constants`.
                Channel::Installer => {
                    return Some(if cfg!(windows) {
                        format!("iwr -useb {} | iex", crate::constants::INSTALL_PS1_URL)
                    } else {
                        format!("curl -fsSL {} | sh", crate::constants::INSTALL_SH_URL)
                    });
                }
                Channel::Cargo => "cargo install dev-prune --force",
                Channel::Npm => "npm install -g dev-prune@latest",
                Channel::UvTool => "uv tool upgrade dev-prune",
                Channel::Pipx => "pipx upgrade dev-prune",
                Channel::Pip => "pip install --upgrade dev-prune",
                Channel::WinGet => {
                    return Some(format!(
                        "winget upgrade {}",
                        crate::constants::WINGET_PACKAGE_ID
                    ));
                }
                Channel::Scoop => "scoop update dev-prune",
                Channel::Homebrew => "brew upgrade dev-prune",
                Channel::Unknown => return None,
            }
            .to_string(),
        )
    }

    /// The command that uninstalls through this channel.
    ///
    /// `None` where there is no manager to tell: the installer's own copy is deleted by
    /// `devp uninstall` itself, and an unrecognised copy is just a file.
    pub fn uninstall_command(self) -> Option<String> {
        Some(
            match self {
                Channel::Cargo => "cargo uninstall dev-prune",
                Channel::Npm => "npm uninstall -g dev-prune",
                Channel::UvTool => "uv tool uninstall dev-prune",
                Channel::Pipx => "pipx uninstall dev-prune",
                Channel::Pip => "pip uninstall dev-prune",
                Channel::WinGet => {
                    return Some(format!(
                        "winget uninstall {}",
                        crate::constants::WINGET_PACKAGE_ID
                    ));
                }
                Channel::Scoop => "scoop uninstall dev-prune",
                Channel::Homebrew => "brew uninstall dev-prune",
                Channel::Installer | Channel::Unknown => return None,
            }
            .to_string(),
        )
    }

    /// Whether a package manager keeps a record of this install that deleting the file
    /// would falsify.
    ///
    /// When true, `devp uninstall` names the manager's own command instead of quietly
    /// removing the file: `pip list` still showing a package whose binary is gone, or
    /// `cargo install` refusing to reinstall over its own bookkeeping, is worse than a
    /// leftover binary the user was told about.
    pub fn owns_its_files(self) -> bool {
        !matches!(self, Channel::Installer | Channel::Unknown)
    }

    /// Whether this channel replaces its install *directory* wholesale on upgrade.
    ///
    /// This is the distinction the old per-command classifiers did not have, and the one
    /// that caused a real bug. WinGet, Scoop and Homebrew each version their package
    /// directory and swap the whole thing — `…\WinGet\Packages\<id>\`, `~/scoop/apps/
    /// <pkg>/<version>/`, `<prefix>/Cellar/<pkg>/<version>/`. Anything dev-prune writes
    /// beside its own executable there is gone at the next upgrade, and anything
    /// *pointing* at it — a scheduled task, a git hook — is left aimed at a path that no
    /// longer exists.
    ///
    /// So nothing durable is ever written into one of these directories. The `devp`
    /// twin goes to the managed `<config>/bin` instead, which this program owns and
    /// which no package manager will replace underneath it.
    pub fn replaces_its_directory(self) -> bool {
        matches!(self, Channel::WinGet | Channel::Scoop | Channel::Homebrew)
    }
}

/// npm's global shims sit *beside* its `node_modules`, not inside it, so the path alone
/// does not identify them.
fn npm_shim_beside(exe: &Path) -> bool {
    exe.parent()
        .is_some_and(|dir| dir.join("node_modules").join("dev-prune").exists())
}

/// pip puts console scripts beside the interpreter that installed them — a system
/// `Scripts`/`bin` directory or a virtualenv's — and there is no marker in the path to
/// say so. The interpreter next door is the only evidence there is.
///
/// Checked last, after uv and pipx: both of those are pip installs underneath, and both
/// have an interpreter beside the script. Their own markers must win, or `devp
/// uninstall` would tell a pipx user to run `pip uninstall` inside a venv they do not
/// know exists.
fn pip_script_beside(exe: &Path) -> bool {
    exe.parent().is_some_and(|dir| {
        ["python.exe", "python", "python3"]
            .iter()
            .any(|interpreter| dir.join(interpreter).exists())
    })
}

/// Every fixed directory a channel installs into, whether or not it is on `PATH`.
///
/// Shared by `devp doctor` (which reports copies running a different version) and `devp
/// uninstall` (which offers to sweep them up). They looked in different places before
/// this was one list, which meant doctor could report a stale copy that uninstall would
/// then fail to find.
///
/// Non-existent entries are included; callers filter. `home` is passed in so the list
/// can be tested without a home directory full of package managers.
pub fn install_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let Some(home) = home else {
        return dirs;
    };
    // `bin` on unix, `Scripts` on Windows — the same venv layout under both uv and
    // pipx, and the reason a Windows uv copy is missed by a unix-shaped guess.
    let scripts = if cfg!(windows) { "Scripts" } else { "bin" };

    dirs.push(home.join(".cargo").join("bin"));
    dirs.push(home.join(".local").join("bin"));
    dirs.push(
        home.join(".local")
            .join("share")
            .join("uv")
            .join("tools")
            .join("dev-prune")
            .join(scripts),
    );
    dirs.push(
        home.join(".local")
            .join("pipx")
            .join("venvs")
            .join("dev-prune")
            .join(scripts),
    );
    dirs.push(
        home.join("pipx")
            .join("venvs")
            .join("dev-prune")
            .join(scripts),
    );

    if cfg!(windows) {
        // uv keeps its tool environments under `%APPDATA%` on Windows, which is not
        // under `.local` at all.
        dirs.push(
            home.join("AppData")
                .join("Roaming")
                .join("uv")
                .join("tools")
                .join("dev-prune")
                .join(scripts),
        );
        dirs.push(home.join("AppData").join("Roaming").join("npm"));
        dirs.push(
            home.join("AppData")
                .join("Local")
                .join("Microsoft")
                .join("WinGet")
                .join("Links"),
        );
        dirs.push(home.join("scoop").join("shims"));
    } else {
        dirs.push(home.join(".npm-global").join("bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/home/linuxbrew/.linuxbrew/bin"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_is_recognised_by_its_marker_directory() {
        let cases: &[(&str, Channel)] = &[
            ("/home/k/.cargo/bin/dev-prune", Channel::Cargo),
            (
                "/usr/lib/node_modules/dev-prune/bin/dev-prune",
                Channel::Npm,
            ),
            (
                "/home/k/.local/share/uv/tools/dev-prune/bin/dev-prune",
                Channel::UvTool,
            ),
            (
                "/home/k/.local/pipx/venvs/dev-prune/bin/dev-prune",
                Channel::Pipx,
            ),
            (
                r"C:\Users\k\AppData\Local\Microsoft\WinGet\Packages\VKrishna04.dev-prune_x\dev-prune.exe",
                Channel::WinGet,
            ),
            (
                r"C:\Users\k\scoop\apps\dev-prune\1.5.1\dev-prune.exe",
                Channel::Scoop,
            ),
            (
                "/opt/homebrew/Cellar/dev-prune/1.5.1/bin/dev-prune",
                Channel::Homebrew,
            ),
            ("/opt/somewhere/dev-prune", Channel::Unknown),
        ];
        for (path, expected) in cases {
            assert_eq!(
                Channel::detect_at(Path::new(path), None),
                *expected,
                "{path}"
            );
        }
    }

    #[test]
    fn the_managed_copy_is_the_installer_channel() {
        // Even a managed directory that happens to live under `.cargo` is the
        // installer's — the managed path is an identity, not a heuristic.
        let managed = Path::new("/home/k/.cargo/odd/dev-prune/bin/dev-prune");
        assert_eq!(
            Channel::detect_at(managed, Some(managed)),
            Channel::Installer
        );
    }

    /// A Rust toolchain installed through Scoop puts `.cargo` under `~/scoop`. Reading
    /// that as `Cargo` would send an upgrade to `cargo install` against a directory
    /// Scoop replaces wholesale, so the directory-owning managers are tested first.
    #[test]
    fn a_directory_owning_manager_wins_over_a_nested_marker() {
        let path = Path::new(r"C:\Users\k\scoop\apps\rust\current\.cargo\bin\dev-prune.exe");
        assert_eq!(Channel::detect_at(path, None), Channel::Scoop);
    }

    /// The three managers that version their whole package directory are exactly the
    /// three nothing durable may be written into. Asserted rather than assumed: adding a
    /// channel without answering this question is how the orphaned-twin bug happened.
    #[test]
    fn exactly_the_versioned_directory_managers_replace_their_directory() {
        let all = [
            Channel::Installer,
            Channel::Cargo,
            Channel::Npm,
            Channel::UvTool,
            Channel::Pipx,
            Channel::Pip,
            Channel::WinGet,
            Channel::Scoop,
            Channel::Homebrew,
            Channel::Unknown,
        ];
        let replacing: Vec<Channel> = all
            .iter()
            .copied()
            .filter(|c| c.replaces_its_directory())
            .collect();
        assert_eq!(
            replacing,
            vec![Channel::WinGet, Channel::Scoop, Channel::Homebrew]
        );
        // Anything that replaces its directory is by definition manager-owned.
        assert!(replacing.iter().all(|c| c.owns_its_files()));
    }

    #[test]
    fn every_managed_channel_can_name_both_of_its_commands() {
        for channel in [
            Channel::Cargo,
            Channel::Npm,
            Channel::UvTool,
            Channel::Pipx,
            Channel::Pip,
            Channel::WinGet,
            Channel::Scoop,
            Channel::Homebrew,
        ] {
            assert!(channel.upgrade_command().is_some(), "{channel:?}");
            assert!(channel.uninstall_command().is_some(), "{channel:?}");
        }
        // The installer replaces its own copy and has no manager to uninstall through.
        assert!(Channel::Installer.upgrade_command().is_some());
        assert!(Channel::Installer.uninstall_command().is_none());
        assert!(Channel::Unknown.upgrade_command().is_none());
        assert!(Channel::Unknown.uninstall_command().is_none());
    }

    #[test]
    fn install_dirs_cover_every_channel_that_installs_outside_path() {
        assert!(install_dirs(None).is_empty());
        let home = Path::new(if cfg!(windows) {
            "C:\\home\\u"
        } else {
            "/home/u"
        });
        let joined = install_dirs(Some(home))
            .iter()
            .map(|d| d.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
            .join("|");
        // A copy nobody can see is a copy nobody upgrades, and it becomes the one that
        // runs the day PATH changes — so each of these is searched whether or not the
        // manager that owns it ever put itself on PATH.
        for marker in ["cargo", "uv", "pipx"] {
            assert!(joined.contains(marker), "{marker} missing from {joined}");
        }
        let platform = if cfg!(windows) { "winget" } else { "homebrew" };
        assert!(
            joined.contains(platform),
            "{platform} missing from {joined}"
        );
    }
}
