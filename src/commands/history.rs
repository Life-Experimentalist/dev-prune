// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune history`.
//
// `devp stats` says 27 GiB across 10 passes. This says which passes, what each one was
// asked to do, and what it took — the question anyone asks straight after reading the
// total, and the one the tool could not answer until 1.17.0 because only the totals were
// ever kept.
//
// The output is deliberately two commands rather than one long one. A pass that cleared
// forty repositories has hundreds of directories in it, and a report that prints them all
// by default is a report nobody reads: the list is one line per pass, and the detail is
// asked for by number. `--export` exists for the case where the answer genuinely is
// "all of it" — a file is a better place for that than a scrollback buffer.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;

use crate::config::{PrunedDir, Registry};
use crate::constants::PRUNE_LOG_STARTS_AT;
use crate::history::{self, Pass};
use crate::output;

/// How many passes the compact list shows without `--limit` or `--all`.
const PASSES_SHOWN: usize = 20;

/// How many directories one pass's detail prints to a terminal before it stops.
///
/// Only to a terminal. A redirect or a pipe has somewhere to put the rest, and silently
/// truncating what someone asked to be written to a file would be the worse failure.
const DETAIL_DIRS_SHOWN: usize = 200;

/// Options for the `history` command, from the CLI.
pub struct HistoryArgs {
    /// Show one pass in full. 1 is the most recent.
    pub pass: Option<usize>,
    /// How many passes the list shows.
    pub limit: Option<usize>,
    /// Show every recorded pass.
    pub all: bool,
    /// Emit the whole log as one JSON document.
    pub json: bool,
    /// Write the JSON document to a file. `Some(None)` means the default location.
    pub export: Option<Option<PathBuf>>,
}

/// Run the `history` command.
pub fn run(args: &HistoryArgs) -> Result<()> {
    let registry = Registry::load()?;
    let passes = history::merged(history::load()?, &registry);

    // Checked once, before anything branches on the output mode: `--pass 40 --json` on
    // a machine with ten passes is the same question with no answer as `--pass 40`, and
    // an empty `passes` array would read as "that pass deleted nothing".
    if let Some(n) = args.pass {
        check_pass_number(&passes, n)?;
    }

    if let Some(destination) = &args.export {
        return export(&passes, args.pass, destination.as_deref());
    }

    if args.json {
        return crate::json::emit(&crate::json::history_document(&passes, args.pass));
    }

    match args.pass {
        Some(n) => print_one_pass(&passes, n),
        None => {
            print_pass_list(&passes, args);
            Ok(())
        }
    }
}

/// Exit 2 for a pass number nobody has.
fn check_pass_number(passes: &[Pass], number: usize) -> Result<()> {
    if number == 0 || number > passes.len() {
        return Err(anyhow::Error::new(crate::UsageError(format!(
            "There is no pass #{number}. {} recorded; `devp history` lists them, newest first.",
            passes.len()
        ))));
    }
    Ok(())
}

/// One line per pass, newest first.
fn print_pass_list(passes: &[Pass], args: &HistoryArgs) {
    output::print_header("Prune passes");

    if passes.is_empty() {
        output::print_info(
            "Nothing recorded yet — `devp run --dry-run` shows what a pass would do.",
        );
        return;
    }

    let shown = if args.all {
        passes.len()
    } else {
        args.limit.unwrap_or(PASSES_SHOWN).min(passes.len())
    };

    for (index, pass) in passes.iter().take(shown).enumerate() {
        let number = index + 1;
        let (started_by, note) = match pass {
            Pass::Detailed(record) => (record.trigger.label().to_string(), String::new()),
            // Not "unknown": the pass is not mysterious, the format was younger than it.
            Pass::Summary { .. } => ("—".to_string(), " (totals only)".to_string()),
        };
        // Padded before colouring: a width applied to a string carrying ANSI escapes
        // counts the escapes as width and the column drifts.
        println!(
            "  {}  {}   {}   {:<10} {} {}, {} {}{}",
            format!("#{number}").dimmed(),
            pass.at().format("%Y-%m-%d %H:%M"),
            format!("{:>10}", output::format_bytes(pass.bytes_freed())).green(),
            started_by,
            pass.dirs_removed(),
            output::plural(pass.dirs_removed(), "directory", "directories"),
            pass.repos_touched(),
            output::plural(pass.repos_touched(), "repository", "repositories"),
            note.dimmed(),
        );
    }

    if shown < passes.len() {
        output::print_info(&format!(
            "{} passes recorded; showing the most recent {shown}. `--all` for every one.",
            passes.len()
        ));
    }

    if passes.iter().any(|p| matches!(p, Pass::Summary { .. })) {
        output::print_dimmed(&format!(
            "  Passes marked \"totals only\" ran before {PRUNE_LOG_STARTS_AT}, which is where \
             the per-directory log starts."
        ));
    }

    output::print_info("What one pass deleted:  devp history --pass 1");
    output::print_info("All of it, as a file:   devp history --export");
}

/// One pass in full.
fn print_one_pass(passes: &[Pass], number: usize) -> Result<()> {
    check_pass_number(passes, number)?;
    let pass = &passes[number - 1];

    output::print_header(&format!("Pass #{number}"));
    output::print_info(&format!(
        "When         {} ({})",
        pass.at().format("%Y-%m-%d %H:%M UTC"),
        describe_age(pass.at()),
    ));

    match pass {
        Pass::Detailed(record) => {
            output::print_info(&format!("Started by   {}", record.trigger.label()));
            output::print_info(&format!("Command      {}", record.command_line()));
            if !record.version.is_empty() {
                output::print_info(&format!("Version      dev-prune {}", record.version));
            }
        }
        Pass::Summary { .. } => {
            output::print_info(&format!(
                "Started by   not recorded — this pass predates {PRUNE_LOG_STARTS_AT}"
            ));
        }
    }

    output::print_info(&format!(
        "Freed        {} from {} {} in {} {}",
        output::format_bytes_styled(pass.bytes_freed()),
        pass.dirs_removed(),
        output::plural(pass.dirs_removed(), "directory", "directories"),
        pass.repos_touched(),
        output::plural(pass.repos_touched(), "repository", "repositories"),
    ));

    let Some(dirs) = pass.dirs() else {
        output::print_header("Directories");
        output::print_info(&format!(
            "Not recorded. Only the totals above were kept before {PRUNE_LOG_STARTS_AT}; every \
             pass from that release on carries its full list."
        ));
        return Ok(());
    };

    print_directories(dirs, number);
    Ok(())
}

/// The directory list, grouped under the repository each one belonged to.
fn print_directories(dirs: &[PrunedDir], number: usize) {
    use std::io::IsTerminal;

    output::print_header("Directories");

    let mut grouped: Vec<(&PathBuf, Vec<&PrunedDir>)> = Vec::new();
    for dir in dirs {
        match grouped.iter_mut().find(|(repo, _)| *repo == &dir.repo_path) {
            Some((_, entries)) => entries.push(dir),
            None => grouped.push((&dir.repo_path, vec![dir])),
        }
    }
    grouped.sort_by_key(|(_, entries)| {
        std::cmp::Reverse(entries.iter().map(|d| d.size_freed).sum::<u64>())
    });

    // Only a terminal has a scrollback to overflow. Redirected or piped, the caller has
    // asked for the whole thing and has somewhere to put it.
    let budget = if std::io::stdout().is_terminal() {
        DETAIL_DIRS_SHOWN
    } else {
        usize::MAX
    };
    let mut printed = 0usize;

    for (repo, entries) in &grouped {
        if printed >= budget {
            break;
        }
        println!("  {}", output::styled_path(repo));
        for dir in entries {
            if printed >= budget {
                break;
            }
            println!(
                "    {}   {}   {}",
                format!("{:>10}", output::format_bytes(dir.size_freed)).green(),
                output::pad_display(&dir.bloat_dir, 32),
                output::styled_adapter(&dir.adapter),
            );
            printed += 1;
        }
    }

    if printed < dirs.len() {
        output::print_info(&format!(
            "{} more not shown. `devp history --pass {number} --json` or `devp history --export` \
             has all {}.",
            dirs.len() - printed,
            dirs.len(),
        ));
    }
}

/// Write the whole log to a file, and say where it went.
fn export(passes: &[Pass], only: Option<usize>, destination: Option<&Path>) -> Result<()> {
    let path = resolve_export_path(destination)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let document = crate::json::history_document(passes, only);
    let contents =
        serde_json::to_string_pretty(&document).context("Failed to serialize history")?;
    std::fs::write(&path, &contents)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    let written = if only.is_some() { 1 } else { passes.len() };
    output::print_success(&format!(
        "{written} {} written to {}",
        output::plural(written, "pass", "passes"),
        output::clean_path(&path),
    ));
    Ok(())
}

/// Where `--export` writes.
///
/// A bare `--export` goes to the documents folder, which is the one directory every
/// desktop has and nothing else writes to on its own. An argument that is an existing
/// directory gets the same filename inside it, because `--export .` meaning "overwrite
/// the current directory" is not what anyone types it for.
pub fn resolve_export_path(destination: Option<&Path>) -> Result<PathBuf> {
    let name = format!("dev-prune-history-{}.json", Utc::now().format("%Y-%m-%d"));
    match destination {
        Some(path) if path.is_dir() => Ok(path.join(name)),
        Some(path) => Ok(path.to_path_buf()),
        None => {
            let base = dirs::document_dir()
                .or_else(dirs::home_dir)
                .context("Could not find a documents or home directory to export into. Pass a path: `devp history --export <FILE>`")?;
            Ok(base.join(name))
        }
    }
}

/// "3 days ago", in the coarsest unit that is not a lie.
fn describe_age(at: DateTime<Utc>) -> String {
    let elapsed = Utc::now().signed_duration_since(at);
    let days = elapsed.num_days();
    if days >= 1 {
        return format!(
            "{days} {} ago",
            output::plural(days as usize, "day", "days")
        );
    }
    let hours = elapsed.num_hours();
    if hours >= 1 {
        return format!(
            "{hours} {} ago",
            output::plural(hours as usize, "hour", "hours")
        );
    }
    let minutes = elapsed.num_minutes().max(0);
    format!(
        "{minutes} {} ago",
        output::plural(minutes as usize, "minute", "minutes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{PassRecord, Trigger};
    use tempfile::TempDir;

    fn pass(at: DateTime<Utc>) -> Pass {
        Pass::Detailed(PassRecord {
            at,
            trigger: Trigger::Manual,
            argv: vec!["run".to_string()],
            version: "1.17.0".to_string(),
            dirs: vec![PrunedDir {
                repo_path: PathBuf::from("/a"),
                bloat_dir: "node_modules".to_string(),
                adapter: "npm".to_string(),
                size_freed: 100,
                runtime: None,
            }],
        })
    }

    #[test]
    fn a_pass_number_nobody_has_is_a_usage_error_not_an_empty_report() {
        let passes = vec![pass(Utc::now())];
        for n in [0usize, 2, 99] {
            let err = print_one_pass(&passes, n).unwrap_err();
            assert!(
                err.downcast_ref::<crate::UsageError>().is_some(),
                "--pass {n} should exit 2"
            );
        }
    }

    #[test]
    fn asking_for_a_pass_on_an_empty_log_is_a_usage_error() {
        let err = print_one_pass(&[], 1).unwrap_err();
        assert!(err.downcast_ref::<crate::UsageError>().is_some());
    }

    #[test]
    fn exporting_into_a_directory_keeps_the_generated_filename() {
        // `devp history --export .` must not try to overwrite the directory itself.
        let tmp = TempDir::new().unwrap();
        let path = resolve_export_path(Some(tmp.path())).unwrap();
        assert_eq!(path.parent().unwrap(), tmp.path());
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dev-prune-history-")
        );
    }

    #[test]
    fn exporting_to_a_named_file_uses_exactly_that_name() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("mine.json");
        assert_eq!(resolve_export_path(Some(&target)).unwrap(), target);
    }

    #[test]
    fn an_export_writes_a_document_that_parses() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("nested").join("history.json");
        export(&[pass(Utc::now())], None, Some(&target)).unwrap();
        let raw = std::fs::read_to_string(&target).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["command"], "history");
        assert_eq!(parsed["passes"].as_array().unwrap().len(), 1);
    }
}
