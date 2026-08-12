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

//! Handler for `dev-prune setup`.
//!
//! The same pass dev-prune runs by itself after an install or an upgrade, available to
//! run by hand — which is what the installer scripts do, and what the error messages
//! point people at when an integration was skipped.

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
    let report = setup::ensure_integrations(&registry);
    report.print(true);
    setup::suppress_next_auto_setup();

    println!();
    if report.needs_attention() {
        output::print_info("Anything skipped above is optional — dev-prune works without it.");
    } else {
        output::print_success("Everything dev-prune needs is in place.");
    }
    output::print_info("Remove all of it again with `devp uninstall`.");

    Ok(())
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

    let skill = setup::skill_path().ok().filter(|p| p.exists());
    println!(
        "  SKILL.md:              {}",
        skill
            .as_ref()
            .map(output::clean_path)
            .unwrap_or_else(|| "not exported".to_string())
    );

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
