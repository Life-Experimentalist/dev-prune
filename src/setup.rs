// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Copyright 2026 VKrishna04
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Idempotent installation of dev-prune's integrations.
//!
//! dev-prune is only really installed once the parts that let it work without being
//! thought about are in place: the `devp` alias, the exported `SKILL.md` that AI
//! assistants read, the Git hooks that keep the registry current, and the OS scheduler
//! that runs the passes. Each one here is created **only when it is missing**, which is
//! what makes it safe to run on every install, reinstall and upgrade — and it does run
//! on each of those, through the version stamp written at the end of a completed pass.
//!
//! Nothing in here is fatal. A machine without `git`, a `core.hooksPath` that belongs to
//! husky, a locked-down scheduler: each is reported and stepped over, because none of
//! them should stop `devp init` from registering repositories.

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

/// Environment variable that suppresses the automatic pass entirely.
///
/// For images, CI and anyone who wants the binary and nothing else. `devp setup` still
/// works when it is set — this only governs the unattended pass.
pub const ENV_NO_AUTO_SETUP: &str = "DEV_PRUNE_NO_AUTO_SETUP";

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
fn managed_exe_path() -> Result<PathBuf> {
    let name = if cfg!(windows) {
        "dev-prune.exe"
    } else {
        "dev-prune"
    };
    Ok(Registry::config_dir()?.join("bin").join(name))
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
    if managed == current || managed.is_file() {
        return managed;
    }

    // Only ever clone something that is actually this CLI. `current_exe()` under `cargo
    // test` is the test harness, and copying that into the config directory would be both
    // wrong and slow.
    let is_cli = current
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem == "dev-prune" || stem == "devp");
    if !is_cli {
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

    if fs::copy(&current, &managed).is_ok() {
        managed
    } else {
        current
    }
}

/// Create the `devp` alias next to the real binary, and keep it current.
///
/// A stale alias is worse than a missing one: it silently runs the previous version
/// after an upgrade that replaced only `dev-prune`. So the alias is replaced whenever it
/// no longer matches the binary that is running.
pub fn ensure_alias() -> Outcome {
    let Ok(current_exe) = std::env::current_exe() else {
        return Outcome::Failed("could not locate the running executable".to_string());
    };
    let Some(parent_dir) = current_exe.parent() else {
        return Outcome::Failed("the running executable has no parent directory".to_string());
    };

    let alias_name = if cfg!(windows) { "devp.exe" } else { "devp" };
    let alias_exe = parent_dir.join(alias_name);

    // Invoked *as* `devp`: the alias is the binary, there is nothing to link.
    if alias_exe == current_exe {
        return Outcome::AlreadyPresent;
    }

    if alias_exe.exists() {
        if same_contents(&alias_exe, &current_exe) {
            return Outcome::AlreadyPresent;
        }
        // Replacing a running executable fails on Windows; that is fine, the alias is
        // simply refreshed by the next invocation that is not itself `devp`.
        if fs::remove_file(&alias_exe).is_err() {
            return Outcome::Skipped(format!(
                "`{alias_name}` is in use and could not be refreshed — re-run `devp setup` \
                 from a terminal that is not running it"
            ));
        }
    }

    if fs::hard_link(&current_exe, &alias_exe).is_ok() {
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
    if alias_exe.exists() {
        return Outcome::AlreadyPresent;
    }

    if fs::copy(&current_exe, &alias_exe).is_ok() {
        Outcome::Installed
    } else {
        Outcome::Failed(format!(
            "could not create `{}`",
            output::clean_path(&alias_exe)
        ))
    }
}

/// Cheap sameness test for two executables: same size and same modification time.
///
/// A hard link makes both true by construction, so the common case answers correctly
/// without hashing megabytes on every invocation.
fn same_contents(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    ma.len() == mb.len() && ma.modified().ok() == mb.modified().ok()
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

/// Install the OS scheduler if it is not already registered.
pub fn ensure_daemon(interval_days: u64) -> Outcome {
    match daemon::daemon_status() {
        Ok(daemon::DaemonStatus::Installed) => Outcome::AlreadyPresent,
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
    std::env::var_os(ENV_NO_AUTO_SETUP).is_none()
        && registry.settings.auto_setup
        && unattended_environment().is_none()
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

    report.push("devp alias", ensure_alias());
    report.push("SKILL.md", ensure_skill_file());
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

/// Put the defaults in front of the user, once, on a fresh install.
///
/// Separate from the integration stamp on purpose. The integrations are re-checked after
/// every upgrade; the settings are not — being asked to reconfirm `idle_days` on each new
/// version would be a nuisance, and the marker only disappears when the config directory
/// does.
///
/// Every condition here is a way of asking "is there a person reading this?", because the
/// alternative to asking is a prompt written into a log nobody will read, on a run that
/// then blocks forever waiting for an answer.
fn first_run_config_review() {
    if !crate::commands::config::config_review_is_due() {
        return;
    }

    use std::io::IsTerminal;
    if unattended_environment().is_some()
        || !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
    {
        crate::commands::config::skip_config_review();
        return;
    }

    // Any error here is the wizard's own reporting; the command the user actually typed
    // still runs. A failed walkthrough must not become a failed `devp status`.
    if let Err(e) = crate::commands::config::run_wizard() {
        output::print_warning(&format!("Could not run the first-run setup ({e:#})."));
        crate::commands::config::skip_config_review();
    }
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
