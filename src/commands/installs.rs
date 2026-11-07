// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// `devp installs`: every devp on this machine, and which one each caller runs.
//
// Read-only on purpose. It never takes the run lock (it changes nothing a prune could
// race with), never writes, and the only processes it starts are the `--version` queries
// inside `installs::discover`. Removing anything is `devp installs clean`, a separate
// subcommand that asks first.

use anyhow::Result;

use crate::constants;
use crate::installs::{self, Discovery, Install, Roles, Version};
use crate::output;

/// `devp installs [--json]`.
pub fn run(json: bool) -> Result<()> {
    let found = installs::discover_machine();
    if json {
        return crate::json::emit(&crate::json::installs_document(&found));
    }
    print_report(&found);
    Ok(())
}

/// `1.23.0`, or `unknown` for a copy that could not say.
pub fn version_text(version: Option<Version>) -> String {
    match version {
        Some((major, minor, patch)) => format!("{major}.{minor}.{patch}"),
        None => "unknown".to_string(),
    }
}

/// The roles of one install, as words for the table.
pub fn role_names(roles: &Roles) -> Vec<&'static str> {
    let mut names = Vec::new();
    if roles.shell {
        names.push("new shells");
    }
    if roles.scheduler {
        names.push("scheduler");
    }
    if roles.hook {
        names.push("git hooks");
    }
    if roles.running {
        names.push("this command");
    }
    names
}

fn print_report(found: &Discovery) {
    output::print_header("Copies of devp on this machine");
    println!();
    println!(
        "  {}{}{}{}{}",
        output::pad_display("Version", 10),
        output::pad_display("Installed with", 16),
        output::pad_display("On PATH", 10),
        output::pad_display("Used by", 34),
        "Location"
    );
    for install in &found.installs {
        print_row(install);
    }

    // A target that matches no copy found above is worth a line of its own: the usual
    // cause is a scheduler or hook pointing at a binary that has since been removed.
    if let Some(target) = &found.scheduler
        && !found.installs.iter().any(|i| i.roles.scheduler)
    {
        output::print_info(&format!(
            "The scheduler runs {}, which is not among the copies above.",
            output::clean_path(target)
        ));
    }
    if let Some(target) = &found.hook
        && !found.installs.iter().any(|i| i.roles.hook)
    {
        output::print_info(&format!(
            "The git hooks call {}, which is not among the copies above.",
            output::clean_path(target)
        ));
    }

    println!();
    output::print_dimmed(constants::INSTALLS_SEARCH_LIMIT_NOTE);
    if let Some(sentence) = verdict(found) {
        println!();
        output::print_wrapped("", &sentence);
    }
}

fn print_row(install: &Install) {
    let version = if install.windowless_only {
        "-".to_string()
    } else {
        version_text(install.version)
    };
    let on_path = match install.path_position {
        Some(position) => format!("yes, #{position}"),
        None => "no".to_string(),
    };
    let mut used_by = role_names(&install.roles).join(", ");
    if install.windowless_only {
        used_by = "scheduler launcher only".to_string();
    }
    println!(
        "  {}{}{}{}{}",
        output::pad_display(&version, 10),
        output::pad_display(install.channel.badge(), 16),
        output::pad_display(&on_path, 10),
        output::pad_display(&used_by, 34),
        output::clean_path(&install.path)
    );
    // Shown only when it differs, so a symlinked install (Homebrew, uv) says where the
    // real file is without doubling every other row.
    let real = output::clean_path(&install.canonical);
    if real != output::clean_path(&install.path) {
        output::print_dimmed(&format!("    (a link to {real})"));
    }
}

/// One plain sentence about what is off, or `None` when the machine is in order.
///
/// Three things count: more than one copy, a new shell running a different copy than the
/// scheduler or the git hooks do, and one copy hiding another on PATH. The owner's rule is
/// one devp per device, so a second copy is a finding even when both are the same version.
pub fn verdict(found: &Discovery) -> Option<String> {
    let real: Vec<&Install> = found
        .installs
        .iter()
        .filter(|i| !i.windowless_only)
        .collect();
    let mut clauses: Vec<String> = Vec::new();

    if real.len() > 1 {
        let mut versions: Vec<Version> = real.iter().filter_map(|i| i.version).collect();
        versions.sort_unstable();
        versions.dedup();
        let names: Vec<String> = versions.iter().map(|v| version_text(Some(*v))).collect();
        if versions.len() > 1 {
            clauses.push(format!(
                "{} copies are installed at different versions ({})",
                real.len(),
                names.join(", ")
            ));
        } else {
            clauses.push(format!("{} copies of devp are installed", real.len()));
        }
    }

    let shell = real.iter().find(|i| i.roles.shell).copied();
    if let Some(shell) = shell {
        for (who, other) in [
            (
                "the scheduler runs",
                real.iter().find(|i| i.roles.scheduler),
            ),
            ("the git hooks call", real.iter().find(|i| i.roles.hook)),
        ] {
            if let Some(other) = other
                && !std::ptr::eq(*other, shell)
            {
                clauses.push(format!(
                    "a new shell runs {} but {who} {}",
                    describe(shell),
                    describe(other)
                ));
            }
        }
        let on_path = real.iter().filter(|i| i.on_path()).count();
        if on_path > 1 {
            let hidden = if on_path == 2 {
                "the other copy".to_string()
            } else {
                format!("the other {} copies", on_path - 1)
            };
            clauses.push(format!(
                "the copy first on PATH ({}) hides {hidden} on PATH",
                output::clean_path(&shell.path)
            ));
        }
    }

    if clauses.is_empty() {
        return None;
    }
    let mut sentence = clauses.join("; ");
    if let Some(first) = sentence.get(..1) {
        sentence = format!("{}{}", first.to_uppercase(), &sentence[1..]);
    }
    Some(format!(
        "{sentence}. `devp installs clean` removes the extras one confirmed step at a time."
    ))
}

fn describe(install: &Install) -> String {
    format!(
        "version {} at {}",
        version_text(install.version),
        output::clean_path(&install.path)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::Channel;
    use std::path::PathBuf;

    fn install(path: &str, version: Option<Version>, position: Option<usize>) -> Install {
        Install {
            path: PathBuf::from(path),
            canonical: PathBuf::from(path),
            channel: Channel::Cargo,
            version,
            path_position: position,
            roles: Roles::default(),
            windowless_only: false,
        }
    }

    fn found(installs: Vec<Install>) -> Discovery {
        Discovery {
            installs,
            scheduler: None,
            hook: None,
        }
    }

    #[test]
    fn a_lone_copy_is_in_order() {
        let mut a = install("/a/devp", Some((1, 23, 0)), Some(1));
        a.roles.shell = true;
        a.roles.scheduler = true;
        assert_eq!(verdict(&found(vec![a])), None);
    }

    #[test]
    fn two_versions_are_named_in_the_sentence() {
        let a = install("/a/devp", Some((1, 23, 0)), None);
        let b = install("/b/devp", Some((1, 22, 0)), None);
        let text = verdict(&found(vec![a, b])).unwrap();
        assert!(
            text.contains("different versions (1.22.0, 1.23.0)"),
            "{text}"
        );
        assert!(text.contains("`devp installs clean`"), "{text}");
    }

    #[test]
    fn two_copies_of_one_version_are_still_a_finding() {
        let a = install("/a/devp", Some((1, 23, 0)), None);
        let b = install("/b/devp", Some((1, 23, 0)), None);
        let text = verdict(&found(vec![a, b])).unwrap();
        assert!(text.starts_with("2 copies of devp are installed"), "{text}");
    }

    #[test]
    fn a_shell_and_a_scheduler_on_different_copies_are_called_out() {
        let mut a = install("/a/devp", Some((1, 23, 0)), Some(1));
        a.roles.shell = true;
        let mut b = install("/b/devp", Some((1, 23, 0)), None);
        b.roles.scheduler = true;
        let text = verdict(&found(vec![a, b])).unwrap();
        assert!(
            text.contains("a new shell runs version 1.23.0 at /a/devp but the scheduler runs version 1.23.0 at /b/devp"),
            "{text}"
        );
    }

    #[test]
    fn two_copies_on_path_say_which_one_hides_the_other() {
        let mut a = install("/a/devp", Some((1, 23, 0)), Some(1));
        a.roles.shell = true;
        let b = install("/b/devp", Some((1, 23, 0)), Some(2));
        let text = verdict(&found(vec![a, b])).unwrap();
        assert!(
            text.contains("the copy first on PATH (/a/devp) hides the other copy on PATH"),
            "{text}"
        );
    }

    #[test]
    fn the_windowless_launcher_is_not_a_second_copy() {
        let mut a = install("/a/devp", Some((1, 23, 0)), Some(1));
        a.roles.shell = true;
        let mut w = install("/a/devpw", None, Some(1));
        w.windowless_only = true;
        assert_eq!(verdict(&found(vec![a, w])), None);
    }

    #[test]
    fn a_copy_without_a_version_reads_unknown() {
        assert_eq!(version_text(None), "unknown");
        assert_eq!(version_text(Some((1, 2, 3))), "1.2.3");
    }

    #[test]
    fn roles_read_as_words() {
        let roles = Roles {
            shell: true,
            scheduler: true,
            hook: false,
            running: true,
        };
        assert_eq!(
            role_names(&roles),
            vec!["new shells", "scheduler", "this command"]
        );
    }
}
