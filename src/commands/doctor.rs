// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune doctor`.
//
// One command that answers "why is this not doing what I expect". Without a path it
// checks the installation — binary, alias, PATH, config, integrations, package managers,
// registry, release check. With a path it checks that one repository and ends by naming
// the reason it would or would not be pruned right now.
//
// The plain doctor is read-only. A diagnostic that repairs things as it goes cannot be
// run twice to see whether the first run helped, so diagnosis and treatment are two
// separate invocations: the report names every finding it could repair, and `--fix` is
// the explicit second step that repairs them. `--fix` only mends what is already
// installed but broken — a missing or stale twin binary, hooks or a scheduler
// registered against a binary that no longer exists, a drifted hook chain, a missing
// `SKILL.md` export, registry entries whose repository is gone. It never installs an
// integration for the first time (`devp setup` and the individual commands are the
// opt-in for that), and it never touches an unreadable `registry.json`, because
// guessing at a config it cannot read is exactly what dev-prune refuses to do.
//
// Nothing here runs a package manager either: `enforce_lockfile` invokes `npm`,
// `cargo` and friends, which is minutes of work and, for the opted-in adapters, writes
// to tracked files. The doctor reports what it can see.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::adapters;
use crate::channel::Channel;
use crate::commands::hook::{self, HookState};
use crate::config::{PerRepoConfig, Registry};
use crate::constants;
use crate::daemon;
use crate::engine::{self, BYTES_PER_MIB, SkipReason};
use crate::output;
use crate::scanner::{self, git};
use crate::setup;
use crate::workspace;

/// One repair `--fix` knows how to make. Every variant mends something that is already
/// installed but broken; none of them installs an integration for the first time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repair {
    /// The `dev-prune`/`devp` pair is missing a member, or the alias is stale.
    Twin,
    /// The `SKILL.md` export is missing or out of date.
    SkillFile,
    /// Installed hooks point at a deleted binary, or a chain has drifted.
    Hooks,
    /// The installed scheduler points at a deleted binary.
    Scheduler,
    /// Registered paths that no longer exist on disk.
    UnlinkMissing,
    /// Registered repositories whose `.devprune.json` cannot be parsed.
    RepoConfigs,
}

/// Tally of everything the report flagged, so the verdict is derived from the same
/// lines the user just read rather than recomputed from scratch.
#[derive(Default)]
struct Findings {
    warnings: Vec<String>,
    problems: Vec<String>,
    /// Repairs `--fix` would make, in the order their findings were reported.
    fixes: Vec<Repair>,
    /// Indices into `problems` that one of `fixes` addresses, so the repair verdict can
    /// tell "fixed by the pass that just ran" apart from "still needs a human".
    fixable_problems: Vec<usize>,
    /// The subset of `fixes` whose finding was a problem rather than a warning. A repair
    /// from this list that does not actually land leaves the problem standing, and the
    /// exit code has to say so.
    problem_repairs: Vec<Repair>,
}

impl Findings {
    /// A check that passed.
    fn ok(&mut self, label: &str, detail: &str) {
        println!("  {label:<22} {} {detail}", "✓".green());
    }

    /// Something to be aware of that is not stopping anything working.
    fn warn(&mut self, label: &str, detail: &str) {
        println!("  {label:<22} {} {detail}", "!".yellow());
        self.warnings.push(format!("{label}: {detail}"));
    }

    /// Something that is actually broken.
    fn problem(&mut self, label: &str, detail: &str) {
        println!("  {label:<22} {} {detail}", "✗".red());
        self.problems.push(format!("{label}: {detail}"));
    }

    /// Record that the most recent `warn` is one `--fix` can repair.
    fn fixable(&mut self, repair: Repair) {
        if !self.fixes.contains(&repair) {
            self.fixes.push(repair);
        }
    }

    /// Record that the most recent `problem` is one `--fix` can repair.
    ///
    /// Called immediately after the `problem` it belongs to, so the index bookkeeping
    /// stays next to the finding it describes.
    fn fixable_problem(&mut self, repair: Repair) {
        self.fixable(repair);
        if !self.problem_repairs.contains(&repair) {
            self.problem_repairs.push(repair);
        }
        if let Some(last) = self.problems.len().checked_sub(1)
            && !self.fixable_problems.contains(&last)
        {
            self.fixable_problems.push(last);
        }
    }

    /// A fact with no verdict attached.
    fn note(&self, label: &str, detail: &str) {
        println!("  {label:<22}   {detail}");
    }

    fn section(&self, title: &str) {
        println!();
        println!("{}", title.bold());
    }
}

// `colored` is used through these three, so the trait import stays local to the file.
use colored::Colorize as _;

/// Run the `doctor` command.
///
/// `path` is whatever the user typed, already tilde-expanded. `None` means the global
/// installation; `Some(".")` is an ordinary path like any other. `fix` applies the
/// repairs the installation check found; clap refuses `--fix` alongside a path, because
/// the repository check has nothing it could safely repair.
pub fn run(path: Option<&str>, fix: bool) -> Result<()> {
    match path {
        Some(p) => check_repository(p),
        None => check_installation(fix),
    }
}

/// Print the verdict and pick the exit code.
///
/// Warnings exit `0`. A doctor that fails the build because the scheduler is not
/// installed is a doctor people stop running; only the things that stop dev-prune doing
/// its job are worth a non-zero status.
fn verdict(f: &Findings, all_clear: &str, headline: Option<&str>) -> Result<()> {
    f.section("Verdict");

    if let Some(line) = headline {
        println!("  {line}");
        println!();
    }

    if f.problems.is_empty() && f.warnings.is_empty() {
        output::print_success(all_clear);
        return Ok(());
    }

    for w in &f.warnings {
        println!("  {} {w}", "!".yellow());
    }
    for p in &f.problems {
        println!("  {} {p}", "✗".red());
    }

    if !f.fixes.is_empty() {
        println!();
        println!(
            "  {} of these can be repaired automatically — run `devp doctor --fix`.",
            f.fixes.len()
        );
    }

    println!();
    println!("  Troubleshooting: {}", constants::TROUBLESHOOTING_URL);

    if f.problems.is_empty() {
        println!();
        output::print_info(&format!(
            "{} {} — nothing broken.",
            f.warnings.len(),
            output::plural(f.warnings.len(), "warning", "warnings")
        ));
        return Ok(());
    }

    anyhow::bail!(
        "{} {} found.",
        f.problems.len(),
        output::plural(f.problems.len(), "problem", "problems")
    )
}

// ---------------------------------------------------------------------------
// Global installation
// ---------------------------------------------------------------------------

fn check_installation(fix: bool) -> Result<()> {
    output::print_header("dev-prune doctor");
    let mut f = Findings::default();

    check_binary(&mut f);
    check_install_channel(&mut f);
    check_other_copies(&mut f);
    let registry = check_configuration(&mut f);
    check_integrations(&mut f, registry.as_ref());
    check_package_managers(&mut f, registry.as_ref());
    check_registry_health(&mut f, registry.as_ref());
    check_release_state(&mut f, registry.as_ref());

    if fix && !f.fixes.is_empty() {
        return apply_repairs(&f, registry.as_ref());
    }
    if fix {
        // `--fix` with nothing repairable falls through to the ordinary verdict: what
        // remains (if anything) needs a human, and saying which things is the verdict's
        // whole job.
        return verdict(&f, "Everything checks out — nothing to repair.", None);
    }
    verdict(&f, "Everything checks out.", None)
}

/// Apply every repair the diagnosis recorded, then give the repair verdict.
///
/// Each repair goes through the same `setup::ensure_*` passes the automatic setup uses,
/// so a repair can never do something the setup pass would not — and like that pass,
/// each one re-checks the state itself before touching anything, so a finding that
/// healed between diagnosis and repair reports "already in place" rather than being
/// re-applied.
///
/// `DEV_PRUNE_NO_AUTO_SETUP` disables every self-installation path, and the repairs
/// that write outside the config directory — the twin binary, the git hooks, the OS
/// scheduler — are exactly that, so with the variable set they are skipped and named.
/// Bookkeeping inside dev-prune's own config directory (the `SKILL.md` export, dead
/// registry entries) is not an installation and still runs.
///
/// Exit code: `0` unless a repair failed outright, a *problem*-level finding was left
/// unrepaired, or a problem remains that no repair addresses. A skipped repair whose
/// finding was only a warning (the twin is a running executable) exits `0` like any
/// other warning — the report says what to do, and nothing is more broken than it
/// already was.
fn apply_repairs(f: &Findings, registry: Option<&Registry>) -> Result<()> {
    f.section("Repairs");

    let ok = |label: &str, detail: &str| println!("  {label:<22} {} {detail}", "✓".green());
    let skipped = |label: &str, detail: &str| println!("  {label:<22} {} {detail}", "!".yellow());
    let failed_line = |label: &str, detail: &str| println!("  {label:<22} {} {detail}", "✗".red());

    let chain = registry
        .map(|r| r.settings.auto_hooks_chain)
        .unwrap_or(false);
    let interval = registry
        .map(|r| r.settings.check_interval_days)
        .unwrap_or(constants::DEFAULT_CHECK_INTERVAL_DAYS);
    let installs_off = setup::no_auto_setup_requested();

    let mut repaired = 0usize;
    let mut failures = 0usize;
    let mut attention = 0usize;
    // Problems whose repair was skipped rather than failed. Failures already count
    // toward the exit code; a skipped problem has to as well, because the breakage the
    // diagnosis reported is still there.
    let mut skipped_problems = 0usize;

    for repair in &f.fixes {
        let is_problem = f.problem_repairs.contains(repair);
        let (label, manual) = match repair {
            Repair::Twin => ("Binary pair", "run `dev-prune setup` yourself"),
            Repair::SkillFile => ("SKILL.md", "run `devp skill` yourself"),
            Repair::Hooks => ("Git hooks", "run `devp hook install` yourself"),
            Repair::Scheduler => ("Scheduler", "run `devp daemon install` yourself"),
            Repair::UnlinkMissing => {
                match crate::commands::link::run_unlink_missing() {
                    Ok(()) => repaired += 1,
                    Err(e) => {
                        failed_line("Registry", &format!("{e:#}"));
                        failures += 1;
                    }
                }
                continue;
            }
            Repair::RepoConfigs => {
                match heal_repo_configs() {
                    Ok(healed) => {
                        ok(
                            "Repo configs",
                            &format!(
                                "{healed} unreadable `.devprune.json` {} replaced with defaults — \
                                 the broken originals are kept beside them as \
                                 `.devprune.json.broken`",
                                output::plural(healed, "file", "files")
                            ),
                        );
                        repaired += 1;
                    }
                    Err(e) => {
                        failed_line("Repo configs", &format!("{e:#}"));
                        failures += 1;
                    }
                }
                continue;
            }
        };
        // These three write outside the config directory, which is precisely what the
        // variable exists to forbid.
        if installs_off && matches!(repair, Repair::Twin | Repair::Hooks | Repair::Scheduler) {
            skipped(
                label,
                &format!("{} is set — {manual}", setup::ENV_NO_AUTO_SETUP),
            );
            attention += 1;
            if is_problem {
                skipped_problems += 1;
            }
            continue;
        }
        let outcome = match repair {
            Repair::Twin => setup::ensure_alias(),
            Repair::SkillFile => setup::ensure_skill_file(),
            Repair::Hooks => setup::ensure_hooks(chain),
            Repair::Scheduler => setup::ensure_daemon(interval),
            Repair::UnlinkMissing | Repair::RepoConfigs => unreachable!("handled above"),
        };
        match outcome {
            setup::Outcome::Installed => {
                ok(label, "repaired");
                repaired += 1;
            }
            setup::Outcome::AlreadyPresent => {
                ok(label, "already in place");
                repaired += 1;
            }
            setup::Outcome::Skipped(why) => {
                skipped(label, &why);
                attention += 1;
                if is_problem {
                    skipped_problems += 1;
                }
            }
            setup::Outcome::Failed(why) => {
                failed_line(label, &why);
                failures += 1;
            }
        }
    }

    f.section("Verdict");
    let unfixable: Vec<&String> = f
        .problems
        .iter()
        .enumerate()
        .filter(|(i, _)| !f.fixable_problems.contains(i))
        .map(|(_, p)| p)
        .collect();
    for p in &unfixable {
        println!("  {} {p} (not auto-repairable)", "✗".red());
    }
    if !unfixable.is_empty() {
        println!();
        println!("  Troubleshooting: {}", constants::TROUBLESHOOTING_URL);
    }
    println!();
    output::print_info(&format!(
        "{repaired} repaired, {attention} skipped, {failures} failed. \
         Run `devp doctor` to confirm."
    ));

    let unresolved = failures + skipped_problems + unfixable.len();
    if unresolved > 0 {
        anyhow::bail!(
            "{unresolved} {} could not be repaired.",
            output::plural(unresolved, "finding", "findings")
        );
    }
    Ok(())
}

fn check_binary(f: &mut Findings) {
    f.section("Installation");
    f.note("Version", constants::VERSION);

    // A 32-bit build runs fine on a 64-bit machine, so this is a warning and never a
    // problem — but nothing else on the machine would ever mention it, and the only
    // symptom is a 4 GB address-space ceiling and a slower binary.
    match crate::native_arch_if_emulated() {
        Some(native) => f.warn(
            "Architecture",
            &format!(
                "this is the {} build, but the machine is {native} — reinstall to get the \
                 native one: `devp update`",
                std::env::consts::ARCH
            ),
        ),
        None => f.ok("Architecture", std::env::consts::ARCH),
    }

    let Ok(exe) = std::env::current_exe() else {
        f.warn("Executable", "the running binary's own path is unavailable");
        return;
    };
    f.note("Executable", &output::clean_path(&exe));

    let Some(dir) = exe.parent() else { return };

    // The pair is one binary under two names, and either one can be the one running
    // right now. Looking for `devp` unconditionally made this check vacuous whenever
    // the user typed `devp doctor` — the file being looked for was the file doing the
    // looking, so it always "passed". Look for whichever name is *not* running.
    let running = if exe
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("devp"))
    {
        "devp"
    } else {
        "dev-prune"
    };
    let twin_stem = if running == "devp" {
        "dev-prune"
    } else {
        "devp"
    };
    let twin = dir.join(if cfg!(windows) {
        format!("{twin_stem}.exe")
    } else {
        twin_stem.to_string()
    });
    if !twin.exists() {
        // npm is the one channel that delivers the second name without a second file:
        // it declares both commands in its own `bin` map and writes a launcher for
        // each. The directory being searched here is the platform package, which is
        // not on `PATH` at all, and anything written into it would be discarded by the
        // next `npm install -g dev-prune` — so a missing file here costs nothing and
        // there is nothing to repair.
        if crate::channel::Channel::detect() == crate::channel::Channel::Npm {
            f.ok(twin_stem, "provided by npm as a command of its own");
        } else {
            f.warn(
                twin_stem,
                &format!("not installed next to {running} — run `{running} setup`"),
            );
            // Either name may recreate a twin that is missing outright.
            f.fixable(Repair::Twin);
        }
    } else if same_binary(&exe, &twin) {
        f.ok(twin_stem, &output::clean_path(&twin));
    } else {
        // An upgrade that could not replace a running executable leaves exactly this
        // state, and the stale name silently runs the previous version from then on.
        f.warn(
            twin_stem,
            &format!(
                "{} is not the same binary as {} — one of the pair is stale and \
                 silently runs a different version. `dev-prune setup` refreshes `devp` \
                 from the canonical `dev-prune`.",
                output::clean_path(&twin),
                output::clean_path(&exe)
            ),
        );
        // Only the canonical `dev-prune` may overwrite a differing twin — `devp`
        // refreshing `dev-prune` could reinstall the version an upgrade just replaced.
        // So this is repairable only from the canonical side.
        if running == "dev-prune" {
            f.fixable(Repair::Twin);
        }
    }

    let sep = if cfg!(windows) { ';' } else { ':' };
    let on_path = std::env::var("PATH")
        .unwrap_or_default()
        .split(sep)
        .any(|p| !p.is_empty() && same_dir(Path::new(p), dir));
    if on_path {
        f.ok("PATH", &output::clean_path(dir));
    } else {
        // A warning, not a problem: the binary demonstrably runs — doctor is it
        // running. Off PATH is a convenience gap (portable installs, cargo target
        // dirs), not breakage, and exit 1 here would fail CI on a healthy install.
        f.warn(
            "PATH",
            &format!(
                "{} is not on PATH — `devp` will not resolve in a new shell. \
                 `dev-prune setup` adds it.",
                output::clean_path(dir)
            ),
        );
    }
}

/// Name the package manager that installed this copy, and the commands that upgrade and
/// remove it through that manager.
///
/// Never a warning. Every channel here is a supported one and an unrecognised location is
/// a perfectly valid way to run a binary — this reports what is true so the next question
/// ("how do I update this?") has an answer on the same screen, rather than sending the
/// user to guess between six install methods they may not remember choosing.
fn check_install_channel(f: &mut Findings) {
    let channel = Channel::detect();
    let detail = match (channel.upgrade_command(), channel.uninstall_command()) {
        (Some(upgrade), Some(uninstall)) => {
            format!(
                "{} — upgrade `{upgrade}`, remove `{uninstall}`",
                channel.label()
            )
        }
        // The installer's own copy: `devp uninstall` removes it, so there is no manager
        // command to name.
        (Some(upgrade), None) => format!("{} — upgrade `{upgrade}`", channel.label()),
        _ => format!(
            "{} — `devp update --install` still upgrades it in place",
            channel.label()
        ),
    };
    f.ok("Install channel", &detail);

    // Only for the copy the receipt actually describes. Any other channel's binary would
    // be shown a date belonging to a different file, which is worse than no date.
    if channel == Channel::Installer
        && let Some(receipt) = crate::receipt::load()
    {
        f.ok("Install receipt", &crate::receipt::summary(&receipt));
    }
}

/// Find every *other* `dev-prune` on the machine and report the ones running a
/// different version.
///
/// dev-prune ships through five channels and each one keeps its own copy. Upgrading via
/// `devp update --install` replaces the copy that matters — the managed one the hooks
/// and the scheduler invoke — and deliberately leaves the channel's own file alone,
/// because rewriting another manager's directory is how installations end up
/// unrepairable. The cost of that choice is a stale binary sitting on `PATH`, and if it
/// comes first the user types `devp` and silently gets the old release, with every
/// symptom pointing at dev-prune rather than at which copy answered.
///
/// So the copies are named. Nothing is deleted: which of them the user wants is a
/// question only they can answer, and the manager that installed one is the only thing
/// that should remove it.
fn check_other_copies(f: &mut Findings) {
    let mine = std::env::current_exe().ok();
    let managed_dir = setup::managed_exe_path()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));

    let search = copy_search_dirs(
        &std::env::var("PATH").unwrap_or_default(),
        dirs::home_dir().as_deref(),
    );
    let copies = binaries_in(&search, managed_dir.as_deref());

    // Asking each copy its own version, rather than comparing bytes: two channels can
    // hold byte-identical files of the same release, and a differing byte is just as
    // likely to be a different target triple as a different version. The question here
    // is only ever "would running this give me a different dev-prune".
    let stale: Vec<String> = copies
        .iter()
        .filter(|path| mine.as_deref() != Some(path.as_path()))
        .filter_map(|path| {
            // A file under this name that cannot state a version is left alone: it is
            // far more likely to be something else entirely — a shell wrapper, a
            // package-manager proxy that refuses to run under another name — than a
            // dev-prune, and naming it would send the user to delete an unrelated file.
            let version = setup::binary_version(path)?;
            let ours = setup::parse_version(constants::VERSION)?;
            (version != ours).then(|| {
                let channel = Channel::detect_at(path, managed_dir.as_deref());
                stale_copy_line(path, version, channel)
            })
        })
        .collect();

    if stale.is_empty() {
        f.ok("Other copies", "none on PATH running a different version");
        return;
    }
    f.warn(
        "Other copies",
        &format!(
            "{} — whichever comes first on PATH is the one `devp` runs, and \
             `devp update --install` only replaces the managed copy.",
            stale.join("; ")
        ),
    );
}

/// One line of the "Other copies" warning: where the copy is, which release it is, and
/// the command that removes it *through whatever put it there*.
///
/// Naming that command per copy is the whole value of the finding. "Remove each through
/// the manager that installed it" is true and useless: the reason a second copy goes
/// unnoticed for months is precisely that nobody remembers installing it, so the
/// instruction to remember is the one thing the user cannot follow.
fn stale_copy_line(path: &Path, version: (u64, u64, u64), channel: Channel) -> String {
    let (major, minor, patch) = version;
    let remedy = match channel.uninstall_command() {
        Some(cmd) => format!("from {}, remove with `{cmd}`", channel.label()),
        // The two channels that have no manager to ask. A copy inside the managed
        // directory never reaches here — `binaries_in` skips that directory outright
        // — but a second directory shaped like the installer's still can.
        None if channel == Channel::Installer => {
            "left by the install script, remove with `devp uninstall`".to_string()
        }
        None => "no package manager owns it; delete the file yourself".to_string(),
    };
    format!(
        "{} (v{major}.{minor}.{patch}, {remedy})",
        output::clean_path(path)
    )
}

/// Every directory that could hold a `dev-prune` this machine might run: `PATH`, plus
/// the fixed per-channel directories from [`crate::channel::install_dirs`] — the same
/// list `devp uninstall` sweeps, so doctor can never report a copy that uninstall then
/// fails to find.
///
/// The channel directories are searched even when they are not on `PATH`, because that
/// is the case that matters most — a copy nobody can see is also a copy nobody
/// upgrades, and it becomes the one that runs the day the user adds the directory to
/// `PATH` or a script calls it by absolute path.
///
/// Lexical and non-existent entries included; [`binaries_in`] does the filtering. Split
/// from it so the directory list can be tested without a home directory full of package
/// managers.
fn copy_search_dirs(path_var: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut dirs: Vec<PathBuf> = path_var
        .split(sep)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect();

    dirs.extend(crate::channel::install_dirs(home));
    dirs
}

/// The `dev-prune` and `devp` files that actually exist in `dirs`, minus everything in
/// `skip_dir`.
///
/// `skip_dir` is the managed `bin` directory, whose contents — the canonical binary, its
/// alias and the windowless twin — are already reported one by one by
/// [`check_binary`] and [`check_scheduler_target`]. Listing them again as "other copies"
/// would report a healthy install as having three stale binaries.
fn binaries_in(dirs: &[PathBuf], skip_dir: Option<&Path>) -> Vec<PathBuf> {
    let names: [&str; 2] = if cfg!(windows) {
        ["dev-prune.exe", "devp.exe"]
    } else {
        ["dev-prune", "devp"]
    };
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if skip_dir.is_some_and(|skip| same_dir(dir, skip)) {
            continue;
        }
        for name in names {
            let candidate = dir.join(name);
            // `PATH` routinely lists the same directory twice, spelled differently, and
            // on Windows `dev-prune.exe` and `devp.exe` are usually hard links to one
            // file — so a copy would otherwise be reported once per name and per
            // spelling.
            if candidate.is_file()
                && !found.iter().any(|seen| {
                    seen == &candidate
                        || (seen.parent() == candidate.parent() && same_binary(seen, &candidate))
                })
            {
                found.push(candidate);
            }
        }
    }
    found
}

/// Whether a PATH entry names the directory the binary lives in.
///
/// Windows paths are case-insensitive but `Path` equality is not, and PATH entries
/// routinely carry a trailing backslash the installer never wrote. Either mismatch made
/// doctor report a perfectly good install as "not on PATH" — as a problem, so `devp
/// doctor` exited 1 on a healthy machine.
fn same_dir(entry: &Path, dir: &Path) -> bool {
    if entry == dir {
        return true;
    }
    cfg!(windows) && {
        let norm = |p: &Path| {
            p.to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase()
        };
        norm(entry) == norm(dir)
    }
}

/// Whether two files hold the same bytes.
///
/// Doctor runs at human speed, so when the cheap size test cannot rule the pair
/// different this reads both files outright — a stale twin left by a failed upgrade can
/// share a size with its replacement, and "same version" is the whole question here.
fn same_binary(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return false;
    };
    if ma.len() != mb.len() {
        return false;
    }
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(ba), Ok(bb)) if ba == bb)
}

/// Read the config directory and validate every stored setting.
///
/// Returns `None` when the registry cannot be read, which is the one failure that makes
/// every later section meaningless — they all need the settings.
fn check_configuration(f: &mut Findings) -> Option<Registry> {
    f.section("Configuration");

    let dir = match Registry::config_dir() {
        Ok(d) => d,
        Err(e) => {
            f.problem("Config directory", &format!("cannot be resolved: {e}"));
            return None;
        }
    };
    f.note("Config directory", &output::clean_path(&dir));
    if std::env::var(constants::ENV_CONFIG_DIR_OVERRIDE).is_ok() {
        f.note(
            "",
            &format!("(set by {})", constants::ENV_CONFIG_DIR_OVERRIDE),
        );
    }

    let path = dir.join(constants::REGISTRY_FILENAME);
    if !path.exists() {
        // Not a fault. Absent configuration means defaults, which is the documented
        // behaviour — it is an unreadable one that dev-prune refuses to guess about.
        f.ok("registry.json", "not created yet — defaults apply");
        return Some(Registry::default());
    }

    let registry = match Registry::load_from(&path) {
        Ok(r) => r,
        Err(e) => {
            f.problem(
                "registry.json",
                &format!(
                    "{} — dev-prune refuses to guess at a config it cannot read. \
                     Fix the syntax, or delete the file to start from defaults.",
                    root_cause(&e)
                ),
            );
            return None;
        }
    };

    f.ok(
        "registry.json",
        &format!(
            "readable — {} {} registered",
            registry.repo_count(),
            output::plural(registry.repo_count(), "repository", "repositories")
        ),
    );

    let invalid = crate::commands::config::invalid_settings(&registry.settings);
    if invalid.is_empty() {
        f.ok(
            "Settings",
            &format!(
                "all {} within range",
                crate::commands::config::setting_count()
            ),
        );
    } else {
        for (key, why) in &invalid {
            f.problem("Settings", &format!("{key}: {why}"));
        }
    }

    Some(registry)
}

fn check_integrations(f: &mut Findings, registry: Option<&Registry>) {
    f.section("Integrations");

    match setup::skill_path() {
        Ok(p) if p.exists() => f.ok("SKILL.md", &output::clean_path(&p)),
        _ => {
            f.warn("SKILL.md", "not exported — run `devp skill`");
            f.fixable(Repair::SkillFile);
        }
    }

    if crate::commands::icon::is_registered() {
        f.ok("File icons", "registered with the file manager");
    } else {
        f.warn("File icons", "not registered — run `devp icon`");
    }

    if !hook::git_available() {
        f.warn(
            "Git hooks",
            "git is not on PATH, so repositories cannot auto-register",
        );
    } else {
        match hook::state() {
            // Reported before the target check because it is the worse of the two: a
            // hook set installed before 1.4.0 looks perfectly healthy from here — the
            // files exist and name a live binary — while Git, which reads
            // `core.hooksPath` instead of `.git/hooks`, is silently running none of the
            // repository's own hooks.
            Ok(HookState::Active) if hook::shims_incomplete() => {
                f.warn(
                    "Git hooks",
                    concat!(
                        "active, but installed without passthrough shims — ",
                        "every repository's own `.git/hooks` is being ignored ",
                        "machine-wide. `devp hook install` rewrites them to forward."
                    ),
                );
                f.fixable(Repair::Hooks);
            }
            Ok(HookState::Active) => check_hook_target(f, "active"),
            Ok(HookState::Absent) => f.warn("Git hooks", "not installed — run `devp hook install`"),
            Ok(HookState::Chained { previous, drifted }) if drifted.is_empty() => {
                check_hook_target(f, &format!("active, chained to `{previous}`"))
            }
            Ok(HookState::Chained { previous, drifted }) => {
                f.warn(
                    "Git hooks",
                    &format!(
                        "chained to `{previous}`, but {} not forwarded ({}) — \
                         re-run `devp hook install --chain`",
                        drifted.len(),
                        drifted.join(", ")
                    ),
                );
                // The chain is installed and merely out of date; reinstalling it is the
                // same repair the automatic setup pass makes.
                f.fixable(Repair::Hooks);
            }
            Ok(HookState::Foreign(p)) => f.warn(
                "Git hooks",
                &format!(
                    "core.hooksPath belongs to `{p}` — install in front of it with \
                     `devp hook install --chain`"
                ),
            ),
            Err(e) => f.warn("Git hooks", &format!("state unknown ({e})")),
        }
    }

    match daemon::daemon_status() {
        Ok(daemon::DaemonStatus::Installed) => check_scheduler_target(f),
        Ok(daemon::DaemonStatus::NotInstalled) => f.warn(
            "Scheduler",
            "not installed — nothing prunes on its own. `devp daemon install` adds it.",
        ),
        Ok(daemon::DaemonStatus::Unknown(why)) => f.warn("Scheduler", &why),
        Err(e) => f.warn("Scheduler", &format!("state unknown ({e})")),
    }

    if let Some(r) = registry {
        f.note(
            "Automatic setup",
            &format!(
                "auto_setup={} auto_hooks={} auto_daemon={}",
                r.settings.auto_setup, r.settings.auto_hooks, r.settings.auto_daemon
            ),
        );
    }

    // Said out loud, because "auto_setup = true" next to integrations that never install
    // is a contradiction the user has no other way to explain.
    if let Some(why) = setup::unattended_environment() {
        f.note("", &format!("unattended installation is off because {why}"));
    }
    // The same presence test the suppression itself uses — `=true`, `=0` and even an
    // empty value all switch setup off, so all of them must be reported here.
    if setup::no_auto_setup_requested() {
        f.note(
            "",
            &format!(
                "{} is set — nothing installs by itself. `devp setup` still works.",
                setup::ENV_NO_AUTO_SETUP
            ),
        );
    }
}

/// Report an installed integration, and whether the binary it will run is still there.
///
/// An installed scheduler and an installed hook are both silent by construction — the
/// scheduled task has no console and the hook throws its own output away — so a recorded
/// path that has since been deleted produces no symptom whatsoever. Every interval, the
/// task fails instantly; every commit, the hook does nothing. This is the only place that
/// says so.
///
/// The path goes stale when the integration is installed from somewhere temporary:
/// `npx dev-prune`, `uvx dev-prune`, or a `target/debug` build during development. Those
/// no longer record the temporary path (see `setup::stable_exe_path`), but entries
/// registered before that are still out there, and a user can always delete the binary
/// out from under a perfectly ordinary install.
fn report_integration_target(
    f: &mut Findings,
    label: &str,
    installed: &str,
    recorded: Option<std::path::PathBuf>,
    repair: &str,
) -> bool {
    match recorded {
        // Nothing to report: the entry is unreadable on this machine, which is not
        // evidence of a problem. Saying so would be a warning nobody can act on.
        None => f.ok(label, installed),
        Some(path) if path.is_file() => f.ok(
            label,
            &format!("{installed} — {}", output::clean_path(&path)),
        ),
        Some(path) => {
            f.problem(
                label,
                &format!(
                    "registered, but `{}` no longer exists — it never runs. {repair}",
                    output::clean_path(&path)
                ),
            );
            return true;
        }
    }
    false
}

fn check_scheduler_target(f: &mut Findings) {
    if report_integration_target(
        f,
        "Scheduler",
        "installed",
        daemon::registered_exe_path(),
        "Re-register it with `devp daemon install`.",
    ) {
        f.fixable_problem(Repair::Scheduler);
    }
}

fn check_hook_target(f: &mut Findings, installed: &str) {
    if report_integration_target(
        f,
        "Git hooks",
        installed,
        hook::registered_exe_path(),
        "Rewrite them with `devp hook install`.",
    ) {
        f.fixable_problem(Repair::Hooks);
    }
}

/// Check the package-manager binaries the registered repositories actually need.
///
/// Every adapter, not just the needed ones, would report `bun: not found` on a machine
/// with no JavaScript on it at all — a warning about a tool the user has deliberately not
/// installed. So the list comes from what is registered, and only falls back to all eight
/// when nothing is registered yet and there is nothing else to go on.
fn check_package_managers(f: &mut Findings, registry: Option<&Registry>) {
    f.section("Package managers");

    // `required` distinguishes a manager some registered repository actually depends on
    // from one merely listed for completeness. Warning that `go` is absent on a machine
    // with no Go project on it is noise the user cannot act on and would not want to.
    let (needed, required): (Vec<String>, bool) = match registry {
        Some(r) if r.repo_count() > 0 => {
            let mut names: Vec<String> = engine::get_full_status(r)
                .into_iter()
                .flat_map(|e| e.adapters)
                .collect();
            names.sort();
            names.dedup();
            (names, true)
        }
        _ => (
            adapters::get_all_adapters()
                .iter()
                .map(|a| a.name().to_string())
                .collect(),
            false,
        ),
    };

    if needed.is_empty() {
        f.note(
            "",
            "no package managers are needed by the registered repositories",
        );
        return;
    }
    if !required {
        f.note("", "nothing is registered yet, so this is the full list");
    }

    for status in adapters::scan_required_binaries(&needed) {
        match (status.available, status.version) {
            (true, Some(v)) => f.ok(&status.name, &v),
            (true, None) => f.ok(&status.name, "available"),
            (false, _) if required => {
                let detail =
                    "not on PATH — projects using it cannot be verified, pruned or restored";
                match adapters::install_hint(&status.name) {
                    Some(hint) => f.warn(&status.name, &format!("{detail}. Install it: {hint}")),
                    None => f.warn(&status.name, detail),
                }
            }
            (false, _) => f.note(&status.name, "not installed"),
        }
    }

    // `venv` is filtered out of the binary scan because it is not a command; its restore
    // path still needs an interpreter, and that is worth saying once.
    if required && needed.iter().any(|n| n == "venv") && !adapters::binary_available("python") {
        f.warn(
            "python",
            "not on PATH — `devp restore` cannot rebuild a plain virtual environment. \
             Install it: https://www.python.org/downloads/",
        );
    }
}

fn check_registry_health(f: &mut Findings, registry: Option<&Registry>) {
    f.section("Registered repositories");

    let Some(registry) = registry else { return };
    if registry.repo_count() == 0 {
        f.note("", "none yet — `devp init ~/Code` or `devp link .`");
        return;
    }

    let entries = engine::get_full_status(registry);
    let count = |want: &SkipReason| {
        entries
            .iter()
            .filter(|e| std::mem::discriminant(&e.reason) == std::mem::discriminant(want))
            .count()
    };
    let reclaimable: u64 = entries.iter().map(|e| e.reclaimable_bytes).sum();

    f.note(
        "Total",
        &format!(
            "{} registered, {} reclaimable",
            entries.len(),
            output::format_bytes(reclaimable)
        ),
    );
    f.note(
        "Breakdown",
        &format!(
            "{} candidates, {} active, {} ignored, {} with no bloat",
            count(&SkipReason::Candidate),
            count(&SkipReason::Active),
            count(&SkipReason::Ignored),
            count(&SkipReason::NoBloat),
        ),
    );

    // A path that has gone is stale bookkeeping, not breakage: the pass reports it and
    // moves on, so this warns rather than failing. Listing thirty of them individually
    // buries every other finding, and thirty `devp unlink` lines is not a fix anyone will
    // run — so they collapse to a count and the one command that clears all of them.
    let missing: Vec<&Path> = entries
        .iter()
        .filter(|e| matches!(e.reason, SkipReason::PathMissing))
        .map(|e| e.path.as_path())
        .collect();

    match missing.len() {
        0 => {}
        1 => {
            f.warn(
                "Missing",
                &format!(
                    "{} no longer exists — `devp unlink {}`",
                    output::clean_path(missing[0]),
                    output::clean_path(missing[0])
                ),
            );
            f.fixable(Repair::UnlinkMissing);
        }
        n => {
            f.warn(
                "Missing",
                &format!(
                    "{n} registered paths no longer exist, starting with {} \
                     — `devp unlink --missing` clears all of them",
                    output::clean_path(missing[0])
                ),
            );
            f.fixable(Repair::UnlinkMissing);
        }
    }

    // An unreadable `.devprune.json` *is* breakage: the file that cannot be read may be
    // the one saying `"ignore": true`, so the repository is skipped until it is fixed.
    for entry in &entries {
        if let SkipReason::ConfigError(e) = &entry.reason {
            f.problem(
                "Unreadable config",
                &format!("{}: {e}", output::clean_path(&entry.path)),
            );
            f.fixable_problem(Repair::RepoConfigs);
        }
    }
}

fn check_release_state(f: &mut Findings, registry: Option<&Registry>) {
    f.section("Release check");

    let Some(registry) = registry else { return };

    // Above the release check rather than beside it, because it outranks the answer:
    // whatever the check finds, a pinned copy is not going to install it.
    if registry.settings.version_lock {
        f.note(
            "version_lock",
            &format!(
                "on — this copy stays at v{}. auto_update, `devp update --install`, \
                 `devp install --channel` and the install scripts all stand down until \
                 `devp config set version_lock false`",
                constants::VERSION
            ),
        );
    }

    if !registry.settings.update_check {
        f.note(
            "update_check",
            "off — dev-prune opens no network connection",
        );
        return;
    }

    f.note(
        "update_check",
        &format!(
            "on, every {} {}",
            registry.settings.update_check_interval_days,
            output::plural(
                registry.settings.update_check_interval_days as usize,
                "day",
                "days"
            )
        ),
    );

    match registry.last_update_check {
        Some(at) => f.note(
            "Last checked",
            &format!(
                "{} ({} days ago)",
                at.format("%Y-%m-%d %H:%M UTC"),
                (Utc::now() - at).num_days()
            ),
        ),
        None => f.note("Last checked", "never"),
    }

    // Compared as versions, not as strings. `!=` reported the *cached* release as an
    // upgrade whenever it differed at all, so a machine running 1.2.0 with 1.1.0 still in
    // the cache was told to upgrade to 1.1.0 — and a development build one commit ahead of
    // the tag was told the same. Only "strictly newer" is an upgrade.
    match registry.latest_known_version.as_deref() {
        Some(latest) => {
            let latest_core = latest.trim_start_matches('v');
            match super::update::compare_versions(constants::VERSION, latest_core) {
                // Not a warning under a pin: being behind is the state that was
                // asked for, and doctor flagging it would be doctor arguing with the
                // configuration.
                Some(Ordering::Less) if registry.settings.version_lock => f.note(
                    "Latest release",
                    &format!(
                        "{latest} is available, and version_lock is holding this copy at v{}",
                        constants::VERSION
                    ),
                ),
                Some(Ordering::Less) => f.warn(
                    "Latest release",
                    &format!("{latest} is available — `devp update` shows how to upgrade"),
                ),
                Some(Ordering::Greater) => f.ok(
                    "Latest release",
                    &format!("{latest} — this build is newer than the last published one"),
                ),
                Some(Ordering::Equal) => f.ok("Latest release", &format!("{latest} — up to date")),
                None => f.note(
                    "Latest release",
                    &format!("{latest} — could not be compared to {}", constants::VERSION),
                ),
            }
        }
        None => f.note("Latest release", "not known yet"),
    }
}

// ---------------------------------------------------------------------------
// One repository
// ---------------------------------------------------------------------------

fn check_repository(path_str: &str) -> Result<()> {
    let path = Path::new(path_str)
        .canonicalize()
        .with_context(|| format!("Path not found: {path_str}"))?;

    output::print_header(&format!("dev-prune doctor ({})", output::clean_path(&path)));
    let mut f = Findings::default();

    // Loaded, not defaulted: a repository's verdict depends on the global thresholds, and
    // silently using the defaults would explain the wrong tool's behaviour.
    let registry = Registry::load().unwrap_or_default();

    let ctx = check_repo_basics(&mut f, &path, &registry);
    let projects = check_repo_projects(&mut f, &path, &ctx);
    let headline = repo_verdict(&ctx, &projects);

    verdict(
        &f,
        &format!("{} is in good shape.", output::clean_path(&ctx.path)),
        Some(&headline),
    )
}

/// Everything about the repository that is decided before any project is looked at.
struct RepoContext {
    path: PathBuf,
    is_git: bool,
    registered: bool,
    opted_out: Option<String>,
    config_broken: bool,
    idle: bool,
    idle_days: u64,
    min_size_bytes: u64,
    depth: usize,
}

fn check_repo_basics(f: &mut Findings, path: &Path, registry: &Registry) -> RepoContext {
    f.section("Repository");

    let is_git = scanner::is_git_repo(path);
    if is_git {
        f.ok("Git repository", "yes");
    } else {
        f.problem(
            "Git repository",
            "no — dev-prune only ever touches Git repositories",
        );
    }

    let key = crate::config::canonical_key(path);
    let entry = registry.repositories.get(&key);
    match entry {
        Some(e) if e.enabled => f.ok(
            "Registered",
            &format!("yes, since {}", e.added_at.format("%Y-%m-%d")),
        ),
        Some(e) => f.warn(
            "Registered",
            &format!(
                "yes since {}, but disabled — `devp config {} --update`",
                e.added_at.format("%Y-%m-%d"),
                output::clean_path(path)
            ),
        ),
        None => f.warn(
            "Registered",
            "no — a prune pass will not visit it. `devp link .` registers it.",
        ),
    }
    if let Some(at) = entry.and_then(|e| e.last_pruned_at) {
        f.note("Last pruned", &at.format("%Y-%m-%d %H:%M UTC").to_string());
    }

    // Read exactly the way the prune pass reads it, refusal to guess included.
    let (per_repo, config_broken) = match PerRepoConfig::load_with_diagnostics(path) {
        Ok(Some(cfg)) => {
            f.ok(constants::PER_REPO_CONFIG_FILE, &describe_overrides(&cfg));
            (Some(cfg), false)
        }
        Ok(None) => {
            f.note(
                constants::PER_REPO_CONFIG_FILE,
                "absent — global settings apply",
            );
            (None, false)
        }
        Err(e) => {
            f.problem(
                constants::PER_REPO_CONFIG_FILE,
                &format!("{e} — the repository is skipped entirely until this parses"),
            );
            (None, true)
        }
    };

    let mut opted_out = None;
    if path.join(constants::DEVPRUNE_IGNORE_FILE).exists() {
        opted_out = Some(format!("{} is present", constants::DEVPRUNE_IGNORE_FILE));
    } else if per_repo.as_ref().is_some_and(|c| c.ignore) {
        opted_out = Some(format!(
            "\"ignore\": true in {}",
            constants::PER_REPO_CONFIG_FILE
        ));
    } else if entry.is_some_and(|e| !e.enabled) {
        opted_out = Some("disabled in the registry".to_string());
    }
    match &opted_out {
        Some(why) => f.note("Opt-out", why),
        None => f.note("Opt-out", "none"),
    }

    // The same three-level resolution the engine performs: the repository's own file
    // beats its registry override, which beats the global setting.
    let idle_days = per_repo
        .as_ref()
        .and_then(|c| c.override_idle_days)
        .or_else(|| entry.and_then(|e| e.override_idle_days))
        .unwrap_or(registry.settings.idle_days);

    let activity = git::get_last_activity(path).ok().flatten();
    let idle = git::is_idle_at(activity, idle_days);
    match activity {
        Some(t) => {
            let days = chrono::DateTime::<Utc>::from(t);
            let ago = (Utc::now() - days).num_days();
            let detail = format!(
                "{} ({ago} {} ago), threshold {idle_days}",
                days.format("%Y-%m-%d"),
                output::plural(ago.unsigned_abs() as usize, "day", "days")
            );
            if idle {
                f.ok("Activity", &format!("{detail} — idle"));
            } else {
                f.note("Activity", &format!("{detail} — active"));
            }
        }
        None => f.note(
            "Activity",
            &format!("no commits or source edits found, threshold {idle_days}"),
        ),
    }

    let min_size_mb = per_repo
        .as_ref()
        .and_then(|c| c.min_size_mb)
        .unwrap_or(registry.settings.min_size_mb);
    f.note(
        "Size floor",
        &if min_size_mb == 0 {
            "none — every recognised directory counts".to_string()
        } else {
            format!("{min_size_mb} MiB")
        },
    );

    let depth = workspace::resolve_depth(path, registry.settings.scan_depth);
    f.note("Scan depth", &format!("{depth} levels below the root"));

    RepoContext {
        path: path.to_path_buf(),
        is_git,
        registered: entry.is_some(),
        opted_out,
        config_broken,
        idle,
        idle_days,
        min_size_bytes: min_size_mb.saturating_mul(BYTES_PER_MIB),
        depth,
    }
}

/// One project's worth of findings, kept so the verdict can reason over all of them.
struct ProjectReport {
    /// Whether any bloat directory here is above the floor and not symlinked.
    prunable: bool,
    /// Whether anything was found at all.
    has_bloat: bool,
}

fn check_repo_projects(f: &mut Findings, path: &Path, ctx: &RepoContext) -> Vec<ProjectReport> {
    f.section("Projects");

    if ctx.config_broken {
        f.note(
            "",
            "not scanned — the configuration above has to parse first",
        );
        return Vec::new();
    }

    let projects = workspace::discover_to_depth(path, ctx.depth);
    if projects.is_empty() {
        f.note(
            "",
            &format!(
                "no recognised package-manager project within {} levels. \
                 Raise it with `devp config set scan_depth N`.",
                ctx.depth
            ),
        );
        return Vec::new();
    }

    let mut reports = Vec::new();
    for project in &projects {
        for adapter in &project.adapters {
            println!();
            println!("  {} ({})", project.relative.bold(), adapter.name());

            // Presence only. Proving a lockfile is *usable* means running the package
            // manager, which is minutes of work and, for cargo and go, a write.
            let missing: Vec<&str> = adapter
                .lockfiles()
                .iter()
                .copied()
                .filter(|n| !project.path.join(n).exists())
                .collect();
            match (adapter.lockfiles().is_empty(), missing.is_empty()) {
                (true, _) => f.note("    Lockfile", "no single file identifies this manager"),
                (false, true) => f.ok(
                    "    Lockfile",
                    &format!("{} present", adapter.lockfiles().join(", ")),
                ),
                // Every listed file absent — for bun, whose two spellings are
                // alternatives, that means neither is there.
                (false, false) if missing.len() == adapter.lockfiles().len() => f.problem(
                    "    Lockfile",
                    &format!(
                        "{} missing — nothing can prove the directory is rebuildable, \
                         so it will never be pruned",
                        missing.join(" / ")
                    ),
                ),
                (false, false) => f.ok(
                    "    Lockfile",
                    &format!(
                        "{} present",
                        adapter
                            .lockfiles()
                            .iter()
                            .filter(|n| !missing.contains(n))
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ),
            }

            let bloat = adapter.bloat_dirs(&project.path);
            if bloat.is_empty() {
                f.note("    Bloat", "nothing installed — nothing to reclaim");
                reports.push(ProjectReport {
                    prunable: false,
                    has_bloat: false,
                });
                continue;
            }

            let mut prunable = false;
            for bd in &bloat {
                let label = workspace::relative_label(path, &bd.path);
                let size = output::format_bytes(bd.size_bytes);

                if std::fs::symlink_metadata(&bd.path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    f.warn(
                        "    Bloat",
                        &format!(
                            "{label} ({size}) is a symlink — refused, because the storage \
                             it points at is not this repository's to delete"
                        ),
                    );
                } else if bd.size_bytes < ctx.min_size_bytes {
                    f.warn(
                        "    Bloat",
                        &format!("{label} ({size}) is below the size floor — left alone"),
                    );
                } else {
                    f.ok("    Bloat", &format!("{label} ({size})"));
                    prunable = true;
                }
            }
            reports.push(ProjectReport {
                prunable,
                has_bloat: true,
            });
        }
    }

    reports
}

/// Name the one reason this repository would not be pruned right now.
///
/// In the order the prune pass applies them, so the answer matches what `devp run` would
/// actually do rather than listing everything that happens to be true.
fn repo_verdict(ctx: &RepoContext, projects: &[ProjectReport]) -> String {
    let clean = output::clean_path(&ctx.path);
    let no = |detail: String| format!("{} Would `devp run` prune this? {detail}", "✗".red());

    if !ctx.is_git {
        no("No — not a Git repository. Nothing else is even checked.".to_string())
    } else if ctx.config_broken {
        no(format!(
            "No — `{}` does not parse, and dev-prune will not guess at a config it \
             cannot read.",
            constants::PER_REPO_CONFIG_FILE
        ))
    } else if let Some(why) = &ctx.opted_out {
        no(format!("No — opted out: {why}."))
    } else if !ctx.registered {
        no(format!(
            "Not in a full pass — it is not registered. `devp link {clean}` adds it; \
             `devp run {clean}` prunes it once without registering."
        ))
    } else if !ctx.idle {
        no(format!(
            "No — active within the last {} {}. `devp --ignore-idle run {clean}` overrides \
             exactly that check and nothing else.",
            ctx.idle_days,
            output::plural(ctx.idle_days as usize, "day", "days")
        ))
    } else if projects.is_empty() {
        no("No — no package-manager project was found to prune.".to_string())
    } else if !projects.iter().any(|p| p.has_bloat) {
        no("No — every project here is already clean.".to_string())
    } else if !projects.iter().any(|p| p.prunable) {
        no("No — everything found is symlinked or below the size floor. See above.".to_string())
    } else {
        format!(
            "{} Would `devp run` prune this? Yes — subject to each lockfile verifying. \
             `devp run {clean} --dry-run` lists what would go.",
            "✓".green()
        )
    }
}

/// One line describing what a `.devprune.json` actually overrides.
fn describe_overrides(cfg: &PerRepoConfig) -> String {
    let mut parts = Vec::new();
    if let Some(name) = &cfg.project_name {
        parts.push(format!("name={name}"));
    }
    if let Some(days) = cfg.override_idle_days {
        parts.push(format!("idle_days={days}"));
    }
    if let Some(mb) = cfg.min_size_mb {
        parts.push(format!("min_size_mb={mb}"));
    }
    if let Some(depth) = cfg.scan_depth {
        parts.push(format!("scan_depth={depth}"));
    }
    if cfg.ignore {
        parts.push("ignore=true".to_string());
    }
    if cfg.disable_hooks {
        parts.push("disable_hooks=true".to_string());
    }
    if cfg.disable_daemon {
        parts.push("disable_daemon=true".to_string());
    }
    if parts.is_empty() {
        "parses; overrides nothing".to_string()
    } else {
        format!("parses; {}", parts.join(", "))
    }
}

/// The innermost cause of an error, which is the part that says what is actually wrong.
///
/// `anyhow`'s `{:#}` prints the whole chain, and the outer links here are all "failed to
/// parse the registry at <path>" — which the report has already said.
fn root_cause(e: &anyhow::Error) -> String {
    e.chain().last().map(|c| c.to_string()).unwrap_or_default()
}

/// Replace every registered repository's unreadable `.devprune.json` with a default.
///
/// The broken file is never destroyed: it is renamed to `.devprune.json.broken`
/// (numbered if that name is taken) beside the fresh one, because it may hold overrides
/// the user meant — an `"ignore": true` with a trailing comma is still a decision, and
/// the person who typed it is the only one who can retype it. The rename-then-write
/// order means a failure between the two leaves the repository with no config at all —
/// which is defaults, the same thing the fresh file says.
fn heal_repo_configs() -> Result<usize> {
    let registry = Registry::load()?;
    let mut healed = 0usize;
    for repo in registry.repositories.keys() {
        if !repo.exists() || PerRepoConfig::load_with_diagnostics(repo).is_ok() {
            continue;
        }
        let file = repo.join(constants::PER_REPO_CONFIG_FILE);
        let mut backup_name = format!("{}.broken", constants::PER_REPO_CONFIG_FILE);
        let mut n = 1;
        while repo.join(&backup_name).exists() {
            n += 1;
            backup_name = format!("{}.broken-{n}", constants::PER_REPO_CONFIG_FILE);
        }
        let backup = repo.join(&backup_name);
        std::fs::rename(&file, &backup).with_context(|| {
            format!(
                "could not move the broken config aside: {}",
                output::clean_path(&file)
            )
        })?;
        PerRepoConfig::default()
            .save_to_repo(repo)
            .with_context(|| {
                format!(
                    "could not write a default config in {}",
                    output::clean_path(repo)
                )
            })?;
        let _ = crate::config::ensure_in_git_exclude(repo, &backup_name);
        output::print_info(&format!(
            "{}: broken config kept as `{}`, defaults written",
            output::clean_path(repo),
            backup_name
        ));
        healed += 1;
    }
    Ok(healed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_stale_copy_names_the_command_that_removes_it() {
        // The finding exists so the user can act on it without first remembering how
        // the copy got there, which is exactly what they have forgotten.
        let line = stale_copy_line(
            Path::new("/usr/local/bin/dev-prune"),
            (1, 6, 0),
            Channel::Cargo,
        );
        assert!(line.contains("v1.6.0"), "{line}");
        assert!(line.contains("cargo uninstall dev-prune"), "{line}");
    }

    #[test]
    fn a_copy_from_the_install_script_is_removed_by_devp_itself() {
        // `Channel::uninstall_command` is None here for a good reason -- there is no
        // manager to ask -- but None must not come out as silence.
        let line = stale_copy_line(
            Path::new("/home/a/.dev-prune/bin/dev-prune"),
            (1, 5, 0),
            Channel::Installer,
        );
        assert!(line.contains("devp uninstall"), "{line}");
    }

    #[test]
    fn a_copy_nothing_owns_says_so_rather_than_naming_a_command() {
        // A file somebody copied into place by hand. Inventing a manager command for it
        // would send the user to run something that reports the package is not installed.
        let line = stale_copy_line(Path::new("/opt/dev-prune"), (0, 9, 1), Channel::Unknown);
        assert!(line.contains("delete the file yourself"), "{line}");
        assert!(!line.contains("uninstall dev-prune"), "{line}");
    }

    #[test]
    fn an_integration_pointing_at_a_deleted_binary_is_a_problem_not_a_warning() {
        // The whole point of the check: this is broken, not merely worth knowing, so it
        // has to reach the non-zero exit code.
        let mut f = Findings::default();
        report_integration_target(
            &mut f,
            "Scheduler",
            "installed",
            Some(PathBuf::from("/nonexistent/dev-prune")),
            "Re-register it.",
        );
        assert_eq!(f.warnings.len(), 0);
        assert_eq!(f.problems.len(), 1);
        assert!(f.problems[0].contains("no longer exists"));
    }

    #[test]
    fn an_integration_whose_binary_is_present_passes() {
        let tmp = TempDir::new().unwrap();
        let exe = tmp.path().join("dev-prune");
        std::fs::write(&exe, b"binary").unwrap();

        let mut f = Findings::default();
        report_integration_target(&mut f, "Scheduler", "installed", Some(exe), "Re-register.");
        assert!(f.problems.is_empty() && f.warnings.is_empty());
    }

    #[test]
    fn an_unreadable_entry_is_not_reported_as_broken() {
        // `None` means the platform could not tell us, which is not evidence of a
        // problem — reporting it would be a warning nobody can act on.
        let mut f = Findings::default();
        report_integration_target(&mut f, "Scheduler", "installed", None, "Re-register.");
        assert!(f.problems.is_empty() && f.warnings.is_empty());
    }

    #[test]
    fn overrides_are_listed_by_name() {
        let mut cfg = PerRepoConfig::default();
        assert_eq!(describe_overrides(&cfg), "parses; overrides nothing");

        cfg.override_idle_days = Some(30);
        cfg.ignore = true;
        assert_eq!(
            describe_overrides(&cfg),
            "parses; idle_days=30, ignore=true"
        );
    }

    /// `min_size_mb: 0` is a value, not an absence — it opts a repository out of a global
    /// floor, so the report has to show it rather than treating it as unset.
    #[test]
    fn a_zero_floor_is_reported_as_an_override() {
        let cfg = PerRepoConfig {
            min_size_mb: Some(0),
            ..PerRepoConfig::default()
        };
        assert_eq!(describe_overrides(&cfg), "parses; min_size_mb=0");
    }

    #[test]
    fn a_repository_that_is_not_a_git_repo_is_the_first_thing_reported() {
        let dir = TempDir::new().unwrap();
        let ctx = RepoContext {
            path: dir.path().to_path_buf(),
            is_git: false,
            registered: false,
            opted_out: Some("ignore.devprune.json is present".to_string()),
            config_broken: true,
            idle: true,
            idle_days: 15,
            min_size_bytes: 0,
            depth: 6,
        };
        // Three reasons are true at once; the verdict names the one the prune pass would
        // hit first, which is the one the user has to fix before any other matters.
        let line = repo_verdict(&ctx, &[]);
        assert!(line.contains("not a Git repository"), "{line}");
    }

    #[test]
    fn warnings_alone_do_not_fail_the_command() {
        let mut f = Findings::default();
        f.warn("Scheduler", "not installed");
        assert!(verdict(&f, "fine", None).is_ok());

        f.problem("PATH", "missing");
        assert!(verdict(&f, "fine", None).is_err());
    }

    #[test]
    #[cfg(windows)]
    fn a_path_entry_matches_regardless_of_case_and_trailing_separator() {
        // Both differences are ones Windows itself ignores, and either used to turn
        // into a "not on PATH" problem — a healthy install failing `devp doctor`.
        let dir = Path::new(r"C:\Users\Someone\AppData\Roaming\dev-prune\bin");
        assert!(same_dir(
            Path::new(r"c:\users\someone\appdata\roaming\dev-prune\bin\"),
            dir
        ));
        assert!(!same_dir(Path::new(r"C:\Windows"), dir));
    }

    #[test]
    #[cfg(not(windows))]
    fn a_path_entry_on_unix_is_matched_exactly() {
        assert!(same_dir(
            Path::new("/usr/local/bin"),
            Path::new("/usr/local/bin")
        ));
        assert!(!same_dir(
            Path::new("/USR/local/bin"),
            Path::new("/usr/local/bin")
        ));
    }

    #[test]
    fn a_stale_twin_is_told_apart_from_a_current_one() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("dev-prune");
        let b = dir.path().join("devp");
        std::fs::write(&a, b"version two").unwrap();
        std::fs::write(&b, b"version two").unwrap();
        assert!(same_binary(&a, &b));

        // Same length, different bytes — the case a size-only test waves through.
        std::fs::write(&b, b"version one").unwrap();
        assert!(!same_binary(&a, &b));

        std::fs::write(&b, b"short").unwrap();
        assert!(!same_binary(&a, &b));
        assert!(!same_binary(&a, &dir.path().join("missing")));
    }

    #[test]
    fn the_channel_directories_are_searched_even_when_they_are_not_on_path() {
        // The whole point of this check: a copy nobody can see is a copy nobody
        // upgrades, and it becomes the one that runs the day PATH changes.
        let home = Path::new(if cfg!(windows) {
            "C:\\home\\u"
        } else {
            "/home/u"
        });
        let dirs = copy_search_dirs("", Some(home));
        let joined = dirs
            .iter()
            .map(|d| d.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
            .join("|");
        for marker in ["cargo", "uv", "pipx"] {
            assert!(
                joined.contains(marker),
                "{marker} directory missing from {joined}"
            );
        }
    }

    #[test]
    fn path_entries_are_searched_and_empty_ones_dropped() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let a = if cfg!(windows) { "C:\\a" } else { "/a" };
        let b = if cfg!(windows) { "C:\\b" } else { "/b" };
        let dirs = copy_search_dirs(&format!("{a}{sep}{sep}{b}"), None);
        assert_eq!(dirs, vec![PathBuf::from(a), PathBuf::from(b)]);
    }

    #[test]
    fn the_managed_directory_is_never_reported_as_another_copy() {
        // Its three files are each reported by name elsewhere; listing them here would
        // tell a healthy install it has stale binaries.
        let tmp = tempfile::tempdir().expect("temp dir");
        let managed = tmp.path().join("bin");
        std::fs::create_dir_all(&managed).expect("create");
        let name = if cfg!(windows) {
            "dev-prune.exe"
        } else {
            "dev-prune"
        };
        std::fs::write(managed.join(name), b"binary").expect("write");

        assert!(binaries_in(std::slice::from_ref(&managed), Some(&managed)).is_empty());
        assert_eq!(binaries_in(std::slice::from_ref(&managed), None).len(), 1);
    }

    #[test]
    fn one_binary_under_both_names_is_reported_once() {
        // `dev-prune` and `devp` in the same directory are the same binary — on Windows
        // usually literally the same file — so a single install must not read as two.
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = tmp.path().to_path_buf();
        let (a, b) = if cfg!(windows) {
            ("dev-prune.exe", "devp.exe")
        } else {
            ("dev-prune", "devp")
        };
        std::fs::write(dir.join(a), b"same bytes").expect("write");
        std::fs::write(dir.join(b), b"same bytes").expect("write");
        assert_eq!(binaries_in(std::slice::from_ref(&dir), None).len(), 1);

        // A genuinely different binary under the second name is a second copy.
        std::fs::write(dir.join(b), b"a different build").expect("write");
        assert_eq!(binaries_in(&[dir], None).len(), 2);
    }
}
