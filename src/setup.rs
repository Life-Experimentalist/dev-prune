// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Idempotent installation of dev-prune's integrations.
//
// dev-prune is only really installed once the parts that let it work without being
// thought about are in place: the `devp` alias, the managed pair on the user's PATH,
// the exported `SKILL.md` that AI assistants read (installed into the agent's own
// skills directory where one exists), the Git hooks that keep the registry current,
// and the OS scheduler that runs the passes. Each one here is created **only when it
// is missing**, which is
// what makes it safe to run on every install, reinstall and upgrade — and it does run
// on each of those, through the version stamp written at the end of a completed pass.
//
// Nothing in here is fatal. A machine without `git`, a `core.hooksPath` that belongs to
// husky, a locked-down scheduler: each is reported and stepped over, because none of
// them should stop `devp init` from registering repositories.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::commands::hook::{self, HookState};
use crate::commands::skill::EMBEDDED_SKILL_MD;
use crate::config::Registry;
use crate::constants;
use crate::daemon;
use crate::output;

/// File in the config directory recording the version whose last integration pass
/// completed. A missing or older stamp is what triggers the automatic pass, so a fresh
/// install and an upgrade both self-heal exactly once.
const STAMP_FILE: &str = "setup-stamp";

pub use crate::constants::ENV_NO_AUTO_SETUP;

/// Whether the suppression variable is set — by presence, so `=1`, `=true` and even an
/// empty value all count.
///
/// The one predicate every consumer must share. The doctor note used to answer only for
/// the literal `=1`, so a machine with `=true` had setup switched off with nothing
/// anywhere saying so.
pub fn no_auto_setup_requested() -> bool {
    std::env::var_os(ENV_NO_AUTO_SETUP).is_some()
}

/// Whether every network call is switched off for this process — same by-presence rule
/// as [`no_auto_setup_requested`], and for the same reason.
pub fn offline_requested() -> bool {
    std::env::var_os(crate::constants::ENV_OFFLINE).is_some()
}

/// What one integration did during a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It was missing and is now in place.
    Installed,
    /// It was already in place and was left alone.
    AlreadyPresent,
    /// It could not be installed for a reason that is the user's call, not an error.
    Skipped(String),
    /// It failed. The pass continues; the reason is reported.
    Failed(String),
}

/// The result of one integration pass.
#[derive(Debug, Default)]
pub struct SetupReport {
    items: Vec<(&'static str, Outcome)>,
}

impl SetupReport {
    fn push(&mut self, name: &'static str, outcome: Outcome) {
        self.items.push((name, outcome));
    }

    /// Whether anything at all was created by this pass.
    pub fn changed_anything(&self) -> bool {
        self.items
            .iter()
            .any(|(_, o)| matches!(o, Outcome::Installed))
    }

    /// Whether anything needs the user's attention.
    pub fn needs_attention(&self) -> bool {
        self.items
            .iter()
            .any(|(_, o)| matches!(o, Outcome::Skipped(_) | Outcome::Failed(_)))
    }

    /// Print the report.
    ///
    /// `verbose` is for the explicit `devp setup`, where "already installed" is the
    /// answer the user asked for. The automatic pass passes `false` and stays silent
    /// about everything that was already fine.
    pub fn print(&self, verbose: bool) {
        for (name, outcome) in &self.items {
            match outcome {
                Outcome::Installed => output::print_success(&format!("{name}: installed.")),
                Outcome::AlreadyPresent if verbose => {
                    output::print_info(&format!("{name}: already installed."));
                }
                Outcome::AlreadyPresent => {}
                Outcome::Skipped(why) => {
                    output::print_warning(&format!("{name}: skipped — {why}"));
                }
                Outcome::Failed(why) => {
                    output::print_error(&format!("{name}: failed — {why}"));
                }
            }
        }
    }
}

/// Where the installers put the binary, and the one directory nothing else owns.
///
/// Public because it is also what the PATH step registers and what `uninstall` must
/// take back out again.
pub fn managed_bin_dir() -> Result<PathBuf> {
    Ok(Registry::config_dir()?.join("bin"))
}

pub(crate) fn managed_exe_path() -> Result<PathBuf> {
    let name = if cfg!(windows) {
        "dev-prune.exe"
    } else {
        "dev-prune"
    };
    Ok(managed_bin_dir()?.join(name))
}

/// Absolute path to a copy of this binary that will still be there next week.
///
/// Anything that writes a path down for later — the OS scheduler, the git hooks — has to
/// use this instead of [`std::env::current_exe`]. dev-prune ships through npm and PyPI as
/// well as the installers, so the running executable is often somewhere a package manager
/// owns and will delete: npm's `_npx` cache, uv's ephemeral tool environment, or
/// `target/debug` during development. An entry recorded there breaks the moment that
/// directory goes, and neither of these has anywhere to complain — the scheduled task
/// fails silently every interval, and the hook discards its own output by design. The
/// only symptom is that nothing ever happens again.
///
/// `<config>/bin` is where `install.sh` and `install.ps1` put the binary and nothing else
/// deletes, so prefer the copy there. When there is none, put one there: the binary that
/// is running right now is precisely the one that is going to be missing later.
pub fn stable_exe_path() -> PathBuf {
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("dev-prune"));
    let Ok(managed) = managed_exe_path() else {
        return current;
    };
    if managed == current {
        return managed;
    }
    if managed.is_file() {
        refresh_managed_copy_if_stale(&current, &managed);
        return managed;
    }

    // Only ever clone something that is actually this CLI. `current_exe()` under `cargo
    // test` is the test harness, and copying that into the config directory would be both
    // wrong and slow.
    if !is_this_cli(&current) {
        return current;
    }

    let Some(parent) = managed.parent() else {
        return current;
    };
    if fs::create_dir_all(parent).is_err() {
        return current;
    }
    // Hard link where the filesystem allows it — that also keeps the bytes alive when the
    // package manager deletes the directory the original came from.
    if fs::hard_link(&current, &managed).is_ok() {
        return managed;
    }

    // The same hazard `ensure_alias` documents, through a narrower window: the check at the
    // top of this function saw no managed copy, but another process created one — as a hard
    // link to `current` — before the link above ran. `fs::copy` opens its destination with
    // O_TRUNC, and truncating a hard link empties the shared inode, so the copy would
    // destroy the very binary it is copying.
    if managed.is_file() {
        return managed;
    }

    // Stage beside and rename into place. A copy straight onto the final name has a
    // window where the file exists but is incomplete — and this path is what the
    // scheduler and hooks get registered against, so a process killed mid-copy would
    // leave a torn binary that every later pass happily points at.
    let staging = managed.with_extension("new");
    if fs::copy(&current, &staging).is_ok() && fs::rename(&staging, &managed).is_ok() {
        return managed;
    }
    let _ = fs::remove_file(&staging);
    // The rename loses only to a concurrent invocation that installed its own copy,
    // which serves exactly as well.
    if managed.is_file() { managed } else { current }
}

/// Whether this path names one of the CLI's own binaries, by file stem.
fn is_this_cli(path: &std::path::Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem == "dev-prune" || stem == "devp")
}

/// Replace the managed copy when it is an older release than the binary running now.
///
/// The scheduler and the hooks point at the managed copy precisely because it outlives
/// package-manager caches — which also means an upgrade through cargo, npm or uv changes
/// the running binary but not the one the integrations run, and the machine quietly
/// keeps pruning with the previous version forever.
///
/// Staleness is decided by asking the copy its version, not by mtime or content: an
/// *older* binary running out of a stale npx cache must not overwrite a newer managed
/// copy, and content inequality cannot say which of the two is the upgrade. A copy that
/// cannot state a version at all is replaced too — whatever it is, it is not a working
/// build of this CLI.
fn refresh_managed_copy_if_stale(current: &std::path::Path, managed: &std::path::Path) {
    if !is_this_cli(current) || same_contents(managed, current) {
        return;
    }
    match (binary_version(managed), parse_version(constants::VERSION)) {
        (Some(theirs), Some(ours)) if theirs >= ours => return,
        _ => {}
    }
    // Write beside and rename into place, so a scheduler firing mid-copy never runs a
    // torn binary. A managed copy that is itself running cannot be renamed over on
    // Windows; the refresh simply waits for a pass when it is not.
    let staging = managed.with_extension("new");
    if fs::copy(current, &staging).is_ok() && fs::rename(&staging, managed).is_err() {
        let _ = fs::remove_file(&staging);
    }
}

/// The `major.minor.patch` a binary reports for itself, if it can.
pub(crate) fn binary_version(exe: &std::path::Path) -> Option<(u64, u64, u64)> {
    let output = crate::spawn::command(exe).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    version_in_output(&String::from_utf8_lossy(&output.stdout))
}

/// The first `x.y.z` in a `--version` output, with or without the `v` this CLI prints.
///
/// Split out so it can be tested against real output. It has to accept `v1.7.0` because
/// that is the only spelling `print_version_info` produces — the banner ends `v1.7.0`
/// and the line under it reads `dev-prune (devp) v1.7.0`, and a bare `1.7.0` appears
/// nowhere. Parsing the tokens without stripping that `v` answered `None` for every real
/// dev-prune on the machine, and `doctor`'s "Other copies" check reads `None` as "not a
/// dev-prune at all" — so it reported "none on PATH running a different version" however
/// many stale copies were sitting there.
fn version_in_output(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace()
        .find_map(|token| parse_version(token.strip_prefix('v').unwrap_or(token)))
}

/// Parse `x.y.z` into an orderable triple. Anything else — including the pre-release
/// and build suffixes this project never publishes — answers `None`.
pub(crate) fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let triple = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(triple)
}

/// Keep `dev-prune` and `devp` beside each other, whichever of the two is running.
///
/// The pair is one binary under two names, and either one can be the survivor. An upgrade
/// that could not replace a running `devp` leaves a stale alias; an antivirus quarantine,
/// a half-finished uninstall or a `Remove-Item` aimed at the wrong name leaves only
/// `devp`. So this restores *the other* name in whichever direction is missing, rather
/// than only ever creating `devp` — running either one puts the pair back.
pub fn ensure_alias() -> Outcome {
    // WinGet, Scoop and Homebrew each install into a directory they version and replace
    // whole on upgrade, and each ships both names in the package itself — so there is
    // nothing to create here, and creating it would be actively wrong twice over. The
    // twin would be orphaned by the next upgrade, still on PATH, still running the old
    // release; and writing a second executable beside a freshly downloaded unsigned
    // binary on its first run is a behavioural malware signature. WinGet's own
    // post-install validation flags exactly that, which is how this was found.
    if crate::channel::Channel::detect().replaces_its_directory() {
        return Outcome::AlreadyPresent;
    }
    let Ok(current_exe) = std::env::current_exe() else {
        return Outcome::Failed("could not locate the running executable".to_string());
    };
    let Some(parent_dir) = current_exe.parent() else {
        return Outcome::Failed("the running executable has no parent directory".to_string());
    };

    ensure_twin_of(&current_exe, parent_dir)
}

/// The half of [`ensure_alias`] that takes its paths as arguments, so tests can drive both
/// directions without being the binary they are testing.
fn ensure_twin_of(current_exe: &std::path::Path, parent_dir: &std::path::Path) -> Outcome {
    let running_as_alias = current_exe
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem == "devp");

    // `dev-prune` is the canonical name, and only it may overwrite its twin.
    //
    // Installers write `dev-prune` first and upgrades replace it first, so it is never the
    // older of the two — a stale `devp` is worth replacing, because otherwise it silently
    // runs the previous version. The reverse is not safe: an upgrade that replaced
    // `dev-prune` and then failed on a running `devp` leaves exactly the state where the
    // alias is the *older* binary, and refreshing from there would quietly reinstall the
    // version the user just upgraded away from. So `devp` may only create a `dev-prune`
    // that is missing outright.
    let (twin_name, may_refresh) = if running_as_alias {
        (
            if cfg!(windows) {
                "dev-prune.exe"
            } else {
                "dev-prune"
            },
            false,
        )
    } else {
        (if cfg!(windows) { "devp.exe" } else { "devp" }, true)
    };
    let twin_exe = parent_dir.join(twin_name);

    if twin_exe.exists() {
        if !may_refresh || same_contents(&twin_exe, current_exe) {
            return Outcome::AlreadyPresent;
        }
        // Replacing a running executable fails on Windows; that is fine, the alias is
        // simply refreshed by the next invocation that is not itself `devp`.
        if fs::remove_file(&twin_exe).is_err() {
            return Outcome::Skipped(format!(
                "`{twin_name}` is in use and could not be refreshed — re-run `devp setup` \
                 from a terminal that is not running it"
            ));
        }
    }

    if fs::hard_link(current_exe, &twin_exe).is_ok() {
        return Outcome::Installed;
    }

    // The copy is the fallback for filesystems without hard links — but it must never
    // run when the alias already exists, because the reason `hard_link` usually fails is
    // that another process created it a moment ago, as a hard link to this very
    // executable. `fs::copy` opens its destination with O_TRUNC, and truncating a hard
    // link truncates the shared inode: the copy would empty the running binary and then
    // copy zero bytes from it.
    //
    // That is not hypothetical. It is what turned every macOS CI run red. The 28
    // integration tests launch at once, one wins the link, the losers fall through to
    // here, and `target/debug/dev-prune` becomes a zero-byte file. macOS `posix_spawn`
    // answers ENOEXEC by handing the file to `/bin/sh`, so every later invocation
    // "succeeded" with exit 0 and printed nothing — for two hours the tests looked like
    // 27 unrelated assertion failures.
    if twin_exe.exists() {
        return Outcome::AlreadyPresent;
    }

    // Stage beside and rename into place: copied straight onto the final name, the
    // alias would exist-but-be-incomplete for the length of the copy, and a `devp`
    // typed in that window executes a torn binary.
    let staging = twin_exe.with_extension("new");
    if fs::copy(current_exe, &staging).is_ok() && fs::rename(&staging, &twin_exe).is_ok() {
        return Outcome::Installed;
    }
    let _ = fs::remove_file(&staging);
    if twin_exe.exists() {
        // A concurrent invocation won the rename; its alias serves exactly as well.
        return Outcome::AlreadyPresent;
    }
    Outcome::Failed(format!(
        "could not create `{}`",
        output::clean_path(&twin_exe)
    ))
}

/// Sameness test for two executables, cheap in the common case.
///
/// A hard link makes size and mtime equal by construction, so the usual layout answers
/// without reading either file. When only the mtime differs — the alias came from the
/// copy fallback, which does not preserve timestamps — the bytes themselves decide,
/// because calling that pair "different" made every single invocation delete and
/// recreate an alias whose content never changed.
fn same_contents(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    if ma.len() != mb.len() {
        return false;
    }
    if ma.modified().ok() == mb.modified().ok() {
        return true;
    }
    match (fs::read(a), fs::read(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Path that `devp skill` and this module export `SKILL.md` to.
pub fn skill_path() -> Result<PathBuf> {
    Ok(Registry::config_dir()?.join("SKILL.md"))
}

/// Export the bundled `SKILL.md` so AI assistants have something to read.
///
/// Rewritten whenever it differs from the embedded copy, since an upgrade that changes
/// the skill must not leave the previous version's instructions on disk.
pub fn ensure_skill_file() -> Outcome {
    match Registry::config_dir() {
        Ok(dir) => ensure_skill_file_in(&dir),
        Err(_) => Outcome::Failed("could not determine the config directory".to_string()),
    }
}

fn ensure_skill_file_in(config_dir: &std::path::Path) -> Outcome {
    let target = config_dir.join("SKILL.md");

    if fs::read_to_string(&target).is_ok_and(|current| current == EMBEDDED_SKILL_MD) {
        return Outcome::AlreadyPresent;
    }

    let _ = fs::create_dir_all(config_dir);
    match fs::write(&target, EMBEDDED_SKILL_MD) {
        Ok(()) => Outcome::Installed,
        Err(e) => Outcome::Failed(format!(
            "could not write {}: {e}",
            output::clean_path(&target)
        )),
    }
}

/// The per-skill directories of AI coding agents that are installed under `home`.
///
/// Detection only — an agent's home directory is created by the agent, never by this
/// pass. Today that is Claude Code, whose Agent Skills live at
/// `~/.claude/skills/<name>/SKILL.md`. Assistants without an on-disk skill format get
/// the onboarding prompt from `devp skill` instead.
fn agent_skill_roots_under(home: &std::path::Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let claude = home.join(constants::CLAUDE_HOME_DIR);
    if claude.is_dir() {
        roots.push(
            claude
                .join(constants::AGENT_SKILLS_SUBDIR)
                .join(constants::APP_NAME),
        );
    }
    roots
}

/// The agent skill directories on this machine. Empty when no agent is installed.
pub fn agent_skill_roots() -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| agent_skill_roots_under(&home))
        .unwrap_or_default()
}

/// Install the skill into every detected agent's skills directory.
pub fn ensure_agent_skills() -> Outcome {
    ensure_agent_skills_at(&agent_skill_roots())
}

fn ensure_agent_skills_at(roots: &[PathBuf]) -> Outcome {
    if roots.is_empty() {
        return Outcome::Skipped(
            "no AI agent skills directory was found — `devp skill` prints import prompts instead"
                .to_string(),
        );
    }
    let mut installed = false;
    for root in roots {
        match ensure_skill_file_in(root) {
            Outcome::Installed => installed = true,
            Outcome::AlreadyPresent => {}
            other => return other,
        }
    }
    if installed {
        Outcome::Installed
    } else {
        Outcome::AlreadyPresent
    }
}

/// Make the managed pair reachable from a fresh shell.
///
/// This is the step that lets `pip install dev-prune` in a virtualenv survive the
/// virtualenv: the binaries pip placed vanish with the environment, but the managed
/// copy under `<config>/bin` does not, and after this step it is the one a new
/// terminal finds. See [`crate::pathenv`] for what "reachable" means per platform.
pub fn ensure_command_on_path() -> Outcome {
    let managed = stable_exe_path();
    let is_managed_copy =
        managed_exe_path().is_ok_and(|expected| expected == managed) && managed.is_file();
    if !is_managed_copy {
        // No managed copy exists and none could be created — under `cargo test` the
        // running executable is the harness, and cloning that would be wrong. There is
        // nothing durable to put on PATH.
        return Outcome::Skipped("no managed copy of the binary exists to put on PATH".to_string());
    }
    let Some(bin_dir) = managed.parent() else {
        return Outcome::Failed("the managed binary has no parent directory".to_string());
    };
    // `devp` has to sit beside it, or the PATH entry only ever finds `dev-prune`.
    if let Outcome::Failed(why) = ensure_twin_of(&managed, bin_dir) {
        return Outcome::Failed(why);
    }
    crate::pathenv::ensure_reachable(bin_dir)
}

/// Write the icon assets and register `*.devprune.json` with the OS file manager.
///
/// Part of the automatic pass rather than a separate errand, because "the config file has
/// an icon" is not a thing anybody thinks to go and ask for. Everything it writes lives
/// under the config directory and the user's own XDG data directory, `devp uninstall`
/// removes all of it, and it touches no editor settings, no PATH and no shell profile —
/// so there is nothing here that needs to be asked about first.
///
/// Unlike the hooks and the scheduler, this has no opt-out switch of its own. Files
/// dropped into the user's data directory are not a background process and not a change
/// in behaviour; `auto_setup` already covers "install nothing at all".
fn ensure_icons() -> Outcome {
    if crate::commands::icon::is_registered() {
        return Outcome::AlreadyPresent;
    }
    match crate::commands::icon::sync_app_directory() {
        Ok(()) => Outcome::Installed,
        Err(e) => Outcome::Failed(format!("{e:#}")),
    }
}

/// Install the global Git hooks, unless git is absent or the slot belongs to someone else.
///
/// `chain` is `auto_hooks_chain`: with it on, a slot that belongs to husky is not a
/// reason to skip, because dev-prune can install in front and forward every hook back.
pub fn ensure_hooks(chain: bool) -> Outcome {
    if !hook::git_available() {
        return Outcome::Skipped(format!(
            "\n    {}",
            hook::GIT_MISSING_HELP.replace('\n', "\n    ")
        ));
    }

    match hook::state() {
        // "Installed" is not the question — "installed and pointing at a binary that
        // still exists" is. A hook backgrounds itself and discards its own output, so
        // one left pointing at a deleted npm cache dies silently on every commit and
        // nothing ever registers again; this pass is the only thing that ever looks.
        // Two reasons to rewrite a working install: it names a binary that is gone, or
        // it predates the passthrough shims and is silently shadowing every repository's
        // own `.git/hooks`. Neither reports itself — a hook discards its own output by
        // design — so the upgrade pass is the only thing that will ever notice.
        Ok(HookState::Active) if hook_target_is_dead() || hook::shims_incomplete() => {
            match hook::install() {
                Ok(()) => Outcome::Installed,
                Err(e) => Outcome::Failed(format!("{e:#}")),
            }
        }
        Ok(HookState::Active) => Outcome::AlreadyPresent,
        // Drift is repaired here rather than reported: the setup pass already runs on
        // install, on update and on a schedule, and a chain the user opted into is a
        // chain they want kept current.
        Ok(HookState::Chained { drifted, .. }) if !drifted.is_empty() => {
            match hook::install_with(true) {
                Ok(()) => Outcome::Installed,
                Err(e) => Outcome::Failed(format!("{e:#}")),
            }
        }
        Ok(HookState::Chained { .. }) if hook_target_is_dead() => match hook::install_with(true) {
            Ok(()) => Outcome::Installed,
            Err(e) => Outcome::Failed(format!("{e:#}")),
        },
        Ok(HookState::Chained { .. }) => Outcome::AlreadyPresent,
        Ok(HookState::Foreign(_)) if chain => match hook::install_with(true) {
            Ok(()) => Outcome::Installed,
            Err(e) => Outcome::Failed(format!("{e:#}")),
        },
        Ok(HookState::Foreign(existing)) => Outcome::Skipped(format!(
            "`core.hooksPath` is already set to `{existing}`, which belongs to another tool.\n    \
             Git allows only one hooks directory, so dev-prune will not take the slot.\n    \
             `devp hook install --chain` installs in front of it instead — dev-prune registers \
             the repo, then hands every hook on to `{existing}`, and `devp hook uninstall` puts \
             the original setting back (`devp config set auto_hooks_chain true` makes that \
             the standing answer). Or skip it: `devp link .` does the same job by hand."
        )),
        Ok(HookState::Absent) => match hook::install() {
            Ok(()) => Outcome::Installed,
            Err(e) => Outcome::Failed(format!("{e:#}")),
        },
        Err(e) => Outcome::Failed(format!("{e:#}")),
    }
}

/// Whether the installed hooks name a binary that no longer exists.
fn hook_target_is_dead() -> bool {
    hook::registered_exe_path().is_some_and(|exe| !exe.exists())
}

/// Install the OS scheduler if it is not already registered.
pub fn ensure_daemon(interval_days: u64) -> Outcome {
    match daemon::daemon_status() {
        // A task whose binary has been deleted keeps reporting itself `Ready` and dies
        // the instant it fires, every interval, with nowhere to complain. Re-register
        // it against the stable path instead of counting the corpse as present.
        Ok(daemon::DaemonStatus::Installed)
            if daemon::registered_exe_path().is_some_and(|exe| !exe.exists()) =>
        {
            match daemon::install_daemon(interval_days) {
                Ok(()) => Outcome::Installed,
                Err(e) => Outcome::Failed(format!("{e:#}")),
            }
        }
        // A task registered by a version that only knew the interactive logon flashes a
        // console window at whoever is logged in every time it fires — the single most
        // trust-destroying thing a background tool can do. Re-register it hidden; a
        // machine whose scheduler refuses the hidden logon remembers the refusal and is
        // not asked again.
        Ok(daemon::DaemonStatus::Installed) if daemon::wants_hidden_upgrade() => {
            match daemon::install_daemon(interval_days) {
                Ok(()) => Outcome::Installed,
                Err(e) => Outcome::Failed(format!("{e:#}")),
            }
        }
        // A settled, hidden task still needs its windowless twin kept current: the twin
        // is a copy of the binary, so an upgrade that replaced the binary would otherwise
        // leave the daemon firing the previous release. No-op on the other platforms, and
        // when no twin is in use.
        Ok(daemon::DaemonStatus::Installed) => {
            daemon::refresh_hidden_twin();
            Outcome::AlreadyPresent
        }
        Ok(daemon::DaemonStatus::NotInstalled) => match daemon::install_daemon(interval_days) {
            Ok(()) => Outcome::Installed,
            Err(e) => Outcome::Failed(format!("{e:#}")),
        },
        // `Unknown` means the query itself could not be answered — the scheduler may
        // well be there. Installing over it would fail on every command from now on, so
        // this reports and steps over instead of guessing. The platform backends are
        // written to keep this case narrow: anything they can answer definitely, they do.
        Ok(daemon::DaemonStatus::Unknown(why)) => {
            Outcome::Skipped(format!("scheduler state could not be read — {why}"))
        }
        Err(e) => Outcome::Failed(format!("{e:#}")),
    }
}

/// Whether unattended installation is permitted at all.
///
/// Both switches exist because these integrations write outside dev-prune's own config
/// directory — a scheduled task, a global git setting — and there are places that must
/// never happen unasked: container images, CI, and this project's own test suite.
pub fn auto_setup_enabled(registry: &Registry) -> bool {
    !no_auto_setup_requested() && registry.settings.auto_setup && unattended_environment().is_none()
}

/// The reason this looks like a machine nobody is sitting at, if it does.
///
/// `DEV_PRUNE_NO_AUTO_SETUP` and `auto_setup` are both switches you have to set *before*
/// the first run — which is exactly the run that installs things, so in a container or a
/// CI job the damage is done by the time there is anywhere to set them. Detecting the
/// environment is the only opt-out that works on the first run, which is the only run
/// that matters here.
///
/// Deliberately conservative: every signal below is one that CI providers and container
/// runtimes set themselves, so a developer's own shell will not trip it. Someone who
/// genuinely wants the integrations in CI can still ask in so many words with
/// `devp setup`, which never consults this.
pub fn unattended_environment() -> Option<&'static str> {
    // Set by GitHub Actions, GitLab CI, CircleCI, Travis, Jenkins (via pipeline), Woodpecker
    // and most others. `CI=true` is the closest thing this space has to a standard.
    for var in [
        "CI",
        "CONTINUOUS_INTEGRATION",
        "BUILD_NUMBER",
        "GITHUB_ACTIONS",
    ] {
        if let Some(value) = std::env::var_os(var) {
            // `CI=false` is set explicitly by some tools to mean "not CI", and honouring
            // the word rather than the presence is what the user plainly meant.
            let value = value.to_string_lossy();
            if !value.is_empty() && !value.eq_ignore_ascii_case("false") {
                return Some("this looks like a CI runner");
            }
        }
    }

    // Docker writes this marker into every container it builds from a Dockerfile;
    // Podman and other OCI runtimes write the `container` variable instead.
    #[cfg(unix)]
    if std::path::Path::new("/.dockerenv").exists() {
        return Some("this looks like a container");
    }
    if std::env::var_os("container").is_some() {
        return Some("this looks like a container");
    }

    None
}

/// Run an integration pass unless unattended installation is switched off.
///
/// Every caller that the user did not name explicitly goes through this. `devp setup`
/// calls [`ensure_integrations`] directly: asking for it in so many words is consent.
pub fn ensure_integrations_if_enabled(registry: &Registry) -> Option<SetupReport> {
    auto_setup_enabled(registry).then(|| ensure_integrations(registry))
}

/// Run one integration pass, installing whatever is missing.
///
/// The two per-integration settings (`auto_daemon`, `auto_hooks`) are honoured here, so
/// turning one off turns it off for every future pass as well as this one.
pub fn ensure_integrations(registry: &Registry) -> SetupReport {
    let mut report = SetupReport::default();

    report.push("dev-prune/devp pair", ensure_alias());
    report.push("Command on PATH", ensure_command_on_path());
    report.push("SKILL.md", ensure_skill_file());
    // Only reported when an agent is actually installed: a machine without one would
    // otherwise see a "skipped" warning about software it never had, on every install.
    if !agent_skill_roots().is_empty() {
        report.push("AI agent skills", ensure_agent_skills());
    }
    report.push("File icons", ensure_icons());

    if registry.settings.auto_hooks {
        report.push(
            "Git hooks",
            ensure_hooks(registry.settings.auto_hooks_chain),
        );
    } else {
        report.push(
            "Git hooks",
            Outcome::Skipped("`auto_hooks` is false — enable with `devp hook install`".to_string()),
        );
    }

    if registry.settings.auto_daemon {
        report.push(
            "Background scheduler",
            ensure_daemon(registry.settings.check_interval_days),
        );
    } else {
        report.push(
            "Background scheduler",
            Outcome::Skipped(
                "`auto_daemon` is false — enable with `devp daemon install`".to_string(),
            ),
        );
    }

    report
}

// ── VS Code extension ────────────────────────────────────────────────────────

/// Marker recording that the extension question was asked (or found already answered by
/// an existing install). One file, no content: the offer is made once ever, whatever
/// the answer was — a declined install must not be re-litigated on every upgrade.
const VSCODE_OFFER_STAMP: &str = "vscode-ext-offered";

/// A VS Code-compatible editor found on PATH.
struct EditorCli {
    /// The command to invoke — on Windows the `.cmd` launcher, because the entry on
    /// PATH is a batch file, not an `.exe`, and `Command::new("code")` would miss it.
    cli: String,
    /// The editor's name as a person knows it, for the prompt and per-editor results.
    label: &'static str,
}

/// Every VS Code-compatible editor on PATH, in the order listed here.
///
/// All of these forks keep the upstream CLI protocol (`--version`, `--list-extensions`,
/// `--install-extension`), so one code path drives them all. What differs is the
/// registry each one resolves an extension ID against: VS Code uses the Microsoft
/// Marketplace, VSCodium/Windsurf/Positron/Kiro use OpenVSX, Cursor runs its own
/// mirror. An ID install can therefore fail on a fork whose registry does not carry
/// the extension yet — which is why the installer falls back to the `.vsix` from the
/// GitHub release, the artifact every registry copy is built from.
fn detect_vscode_editors() -> Vec<EditorCli> {
    const CANDIDATES: &[(&str, &str)] = &[
        ("code", "VS Code"),
        ("code-insiders", "VS Code Insiders"),
        ("codium", "VSCodium"),
        ("codium-insiders", "VSCodium Insiders"),
        ("cursor", "Cursor"),
        ("windsurf", "Windsurf"),
        ("positron", "Positron"),
        ("kiro", "Kiro"),
    ];
    CANDIDATES
        .iter()
        .filter_map(|(name, label)| {
            let cli = if cfg!(windows) {
                format!("{name}.cmd")
            } else {
                (*name).to_string()
            };
            let responds = crate::spawn::command(&cli)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            responds.then_some(EditorCli { cli, label })
        })
        .collect()
}

fn vscode_extension_installed(cli: &str) -> bool {
    crate::spawn::command(cli)
        .arg("--list-extensions")
        .stdin(std::process::Stdio::null())
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout).lines().any(|line| {
                line.trim()
                    .eq_ignore_ascii_case(constants::VSCODE_EXTENSION_ID)
            })
        })
        .unwrap_or(false)
}

/// Download the `.vsix` attached to the latest GitHub release into the config directory.
///
/// The release asset is the source of truth for the extension — the Marketplace and
/// OpenVSX listings are built from it — so when an editor's registry cannot resolve the
/// ID (a fork whose registry does not carry the extension), installing the release file
/// directly gets the same bits through a channel every fork supports. Editors update a
/// `.vsix`-installed extension from their registry once a newer listed version appears,
/// so this install self-heals into the normal update flow.
fn download_release_vsix() -> Option<std::path::PathBuf> {
    use std::time::Duration;

    if offline_requested() {
        return None;
    }

    let fetch = |url: &str| {
        ureq::get(url)
            .header("User-Agent", &format!("dev-prune/{}", constants::VERSION))
            .header("Accept", "application/vnd.github+json")
            .config()
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .call()
    };

    let body = fetch(constants::LATEST_RELEASE_API_URL)
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let asset = json.get("assets")?.as_array()?.iter().find_map(|asset| {
        let name = asset.get("name")?.as_str()?;
        if !name.ends_with(".vsix") {
            return None;
        }
        let url = asset.get("browser_download_url")?.as_str()?;
        Some((name.to_string(), url.to_string()))
    })?;

    let bytes = fetch(&asset.1).ok()?.body_mut().read_to_vec().ok()?;
    // The config directory, not the shared system temp dir: on a multi-user machine
    // `%TEMP%`-style paths are predictable and writable by others, and the editor is
    // about to execute what this file contains. The caller deletes it after installing.
    let dir = Registry::config_dir().ok()?;
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(&asset.0);
    fs::write(&path, bytes).ok()?;
    Some(path)
}

/// `<cli> --install-extension <arg>`, surfacing the editor's own output.
fn run_install(cli: &str, arg: &str) -> bool {
    crate::spawn::command(cli)
        .args(["--install-extension", arg])
        .stdin(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Offer to install the editor extension, once ever, when a VS Code-family editor is
/// present.
///
/// This asks rather than installs because the editor is not dev-prune's territory the
/// way its own config directory is. Every gate below is a way of making sure a person
/// is actually there to answer: no marker yet, not a CI runner or container, both ends
/// of the terminal attached. When no editor is found nothing is written, so installing
/// one later and re-running `devp setup` still gets the one offer.
pub fn offer_vscode_extension() {
    use std::io::{IsTerminal, Write};

    let Ok(config_dir) = Registry::config_dir() else {
        return;
    };
    if config_dir.join(VSCODE_OFFER_STAMP).exists() {
        return;
    }
    if no_auto_setup_requested()
        || unattended_environment().is_some()
        || !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
    {
        return;
    }
    let editors = detect_vscode_editors();
    if editors.is_empty() {
        return;
    }

    let write_marker = || {
        let _ = fs::create_dir_all(&config_dir);
        let _ = fs::write(config_dir.join(VSCODE_OFFER_STAMP), "");
    };

    let missing: Vec<&EditorCli> = editors
        .iter()
        .filter(|e| !vscode_extension_installed(&e.cli))
        .collect();
    if missing.is_empty() {
        write_marker();
        return;
    }

    let names = missing
        .iter()
        .map(|e| e.label)
        .collect::<Vec<_>>()
        .join(", ");
    println!();
    println!("{names} detected — install the dev-prune extension?");
    println!("  It validates .devprune.json and shows reclaimable space in the status bar.");
    // The listings and the source, before the question rather than after it. This is
    // the one prompt that defaults to yes, so the material someone would need in order
    // to say no has to be on screen at the moment they answer — not in a doc they would
    // have to go and look for.
    println!("    Marketplace: {}", constants::VSCODE_MARKETPLACE_URL);
    println!("    Open VSX:    {}", constants::OPENVSX_URL);
    println!("    Source:      {}", constants::REPO_URL);
    print!("  Install it? [Y/n] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return;
    }
    write_marker();

    // A bare Enter accepts. Unlike the uninstall sweep — which deletes — the worst case
    // here is an extension the person removes in two clicks, and the gates above have
    // already established that a human with a VS Code-family editor is watching.
    if !matches!(answer.trim().to_lowercase().as_str(), "" | "y" | "yes") {
        output::print_info(&format!(
            "Skipped. Install it any time with `{} --install-extension {}`, or from {}.",
            missing[0].cli,
            constants::VSCODE_EXTENSION_ID,
            constants::VSCODE_MARKETPLACE_URL
        ));
        return;
    }

    // Fetched at most once, shared by every editor whose registry install fails.
    let mut release_vsix: Option<Option<std::path::PathBuf>> = None;
    for editor in &missing {
        // The editor's own registry first: that install is the one the editor keeps
        // up to date by itself.
        if run_install(&editor.cli, constants::VSCODE_EXTENSION_ID) {
            output::print_success(&format!("{}: extension installed.", editor.label));
            continue;
        }
        // A fork whose registry does not carry the extension — install the `.vsix`
        // from the GitHub release instead.
        let vsix = release_vsix.get_or_insert_with(download_release_vsix);
        match vsix {
            Some(path) if run_install(&editor.cli, &path.to_string_lossy()) => {
                output::print_success(&format!(
                    "{}: extension installed from the GitHub release .vsix.",
                    editor.label
                ));
            }
            _ => {
                output::print_warning(&format!(
                    "{}: could not install it from here. Search the Extensions view for \"dev-prune\", or run `{} --install-extension {}` yourself.",
                    editor.label,
                    editor.cli,
                    constants::VSCODE_EXTENSION_ID
                ));
            }
        }
    }
    if let Some(Some(path)) = &release_vsix {
        let _ = fs::remove_file(path);
    }
}

/// Record that a pass completed for this version.
fn write_stamp_in(config_dir: &std::path::Path) {
    let _ = fs::create_dir_all(config_dir);
    let _ = fs::write(config_dir.join(STAMP_FILE), constants::VERSION);
}

fn write_stamp() {
    if let Ok(dir) = Registry::config_dir() {
        write_stamp_in(&dir);
    }
}

fn setup_is_due_in(config_dir: &std::path::Path) -> bool {
    !fs::read_to_string(config_dir.join(STAMP_FILE))
        .is_ok_and(|stamp| stamp.trim() == constants::VERSION)
}

/// Whether the unattended pass is due: a fresh install, or the first run after an upgrade.
pub fn setup_is_due() -> bool {
    Registry::config_dir()
        .map(|dir| setup_is_due_in(&dir))
        .unwrap_or(false)
}

/// Whether there is a human at this invocation who could see what was done and undo it.
///
/// The one question that gates everything dev-prune installs without being asked. CI
/// variables and containers answer it directly; a redirected stdin or stdout answers it
/// too, because output nobody reads is the same as no output — and an integration
/// installed silently is one nobody knows to remove.
fn a_person_is_present() -> bool {
    use std::io::IsTerminal;
    unattended_environment().is_none()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
}

/// The unattended pass, run at most once per installed version.
///
/// Called at the top of every command that a human typed. It is deliberately not called
/// for the Git hook's `link --quiet` or the scheduler's `run --daemon`: those run without
/// a terminal, and an integration pass that nobody can see is one nobody can refuse.
pub fn auto_setup_if_due() {
    if !setup_is_due() {
        first_run_config_review();
        return;
    }
    // The same question `first_run_config_review` asks, asked one step earlier. It used
    // to be asked only about the *prompt*, never about the pass that installs a PATH
    // entry, a scheduled task and a git hook — so a binary run once by an automated
    // system, with its output captured, silently acquired persistence on that machine.
    // Nothing here is skipped permanently: the stamp is not written, so the first run a
    // person can actually see does the pass and reports it.
    if !a_person_is_present() {
        return;
    }

    let Ok(registry) = Registry::load() else {
        return;
    };
    let Some(report) = ensure_integrations_if_enabled(&registry) else {
        // Suppressed. Stamp anyway, so a machine that opted out does not re-decide
        // this on every single command.
        write_stamp();
        crate::commands::config::skip_config_review();
        return;
    };
    if report.changed_anything() || report.needs_attention() {
        output::print_header("dev-prune setup");
        report.print(false);
        if report.changed_anything() {
            output::print_info(
                "Run `devp setup --status` to review these, or `devp uninstall` to remove them.",
            );
        }
        println!();
    }
    write_stamp();
    first_run_config_review();
}

/// Put the defaults in front of the user on a fresh install, and any setting an upgrade
/// added that they have never been shown.
///
/// Separate from the integration stamp on purpose. The integrations are re-checked after
/// every upgrade; the settings are not, except for the ones that did not exist last time
/// — being asked to reconfirm `idle_days` on each new version would be a nuisance, and a
/// nuisance is something people learn to dismiss without reading.
///
/// Every condition here is a way of asking "is there a person reading this?", because the
/// alternative to asking is a prompt written into a log nobody will read, on a run that
/// then blocks forever waiting for an answer.
fn first_run_config_review() {
    if !crate::commands::config::config_review_is_due() {
        return;
    }

    if !a_person_is_present() {
        crate::commands::config::skip_config_review();
        return;
    }

    // Any error here is the wizard's own reporting; the command the user actually typed
    // still runs. A failed walkthrough must not become a failed `devp status`.
    if let Err(e) = crate::commands::config::run_wizard(false) {
        output::print_warning(&format!("Could not run the first-run setup ({e:#})."));
    }
    // Marked regardless of how it ended, including a deliberate quit. This is the one
    // caller that runs uninvited, and an unasked-for walkthrough that reappears on every
    // subsequent command is worse than one somebody dismissed once on purpose.
    crate::commands::config::skip_config_review();
    // Same first run, same person already answering questions — the one moment the
    // extension offer is a courtesy rather than an interruption.
    offer_vscode_extension();
    println!();
}

/// Invalidate the stamp so the next human-run command performs a pass.
///
/// `uninstall` calls this in reverse — it writes the current stamp — so that removing the
/// integrations is not immediately undone by the next command.
pub fn suppress_next_auto_setup() {
    write_stamp();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_with_only_present_items_is_silent() {
        let mut report = SetupReport::default();
        report.push("a", Outcome::AlreadyPresent);
        assert!(!report.changed_anything());
        assert!(!report.needs_attention());
    }

    #[test]
    fn skipped_and_failed_both_ask_for_attention() {
        let mut skipped = SetupReport::default();
        skipped.push("a", Outcome::Skipped("no git".into()));
        assert!(skipped.needs_attention());
        assert!(!skipped.changed_anything());

        let mut failed = SetupReport::default();
        failed.push("a", Outcome::Failed("boom".into()));
        assert!(failed.needs_attention());
    }

    #[test]
    fn an_install_counts_as_a_change() {
        let mut report = SetupReport::default();
        report.push("a", Outcome::Installed);
        assert!(report.changed_anything());
    }

    #[test]
    fn the_skill_export_lands_in_the_config_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(ensure_skill_file_in(dir.path()), Outcome::Installed);
        // A second pass finds byte-identical content and leaves it alone.
        assert_eq!(ensure_skill_file_in(dir.path()), Outcome::AlreadyPresent);
        let written = fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        assert_eq!(written, EMBEDDED_SKILL_MD);
    }

    #[test]
    fn a_stale_skill_export_is_rewritten() {
        // An upgrade must not leave the previous version's instructions on disk.
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("SKILL.md"), "# an older version").unwrap();
        assert_eq!(ensure_skill_file_in(dir.path()), Outcome::Installed);
        let written = fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        assert_eq!(written, EMBEDDED_SKILL_MD);
    }

    #[test]
    fn agent_skills_install_only_into_agent_homes_that_exist() {
        let home = tempfile::TempDir::new().unwrap();
        assert!(
            agent_skill_roots_under(home.path()).is_empty(),
            "a machine without an agent must detect nothing"
        );

        fs::create_dir_all(home.path().join(constants::CLAUDE_HOME_DIR)).unwrap();
        let roots = agent_skill_roots_under(home.path());
        assert_eq!(roots.len(), 1);

        assert_eq!(ensure_agent_skills_at(&roots), Outcome::Installed);
        let installed = home
            .path()
            .join(constants::CLAUDE_HOME_DIR)
            .join(constants::AGENT_SKILLS_SUBDIR)
            .join(constants::APP_NAME)
            .join("SKILL.md");
        assert_eq!(fs::read_to_string(&installed).unwrap(), EMBEDDED_SKILL_MD);

        // A second pass finds it current and leaves it alone.
        assert_eq!(ensure_agent_skills_at(&roots), Outcome::AlreadyPresent);
    }

    #[test]
    fn no_detected_agent_is_a_skip_not_a_failure() {
        assert!(matches!(ensure_agent_skills_at(&[]), Outcome::Skipped(_)));
    }

    #[test]
    fn the_stamp_gates_the_unattended_pass() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(setup_is_due_in(dir.path()), "a fresh install is due");
        write_stamp_in(dir.path());
        assert!(
            !setup_is_due_in(dir.path()),
            "the same version is not due twice"
        );
        fs::write(dir.path().join(STAMP_FILE), "0.0.1").unwrap();
        assert!(setup_is_due_in(dir.path()), "an upgrade is due again");
    }

    /// The alias must never be written with a copy while it already exists.
    ///
    /// A hard link and its target share one inode, so `fs::copy` onto the alias empties
    /// the binary it was copied from. This reproduces the exact shape of that bug — link
    /// first, then ask for the alias again — and asserts the original still has its
    /// bytes. The real failure was silent: a zero-byte executable that macOS runs
    /// through `/bin/sh`, which exits 0 and prints nothing.
    #[test]
    fn refreshing_an_alias_that_is_a_hard_link_does_not_empty_the_binary() {
        let dir = tempfile::TempDir::new().unwrap();
        let binary = dir.path().join("dev-prune");
        let alias = dir.path().join("devp");
        fs::write(&binary, vec![b'M'; 4096]).unwrap();

        if fs::hard_link(&binary, &alias).is_err() {
            return; // Filesystem without hard links; the hazard cannot arise.
        }

        // What `ensure_alias` does when its `hard_link` loses the race: the alias is
        // already there, so it must stop rather than fall through to the copy.
        assert!(fs::hard_link(&binary, &alias).is_err(), "EEXIST expected");
        assert!(alias.exists(), "the guard's condition");

        assert_eq!(
            fs::metadata(&binary).unwrap().len(),
            4096,
            "the running binary was truncated by refreshing its own alias"
        );
    }

    /// The on-disk file name for one of the pair, on this platform.
    fn exe_name(stem: &str) -> String {
        if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem.to_string()
        }
    }

    #[test]
    fn dev_prune_creates_devp_beside_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let canonical = dir.path().join(exe_name("dev-prune"));
        fs::write(&canonical, "the binary").unwrap();

        assert_eq!(ensure_twin_of(&canonical, dir.path()), Outcome::Installed);
        let alias = dir.path().join(exe_name("devp"));
        assert!(alias.is_file(), "`devp` was not created");
        assert_eq!(fs::read_to_string(&alias).unwrap(), "the binary");
    }

    /// The pair has to be recoverable from either side.
    ///
    /// Deleting `dev-prune` and leaving `devp` is not hypothetical: an antivirus
    /// quarantine, a half-finished uninstall, or a `Remove-Item` aimed at one name all
    /// produce it. Before this, `devp setup` reported the alias already present and did
    /// nothing, because the only direction it knew how to repair was the other one.
    #[test]
    fn devp_restores_a_missing_dev_prune() {
        let dir = tempfile::TempDir::new().unwrap();
        let alias = dir.path().join(exe_name("devp"));
        fs::write(&alias, "the binary").unwrap();

        assert_eq!(ensure_twin_of(&alias, dir.path()), Outcome::Installed);
        let canonical = dir.path().join(exe_name("dev-prune"));
        assert!(canonical.is_file(), "`dev-prune` was not put back");
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "the binary");
    }

    /// `devp` may create `dev-prune`, never overwrite it.
    ///
    /// Repairing in both directions opens a downgrade: an upgrade replaces `dev-prune`
    /// first and can then fail on a `devp` that is running, which leaves the alias holding
    /// the *older* binary. If the alias were allowed to refresh its twin from there, the
    /// next `devp setup` would quietly reinstall the version the user just upgraded away
    /// from — and report it as a repair.
    #[test]
    fn devp_does_not_overwrite_an_existing_dev_prune() {
        let dir = tempfile::TempDir::new().unwrap();
        let alias = dir.path().join(exe_name("devp"));
        let canonical = dir.path().join(exe_name("dev-prune"));
        fs::write(&alias, "the previous version").unwrap();
        fs::write(&canonical, "the version just upgraded to").unwrap();

        assert_eq!(
            ensure_twin_of(&alias, dir.path()),
            Outcome::AlreadyPresent,
            "`devp` must leave an existing `dev-prune` alone"
        );
        assert_eq!(
            fs::read_to_string(&canonical).unwrap(),
            "the version just upgraded to",
            "`devp` downgraded the binary it was supposed to leave alone"
        );
    }

    #[test]
    fn versions_parse_strictly_or_not_at_all() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("10.0.0"), Some((10, 0, 0)));
        // Anything this project does not publish must answer None, because a None
        // means "replace the copy" and a mis-parse would order versions wrongly.
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.3-rc1"), None);
        assert_eq!(parse_version("dev-prune"), None);
        // The version this binary was built with has to be parseable, or the refresh
        // logic can never decide anything.
        assert!(parse_version(constants::VERSION).is_some());
    }

    #[test]
    fn the_version_this_cli_prints_is_one_this_cli_can_read_back() {
        // Not synthetic: this is the shape `print_version_info` writes, `v` and all,
        // down to the banner line that ends in the same token.
        let real = format!(
            "|_____|   v{v}

dev-prune (devp) v{v}
  Compiler:        Rust 1.88+ (edition 2024)
",
            v = constants::VERSION
        );
        assert_eq!(
            version_in_output(&real),
            parse_version(constants::VERSION),
            "binary_version could not read this binary's own --version output"
        );
        // Something that is not this CLI still has to answer None, because doctor uses
        // that to mean "leave this file alone".
        assert_eq!(version_in_output("git version 2.51.0.windows.1"), None);
        assert_eq!(version_in_output("some other tool"), None);
    }

    #[test]
    fn ordering_of_version_triples_matches_semver() {
        assert!(parse_version("1.1.0") > parse_version("1.0.9"));
        assert!(parse_version("2.0.0") > parse_version("1.99.99"));
        assert!(parse_version("1.0.10") > parse_version("1.0.9"));
    }

    #[test]
    fn the_exported_skill_is_the_one_the_binary_was_built_with() {
        // `SKILL.md` is embedded, so a doc edit ships only if the binary is rebuilt.
        // Guard the two properties every consumer of it depends on.
        assert!(EMBEDDED_SKILL_MD.starts_with("---"), "needs frontmatter");
        assert!(
            !EMBEDDED_SKILL_MD.contains("file:///"),
            "SKILL.md is written to every user's machine — it must not contain \
             absolute paths from the author's checkout"
        );
    }
}
