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

//! Handler for `dev-prune doctor`.
//!
//! One command that answers "why is this not doing what I expect". Without a path it
//! checks the installation — binary, alias, PATH, config, integrations, package managers,
//! registry, release check. With a path it checks that one repository and ends by naming
//! the reason it would or would not be pruned right now.
//!
//! Everything here is read-only. A diagnostic that repairs things as it goes cannot be
//! run twice to see whether the first run helped, and `devp setup` already exists for the
//! repairing. Nothing runs a package manager either: `enforce_lockfile` invokes `npm`,
//! `cargo` and friends, which is minutes of work and, for the opted-in adapters, writes
//! to tracked files. The doctor reports what it can see.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::adapters;
use crate::commands::hook::{self, HookState};
use crate::config::{PerRepoConfig, Registry};
use crate::constants;
use crate::daemon;
use crate::engine::{self, BYTES_PER_MIB, SkipReason};
use crate::output;
use crate::scanner::{self, git};
use crate::setup;
use crate::workspace;

/// Tally of everything the report flagged, so the verdict is derived from the same
/// lines the user just read rather than recomputed from scratch.
#[derive(Default)]
struct Findings {
    warnings: Vec<String>,
    problems: Vec<String>,
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
/// installation; `Some(".")` is an ordinary path like any other.
pub fn run(path: Option<&str>) -> Result<()> {
    match path {
        Some(p) => check_repository(p),
        None => check_installation(),
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

fn check_installation() -> Result<()> {
    output::print_header("dev-prune doctor");
    let mut f = Findings::default();

    check_binary(&mut f);
    let registry = check_configuration(&mut f);
    check_integrations(&mut f, registry.as_ref());
    check_package_managers(&mut f, registry.as_ref());
    check_registry_health(&mut f, registry.as_ref());
    check_release_state(&mut f, registry.as_ref());

    verdict(&f, "Everything checks out.", None)
}

fn check_binary(f: &mut Findings) {
    f.section("Installation");
    f.note("Version", constants::VERSION);

    let Ok(exe) = std::env::current_exe() else {
        f.warn("Executable", "the running binary's own path is unavailable");
        return;
    };
    f.note("Executable", &output::clean_path(&exe));

    let Some(dir) = exe.parent() else { return };

    // The `devp` name people actually type. It is a real executable next to `dev-prune`,
    // not a shell alias, so this is a file that either exists or does not.
    let alias = dir.join(if cfg!(windows) { "devp.exe" } else { "devp" });
    if alias.exists() {
        f.ok("devp", &output::clean_path(&alias));
    } else {
        f.warn(
            "devp",
            "not installed next to dev-prune — run `dev-prune setup`",
        );
    }

    let sep = if cfg!(windows) { ';' } else { ':' };
    let on_path = std::env::var("PATH")
        .unwrap_or_default()
        .split(sep)
        .any(|p| !p.is_empty() && Path::new(p) == dir);
    if on_path {
        f.ok("PATH", &output::clean_path(dir));
    } else {
        f.problem(
            "PATH",
            &format!(
                "{} is not on PATH — `devp` will not resolve in a new shell",
                output::clean_path(dir)
            ),
        );
    }
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
        _ => f.warn("SKILL.md", "not exported — run `devp skill`"),
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
            Ok(HookState::Active) => check_hook_target(f, "active"),
            Ok(HookState::Absent) => f.warn("Git hooks", "not installed — run `devp hook install`"),
            Ok(HookState::Chained { previous, drifted }) if drifted.is_empty() => {
                check_hook_target(f, &format!("active, chained to `{previous}`"))
            }
            Ok(HookState::Chained { previous, drifted }) => f.warn(
                "Git hooks",
                &format!(
                    "chained to `{previous}`, but {} not forwarded ({}) — \
                     re-run `devp hook install --chain`",
                    drifted.len(),
                    drifted.join(", ")
                ),
            ),
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
    if std::env::var(setup::ENV_NO_AUTO_SETUP).as_deref() == Ok("1") {
        f.note(
            "",
            &format!(
                "{}=1 is set — nothing installs by itself. `devp setup` still works.",
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
) {
    match recorded {
        // Nothing to report: the entry is unreadable on this machine, which is not
        // evidence of a problem. Saying so would be a warning nobody can act on.
        None => f.ok(label, installed),
        Some(path) if path.is_file() => f.ok(
            label,
            &format!("{installed} — {}", output::clean_path(&path)),
        ),
        Some(path) => f.problem(
            label,
            &format!(
                "registered, but `{}` no longer exists — it never runs. {repair}",
                output::clean_path(&path)
            ),
        ),
    }
}

fn check_scheduler_target(f: &mut Findings) {
    report_integration_target(
        f,
        "Scheduler",
        "installed",
        daemon::registered_exe_path(),
        "Re-register it with `devp daemon install`.",
    );
}

fn check_hook_target(f: &mut Findings, installed: &str) {
    report_integration_target(
        f,
        "Git hooks",
        installed,
        hook::registered_exe_path(),
        "Rewrite them with `devp hook install`.",
    );
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
            (false, _) if required => f.warn(
                &status.name,
                "not on PATH — projects using it cannot be verified, pruned or restored",
            ),
            (false, _) => f.note(&status.name, "not installed"),
        }
    }

    // `venv` is filtered out of the binary scan because it is not a command; its restore
    // path still needs an interpreter, and that is worth saying once.
    if required && needed.iter().any(|n| n == "venv") && !adapters::binary_available("python") {
        f.warn(
            "python",
            "not on PATH — `devp restore` cannot rebuild a plain virtual environment",
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
        1 => f.warn(
            "Missing",
            &format!(
                "{} no longer exists — `devp unlink {}`",
                output::clean_path(missing[0]),
                output::clean_path(missing[0])
            ),
        ),
        n => {
            f.warn(
                "Missing",
                &format!(
                    "{n} registered paths no longer exist, starting with {} \
                     — `devp unlink --missing` clears all of them",
                    output::clean_path(missing[0])
                ),
            );
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
        }
    }
}

fn check_release_state(f: &mut Findings, registry: Option<&Registry>) {
    f.section("Release check");

    let Some(registry) = registry else { return };
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

    match registry.latest_known_version.as_deref() {
        Some(latest) if latest.trim_start_matches('v') != constants::VERSION => f.warn(
            "Latest release",
            &format!("{latest} is available — `devp update` shows how to upgrade"),
        ),
        Some(latest) => f.ok("Latest release", &format!("{latest} — up to date")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
}
