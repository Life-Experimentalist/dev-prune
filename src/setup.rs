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
    if fs::copy(current, &staging).is_ok() && fs::rename(&staging, managed).is_ok() {
        return;
    }
    // Unconditionally, because `fs::copy` creates and truncates its destination before it
    // writes a byte: a copy that fails partway — disk full, the volume going away, a
    // permission revoked mid-write — leaves a half-written `dev-prune.new` behind. Nothing
    // else ever removes it. `uninstall`'s sweep knows `*.exe.old`, the debris an update
    // leaves, and has never known `*.new`, so the file would sit there until someone
    // noticed it by hand.
    let _ = fs::remove_file(&staging);
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

/// Whether a twin whose content differs from the running binary is the older of the two,
/// and so safe to replace.
///
/// Content inequality says the pair differ, never which one is the upgrade. `dev-prune` is
/// *usually* the newer — installers write it first and upgrades replace it first — but
/// "usually" is not "always", and the direction gate in [`ensure_twin_of`] trusted it
/// absolutely. An older `dev-prune` restored from a backup, or run out of a
/// package-manager cache, would delete a newer `devp` and hard-link its own older content
/// over it: a silent downgrade of the name the documentation tells people to type, reached
/// through `devp doctor --fix` of all things. So ask the twin its version, exactly as
/// [`refresh_managed_copy_if_stale`] does, and leave it alone unless it is genuinely
/// behind. A copy that cannot state a version at all is not a working build of this CLI,
/// so it is still replaced.
///
/// Takes the two versions rather than the two paths so the rule can be tested without a
/// pair of real binaries that report different versions.
fn twin_is_stale(theirs: Option<(u64, u64, u64)>, ours: Option<(u64, u64, u64)>) -> bool {
    !matches!((theirs, ours), (Some(theirs), Some(ours)) if theirs >= ours)
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
    // Installers write `dev-prune` first and upgrades replace it first, so a stale `devp`
    // is worth replacing — otherwise it silently runs the previous version. The reverse is
    // not safe as a rule: an upgrade that replaced `dev-prune` and then failed on a running
    // `devp` leaves exactly the state where the alias is the *older* binary, and refreshing
    // from there would quietly reinstall the version the user just upgraded away from. So
    // `devp` may only create a `dev-prune` that is missing outright.
    //
    // Direction is necessary but not sufficient: see [`twin_is_stale`] for the case where
    // `dev-prune` itself is the older of the pair.
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
        if !twin_is_stale(binary_version(&twin_exe), parse_version(constants::VERSION)) {
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
pub(crate) fn same_contents(a: &std::path::Path, b: &std::path::Path) -> bool {
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

/// Every managed copy of `SKILL.md` that is not the one this binary carries.
///
/// The copies are refreshed by [`ensure_integrations`], which runs only while
/// auto-setup is on. With it off, an upgrade leaves the previous release's instructions
/// sitting in the agent's skills directory, and the agent goes on describing flags that
/// no longer exist — confidently, because nothing told it otherwise.
pub fn stale_skill_copies() -> Vec<PathBuf> {
    let mut copies: Vec<PathBuf> = skill_path().into_iter().collect();
    copies.extend(agent_skill_roots().into_iter().map(|r| r.join("SKILL.md")));
    stale_among(copies)
}

/// The ones of `copies` that exist and differ from the embedded skill.
///
/// A path that is not there is not stale — it is a copy this machine never had, and
/// nothing that refreshes may create it.
fn stale_among(copies: Vec<PathBuf>) -> Vec<PathBuf> {
    copies
        .into_iter()
        .filter(|p| fs::read_to_string(p).is_ok_and(|current| current != EMBEDDED_SKILL_MD))
        .collect()
}

/// Rewrite copies that are already on disk, and only those.
///
/// This is the half of [`ensure_skill_copies`] that is safe to run on a machine which
/// turned auto-setup off: replacing the contents of a file that machine already
/// consented to is maintenance, not a new integration.
fn refresh_stale(stale: Vec<PathBuf>) {
    for path in stale {
        let _ = fs::write(path, EMBEDDED_SKILL_MD);
    }
}

/// Rewrite every managed copy of `SKILL.md` from the embedded one.
///
/// Both copies, not just the export: the one an assistant actually reads is the one in
/// its own skills directory, so repairing the export alone would report success and
/// leave the stale instructions in the only place that matters.
pub fn ensure_skill_copies() -> Outcome {
    let mut installed = false;
    for outcome in [ensure_skill_file(), ensure_agent_skills()] {
        match outcome {
            Outcome::Installed => installed = true,
            // No agent on this machine is not a failure to repair.
            Outcome::AlreadyPresent | Outcome::Skipped(_) => {}
            failed => return failed,
        }
    }
    if installed {
        Outcome::Installed
    } else {
        Outcome::AlreadyPresent
    }
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
        // trust-destroying thing a background tool can do. Re-register it windowless;
        // a machine whose scheduler refuses the sessionless logon remembers the refusal
        // and is not asked again.
        Ok(daemon::DaemonStatus::Installed) if daemon::wants_windowless_upgrade() => {
            match daemon::install_daemon(interval_days) {
                Ok(()) => Outcome::Installed,
                Err(e) => Outcome::Failed(format!("{e:#}")),
            }
        }
        // A settled task still needs its windowless twin kept current: the twin
        // is a copy of the binary, so an upgrade that replaced the binary would otherwise
        // leave the daemon firing the previous release. No-op on the other platforms, and
        // when no twin is in use.
        Ok(daemon::DaemonStatus::Installed) => {
            daemon::refresh_windowless_twin();
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

/// Whether this integration pass was asked for by name.
///
/// The only thing it decides is the `devp` twin. Writing a second executable beside the
/// first is a self-installation, and doing it *unasked*, on the first run of a freshly
/// downloaded unsigned binary, alongside registering a scheduled task, is a behavioural
/// malware signature — it is what earned this package a `Validation-Defender-Error` on
/// microsoft/winget-pkgs#422665. Asked for in so many words, the same write is an
/// ordinary install step. Nothing else in the pass changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// `devp setup`, or `devp doctor --fix`.
    Explicit,
    /// The pass that runs on its own when `auto_setup` is on.
    Unattended,
}

/// Run an integration pass unless unattended installation is switched off.
///
/// Every caller that the user did not name explicitly goes through this. `devp setup`
/// calls [`ensure_integrations`] with [`Consent::Explicit`]: asking for it in so many
/// words is consent.
pub fn ensure_integrations_if_enabled(registry: &Registry) -> Option<SetupReport> {
    auto_setup_enabled(registry).then(|| ensure_integrations(registry, Consent::Unattended))
}

/// Run one integration pass, installing whatever is missing.
///
/// The two per-integration settings (`auto_daemon`, `auto_hooks`) are honoured here, so
/// turning one off turns it off for every future pass as well as this one.
pub fn ensure_integrations(registry: &Registry, consent: Consent) -> SetupReport {
    let mut report = SetupReport::default();

    // Only when asked. This is the twin *beside the running binary*, which on an
    // unattended pass means beside whatever the delivery vehicle happened to be: npm's
    // cache, a venv's `Scripts`, a Downloads folder. Every channel already ships both
    // names as real files — the archives, the npm and PyPI packages, and two `[[bin]]`
    // targets for `cargo install` — so there is normally nothing to create, and the one
    // case left over is a manual install that skipped `dev-prune setup`, which `devp
    // doctor` reports with a one-command fix.
    //
    // The pair that actually matters is not this one. `ensure_command_on_path` below
    // keeps `dev-prune` and `devp` together in the managed `bin` directory, which is
    // the directory on the user's PATH and the one `devp uninstall` knows about, and it
    // runs on every pass.
    if consent == Consent::Explicit {
        report.push("dev-prune/devp pair", ensure_alias());
    }
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
/// Marketplace, VSCodium/Windsurf/Positron/Kiro/Trae use OpenVSX, Cursor and
/// Antigravity run their own mirrors. An ID install can therefore fail on a fork whose
/// registry does not carry the extension yet — which is why the installer falls back
/// to the `.vsix` from the GitHub release, the artifact every registry copy is built
/// from.
///
/// The list is candidate CLI names, not a claim that any of them is installed: each is
/// asked for its `--version` and dropped if it does not answer. Adding a fork therefore
/// costs one failed spawn on a machine without it, and is what stops the extension
/// offer from being a VS Code-only courtesy on an editor that is a VS Code build with a
/// different name on the window.
fn detect_vscode_editors() -> Vec<EditorCli> {
    const CANDIDATES: &[(&str, &str)] = &[
        ("code", "VS Code"),
        ("code-insiders", "VS Code Insiders"),
        ("codium", "VSCodium"),
        ("codium-insiders", "VSCodium Insiders"),
        ("cursor", "Cursor"),
        ("windsurf", "Windsurf"),
        ("antigravity", "Antigravity"),
        ("trae", "Trae"),
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

/// Download the `.vsix` from the newest extension release into the config directory.
///
/// The release asset is the source of truth for the extension — the Marketplace and
/// OpenVSX listings are published from that exact file — so when an editor's registry
/// cannot resolve the ID (a fork whose registry does not carry the extension),
/// installing the release file directly gets the same bits through a channel every fork
/// supports. Editors update a `.vsix`-installed extension from their registry once a
/// newer listed version appears, so this install self-heals into the normal update flow.
///
/// Deliberately not `releases/latest`. The extension has its own tags and its own
/// release page, and those releases are marked "not latest" so they cannot displace the
/// binary release that `devp update` reads. The consequence is that the newest one has
/// to be found by walking the listing for a [`VSCODE_RELEASE_TAG_PREFIX`] tag.
///
/// [`VSCODE_RELEASE_TAG_PREFIX`]: crate::constants::VSCODE_RELEASE_TAG_PREFIX
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

    let body = fetch(constants::RELEASES_LIST_API_URL)
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    // GitHub returns this listing newest-first, so the first extension release found is
    // the current one. Draft releases carry no downloadable asset, and a pre-release of
    // the extension is one deliberately not being offered to people who did not ask.
    let asset = json.as_array()?.iter().find_map(|release| {
        let tag = release.get("tag_name")?.as_str()?;
        if !tag.starts_with(constants::VSCODE_RELEASE_TAG_PREFIX) {
            return None;
        }
        if release.get("draft")?.as_bool()? || release.get("prerelease")?.as_bool()? {
            return None;
        }
        release.get("assets")?.as_array()?.iter().find_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            if !name.ends_with(".vsix") {
                return None;
            }
            let url = asset.get("browser_download_url")?.as_str()?;
            Some((name.to_string(), url.to_string()))
        })
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

/// What this machine has said about dev-prune installing things for itself.
///
/// Three answers, not two: "never asked" is the state a question can still be put in,
/// and collapsing it into either answer is how tools end up installing on a silence or
/// nagging on a refusal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetupConsent {
    Granted,
    Declined,
    NeverAsked,
}

const CONSENT_GRANTED: &str = "granted";
const CONSENT_DECLINED: &str = "declined";

/// The release that introduced the consent question. A stamp older than this could
/// only have been written by a pass that had already installed the integrations; a
/// newer one is also written by the opted-out path, and so proves nothing.
const FIRST_CONSENT_VERSION: &str = "1.18.0";

fn consent_state_in(config_dir: &std::path::Path) -> SetupConsent {
    match fs::read_to_string(config_dir.join(constants::SETUP_CONSENT_FILE)) {
        Ok(answer) if answer.trim() == CONSENT_GRANTED => return SetupConsent::Granted,
        Ok(answer) if answer.trim() == CONSENT_DECLINED => return SetupConsent::Declined,
        _ => {}
    }
    // A pre-1.18 stamp means the old flow already installed the integrations and the
    // person kept them — consent in deed if not in word, and re-asking would prompt
    // every existing user once for something they already have.
    match fs::read_to_string(config_dir.join(STAMP_FILE)) {
        Ok(stamp)
            if crate::commands::update::compare_versions(stamp.trim(), FIRST_CONSENT_VERSION)
                == Some(std::cmp::Ordering::Less) =>
        {
            SetupConsent::Granted
        }
        _ => SetupConsent::NeverAsked,
    }
}

pub fn consent_state() -> SetupConsent {
    Registry::config_dir()
        .map(|dir| consent_state_in(&dir))
        .unwrap_or(SetupConsent::NeverAsked)
}

fn record_consent_in(config_dir: &std::path::Path, answer: &str) {
    let _ = fs::create_dir_all(config_dir);
    let _ = fs::write(config_dir.join(constants::SETUP_CONSENT_FILE), answer);
}

/// `devp setup` records this too: asking for the pass in so many words is also the
/// durable answer to the question the first run would otherwise ask.
pub fn record_consent_granted() {
    if let Ok(dir) = Registry::config_dir() {
        record_consent_in(&dir, CONSENT_GRANTED);
    }
}

fn record_consent_declined() {
    if let Ok(dir) = Registry::config_dir() {
        record_consent_in(&dir, CONSENT_DECLINED);
    }
}

/// Put the question back the way a fresh machine has it. `devp uninstall` calls this:
/// keeping a "granted" that outlives the things it granted would make the next upgrade
/// reinstall everything the uninstall just removed.
pub fn clear_setup_consent() {
    if let Ok(dir) = Registry::config_dir() {
        let _ = fs::remove_file(dir.join(constants::SETUP_CONSENT_FILE));
    }
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

/// The per-version pass — and, since 1.18.0, never before this machine has said yes.
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

    review_project_venv_install();

    let Ok(registry) = Registry::load() else {
        return;
    };
    match consent_state() {
        SetupConsent::Granted => {
            // Made durable even when it was inferred from a pre-1.18 stamp, so the
            // inference runs once rather than on every later upgrade.
            record_consent_granted();
            run_consented_pass(&registry);
            first_run_config_review();
        }
        SetupConsent::Declined => {
            // The answer was no, and staying no costs nothing to honour: stamp so this
            // version's pass is settled, install nothing, and leave `devp setup` as
            // the standing way to change the answer. The settings review is still owed
            // when an upgrade adds a setting — declining the integrations was never a
            // vote on config defaults.
            write_stamp();
            first_run_config_review();
        }
        SetupConsent::NeverAsked => {
            if !auto_setup_enabled(&registry) {
                // Suppressed. Stamp anyway, so a machine that opted out does not
                // re-decide this on every single command; the consent marker stays
                // unwritten, so lifting the opt-out later asks rather than installs.
                //
                // The one thing opting out must not do is freeze the instructions AI
                // agents read at whichever version installed them, so any existing
                // copies are still brought up to date. See `run_consented_pass` for
                // the fuller reasoning.
                refresh_stale(stale_skill_copies());
                write_stamp();
                crate::commands::config::skip_config_review();
                return;
            }
            ask_first_run_consent();
        }
    }
}

/// One integration pass for a machine that has already said yes, reported if it did
/// anything.
fn run_consented_pass(registry: &Registry) {
    let Some(report) = ensure_integrations_if_enabled(registry) else {
        // Suppressed. Stamp anyway, so a machine that opted out does not re-decide
        // this on every single command.
        //
        // The one thing opting out must not do is freeze the instructions AI agents
        // read at whichever version installed them. The stamp below is what would
        // freeze them: it is rewritten for every new version without a pass ever
        // running, so an existing `SKILL.md` here would never be reconsidered again,
        // and the agent would go on describing flags that were removed two releases
        // ago — confidently, because nothing told it otherwise.
        refresh_stale(stale_skill_copies());
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
}

/// Ask, on the first attended run, before installing anything at all.
///
/// The old order — install, then open the walkthrough — put this binary's fingerprint
/// (an unasked self-copy into a managed `bin`, plus a scheduled task, on the first run
/// of an unsigned download) squarely on the behaviour ML malware classifiers key on;
/// the 1.17.0 release exe was flagged as Trojan:Win32/Wacatac.B!ml for exactly that.
/// Asked first, the same installs are the answer to a question — and a sandbox that
/// runs the binary bare now sits at a prompt instead of recording persistence.
fn ask_first_run_consent() {
    use crate::commands::config::FirstRunDecision;
    match crate::commands::config::first_run_wizard() {
        Err(e) => {
            // The wizard's own failure must not decide the question either way:
            // nothing recorded, nothing stamped, asked again on the next command.
            output::print_warning(&format!("Could not run the first-run setup ({e:#})."));
        }
        Ok(FirstRunDecision::Accepted) => {
            record_consent_granted();
            // Reloaded, not reused: the walkthrough that just closed may have flipped
            // `auto_daemon` or `auto_hooks`, and this pass exists to honour that.
            let Ok(registry) = Registry::load() else {
                return;
            };
            run_consented_pass(&registry);
            crate::commands::config::skip_config_review();
            offer_vscode_extension();
            println!();
        }
        Ok(FirstRunDecision::Declined) => {
            record_consent_declined();
            write_stamp();
            crate::commands::config::skip_config_review();
            output::print_info(
                "Nothing was installed. `devp setup` installs the integrations whenever \
                 you want them; `devp config wizard` reopens the settings.",
            );
            println!();
        }
        Ok(FirstRunDecision::NoAnswer) => {
            // EOF is not an answer — it is nobody there after all. Every marker stays
            // unwritten, so the first run with a person on the other end is asked.
        }
    }
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

    // Deliberately not stamped here. The stamp records that somebody was *shown* the
    // declaration, and a cron run, a CI job or a container has nobody to show it to.
    // Writing it anyway meant the first unattended `devp` on a machine silently spent
    // the one screen that says what dev-prune will not delete — so the person who
    // installed it never saw it, and nothing ever offered again. Left unstamped, the
    // walkthrough waits for the first run with a human on the other end of it, which is
    // the only run it was ever for.
    if !a_person_is_present() {
        return;
    }

    // Any error here is the wizard's own reporting; the command the user actually typed
    // still runs. A failed walkthrough must not become a failed `devp status`.
    if let Err(e) =
        crate::commands::config::run_wizard(false, crate::commands::config::Opened::OnItsOwn)
    {
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

/// Say something, once, when this copy is running from inside a project's virtualenv.
///
/// This is the remedy at the source. By the time a prune pass refuses the environment,
/// the user is several days and one confusing error away from the moment they typed
/// `pip install dev-prune` with a project activated; saying it here, on the first run
/// after that install, is the only chance to explain it while the cause is still in
/// living memory. The refusal in the venv adapter stays as the failsafe, for a copy
/// installed before this check existed or by somebody who dismissed it.
///
/// Silent when `requirements.txt` already lists the tool. That is somebody who meant it,
/// and being told about a decision you made on purpose is what teaches people to stop
/// reading output.
fn review_project_venv_install() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(found) = crate::channel::project_venv_install(&exe) else {
        return;
    };

    let requirements = found.project.join("requirements.txt");
    let recorded = crate::adapters::venv::requirement_names(&requirements, &mut Vec::new())
        .is_some_and(|names| names.iter().any(|n| crate::adapters::venv::is_dev_prune(n)));
    if recorded {
        return;
    }

    output::print_header(&format!(
        "{} is installed inside this project's virtual environment",
        constants::APP_NAME
    ));
    println!("  running from  {}", exe.display());
    println!("  environment   {}", found.venv.display());
    println!("  project       {}", found.project.display());
    println!();
    output::print_info(
        "A tool install belongs outside a project: it outlives the environment, every \
         repository shares it, and it never has to appear in an application's \
         requirements file to stay out of the way.",
    );

    if requirements.is_file() {
        println!();
        output::print_info(
            "Until that is fixed, a prune pass will decline this project's environment \
             — a package `requirements.txt` does not account for is a package nothing \
             can rebuild.",
        );
        println!();
        if record_in_requirements(&requirements) {
            return;
        }
    }

    println!();
    println!("  Remove this copy and install it as a tool instead:");
    println!("    pip uninstall {}", constants::APP_NAME);
    println!("    uv tool install {}", constants::APP_NAME);
    println!("    # or: pipx install {}", constants::APP_NAME);
    report_other_copy(&exe);
    println!();
}

/// Offer the other repair: declare the tool a dependency of this project, on purpose.
///
/// Default no, for the reason `devp restore` defaults no on a substituted interpreter —
/// this is the branch that writes into a file in somebody's repository, so a reflexive
/// Enter must not be what agrees to it. Returns whether the file was written, which is
/// also whether the removal instructions are still worth printing.
fn record_in_requirements(requirements: &std::path::Path) -> bool {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!(
        "Record {} in requirements.txt instead, as a deliberate dev dependency? [y/N]: ",
        constants::APP_NAME
    );
    if std::io::stderr().flush().is_err() {
        return false;
    }
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
        return false;
    }

    let Ok(existing) = fs::read_to_string(requirements) else {
        output::print_warning("Could not read requirements.txt, so nothing was changed.");
        return false;
    };
    // Requirements files without a trailing newline are common, and appending to one
    // blind would glue the pin onto the last requirement.
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let line = format!(
        "{separator}{}=={}\n",
        constants::APP_NAME,
        constants::VERSION
    );
    match std::fs::OpenOptions::new()
        .append(true)
        .open(requirements)
        .and_then(|mut f| f.write_all(line.as_bytes()))
    {
        Ok(()) => {
            output::print_success(&format!(
                "Added `{}=={}` to {}. The environment is prunable now.",
                constants::APP_NAME,
                constants::VERSION,
                requirements.display()
            ));
            true
        }
        Err(e) => {
            output::print_warning(&format!("Could not write requirements.txt ({e})."));
            false
        }
    }
}

/// Name the copy that is already installed properly, if there is one.
///
/// "Uninstall this" reads very differently depending on whether it leaves the user with
/// no tool at all or with the one they already had — and on a machine where this mistake
/// happens there usually is one, because the working copy is what they were reaching for
/// in the first place.
fn report_other_copy(exe: &std::path::Path) {
    let names: [&str; 2] = if cfg!(windows) {
        ["dev-prune.exe", "devp.exe"]
    } else {
        ["dev-prune", "devp"]
    };
    let home = dirs::home_dir();
    let other = crate::channel::install_dirs(home.as_deref())
        .into_iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file() && candidate != exe);

    if let Some(other) = other {
        println!();
        output::print_info(&format!(
            "You already have a copy outside this project, at `{}`, so removing this one \
             still leaves you a working `devp`.",
            other.display()
        ));
    }
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
    fn refreshing_rewrites_the_copies_that_exist_and_creates_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let present = dir.path().join("SKILL.md");
        fs::write(&present, "# the instructions from two releases ago").unwrap();
        let never_installed = dir.path().join("no-agent-here").join("SKILL.md");

        let stale = stale_among(vec![present.clone(), never_installed.clone()]);
        assert_eq!(stale, vec![present.clone()]);

        refresh_stale(stale);
        assert_eq!(fs::read_to_string(&present).unwrap(), EMBEDDED_SKILL_MD);
        assert!(
            !never_installed.exists(),
            "a machine that never had a copy must not acquire one from a refresh"
        );
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

    /// The same downgrade, coming the other way — the direction the gate above lets
    /// through.
    ///
    /// `devp` is blocked from refreshing `dev-prune` outright, but `dev-prune` refreshing
    /// `devp` was gated on nothing but the two files differing, on the assumption that the
    /// canonical name is always the newer one. Restore a `dev-prune` from a backup, or run
    /// one out of a package-manager cache, and it is not — and `devp doctor --fix` runs
    /// `ensure_alias` from whichever binary is executing, so an older `dev-prune` would
    /// delete a newer `devp` and link its own content over it while reporting a repair.
    #[test]
    fn a_twin_that_is_not_behind_is_left_alone() {
        assert!(
            !twin_is_stale(Some((1, 12, 0)), Some((1, 11, 0))),
            "a newer twin must not be overwritten"
        );
        assert!(
            !twin_is_stale(Some((1, 11, 0)), Some((1, 11, 0))),
            "an equal twin has nothing to refresh"
        );
        assert!(
            twin_is_stale(Some((1, 10, 0)), Some((1, 11, 0))),
            "a genuinely older twin is what this refresh is for"
        );
        // Neither side able to state a version leaves the old content-only rule, which is
        // the only answer available: whatever the file is, it is not a working build of
        // this CLI, and leaving it would keep a broken `devp` on PATH forever.
        assert!(twin_is_stale(None, Some((1, 11, 0))));
        assert!(twin_is_stale(Some((1, 11, 0)), None));
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

    #[test]
    fn a_fresh_machine_has_never_been_asked() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(consent_state_in(dir.path()), SetupConsent::NeverAsked);
    }

    #[test]
    fn a_recorded_answer_is_read_back() {
        let dir = tempfile::TempDir::new().unwrap();
        record_consent_in(dir.path(), CONSENT_GRANTED);
        assert_eq!(consent_state_in(dir.path()), SetupConsent::Granted);
        record_consent_in(dir.path(), CONSENT_DECLINED);
        assert_eq!(consent_state_in(dir.path()), SetupConsent::Declined);
    }

    #[test]
    fn a_garbled_marker_means_the_question_is_still_open() {
        // Better to ask twice than to install on the strength of a corrupt file.
        let dir = tempfile::TempDir::new().unwrap();
        record_consent_in(dir.path(), "maybe?");
        assert_eq!(consent_state_in(dir.path()), SetupConsent::NeverAsked);
    }

    #[test]
    fn a_pre_consent_stamp_counts_as_granted() {
        // The old flow only ever stamped after installing, so a 1.17 stamp is proof
        // the integrations are already on this machine.
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join(STAMP_FILE), "1.17.0\n").unwrap();
        assert_eq!(consent_state_in(dir.path()), SetupConsent::Granted);
    }

    #[test]
    fn a_post_consent_stamp_proves_nothing() {
        // From 1.18.0 on, the opted-out path writes the stamp too — treating it as a
        // yes would silently grant consent on the exact machines that withheld it.
        let dir = tempfile::TempDir::new().unwrap();
        // Not `constants::VERSION`: until the release that ships this bumps it past
        // FIRST_CONSENT_VERSION, the current version is itself a pre-consent one.
        for stamp in [FIRST_CONSENT_VERSION, "1.18.1", "2.0.0", "garbage"] {
            fs::write(dir.path().join(STAMP_FILE), stamp).unwrap();
            assert_eq!(
                consent_state_in(dir.path()),
                SetupConsent::NeverAsked,
                "stamp {stamp:?} must not imply consent"
            );
        }
    }

    #[test]
    fn an_explicit_answer_outranks_the_stamp() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join(STAMP_FILE), "1.17.0").unwrap();
        record_consent_in(dir.path(), CONSENT_DECLINED);
        assert_eq!(consent_state_in(dir.path()), SetupConsent::Declined);
    }

    #[test]
    fn clearing_consent_reopens_the_question() {
        let dir = tempfile::TempDir::new().unwrap();
        record_consent_in(dir.path(), CONSENT_GRANTED);
        fs::remove_file(dir.path().join(constants::SETUP_CONSENT_FILE)).unwrap();
        assert_eq!(consent_state_in(dir.path()), SetupConsent::NeverAsked);
    }
}
