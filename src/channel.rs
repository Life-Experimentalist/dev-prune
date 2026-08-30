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
    // The three npm-compatible clients, which have to be told apart from npm itself and
    // from each other. All four end up with the executable inside a `node_modules` tree,
    // so `NPM` matches every one of them and these have to be tried first.
    pub const BUN: &[&str] = &["/.bun/"];
    // `/pnpm/` and not `/pnpm/global/`, because the package lives under `global/` but the
    // executable on PATH does not: pnpm puts its shim straight in `PNPM_HOME`
    // (`~/.local/share/pnpm`, `%LOCALAPPDATA%\pnpm`), one level above. `uninstall`'s stray
    // sweep looks in exactly that directory, so the narrower fragment made the one copy the
    // sweep can actually find read as `Unknown` — and `Unknown` may be deleted directly,
    // which removed pnpm's shim without ever running `pnpm remove -g`. Broad is safe here:
    // `node_modules/.pnpm` is spelled with the dot and does not match.
    pub const PNPM: &[&str] = &["/pnpm/", "/.pnpm-global/"];
    pub const YARN: &[&str] = &[
        "/yarn/global/",
        "/yarn/data/global/",
        "/.yarn/bin/",
        "/yarn/bin/",
    ];
    pub const NPM: &[&str] = &["/node_modules/", "/_npx/"];
    pub const UV_TOOL: &[&str] = &["/uv/tools/", "/uv-tool/"];
    pub const PIPX: &[&str] = &["/pipx/"];

    /// Trees that belong to a manager whose commands dev-prune does not know, paired
    /// with the name to print. Each of these installs global executables and none of
    /// them leaves a fragment any marker above matches, so before this list a copy in
    /// one of them was indistinguishable from a loose file -- and got deleted.
    ///
    /// Detection only. There is deliberately no install or upgrade command for any of
    /// them: none is installed on the machine this list was written on, so any command
    /// here would be a guess, and a wrong upgrade command is worse than none.
    pub const FOREIGN: &[(&str, &str)] = &[
        ("/.deno/bin/", "Deno"),
        ("/.volta/bin/", "Volta"),
        // Both spellings of the tools tree: `~/.volta/tools/` on Unix, where the dot
        // keeps the slash-anchored fragment below from matching, and
        // `%LOCALAPPDATA%\Volta\tools\` on Windows, which has no dot to hide behind.
        ("/.volta/tools/", "Volta"),
        ("/volta/tools/", "Volta"),
        ("/mise/shims/", "mise"),
        ("/mise/installs/", "mise"),
        ("/.asdf/shims/", "asdf"),
        ("/nix/store/", "Nix"),
        // Not `/usr/local/bin`, which is where a person putting a binary somewhere by
        // hand puts it. `/usr/bin` is the distribution's, and on every distribution
        // that packages anything, deleting out of it desynchronises the package
        // database exactly the way deleting cargo's copy desynchronises `.crates.toml`.
        ("/usr/bin/", "the system package manager"),
    ];
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
    /// `bun add -g` — under `~/.bun/install/global`.
    Bun,
    /// `pnpm add -g` — under pnpm's own global store.
    Pnpm,
    /// `yarn global add` (Yarn 1.x) — under `~/.config/yarn/global`, shimmed from
    /// `~/.yarn/bin`.
    Yarn,
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
    /// A copy inside a tree that is recognisably some manager's, where dev-prune knows
    /// the manager's name and not its commands.
    ///
    /// The distinction that matters is against [`Channel::Unknown`], not against the
    /// named channels: `Unknown` means *nothing on this machine claims this file*, and
    /// `devp uninstall` deletes those because the file is the whole install. This means
    /// *something claims it and dev-prune cannot speak to it*, which is the one case
    /// where the only safe move is to name the manager and stop.
    Foreign(&'static str),
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
    /// Order is load-bearing twice over. A Scoop install of a Rust toolchain can put
    /// `.cargo` inside `~/scoop`, and misreading that as `Cargo` would send `devp update`
    /// to run `cargo install` against a directory Scoop replaces wholesale. And bun,
    /// pnpm and yarn all install npm packages into a `node_modules` tree of their own, so
    /// each of them matches npm's marker as well as its own and has to be tried
    /// before it.
    pub fn detect_at(exe: &Path, managed: Option<&Path>) -> Self {
        // The *directory*, not the file. `managed_exe_path` names `dev-prune`, and the
        // install scripts put `devp` beside it — so comparing whole paths recognised the
        // long name and classified the short one, which is the one the documentation
        // tells people to type, as `Unknown`. The symptom was `devp update` answering "no
        // package manager owns this copy" to someone who had installed with the install
        // script two minutes earlier. `<config>/bin` holds dev-prune's own binaries and
        // nothing else, so anything running from it is the installer's copy under one of
        // its two names.
        if let Some(managed) = managed
            && let Some(managed_dir) = managed.parent()
            && exe.parent() == Some(managed_dir)
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
        } else if let Some((_, name)) = marker::FOREIGN.iter().find(|(m, _)| path.contains(m)) {
            // Ahead of the npm and pip families, not just the two beside-file checks
            // below. A Volta npm install runs from `~/.volta/tools/image/packages/…/
            // lib/node_modules/…`, a mise npm backend from `…/mise/installs/…/lib/
            // node_modules/…`, and a Nix-packaged npm tool from `/nix/store/…/lib/
            // node_modules/…` — every one matches npm's `/node_modules/` fragment, and
            // `Npm` here meant `devp update` writing release bytes into a tree Volta,
            // mise or Nix owns. A tree that names its manager outranks a structure many
            // managers share. The beside-file checks have the same problem one step
            // later: `/usr/bin` holds a `python3` on every Linux, and mise and asdf
            // keep a `python` shim beside every other shim, so all three read as pip
            // installs — and `/usr/bin/dev-prune` would be handed `pip install
            // --upgrade`, which is the distribution's copy and none of pip's business.
            Channel::Foreign(name)
        } else if any(marker::BUN) {
            Channel::Bun
        } else if any(marker::PNPM) {
            Channel::Pnpm
        } else if any(marker::YARN) {
            Channel::Yarn
        } else if any(marker::NPM) {
            Channel::Npm
        } else if any(marker::UV_TOOL) {
            Channel::UvTool
        } else if any(marker::PIPX) {
            Channel::Pipx
        } else if npm_shim_beside(exe) {
            Channel::Npm
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
            Channel::Bun => "bun",
            Channel::Pnpm => "pnpm",
            Channel::Yarn => "yarn",
            Channel::UvTool => "uv",
            Channel::Pipx => "pipx",
            Channel::Pip => "pip",
            Channel::WinGet => "WinGet",
            Channel::Scoop => "Scoop",
            Channel::Homebrew => "Homebrew",
            Channel::Unknown => "an unrecognised location",
            Channel::Foreign(name) => name,
        }
    }

    /// How to name this channel beside the version number, where there is room for a
    /// word and not a clause.
    ///
    /// Deliberately not [`Self::label`]. That one is written to drop into a sentence —
    /// "installed with the install script", "this copy came from an unrecognised
    /// location" — and both of those read as noise next to a version. The `Unknown` case
    /// is the one worth spelling differently rather than shortening: `standalone` says
    /// the file *is* the whole install, which is the fact behind every other thing
    /// dev-prune says about that copy.
    pub fn badge(self) -> &'static str {
        match self {
            Channel::Installer => "install script",
            Channel::Unknown => "standalone",
            named => named.label(),
        }
    }

    /// The command that upgrades through this channel, as the user would type it.
    ///
    /// `None` for [`Channel::Unknown`] only: there is no command to name for a binary
    /// somebody copied into place by hand.
    pub fn upgrade_command(self) -> Option<String> {
        self.upgrade_argv().map(|argv| self.typed_form(&argv))
    }

    /// The command that uninstalls through this channel.
    ///
    /// `None` where there is no manager to tell: the installer’s own copy is deleted by
    /// `devp uninstall` itself, and an unrecognised copy is just a file.
    pub fn uninstall_command(self) -> Option<String> {
        self.uninstall_argv().map(|argv| self.typed_form(&argv))
    }

    /// The command that installs dev-prune fresh through this channel, as the user
    /// would type it. Does not include [`Self::install_sources`].
    pub fn install_command(self) -> Option<String> {
        self.install_argv().map(|argv| self.typed_form(&argv))
    }

    /// Sources that must exist before [`Self::install_argv`] can resolve dev-prune.
    ///
    /// Homebrew and Scoop are the only reason this exists. The formula and the manifest
    /// live in this project’s own tap and bucket rather than the default index, and
    /// `brew install dev-prune` without the tap resolves against homebrew-core, where
    /// dev-prune is not published. Adding a source that is already added reports
    /// failure, so these steps are best-effort; the install itself is not.
    pub fn install_sources(self) -> Vec<Vec<String>> {
        match self {
            Channel::Scoop => vec![owned(&[
                "scoop",
                "bucket",
                "add",
                crate::constants::SCOOP_BUCKET_NAME,
                crate::constants::SCOOP_BUCKET_URL,
            ])],
            Channel::Homebrew => vec![owned(&["brew", "tap", crate::constants::HOMEBREW_TAP])],
            _ => Vec::new(),
        }
    }

    /// The command that installs dev-prune fresh through this channel, once
    /// [`Self::install_sources`] has run.
    ///
    /// `None` for `Pip` and `Unknown`: a bare `pip install` of a CLI puts the console
    /// script wherever the active interpreter happens to be, which is the ambiguity `uv
    /// tool` and `pipx` exist to remove, and nothing installs *into* an unrecognised
    /// location on purpose.
    pub fn install_argv(self) -> Option<Vec<String>> {
        Some(match self {
            // Same preference as the upgrade: binstall fetches the prebuilt release,
            // a plain `cargo install` compiles for minutes.
            Channel::Cargo => {
                if crate::adapters::binary_available("cargo-binstall") {
                    owned(&["cargo", "binstall", "dev-prune", "-y"])
                } else {
                    owned(&["cargo", "install", "dev-prune"])
                }
            }
            Channel::Npm => owned(&["npm", "install", "-g", "dev-prune"]),
            Channel::Bun => owned(&["bun", "add", "-g", "dev-prune"]),
            Channel::Pnpm => owned(&["pnpm", "add", "-g", "dev-prune"]),
            Channel::Yarn => owned(&["yarn", "global", "add", "dev-prune"]),
            // `@latest` because `uv tool install dev-prune` against an environment uv
            // already has prints "already installed" and exits successfully without
            // changing anything — which reads, from here, as a move that worked.
            Channel::UvTool => owned(&["uv", "tool", "install", "dev-prune@latest"]),
            Channel::Pipx => owned(&["pipx", "install", "dev-prune"]),
            Channel::WinGet => vec![
                "winget".to_string(),
                "install".to_string(),
                "--id".to_string(),
                crate::constants::WINGET_PACKAGE_ID.to_string(),
                "--accept-package-agreements".to_string(),
                "--accept-source-agreements".to_string(),
            ],
            Channel::Scoop => owned(&["scoop", "install", "dev-prune"]),
            Channel::Homebrew => owned(&["brew", "install", "dev-prune"]),
            Channel::Installer => self.installer_argv(),
            Channel::Pip | Channel::Unknown | Channel::Foreign(_) => return None,
        })
    }

    /// The command that upgrades the copy this channel installed.
    pub fn upgrade_argv(self) -> Option<Vec<String>> {
        Some(match self {
            Channel::Cargo => {
                if crate::adapters::binary_available("cargo-binstall") {
                    owned(&["cargo", "binstall", "dev-prune", "--force", "-y"])
                } else {
                    owned(&["cargo", "install", "dev-prune", "--force"])
                }
            }
            // The four npm-compatible clients, each run through itself. `@latest` is
            // load-bearing for the first three: given a bare name they resolve against a
            // manifest they already have and report the installed version as current.
            Channel::Npm => owned(&["npm", "install", "-g", "dev-prune@latest"]),
            Channel::Bun => owned(&["bun", "add", "-g", "dev-prune@latest"]),
            Channel::Pnpm => owned(&["pnpm", "add", "-g", "dev-prune@latest"]),
            // Yarn 1.x, which is the only Yarn that has `yarn global` at all. Berry
            // removed it and prints its own explanation of what to use instead — a
            // better message than any guess this could make on its behalf.
            Channel::Yarn => owned(&["yarn", "global", "upgrade", "dev-prune"]),
            Channel::UvTool => owned(&["uv", "tool", "upgrade", "dev-prune"]),
            Channel::Pipx => owned(&["pipx", "upgrade", "dev-prune"]),
            Channel::Pip => owned(&["pip", "install", "--upgrade", "dev-prune"]),
            // The three that own their whole package directory. Each is given its own
            // command rather than the direct download, because replacing a file inside a
            // versioned package directory desynchronises the manager from what is on
            // disk — and the next `winget upgrade` or `brew upgrade` would put the old
            // binary back.
            Channel::WinGet => vec![
                "winget".to_string(),
                "upgrade".to_string(),
                "--id".to_string(),
                crate::constants::WINGET_PACKAGE_ID.to_string(),
                "--accept-package-agreements".to_string(),
                "--accept-source-agreements".to_string(),
            ],
            Channel::Scoop => owned(&["scoop", "update", "dev-prune"]),
            Channel::Homebrew => owned(&["brew", "upgrade", "dev-prune"]),
            Channel::Installer => self.installer_argv(),
            Channel::Unknown | Channel::Foreign(_) => return None,
        })
    }

    /// The command that removes the copy this channel installed *and* clears the record
    /// the manager keeps of it.
    ///
    /// Running this is the only correct way to remove a manager-owned copy, and the
    /// reason is not tidiness. Deleting the file behind cargo’s back leaves
    /// `.crates.toml` naming a binary that is gone, and `cargo uninstall dev-prune` then
    /// exits 101 with `corrupt metadata, ... does not exist when it should` — without
    /// clearing the entry. The manager has to be told first, or it can never be told at
    /// all.
    ///
    /// `None` where no manager holds a record: the installer’s own copy is deleted by
    /// `devp uninstall` itself, and an unrecognised copy is just a file.
    pub fn uninstall_argv(self) -> Option<Vec<String>> {
        Some(match self {
            Channel::Cargo => owned(&["cargo", "uninstall", "dev-prune"]),
            Channel::Npm => owned(&["npm", "uninstall", "-g", "dev-prune"]),
            Channel::Bun => owned(&["bun", "remove", "-g", "dev-prune"]),
            Channel::Pnpm => owned(&["pnpm", "remove", "-g", "dev-prune"]),
            Channel::Yarn => owned(&["yarn", "global", "remove", "dev-prune"]),
            Channel::UvTool => owned(&["uv", "tool", "uninstall", "dev-prune"]),
            Channel::Pipx => owned(&["pipx", "uninstall", "dev-prune"]),
            // `-y`: pip asks on stdin, and whatever ran this has already asked.
            Channel::Pip => owned(&["pip", "uninstall", "-y", "dev-prune"]),
            Channel::WinGet => vec![
                "winget".to_string(),
                "uninstall".to_string(),
                "--id".to_string(),
                crate::constants::WINGET_PACKAGE_ID.to_string(),
            ],
            Channel::Scoop => owned(&["scoop", "uninstall", "dev-prune"]),
            Channel::Homebrew => owned(&["brew", "uninstall", "dev-prune"]),
            Channel::Installer | Channel::Unknown | Channel::Foreign(_) => return None,
        })
    }

    /// The install one-liner, wrapped in the shell that runs it.
    fn installer_argv(self) -> Vec<String> {
        if cfg!(windows) {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!("iwr -useb {} | iex", crate::constants::INSTALL_PS1_URL),
            ]
        } else {
            vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("curl -fsSL {} | sh", crate::constants::INSTALL_SH_URL),
            ]
        }
    }

    /// An argv as a user would type it.
    ///
    /// Joining the arguments is right for every channel but one: the installer’s argv
    /// wraps a shell one-liner in `powershell -Command` or `sh -c`, and printing the
    /// wrapper would hand the reader something they cannot paste.
    fn typed_form(self, argv: &[String]) -> String {
        if self == Channel::Installer {
            return argv.last().cloned().unwrap_or_default();
        }
        argv.join(" ")
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

    /// Whether `devp uninstall` may delete this copy with `fs::remove_file`.
    ///
    /// True in exactly two cases, and the two look identical from a path, which is why
    /// this is asked as its own question. The installer keeps no record beyond the file
    /// it wrote, and a copy in a location nothing claims is a file somebody moved there.
    /// Everything else -- a manager with a command, and a manager without one -- is
    /// removed by its manager or not at all: [`Self::uninstall_argv`] explains what
    /// deleting the file first costs, and a [`Channel::Foreign`] copy costs the same
    /// with no way to repair it afterwards.
    pub fn may_delete_directly(self) -> bool {
        matches!(self, Channel::Installer | Channel::Unknown)
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

/// A borrowed argv as an owned one.
fn owned(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
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

/// This binary, running from inside a project's own virtual environment.
///
/// The distinction that matters is not "was this installed by pip" — a machine-wide
/// `pip install` is a perfectly good way to get the tool. It is "does this copy live
/// inside one project's environment", because such a copy dies with the environment,
/// and until it does it is a package that project's `requirements.txt` has to account
/// for before the environment can ever be pruned.
///
/// `pyvenv.cfg` one directory above the script is what separates the two: every virtual
/// environment has one and no system install does.
pub struct ProjectVenvInstall {
    /// The environment root — the directory holding `pyvenv.cfg`.
    pub venv: PathBuf,
    /// The directory the environment sits in, which is the project in every layout
    /// anyone actually uses.
    pub project: PathBuf,
}

/// Detect a [`ProjectVenvInstall`] for `exe`, or `None` if this copy lives anywhere else.
///
/// Takes the executable rather than reading `current_exe` so the detection can be tested
/// against a directory tree instead of against whichever machine runs the suite.
pub fn project_venv_install(exe: &Path) -> Option<ProjectVenvInstall> {
    if !pip_script_beside(exe) {
        return None;
    }
    let venv = exe.parent()?.parent()?;
    if !venv.join("pyvenv.cfg").exists() {
        return None;
    }
    Some(ProjectVenvInstall {
        venv: venv.to_path_buf(),
        project: venv.parent()?.to_path_buf(),
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

    // bun keeps its global bin in the same place on every platform.
    dirs.push(home.join(".bun").join("bin"));

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
        dirs.push(home.join("AppData").join("Local").join("pnpm"));
        dirs.push(home.join("AppData").join("Local").join("Yarn").join("bin"));
    } else {
        dirs.push(home.join(".npm-global").join("bin"));
        dirs.push(home.join(".local").join("share").join("pnpm"));
        dirs.push(home.join(".yarn").join("bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/home/linuxbrew/.linuxbrew/bin"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn each_channel_is_recognised_by_its_marker_directory() {
        let cases: &[(&str, Channel)] = &[
            ("/home/k/.cargo/bin/dev-prune", Channel::Cargo),
            (
                "/usr/lib/node_modules/dev-prune/bin/dev-prune",
                Channel::Npm,
            ),
            // The platform package, which is where the executable npm actually runs
            // lives. `devp doctor` suppresses its missing-twin warning on the strength
            // of this: npm ships the second name as a launcher of its own, so there is
            // no file to look for beside this one.
            (
                "/usr/lib/node_modules/dev-prune-linux-x64/bin/dev-prune",
                Channel::Npm,
            ),
            // The three npm-compatible clients, at the path a *global* install of
            // dev-prune actually produces: the npm package is a dispatcher plus one
            // platform package, so the executable is always inside a `node_modules`
            // tree and every one of these used to read as `Channel::Npm`.
            (
                "/home/k/.bun/install/global/node_modules/@dev-prune/linux-x64/dev-prune",
                Channel::Bun,
            ),
            (
                "/home/k/.local/share/pnpm/global/5/node_modules/@dev-prune/linux-x64/dev-prune",
                Channel::Pnpm,
            ),
            (
                "/home/k/.config/yarn/global/node_modules/@dev-prune/linux-x64/dev-prune",
                Channel::Yarn,
            ),
            (
                r"C:\Users\k\AppData\Local\pnpm\global\5\node_modules\@dev-prune\win32-x64\dev-prune.exe",
                Channel::Pnpm,
            ),
            (
                r"C:\Users\k\AppData\Local\Yarn\Data\global\node_modules\@dev-prune\win32-x64\dev-prune.exe",
                Channel::Yarn,
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

    /// The paths above are where the pnpm *package* lands. The executable on PATH is the
    /// shim one level up, straight in `PNPM_HOME` — and that is the only one of the two
    /// `sweep_dirs` looks in, so it is the copy `devp uninstall` actually finds. While the
    /// marker required `global/`, that shim read as `Unknown`, which
    /// [`Channel::may_delete_directly`] permits deleting outright: the sweep removed the
    /// file pnpm's own manifest still points at, without ever running `pnpm remove -g`.
    #[test]
    fn the_pnpm_shim_is_pnpm_and_not_an_unowned_file() {
        for path in [
            "/home/k/.local/share/pnpm/devp",
            "/home/k/.local/share/pnpm/dev-prune",
            r"C:\Users\k\AppData\Local\pnpm\devp.exe",
        ] {
            let channel = Channel::detect_at(Path::new(path), None);
            assert_eq!(channel, Channel::Pnpm, "{path}");
            assert!(!channel.may_delete_directly(), "{path}");
            assert!(channel.uninstall_argv().is_some(), "{path}");
        }
        // The fragment is broad enough to catch the shim without catching pnpm's virtual
        // store, which spells the directory with a leading dot.
        assert_eq!(
            Channel::detect_at(
                Path::new(
                    "/w/app/node_modules/.pnpm/dev-prune@1.0.0/node_modules/dev-prune/bin/dev-prune"
                ),
                None
            ),
            Channel::Npm
        );
    }

    /// Before `Channel::Foreign` these were `Unknown`, and `devp uninstall --yes`
    /// deleted them. A Deno or Volta or mise install leaves no fragment any other
    /// marker matches, so nothing distinguished one from a binary somebody copied.
    #[test]
    fn a_managed_tree_with_no_known_commands_is_foreign_rather_than_unknown() {
        for (path, name) in [
            ("/home/k/.deno/bin/dev-prune", "Deno"),
            ("/home/k/.volta/bin/dev-prune", "Volta"),
            ("/home/k/.local/share/mise/shims/dev-prune", "mise"),
            ("/usr/bin/dev-prune", "the system package manager"),
        ] {
            assert_eq!(
                Channel::detect_at(Path::new(path), None),
                Channel::Foreign(name),
                "{path} was not read as {name}'s"
            );
        }
    }

    /// The tree the binary is in outranks whatever else happens to be in it.
    ///
    /// `/usr/bin` holds a `python3` on every Linux, and mise and asdf keep a `python`
    /// shim beside every other shim, so all three answered `pip_script_beside` and were
    /// read as pip installs — `/usr/bin/dev-prune`, the distribution's own copy, would
    /// have been handed `pip install --upgrade`.
    #[test]
    fn a_python_next_door_does_not_make_a_foreign_tree_pips() {
        let tmp = TempDir::new().unwrap();
        let shims = tmp.path().join(".asdf/shims");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::write(shims.join("python3"), "").unwrap();
        std::fs::write(shims.join("python.exe"), "").unwrap();

        let exe = shims.join("dev-prune");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(Channel::detect_at(&exe, None), Channel::Foreign("asdf"));

        // Same for the other inference: a `node_modules/dev-prune` beside it does not
        // make the tree npm's either.
        std::fs::create_dir_all(shims.join("node_modules/dev-prune")).unwrap();
        assert_eq!(Channel::detect_at(&exe, None), Channel::Foreign("asdf"));

        // And neither check is broken, only outranked: the same neighbours in a tree
        // nothing claims still identify it.
        let loose = tmp.path().join("bin");
        std::fs::create_dir_all(loose.join("node_modules/dev-prune")).unwrap();
        let exe = loose.join("dev-prune");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(Channel::detect_at(&exe, None), Channel::Npm);
    }

    /// `/usr/local/bin` is where a person putting a binary somewhere by hand puts it,
    /// and reading it as the distribution's would make the sweep refuse to clean up
    /// after itself.
    ///
    /// Under a temp root rather than at the real path: `detect_at` reads the filesystem
    /// for its last two checks, and on the macOS runner Homebrew keeps a `python3` in
    /// the real `/usr/local/bin` — which makes that directory pip's on that machine, and
    /// makes the literal path a question about the runner instead of about the marker.
    #[test]
    fn usr_local_bin_stays_unclaimed() {
        let tmp = TempDir::new().unwrap();
        let bin = tmp.path().join("usr/local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert_eq!(
            Channel::detect_at(&bin.join("dev-prune"), None),
            Channel::Unknown
        );
    }

    /// Three channels have no `uninstall_argv`, and only two of them may be deleted.
    /// Conflating those was the bug: `Foreign` is a manager's file with no command to
    /// repair it afterwards, which makes deleting it the one move with no way back.
    #[test]
    fn only_the_installer_and_an_unclaimed_copy_may_be_deleted_outright() {
        for channel in [Channel::Installer, Channel::Unknown] {
            assert!(channel.uninstall_argv().is_none());
            assert!(channel.may_delete_directly(), "{channel:?}");
        }
        let foreign = Channel::Foreign("Deno");
        assert!(foreign.uninstall_argv().is_none());
        assert!(!foreign.may_delete_directly());
        // Nor is a command guessed for it anywhere else.
        assert!(foreign.install_argv().is_none());
        assert!(foreign.upgrade_argv().is_none());
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

    /// The install scripts write both names into `<config>/bin`, and `managed_exe_path`
    /// can only name one of them. Matching on the file made `devp` — the name every page
    /// of the documentation uses — come out as `Unknown`, so `devp update` told a user who
    /// had just run `install.ps1` that no package manager owned their copy while
    /// `dev-prune update`, the same binary under its other name, answered correctly.
    #[test]
    fn either_name_in_the_managed_directory_is_the_installer() {
        // Spelled per-platform: a `C:\…` raw string is a single relative component on
        // Unix, where `\` is an ordinary character — all three paths shared the same
        // empty parent there, and the sibling-directory case below passed as Installer.
        let (managed, twin, outside) = if cfg!(windows) {
            (
                r"C:\Users\k\AppData\Roaming\dev-prune\bin\dev-prune.exe",
                r"C:\Users\k\AppData\Roaming\dev-prune\bin\devp.exe",
                r"C:\Users\k\AppData\Roaming\dev-prune\bin2\devp.exe",
            )
        } else {
            (
                "/home/k/.config/dev-prune/bin/dev-prune",
                "/home/k/.config/dev-prune/bin/devp",
                "/home/k/.config/dev-prune/bin2/devp",
            )
        };
        let managed = Path::new(managed);
        for exe in [managed, Path::new(twin)] {
            assert_eq!(
                Channel::detect_at(exe, Some(managed)),
                Channel::Installer,
                "{}",
                exe.display()
            );
            // The whole point of getting this right: the installer channel can name its
            // own upgrade command, and `Unknown` cannot name anything.
            assert!(
                Channel::detect_at(exe, Some(managed))
                    .upgrade_command()
                    .is_some()
            );
        }
        // A sibling directory is not the managed one, however similar the name.
        assert_eq!(
            Channel::detect_at(Path::new(outside), Some(managed)),
            Channel::Unknown
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

    /// Volta, mise and Nix all install npm packages into a `node_modules` tree of their
    /// own, so each of these paths matches npm's marker as well as its manager's. `Npm`
    /// here meant `devp update` writing release bytes into a tree the real manager
    /// versions — the foreign name has to win before the npm family is consulted.
    #[test]
    fn a_foreign_tree_wins_over_the_node_modules_inside_it() {
        for (path, name) in [
            (
                "/home/k/.volta/tools/image/packages/dev-prune/lib/node_modules/dev-prune/bin/dev-prune",
                "Volta",
            ),
            (
                "/home/k/.local/share/mise/installs/npm-dev-prune/1.12.0/lib/node_modules/dev-prune/bin/dev-prune",
                "mise",
            ),
            (
                "/nix/store/abc123-dev-prune-1.12.0/lib/node_modules/dev-prune/bin/dev-prune",
                "Nix",
            ),
        ] {
            assert_eq!(
                Channel::detect_at(Path::new(path), None),
                Channel::Foreign(name),
                "{path}"
            );
        }
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
            Channel::Bun,
            Channel::Pnpm,
            Channel::Yarn,
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

    /// The badge is printed one space after the version, so anything that reads as a
    /// sentence fragment there is a bug in the banner rather than in the prose. A leading
    /// article is how `label()` phrases itself for a sentence, and it is the thing that
    /// looks wrong beside a version number.
    #[test]
    fn no_badge_reads_as_a_sentence_fragment() {
        let all = [
            Channel::Installer,
            Channel::Cargo,
            Channel::Npm,
            Channel::Bun,
            Channel::Pnpm,
            Channel::Yarn,
            Channel::UvTool,
            Channel::Pipx,
            Channel::Pip,
            Channel::WinGet,
            Channel::Scoop,
            Channel::Homebrew,
            Channel::Unknown,
            Channel::Foreign("Nix"),
        ];
        for channel in all {
            let badge = channel.badge();
            assert!(!badge.is_empty(), "{channel:?} has no badge");
            assert!(
                !badge.starts_with("the ") && !badge.starts_with("an "),
                "{channel:?} badges as {badge:?}, which is a clause"
            );
            assert!(
                !badge.contains('\n'),
                "{channel:?} badges across two lines: {badge:?}"
            );
        }
    }

    /// A hand-placed copy is the case the banner exists to name. `Unknown` is what
    /// [`Channel::detect_at`] returns for a binary somebody downloaded from the releases
    /// page and dropped somewhere, and every other thing dev-prune says about that copy —
    /// that `devp update` has no manager to call, that `devp uninstall` may delete the
    /// file outright — follows from it.
    #[test]
    fn a_downloaded_copy_badges_as_standalone() {
        let dir = TempDir::new().unwrap();
        let exe = dir.path().join("dev-prune.exe");
        let channel = Channel::detect_at(&exe, None);
        assert_eq!(channel, Channel::Unknown);
        assert_eq!(channel.badge(), "standalone");
        assert!(channel.upgrade_command().is_none());
        assert!(channel.may_delete_directly());
    }

    #[test]
    fn every_managed_channel_can_name_both_of_its_commands() {
        for channel in [
            Channel::Cargo,
            Channel::Npm,
            Channel::Bun,
            Channel::Pnpm,
            Channel::Yarn,
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
        // Each of the four npm-compatible clients has to name *its own* client. Getting
        // this wrong is not a cosmetic slip: it installs a second copy under a second
        // manager's prefix and leaves the first one stale and still on PATH.
        for (channel, client) in [
            (Channel::Npm, "npm"),
            (Channel::Bun, "bun"),
            (Channel::Pnpm, "pnpm"),
            (Channel::Yarn, "yarn"),
        ] {
            for command in [
                channel.upgrade_command().unwrap(),
                channel.uninstall_command().unwrap(),
            ] {
                assert!(
                    command.starts_with(client),
                    "{channel:?} names `{command}`, not {client}"
                );
            }
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
        for marker in ["cargo", "uv", "pipx", "bun", "pnpm", "yarn"] {
            assert!(joined.contains(marker), "{marker} missing from {joined}");
        }
        let platform = if cfg!(windows) { "winget" } else { "homebrew" };
        assert!(
            joined.contains(platform),
            "{platform} missing from {joined}"
        );
    }

    /// A virtual environment on disk: the interpreter beside the script, and the
    /// `pyvenv.cfg` one level up that no system-wide install has.
    fn make_project_venv(root: &Path, with_cfg: bool, with_python: bool) -> PathBuf {
        let venv = root.join("proj").join(".venv");
        let scripts = venv.join("bin");
        std::fs::create_dir_all(&scripts).unwrap();
        if with_python {
            std::fs::write(scripts.join("python"), "").unwrap();
        }
        if with_cfg {
            std::fs::write(venv.join("pyvenv.cfg"), "").unwrap();
        }
        let exe = scripts.join("devp");
        std::fs::write(&exe, "").unwrap();
        exe
    }

    #[test]
    fn a_copy_inside_a_project_venv_is_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let exe = make_project_venv(dir.path(), true, true);
        let found = project_venv_install(&exe).expect("a venv install");
        assert_eq!(found.venv, dir.path().join("proj").join(".venv"));
        assert_eq!(found.project, dir.path().join("proj"));
    }

    #[test]
    fn a_machine_wide_pip_install_is_not_a_project_venv() {
        // An interpreter beside the script is not enough on its own: `/usr/bin` has one
        // too, and telling somebody their machine-wide install is in the wrong place is
        // both wrong and unfixable.
        let dir = tempfile::tempdir().unwrap();
        let exe = make_project_venv(dir.path(), false, true);
        assert!(project_venv_install(&exe).is_none());
    }

    #[test]
    fn a_copy_with_no_interpreter_beside_it_is_not_a_venv_install() {
        let dir = tempfile::tempdir().unwrap();
        let exe = make_project_venv(dir.path(), true, false);
        assert!(project_venv_install(&exe).is_none());
    }
}
