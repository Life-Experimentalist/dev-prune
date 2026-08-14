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
// The command deliberately does not download or install anything. Replacing a running
// binary is the package manager's job, and doing it ourselves would mean writing to a
// PATH directory with whatever privileges the user happened to have.

use std::cmp::Ordering;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::config::Registry;
use crate::constants;
use crate::output;

pub fn run(offline: bool) -> Result<()> {
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
    println!("    curl -fsSL https://devprune.vkrishna04.me/install.sh | sh");
    println!("    iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex");
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

    if let Some(latest) = registry.latest_known_version.as_deref() {
        if compare_versions(constants::VERSION, latest) == Some(Ordering::Less) {
            output::print_info(&format!(
                "dev-prune v{latest} is out (you have v{}). `devp update` has the commands; \
                 `devp config set update_check false` silences this.",
                constants::VERSION
            ));
        }
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
    fn a_recent_check_is_not_repeated() {
        let mut registry = Registry::default();
        let stamp = Utc::now() - ChronoDuration::days(constants::UPDATE_CHECK_INTERVAL_DAYS - 1);
        registry.last_update_check = Some(stamp);
        // No network call, so the stamp survives untouched and nothing needs saving.
        assert!(!notify_if_outdated(&mut registry));
        assert_eq!(registry.last_update_check, Some(stamp));
    }
}
