// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for the `dev-prune daemon` command.
//
// Manages the OS-native background scheduler (install, uninstall, status).

use anyhow::Result;

use crate::daemon;
use crate::output;

/// Show daemon status.
pub fn run_status() -> Result<()> {
    output::print_header("dev-prune daemon status");
    let status = daemon::daemon_status()?;
    output::print_info(&format!("Status: {status}"));
    Ok(())
}

/// Install the daemon.
pub fn run_install() -> Result<()> {
    output::print_header("dev-prune daemon install");
    let settings = crate::config::Registry::load()
        .map(|r| r.settings)
        .unwrap_or_default();
    daemon::install_daemon(settings.check_interval_days)?;
    output::print_success("Background daemon installed successfully");
    output::print_info(&format!(
        "dev-prune will run automatically every {} day(s), pruning repos idle for {}+ days.",
        settings.check_interval_days.max(1),
        settings.idle_days
    ));
    output::print_info(
        "Background runs are non-interactive: they prune every idle repo whose lockfile \
         verifies, without asking. Change the cadence with `devp config set check_interval_days <n>`, \
         or remove it with `devp daemon uninstall`.",
    );
    Ok(())
}

/// Uninstall the daemon.
pub fn run_uninstall() -> Result<()> {
    output::print_header("dev-prune daemon uninstall");
    daemon::uninstall_daemon()?;
    output::print_success("Background daemon removed");
    Ok(())
}
