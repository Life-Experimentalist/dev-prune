// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune uninstall` command.
//
// Provides two uninstall modes:
// - Light (default): Removes OS background daemon, global Git hooks, and binary aliases.
//   Preserves configuration registry for seamless future re-installations.
// - Deep (`devp uninstall --deep`): Complete purge — removes daemon, hooks, binary aliases,
//   global configuration folder, and removes `.devprune.json` from registered repos.

use anyhow::Result;
use std::fs;

use crate::commands::hook;
use crate::config::Registry;
use crate::output;

pub fn run(deep: bool, yes: bool) -> Result<()> {
    output::print_header(if deep {
        "dev-prune Deep Uninstaller (Full Purge)"
    } else {
        "dev-prune Light Uninstaller"
    });

    let registry = Registry::load().ok();

    // A deep uninstall deletes files inside the user's own repositories and destroys
    // the prune history. That is not something to do on a mistyped flag.
    if deep && !yes {
        use std::io::{IsTerminal, Write};
        let repo_count = registry.as_ref().map(|r| r.repo_count()).unwrap_or(0);
        output::print_warning(&format!(
            "This deletes the global config directory (including prune history) and \
             removes `.devprune.json` from {repo_count} registered repositories."
        ));
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Refusing to deep-uninstall without confirmation. Re-run with `--yes`.");
        }
        print!("Continue? [y/N]: ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
            output::print_info("Deep uninstall cancelled.");
            return Ok(());
        }
    }

    // Steps 1 and 2 keep going when one of them fails — a scheduler that refuses to
    // uninstall must not stop the hooks being removed — but neither is silent about it.
    // Both leave the machine in a half-integrated state that the user has to know about.
    let mut left_behind: Vec<String> = Vec::new();

    // 1. Remove background daemon
    output::print_info("Removing background daemon scheduler...");
    if let Err(e) = crate::daemon::uninstall_daemon() {
        output::print_error(&format!("Background scheduler: {e:#}"));
        left_behind.push("the background scheduler".to_string());
    }

    // 2. Remove global Git hooks
    output::print_info("Removing global Git auto-registration hooks...");
    if let Err(e) = hook::run_uninstall() {
        output::print_error(&format!("Git hooks: {e:#}"));
        left_behind.push("the global Git hooks".to_string());
    }

    // 3. Remove the `devp` alias, so deleting the binary afterwards leaves nothing.
    //
    // Every invocation recreates it (`ensure_devp_alias` at the top of `run_cli`), this
    // one included — so it is back the moment dev-prune runs again. Removing it here is
    // only worth anything as the first half of "and now delete the binary", which is why
    // the binary's location is printed below rather than left for the user to find.
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            #[cfg(windows)]
            let alias = parent.join("devp.exe");
            #[cfg(not(windows))]
            let alias = parent.join("devp");

            if alias.exists() && alias != current_exe {
                let _ = fs::remove_file(alias);
            }
        }
    }

    // 4. Take the `*.devprune.json` file type back out of the desktop database.
    crate::commands::icon::unregister_file_type();

    if deep {
        // Deep uninstall: remove per-repo configs & global config dir
        if let Some(reg) = registry {
            for repo_path in reg.repositories.keys() {
                let cfg_file = repo_path.join(crate::constants::PER_REPO_CONFIG_FILE);
                if cfg_file.exists() {
                    let _ = fs::remove_file(cfg_file);
                }
            }
        }

        if let Ok(config_dir) = Registry::config_dir() {
            if config_dir.exists() {
                match fs::remove_dir_all(&config_dir) {
                    Ok(()) => output::print_info("Removed global configuration directory."),
                    Err(e) => {
                        // Routine on Windows: the managed copy under `<config>/bin` is
                        // often the very binary running this command, and a running
                        // executable cannot be deleted. Claiming success here left a
                        // directory the user believed purged.
                        let running_inside =
                            std::env::current_exe().is_ok_and(|exe| exe.starts_with(&config_dir));
                        let hint = if running_inside {
                            " The running binary lives inside it — delete the directory \
                             by hand once this command exits."
                        } else {
                            ""
                        };
                        output::print_error(&format!(
                            "Could not remove {}: {e}.{hint}",
                            output::clean_path(&config_dir)
                        ));
                        left_behind.push("the global configuration directory".to_string());
                    }
                }
            }
        }

        if left_behind.is_empty() {
            output::print_success(
                "Deep uninstall complete: All configurations, background daemons, and registry files purged.",
            );
        }
        // The stamp that suppresses the automatic pass lived in the directory that was
        // just deleted, so the next dev-prune command looks like a fresh install and
        // puts the hooks and the scheduler straight back. Deleting the binary is the
        // only thing that ends it, so say so instead of letting it surprise them.
        output::print_warning(
            "Running dev-prune again would reinstall the hooks and the scheduler — a \
             machine with no config directory looks like a fresh install.",
        );
        print_binary_removal_hint();
    } else {
        // Stamp the current version so the automatic pass does not reinstall, on the
        // very next command, everything this command was run to remove.
        crate::setup::suppress_next_auto_setup();
        if left_behind.is_empty() {
            output::print_success(
                "Light uninstall complete: Background daemon and Git hooks removed. Configuration preserved for future reinstall.",
            );
        }
        output::print_info("Put them back at any time with `devp setup`.");
        // Honesty about the stamp: it suppresses the pass for *this* version only.
        output::print_info(
            "The next upgrade will reinstall them. To keep them off permanently: \
             `devp config set auto_setup false`.",
        );
    }

    if !left_behind.is_empty() {
        anyhow::bail!("Uninstall finished, but {} is still installed.", {
            left_behind.join(" and ")
        });
    }

    Ok(())
}

/// Where the binaries themselves live, since nothing above removes them.
///
/// Both names, not just the one that is running: invoked as `devp`, step 3 above could
/// not remove the alias (it *is* this process), and the canonical `dev-prune` is never
/// removed by anything — a hint that named only one of the pair left the other behind.
fn print_binary_removal_hint() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut targets = vec![output::clean_path(&exe)];
    if let Some(parent) = exe.parent() {
        for stem in ["dev-prune", "devp"] {
            let name = if cfg!(windows) {
                format!("{stem}.exe")
            } else {
                stem.to_string()
            };
            let twin = parent.join(name);
            if twin.exists() && twin != exe {
                targets.push(output::clean_path(&twin));
            }
        }
    }
    output::print_info(&format!(
        "To finish, delete {}: {}",
        if targets.len() > 1 {
            "the binaries"
        } else {
            "the binary"
        },
        targets.join(" and ")
    ));
}
