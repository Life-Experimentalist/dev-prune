// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune update`, and the periodic release check behind it.
//
// The check is opt-*out*. An out-of-date cleanup tool is a tool whose safety fixes you do
// not have, so `devp update` asks GitHub for the latest release by default, and `devp
// run` / `devp status` repeat that quietly at most once a week. Both are switched off by
// `devp config set update_check false`, and `devp update --offline` skips a single run.
//
// What leaves the machine is one unauthenticated GET to the public releases endpoint. It
// carries no identifier, no configuration, no repository paths and no usage data — the
// only thing the server learns is that some copy of dev-prune asked what the latest
// version is. Nothing else in the binary opens a socket. See `docs/PRIVACY.md`.
//
// By default the command does not download or install anything: replacing a binary is
// the package manager's job, and doing it ourselves would mean writing to a PATH
// directory with whatever privileges the user happened to have. `--install` keeps that
// division of labour — it works out which package manager owns the running binary and
// runs *that manager's* own upgrade command, rather than writing files itself. The
// scheduled pass is never interrupted by an upgrade: it runs the managed copy under
// `<config>/bin`, which is replaced by atomic rename and refreshed from the new binary
// on the next healthy run (`setup::stable_exe_path`), so a pass already in flight keeps
// its loaded image and the next pass picks up the new one.

use std::cmp::Ordering;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::config::Registry;
use crate::constants;
use crate::output;

pub fn run(offline: bool, install: bool) -> Result<()> {
    if install {
        return run_install();
    }
    output::print_header("dev-prune version & upgrade");

    output::print_info(&format!("Installed version: v{}", constants::VERSION));

    if offline {
        output::print_info("Skipping the release check because `--offline` was passed.");
    } else if let Ok(mut registry) = Registry::load() {
        if registry.settings.update_check {
            // An explicit `devp update` always asks, regardless of when the last
            // automatic check ran — the user is standing there waiting for the answer.
            match refresh_latest(&mut registry) {
                Ok(latest) => report_comparison(&latest),
                // A failed check is not a failed command. Someone offline, behind a
                // proxy, or hitting a rate limit still wants the upgrade instructions.
                Err(e) => output::print_warning(&format!(
                    "Could not reach the release API ({e}). The upgrade commands below still apply."
                )),
            }
            let _ = registry.save();
        } else {
            output::print_info(
                "The release check is off (`devp config set update_check true` re-enables it).",
            );
        }
    }

    println!();
    println!("  Latest releases:  {}", constants::RELEASES_URL);
    println!();
    print_upgrade_commands();

    Ok(())
}

/// Ask GitHub right now — no interval — and say where the installed build stands.
///
/// For `devp init`, which is deliberate and infrequent enough to be worth a round trip:
/// setting a machine up is exactly the moment to learn the binary is a version behind.
/// `devp run` deliberately does not use this; it goes through [`notify_if_outdated`],
/// which is interval-gated so everyday work never waits on the network.
///
/// Returns `true` when the registry changed and needs saving.
pub fn check_now(registry: &mut Registry) -> bool {
    if !registry.settings.update_check {
        return false;
    }

    match refresh_latest(registry) {
        Ok(latest) => {
            report_comparison(&latest);
            if compare_versions(constants::VERSION, &latest) == Some(Ordering::Less) {
                print_upgrade_commands();
            }
        }
        // Not being able to reach GitHub is not a failed `init`.
        Err(e) => output::print_info(&format!("Could not check for a newer release ({e}).")),
    }
    true
}

/// Every install channel, in one place so they cannot drift apart.
fn print_upgrade_commands() {
    println!("  Upgrade with whichever channel you installed from:");
    println!("    cargo binstall dev-prune --force");
    println!("    cargo install dev-prune --force");
    println!("    npm install -g dev-prune@latest");
    println!("    uv tool upgrade dev-prune  /  pipx upgrade dev-prune");
    println!("    curl -fsSL {} | sh", constants::INSTALL_SH_URL);
    println!("    iwr -useb {} | iex", constants::INSTALL_PS1_URL);
}

/// The package manager that owns the running binary — the one whose upgrade command
/// `--install` runs. One channel owns one binary: a copy installed through uv is
/// upgraded through uv, never through npm, because two managers writing the same PATH
/// entry would fight over it forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    /// `install.sh` / `install.ps1` put it under the managed `<config>/bin`.
    Installer,
    /// `cargo install` / `cargo binstall` put it under `~/.cargo/bin`.
    Cargo,
    /// `npm install -g` — the binary lives under a `node_modules` tree.
    Npm,
    /// `uv tool install` — under uv's tool environments.
    UvTool,
    /// `pipx install` — under a `pipx` venv.
    Pipx,
    /// Anywhere else: a dev build, a hand-copied binary, a distro package.
    Unknown,
}

/// Classify where `exe` came from by the directories in its path.
///
/// Purely lexical on purpose: this must not touch the network or spawn anything, and
/// each channel's layout is stable enough that its marker directory is a reliable
/// fingerprint. `managed` is passed in (rather than resolved here) so tests can probe
/// the classification without a config directory on disk.
fn detect_channel(exe: &std::path::Path, managed: Option<&std::path::Path>) -> Channel {
    if let Some(managed) = managed
        && exe == managed
    {
        return Channel::Installer;
    }
    let has_dir = |name: &str| {
        exe.components()
            .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(name))
    };
    if has_dir(".cargo") {
        Channel::Cargo
    } else if has_dir("node_modules") {
        Channel::Npm
    } else if has_dir("uv") || has_dir("uv-tool") {
        Channel::UvTool
    } else if has_dir("pipx") {
        Channel::Pipx
    } else {
        Channel::Unknown
    }
}

/// `devp update --install`: upgrade this binary through the channel that installed it.
fn run_install() -> Result<()> {
    output::print_header("dev-prune self-update");

    if crate::setup::offline_requested() {
        anyhow::bail!(
            "{} is set — an install needs the network by definition.",
            constants::ENV_OFFLINE
        );
    }

    // Know before downloading whether there is anything to download. A failed check is
    // fatal here (unlike `devp update`): running an installer blind would "upgrade" to
    // the version already installed.
    let mut registry = Registry::load()?;
    let latest = refresh_latest(&mut registry)?;
    let _ = registry.save();
    if compare_versions(constants::VERSION, &latest) != Some(Ordering::Less) {
        output::print_success(&format!(
            "v{} is already the latest release — nothing to install.",
            constants::VERSION
        ));
        return Ok(());
    }
    output::print_info(&format!("Upgrading v{} -> v{latest} …", constants::VERSION));

    let exe = std::env::current_exe().context("could not locate the running binary")?;
    let managed = crate::setup::managed_exe_path().ok();
    let channel = detect_channel(&exe, managed.as_deref());

    // On Windows a running executable's file is locked against replacement but not
    // against rename. Moving it aside first lets the channel write a fresh file at the
    // real path; the `.old` left behind is swept up by the *next* run, when nothing is
    // executing it any more.
    #[cfg(windows)]
    let aside = {
        let aside = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&aside);
        std::fs::rename(&exe, &aside).ok().map(|_| aside)
    };

    let result = spawn_channel_upgrade(channel);

    #[cfg(windows)]
    if let Some(aside) = aside {
        if result.is_ok() {
            // Best effort: the file is still our running image, so Windows may refuse
            // the delete. The sweep at the top of the next `--install` gets it then.
            let _ = std::fs::remove_file(&aside);
        } else if !exe.exists() {
            // The upgrade never wrote a new binary — put the old one back so the
            // command the user has on PATH still exists.
            let _ = std::fs::rename(&aside, &exe);
        }
    }
    result?;

    output::print_success(&format!("dev-prune v{latest} installed."));
    output::print_info(
        "The scheduled pass was not interrupted: it runs the managed copy, which \
         refreshes itself from the new binary on its next run.",
    );
    Ok(())
}

/// Run one channel's own upgrade command, wired to the terminal so its progress and
/// prompts reach the user directly.
fn spawn_channel_upgrade(channel: Channel) -> Result<()> {
    let install_ps1 = format!("iwr -useb {} | iex", constants::INSTALL_PS1_URL);
    let install_sh = format!("curl -fsSL {} | sh", constants::INSTALL_SH_URL);
    let argv: Vec<&str> = match channel {
        Channel::Cargo => {
            // binstall pulls the prebuilt release; plain `cargo install` compiles for
            // minutes. Prefer the fast one when it exists.
            if crate::adapters::binary_available("cargo-binstall") {
                vec!["cargo", "binstall", "dev-prune", "--force", "-y"]
            } else {
                vec!["cargo", "install", "dev-prune", "--force"]
            }
        }
        Channel::Npm => vec!["npm", "install", "-g", "dev-prune@latest"],
        Channel::UvTool => vec!["uv", "tool", "upgrade", "dev-prune"],
        Channel::Pipx => vec!["pipx", "upgrade", "dev-prune"],
        Channel::Installer => {
            if cfg!(windows) {
                vec!["powershell", "-NoProfile", "-Command", &install_ps1]
            } else {
                vec!["sh", "-c", &install_sh]
            }
        }
        Channel::Unknown => {
            output::print_warning(
                "Could not tell which channel installed this binary, so nothing was \
                 changed. Upgrade it yourself with one of:",
            );
            print_upgrade_commands();
            anyhow::bail!("unrecognised install channel");
        }
    };

    output::print_info(&format!("Running: {}", argv.join(" ")));
    let status = crate::spawn::command(crate::adapters::resolve_program(argv[0]))
        .args(&argv[1..])
        .status()
        .with_context(|| format!("could not start `{}`", argv[0]))?;
    if !status.success() {
        anyhow::bail!("`{}` exited with {status}", argv.join(" "));
    }
    Ok(())
}

/// The end-of-run hook behind `auto_update`: when the setting is on and the last release
/// check already knows a newer version exists, run the self-update without being asked.
///
/// Warn-never-fail, like everything else that runs as a side effect of `devp run` — a
/// broken upgrade path must not turn a successful prune into a failed command.
pub fn maybe_auto_update(registry: &Registry) {
    if !registry.settings.auto_update
        || crate::setup::offline_requested()
        || crate::setup::no_auto_setup_requested()
    {
        return;
    }
    let Some(latest) = registry.latest_known_version.as_deref() else {
        return;
    };
    if compare_versions(constants::VERSION, latest) != Some(Ordering::Less) {
        return;
    }
    println!();
    if let Err(e) = run_install() {
        output::print_warning(&format!(
            "Automatic update failed ({e}). Run `devp update --install` yourself, or \
             `devp config set auto_update false` to stop trying."
        ));
    }
}

/// Quietly keep the release check current and print a one-line notice when the installed
/// build is behind. Returns `true` when the registry changed and needs saving.
///
/// Called from `devp run` and `devp status`. Never returns an error: a background
/// convenience must not be able to fail the command the user actually asked for.
pub fn notify_if_outdated(registry: &mut Registry) -> bool {
    if !registry.settings.update_check {
        return false;
    }

    let interval = registry.settings.update_check_interval_days;
    let due = registry
        .last_update_check
        .is_none_or(|last| Utc::now().signed_duration_since(last).num_days() >= interval);

    if due {
        // The result is deliberately ignored: `refresh_latest` moves the timestamp even
        // when the request fails, and retrying on every command while the machine is
        // offline would put a five-second stall in front of everyday work.
        let _ = refresh_latest(registry);
    }

    if let Some(latest) = registry.latest_known_version.as_deref()
        && compare_versions(constants::VERSION, latest) == Some(Ordering::Less)
    {
        output::print_info(&format!(
            "dev-prune v{latest} is out (you have v{}). `devp update` has the commands; \
                 `devp config set update_check false` silences this.",
            constants::VERSION
        ));
    }

    due
}

/// Ask GitHub for the latest release and record the answer on the registry.
///
/// The caller is responsible for saving; that keeps this usable from both the
/// already-loaded-registry path and the standalone command.
fn refresh_latest(registry: &mut Registry) -> Result<String> {
    let result = latest_release(registry.settings.update_check_timeout_secs);
    registry.last_update_check = Some(Utc::now());
    let latest = result?;
    registry.latest_known_version = Some(latest.clone());
    Ok(latest)
}

/// Say whether the installed build is behind, current, or ahead of the latest release.
fn report_comparison(latest: &str) {
    let installed = constants::VERSION;
    match compare_versions(installed, latest) {
        Some(Ordering::Less) => {
            output::print_warning(&format!(
                "Latest release:    v{latest} — an upgrade is available."
            ));
        }
        Some(Ordering::Equal) => {
            output::print_success(&format!(
                "Latest release:    v{latest} — you are up to date."
            ));
        }
        Some(Ordering::Greater) => {
            // Normal when running a local build between releases.
            output::print_info(&format!(
                "Latest release:    v{latest} — your build is newer than the last published one."
            ));
        }
        None => {
            output::print_info(&format!(
                "Latest release:    v{latest} (could not compare it to v{installed})."
            ));
        }
    }
}

/// Fetch the tag name of the most recent published release.
///
/// Returns the version without any leading `v`, so it can be compared to
/// `CARGO_PKG_VERSION` directly.
fn latest_release(timeout_secs: u64) -> Result<String> {
    if crate::setup::offline_requested() {
        anyhow::bail!("{} is set", constants::ENV_OFFLINE);
    }
    let body = ureq::get(constants::LATEST_RELEASE_API_URL)
        .header("User-Agent", &format!("dev-prune/{}", constants::VERSION))
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(Duration::from_secs(timeout_secs.max(1))))
        .build()
        .call()
        .context("request failed")?
        .body_mut()
        .read_to_string()
        .context("could not read the response")?;

    let json: serde_json::Value =
        serde_json::from_str(&body).context("the response was not JSON")?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("the response carried no tag_name")?;

    Ok(tag.trim_start_matches('v').to_string())
}

/// Compare two dotted numeric versions, ignoring any pre-release suffix.
///
/// Returns `None` when either side is not `major.minor.patch` — better to say "could not
/// compare" than to claim an upgrade exists because `1.0.0` sorts before `1.0.0-rc.1`
/// as a string.
fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let parse = |v: &str| -> Option<[u64; 3]> {
        let core = v.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let out = [
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ];
        // A fourth component means this is not the scheme we release under.
        if parts.next().is_some() {
            return None;
        }
        Some(out)
    };
    Some(parse(a)?.cmp(&parse(b)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn orders_by_component_not_lexically() {
        // "1.10.0" < "1.9.0" as strings, which is the bug this function exists to avoid.
        assert_eq!(compare_versions("1.9.0", "1.10.0"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Some(Ordering::Equal));
        assert_eq!(
            compare_versions("2.0.0", "1.99.99"),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn pre_release_suffixes_compare_by_their_core() {
        assert_eq!(
            compare_versions("1.0.0", "1.0.0-rc.1"),
            Some(Ordering::Equal)
        );
        assert_eq!(
            compare_versions("1.0.0+build7", "1.0.1"),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn unparseable_versions_report_no_answer_rather_than_a_wrong_one() {
        assert_eq!(compare_versions("1.0", "1.0.0"), None);
        assert_eq!(compare_versions("1.0.0.1", "1.0.0"), None);
        assert_eq!(compare_versions("nightly", "1.0.0"), None);
    }

    #[test]
    fn the_check_is_on_unless_the_user_turns_it_off() {
        assert!(Registry::default().settings.update_check);
    }

    #[test]
    fn a_disabled_check_touches_neither_the_network_nor_the_registry() {
        let mut registry = Registry::default();
        registry.settings.update_check = false;
        assert!(!notify_if_outdated(&mut registry));
        assert!(registry.last_update_check.is_none());
    }

    #[test]
    fn each_channel_is_recognised_by_its_marker_directory() {
        use std::path::Path;
        let cases: &[(&str, Channel)] = &[
            ("/home/k/.cargo/bin/dev-prune", Channel::Cargo),
            (
                "/usr/lib/node_modules/dev-prune/bin/dev-prune",
                Channel::Npm,
            ),
            (
                "/home/k/.local/share/uv/tools/dev-prune/bin/dev-prune",
                Channel::UvTool,
            ),
            (
                "/home/k/.local/pipx/venvs/dev-prune/bin/dev-prune",
                Channel::Pipx,
            ),
            ("/opt/somewhere/dev-prune", Channel::Unknown),
        ];
        for (path, expected) in cases {
            assert_eq!(detect_channel(Path::new(path), None), *expected, "{path}");
        }
    }

    #[test]
    fn the_managed_copy_wins_over_every_path_heuristic() {
        use std::path::Path;
        // Even a managed dir that happens to live under `.cargo` is the installer's.
        let managed = Path::new("/home/k/.cargo/odd/dev-prune/bin/dev-prune");
        assert_eq!(detect_channel(managed, Some(managed)), Channel::Installer);
    }

    #[test]
    fn auto_update_is_off_by_default_and_silent_when_off() {
        let registry = Registry::default();
        assert!(!registry.settings.auto_update);
        // Must return without touching the network or the terminal.
        maybe_auto_update(&registry);
    }

    #[test]
    fn a_recent_check_is_not_repeated() {
        let mut registry = Registry::default();
        let stamp = Utc::now() - ChronoDuration::days(constants::UPDATE_CHECK_INTERVAL_DAYS - 1);
        registry.last_update_check = Some(stamp);
        // No network call, so the stamp survives untouched and nothing needs saving.
        assert!(!notify_if_outdated(&mut registry));
        assert_eq!(registry.last_update_check, Some(stamp));
    }
}
