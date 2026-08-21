// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `devp man`.
//
// The pages are rendered from the same clap definition the binary parses arguments
// with — the same `long_about` texts `--help` prints — so a flag cannot exist in the
// manual and be missing from the program, or the other way round. That is the whole
// reason this is a subcommand rather than checked-in roff files that go stale.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_mangen::Man;

use crate::Cli;
use crate::output;

/// Print the main page to stdout, or with `--dir` write the full set: `devp.1`,
/// `dev-prune.1` (the same page under the binary's other name) and one
/// `devp-<command>.1` per subcommand.
pub fn run(dir: Option<&str>) -> Result<()> {
    let mut command = Cli::command().name("devp");
    command.build();

    let Some(dir) = dir else {
        // Piped, like `completions`: `devp man | man -l -` must see roff and nothing
        // else, so no header and no attribution line.
        let mut out = Vec::new();
        Man::new(command).render(&mut out)?;
        use std::io::Write;
        std::io::stdout().write_all(&out)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_full_set_covers_every_subcommand() {
        let tmp = tempfile::tempdir().unwrap();
        run(Some(tmp.path().to_str().unwrap())).unwrap();

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
        run(Some(tmp.path().to_str().unwrap())).unwrap();
        let run_page = fs::read_to_string(tmp.path().join("devp-run.1")).unwrap();
        // A phrase from help::RUN_LONG — the proof the manual and --help are one text.
        assert!(run_page.contains("gauntlet"), "{run_page}");
    }
}
