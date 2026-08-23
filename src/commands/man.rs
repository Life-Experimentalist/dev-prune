// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `devp man`.
//
// The pages are rendered from the same clap definition the binary parses arguments
// with — the same `long_about` texts `--help` prints — so a flag cannot exist in the
// manual and be missing from the program, or the other way round. That is the whole
// reason this is a subcommand rather than checked-in roff files that go stale.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_mangen::Man;
use colored::Colorize;

use crate::Cli;
use crate::output;

/// Render the manual: a readable contents page on a terminal, one command's page when
/// a command is named, roff when redirected, and with `--dir` the full set of files —
/// `devp.1`, `dev-prune.1` (the same page under the binary's other name) and one
/// `devp-<command>.1` per subcommand.
pub fn run(command_name: Option<&str>, dir: Option<&str>, roff: bool) -> Result<()> {
    let mut command = Cli::command().name("devp");
    command.build();

    // A named command is a request to read one page, and it is the same page `--dir`
    // would write for it: roff when the output is going somewhere that formats roff,
    // that command's own long help when a person is reading it.
    if let Some(name) = command_name {
        let Some(sub) = command
            .get_subcommands()
            .find(|s| s.get_name() == name || s.get_all_aliases().any(|a| a == name))
            .cloned()
        else {
            let names: Vec<&str> = command
                .get_subcommands()
                .map(|s| s.get_name())
                .filter(|n| *n != "help")
                .collect();
            anyhow::bail!(
                "no such command: `{name}`. Try one of: {}",
                names.join(", ")
            );
        };
        if roff || !std::io::stdout().is_terminal() {
            let page = format!("devp-{name}");
            let mut out = Vec::new();
            Man::new(sub.name(page.leak() as &str)).render(&mut out)?;
            std::io::stdout().write_all(&out)?;
            return Ok(());
        }
        let mut sub = sub;
        sub.print_long_help()?;
        return Ok(());
    }

    let Some(dir) = dir else {
        // Someone who ran `devp man` at a prompt used to get raw troff — `.TH`,
        // `\fB\-\-dry\-run\fR` and the rest — because the output assumed a `man`
        // on the other end of a pipe. On Windows there is no `man` to pipe into at
        // all, so the markup was the whole experience. Redirected output still gets
        // roff, so `devp man > devp.1` and `devp man | man -l -` are unchanged.
        if roff || !std::io::stdout().is_terminal() {
            let mut out = Vec::new();
            Man::new(command).render(&mut out)?;
            std::io::stdout().write_all(&out)?;
            return Ok(());
        }

        // A contents page rather than the top-level long help. The long help is one
        // screen-and-a-half of prose followed by every subcommand's one-liner, which
        // is a reasonable answer to `devp --help` and a poor answer to "show me the
        // manual": nothing on it tells the reader where they are or how to get to the
        // page they actually want. This says both, in that order.
        print_contents();
        return Ok(());
    };

    let dir = Path::new(dir);
    fs::create_dir_all(dir)
        .with_context(|| format!("could not create {}", output::clean_path(dir)))?;

    let mut written = 0usize;
    let mut render_to = |name: &str, man: Man| -> Result<()> {
        let path = dir.join(format!("{name}.1"));
        let mut buf = Vec::new();
        man.render(&mut buf)?;
        fs::write(&path, buf)
            .with_context(|| format!("could not write {}", output::clean_path(&path)))?;
        written += 1;
        Ok(())
    };

    for sub in command.get_subcommands() {
        // `help` documents itself; a `devp-help.1` would be a page about a page.
        if sub.get_name() == "help" {
            continue;
        }
        let name = format!("devp-{}", sub.get_name());
        // clap's `Str` only converts from `&'static str` without its "string" feature;
        // leaking a dozen page names in a process about to exit is the honest trade.
        render_to(
            &name,
            Man::new(sub.clone().name(name.clone().leak() as &str)),
        )?;
    }
    render_to("devp", Man::new(command.clone()))?;
    // The same executable answers to both names, and `man dev-prune` should work for
    // the person who never learned the short one.
    render_to("dev-prune", Man::new(command.clone().name("dev-prune")))?;

    output::print_success(&format!(
        "{written} man pages written to {}",
        output::clean_path(dir)
    ));
    output::print_info(
        "Install them by copying into a directory on `manpath`, e.g. `/usr/local/share/man/man1/`.",
    );
    Ok(())
}

/// How the contents page groups the commands, and the one line each gets.
///
/// The lines are written here rather than taken from clap's `about`, which is phrased
/// to sit in a `--help` listing and truncates into nonsense at this width ("Export
/// SKILL", "View system dashboard"). A test checks this table against the real command
/// list, so a command added without a line here fails the build rather than going
/// missing from the only page a reader navigates from.
const CONTENTS_GROUPS: [(&str, &[(&str, &str)]); 5] = [
    (
        "Register repositories",
        &[
            (
                "init",
                "find every Git repository under a path, register them",
            ),
            ("link", "register one repository"),
            ("unlink", "forget one — deletes nothing"),
            ("undo", "revert the last init or link"),
        ],
    ),
    (
        "Prune and put back",
        &[
            ("run", "delete what a lockfile proves comes back"),
            ("restore", "reinstall what was deleted"),
        ],
    ),
    (
        "Look at what is going on",
        &[
            ("status", "every repository, its size and its idle days"),
            ("stats", "space reclaimed over time"),
            ("caches", "package manager caches on this machine"),
            ("doctor", "what is broken, and how to fix it"),
            ("trust", "what this program may do on this machine"),
        ],
    ),
    (
        "Settings and integration",
        &[
            ("config", "settings, the scheduler, Git hooks, icons"),
            ("setup", "install whatever integration is missing"),
            ("skill", "rules files for your editor's AI agent"),
            ("completions", "a completion script for your shell"),
            ("man", "this manual"),
        ],
    ),
    (
        "The program itself",
        &[
            ("update", "check for a newer release, and install it"),
            ("install", "move it to another package manager"),
            ("uninstall", "remove it, integration included"),
        ],
    ),
];

/// The manual's contents page: what this is, what every command does in one line each,
/// and the one command that opens any of them.
///
/// Grouped rather than alphabetical. `devp --help` already lists them in definition
/// order, and definition order answers "what exists"; a reader who opens the manual is
/// usually asking "which one do I want", and that is a question about what a command is
/// *for*.
fn print_contents() {
    output::print_header("dev-prune manual");
    println!();
    output::print_wrapped(
        "  ",
        "Every page below is generated from the definitions the binary parses arguments \
         with, so the manual cannot describe a flag the program does not have.",
    );
    println!();
    println!("  {}", "Read one page:".bold());
    println!("    devp man <command>          e.g. `devp man run`, `devp man config`");
    println!("    devp <command> --help       the same text, from the command itself");
    println!();

    for (title, entries) in CONTENTS_GROUPS {
        println!("  {}", title.bold());
        for (name, line) in entries {
            println!("    {:<12}  {line}", name.cyan());
        }
        println!();
    }

    // Named separately because they are the ones that go *before* the subcommand, and
    // that is the mistake everyone makes once.
    println!("  {}", "Flags that go before the command".bold());
    println!("    --dry-run                   simulate, delete nothing");
    println!("    --ignore-idle               prune repositories you are still working in");
    println!("    --yes / -y                  answer yes to confirmations");
    println!();
    println!("  {}", "Exit codes".bold());
    println!("    0 success    1 failure    2 usage error");
    println!();
    output::print_info(
        "`devp man --roff` prints the roff source; `devp man --dir <DIR>` writes the full set of pages.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_full_set_covers_every_subcommand() {
        let tmp = tempfile::tempdir().unwrap();
        run(None, Some(tmp.path().to_str().unwrap()), false).unwrap();

        // One page per visible subcommand, plus the two top-level names.
        let mut command = Cli::command();
        command.build();
        for sub in command.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }
            let page = tmp.path().join(format!("devp-{}.1", sub.get_name()));
            assert!(page.exists(), "missing {}", page.display());
        }
        assert!(tmp.path().join("devp.1").exists());
        assert!(tmp.path().join("dev-prune.1").exists());
    }

    #[test]
    fn a_page_carries_the_long_about_text() {
        let tmp = tempfile::tempdir().unwrap();
        run(None, Some(tmp.path().to_str().unwrap()), false).unwrap();
        let run_page = fs::read_to_string(tmp.path().join("devp-run.1")).unwrap();
        // A phrase from help::RUN_LONG — the proof the manual and --help are one text.
        assert!(run_page.contains("gauntlet"), "{run_page}");
    }

    #[test]
    fn the_contents_page_names_every_command_and_no_others() {
        // The grouping is hand-written, so it is the one part of this file that can
        // drift from the CLI. A command added without a group would be missing from
        // the manual's only navigable page, which is exactly the failure this whole
        // command exists to fix.
        let mut command = Cli::command();
        command.build();
        let real: Vec<&str> = command
            .get_subcommands()
            .map(|s| s.get_name())
            .filter(|n| *n != "help")
            .collect();
        let listed: Vec<&str> = CONTENTS_GROUPS
            .iter()
            .flat_map(|(_, e)| e.iter().map(|(n, _)| *n))
            .collect();

        for name in &real {
            assert!(listed.contains(name), "`{name}` is in no manual group");
        }
        for name in &listed {
            assert!(
                real.contains(name),
                "manual lists `{name}`, which is not a command"
            );
        }
    }

    #[test]
    fn a_named_command_renders_its_own_page() {
        // Not a terminal under `cargo test`, so this is the roff branch — which is
        // the one that has to name the right page.
        let mut command = Cli::command().name("devp");
        command.build();
        let sub = command
            .get_subcommands()
            .find(|s| s.get_name() == "run")
            .cloned()
            .unwrap();
        let mut out = Vec::new();
        Man::new(sub.name("devp-run")).render(&mut out).unwrap();
        let page = String::from_utf8(out).unwrap();
        assert!(page.contains("devp"), "{page}");
        assert!(page.contains("gauntlet"), "{page}");
    }

    #[test]
    fn an_unknown_command_lists_the_real_ones() {
        let err = run(Some("nosuchthing"), None, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no such command"), "{err}");
        assert!(err.contains("run"), "{err}");
    }
}
