// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune setup`.
//
// The same pass dev-prune runs by itself after an install or an upgrade, available to
// run by hand — which is what the installer scripts do, and what the error messages
// point people at when an integration was skipped.

use anyhow::Result;

use crate::commands::hook::{self, HookState};
use crate::config::Registry;
use crate::daemon;
use crate::output;
use crate::setup;

pub fn run(status_only: bool) -> Result<()> {
    if status_only {
        return run_status();
    }

    output::print_header("dev-prune setup");
    let registry = Registry::load()?;
    let report = setup::ensure_integrations(&registry, setup::Consent::Explicit);
    report.print(true);
    setup::suppress_next_auto_setup();

    println!();
    if report.needs_attention() {
        output::print_info("Anything skipped above is optional — dev-prune works without it.");
    } else {
        output::print_success("Everything dev-prune needs is in place.");
    }
    output::print_info("Remove all of it again with `devp uninstall`.");

    setup::offer_vscode_extension();

    // Installing dev-prune tracks nothing on its own, and a first-time user who stops here
    // gets an empty `devp status` with no hint about why. The installer scripts say this
    // too, but `cargo install`, `npm i -g` and `pipx install` never run one — `devp setup`
    // is the only step every channel has in common.
    if registry.repositories.is_empty() {
        println!();
        output::print_info("No repositories are tracked yet. Register them either way:");
        println!(
            "    devp init {}  # crawl one folder for every Git repo inside it",
            example_projects_dir()
        );
        // Both example directories are six characters wide, so one padding works for both.
        println!("    devp link .       # or, from inside one project, register just that one");
    }

    Ok(())
}

/// A plausible "where your projects live" path to show in the onboarding hint.
///
/// Purely cosmetic, but a Windows user reading `~/code` and a Linux user reading `~\Code`
/// both have to translate before they can paste, and the first command a new user runs is
/// the worst place to make them do that.
fn example_projects_dir() -> &'static str {
    if cfg!(windows) { "~\\Code" } else { "~/code" }
}

/// Report each integration without touching anything.
fn run_status() -> Result<()> {
    output::print_header("dev-prune setup status");
    let registry = Registry::load()?;

    let alias = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
        .map(|dir| dir.join(if cfg!(windows) { "devp.exe" } else { "devp" }))
        .filter(|p| p.exists());
    println!(
        "  devp alias:            {}",
        alias
            .as_ref()
            .map(output::clean_path)
            .unwrap_or_else(|| "not installed".to_string())
    );

    print!("  Command on PATH:       ");
    match setup::managed_bin_dir() {
        Ok(dir) if crate::pathenv::is_reachable(&dir) => {
            println!("{} is on your PATH", output::clean_path(&dir))
        }
        Ok(dir) => println!("{} is not on your PATH", output::clean_path(&dir)),
        Err(_) => println!("unknown (no config directory)"),
    }

    let skill = setup::skill_path().ok().filter(|p| p.exists());
    println!(
        "  SKILL.md:              {}",
        skill
            .as_ref()
            .map(output::clean_path)
            .unwrap_or_else(|| "not exported".to_string())
    );

    let agent_roots = setup::agent_skill_roots();
    if agent_roots.is_empty() {
        println!("  AI agent skills:       no AI agent detected");
    } else {
        for root in &agent_roots {
            println!(
                "  AI agent skills:       {} ({})",
                output::clean_path(root),
                if root.join("SKILL.md").is_file() {
                    "installed"
                } else {
                    "not installed"
                }
            );
        }
    }

    println!(
        "  File icons:            {}",
        if crate::commands::icon::is_registered() {
            "registered"
        } else {
            "not registered"
        }
    );

    print!("  Git hooks:             ");
    if !hook::git_available() {
        println!("git is not on PATH");
    } else {
        match hook::state() {
            Ok(HookState::Active) => println!("active"),
            Ok(HookState::Absent) => println!("not installed"),
            Ok(HookState::Chained { previous, drifted }) if drifted.is_empty() => {
                println!("active, chained to `{previous}`")
            }
            Ok(HookState::Chained { previous, drifted }) => println!(
                "active, chained to `{previous}` — {} not forwarded ({}); run `devp hook install --chain`",
                drifted.len(),
                drifted.join(", ")
            ),
            Ok(HookState::Foreign(p)) => println!(
                "core.hooksPath belongs to `{p}` (install in front of it with `devp hook install --chain`)"
            ),
            Err(e) => println!("unknown ({e})"),
        }
    }

    println!(
        "  Background scheduler:  {}",
        daemon::daemon_status()
            .map(|s| s.to_string())
            .unwrap_or_else(|e| format!("unknown ({e})"))
    );

    println!();
    println!("  auto_setup  = {}", registry.settings.auto_setup);
    println!("  auto_hooks  = {}", registry.settings.auto_hooks);
    println!("  auto_daemon = {}", registry.settings.auto_daemon);

    // Said out loud, because otherwise "auto_setup = true" and nothing ever installing
    // is a contradiction the user has no way to explain.
    if let Some(why) = setup::unattended_environment() {
        println!();
        output::print_info(&format!(
            "Unattended installation is off because {why}. `devp setup` still works when asked."
        ));
    }

    println!();
    if setup::setup_is_due() {
        output::print_info("A setup pass is due. Run `devp setup`.");
    } else {
        output::print_info("Install anything missing with `devp setup`.");
    }

    Ok(())
}
