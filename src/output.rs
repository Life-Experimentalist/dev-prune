// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Pretty-print helpers for terminal output.
//
// Provides colored, formatted output for CLI commands and terminal spinners.

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Truncate `s` to at most `cells` terminal columns, marking the cut with an ellipsis.
///
/// Returns a string exactly `cells` columns wide whenever it truncates. A wide
/// character straddling the boundary is dropped rather than split, which can leave the
/// result one column short — the trailing space closes that gap, so callers can rely on
/// the width being exact.
pub fn truncate_display(s: &str, cells: usize) -> String {
    if UnicodeWidthStr::width(s) <= cells {
        return s.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    // One column is reserved for the ellipsis itself.
    let budget = cells - 1;
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    used += 1;
    out.extend(std::iter::repeat_n(' ', cells.saturating_sub(used)));
    out
}

/// Left-align `s` in a column exactly `cells` terminal columns wide.
///
/// This is `{:<width$}` corrected for the fact that Rust pads to a count of `char`s and
/// a terminal draws in columns. A CJK or emoji character occupies two of them, so a
/// path whose name is eight Chinese characters measures 8 and draws 16 — and under
/// `{:<35}` every column to its right shifts by eight. Anything wider than the column
/// is truncated rather than allowed to push its neighbours off the edge.
pub fn pad_display(s: &str, cells: usize) -> String {
    let width = UnicodeWidthStr::width(s);
    if width > cells {
        return truncate_display(s, cells);
    }
    let mut out = s.to_string();
    out.extend(std::iter::repeat_n(' ', cells - width));
    out
}

/// Helper to strip Windows UNC `\\?\` prefix, macOS `/private/` prefix, and collapse double slashes.
pub fn clean_path<P: AsRef<Path>>(path: P) -> String {
    let s = path.as_ref().display().to_string();
    // `\\?\UNC\server\share` is the verbatim spelling of `\\server\share` — dropping
    // the whole prefix must put the `\\` back, or the result names a relative path
    // `UNC\server\share` that nothing can open.
    let s = if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{stripped}")
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        s
    };
    let s = if let Some(stripped) = s.strip_prefix("/private/var/") {
        format!("/var/{stripped}")
    } else if let Some(stripped) = s.strip_prefix("/private/tmp/") {
        format!("/tmp/{stripped}")
    } else {
        s
    };
    // Collapse doubled separators left by path joins — but never a leading `//`:
    // `//server/share` names a network share, and `/server/share` does not. A single
    // `replace` also leaves `///` half-collapsed, so loop until settled.
    let (head, tail) = match s.strip_prefix("//") {
        Some(rest) => ("//", rest),
        None => ("", s.as_str()),
    };
    let mut tail = tail.to_string();
    while tail.contains("//") {
        tail = tail.replace("//", "/");
    }
    format!("{head}{tail}")
}

/// Reduce a package manager's failure output to the part that says what went wrong.
///
/// A failing `npm ci` prints its entire usage screen — around a hundred and twenty lines
/// of flags — and dev-prune used to relay every one of them into the middle of a prune
/// report. The three lines that identified the problem were somewhere in there, and the
/// report they were in became unreadable.
///
/// So: if any line looks like a diagnostic, show only those; otherwise show the first
/// few lines, which is where a tool that is not npm usually puts its complaint. The
/// count of what was dropped is always printed, and the line naming a full log file is
/// always kept — the whole point of condensing is that the full text stays reachable.
pub fn condense_tool_output(raw: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if lines.len() <= max_lines {
        return lines.join("\n");
    }

    let is_diagnostic = |l: &&str| {
        let low = l.to_lowercase();
        low.contains("error")
            || low.contains("err!")
            || low.contains("fatal")
            || low.contains("failed")
            || low.contains("cannot")
            || low.contains("unable to")
            || low.contains("not found")
            || low.contains("warn")
    };
    // The log-file pointer is the escape hatch, so it survives even when it is neither a
    // diagnostic nor near the top.
    let is_log_pointer = |l: &&str| l.to_lowercase().contains("log of this run can be found");

    let diagnostics: Vec<&str> = lines.iter().copied().filter(is_diagnostic).collect();
    let mut kept: Vec<&str> = if diagnostics.is_empty() {
        lines.iter().copied().take(max_lines).collect()
    } else {
        diagnostics.into_iter().take(max_lines).collect()
    };
    for line in lines.iter().copied().filter(is_log_pointer) {
        if !kept.contains(&line) {
            kept.push(line);
        }
    }

    let dropped = lines.len().saturating_sub(kept.len());
    let mut out = kept.join("\n");
    if dropped > 0 {
        out.push_str(&format!(
            "\n… {dropped} more {} of output",
            plural(dropped, "line", "lines")
        ));
    }
    out
}

/// Create an animated terminal loading spinner for long-running operations.
pub fn create_spinner(msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .expect("Invalid progress bar template"),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// A determinate progress bar for a pass whose total is known up front.
///
/// A spinner says only "still going". Sizing eighty repositories takes long enough that
/// the difference matters: `41/80` and a bar that visibly moves is the difference
/// between waiting and reaching for Ctrl-C. Use it wherever the count is known before
/// the work starts, and [`create_spinner`] only where it genuinely is not.
///
/// Safe to advance from several threads at once — `indicatif` synchronises internally,
/// which is what lets the parallel status scan report from every worker.
pub fn create_progress_bar(msg: &'static str, total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            // Eighth-block partials, so the bar advances smoothly at one repository per
            // step instead of jumping a whole cell every third one.
            .progress_chars("█▉▊▋▌▍▎▏ ")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg} {bar:28.green/dim} {pos}/{len}  {elapsed}")
            .expect("Invalid progress bar template"),
    );
    pb.set_message(msg);
    // The spinner has to keep turning between updates: a repository holding a multi-
    // gigabyte dependency tree can hold its worker for seconds, and a frozen bar during
    // that is exactly the impression this is here to avoid.
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

const ITEM_WAITING: u8 = 0;
const ITEM_RUNNING: u8 = 1;
const ITEM_DONE: u8 = 2;

/// Which of a pass's items are waiting, running and finished, and since when.
///
/// The thing a live progress line reads. Workers call [`begin`](Self::begin) and
/// [`end`](Self::end) from any thread; the line is drawn from another, so everything here
/// is atomic and nothing blocks.
pub struct LiveTracker {
    epoch: std::time::Instant,
    /// What each item is called, and where it is, for the line that names it.
    items: Vec<(String, String)>,
    state: Vec<std::sync::atomic::AtomicU8>,
    /// When each item began, in milliseconds after `epoch`.
    begun: Vec<std::sync::atomic::AtomicU64>,
}

impl LiveTracker {
    /// One item per `(name, detail)` pair, all waiting.
    pub fn new(items: Vec<(String, String)>) -> Self {
        let n = items.len();
        Self {
            epoch: std::time::Instant::now(),
            items,
            state: (0..n)
                .map(|_| std::sync::atomic::AtomicU8::new(ITEM_WAITING))
                .collect(),
            begun: (0..n).map(|_| Default::default()).collect(),
        }
    }

    pub fn begin(&self, i: usize) {
        use std::sync::atomic::Ordering::{Relaxed, Release};
        self.begun[i].store(self.epoch.elapsed().as_millis() as u64, Relaxed);
        self.state[i].store(ITEM_RUNNING, Release);
    }

    pub fn end(&self, i: usize) {
        self.state[i].store(ITEM_DONE, std::sync::atomic::Ordering::Release);
    }

    fn done(&self) -> usize {
        self.state
            .iter()
            .filter(|s| s.load(std::sync::atomic::Ordering::Acquire) == ITEM_DONE)
            .count()
    }

    /// The item that has been running longest, how long, and how many are running.
    fn oldest_running(&self) -> Option<(usize, Duration, usize)> {
        use std::sync::atomic::Ordering::{Acquire, Relaxed};
        let now = self.epoch.elapsed().as_millis() as u64;
        let mut oldest: Option<(usize, u64)> = None;
        let mut running = 0;
        for (i, s) in self.state.iter().enumerate() {
            if s.load(Acquire) != ITEM_RUNNING {
                continue;
            }
            running += 1;
            let begun = self.begun[i].load(Relaxed);
            if oldest.is_none_or(|(_, b)| begun < b) {
                oldest = Some((i, begun));
            }
        }
        oldest.map(|(i, b)| (i, Duration::from_millis(now.saturating_sub(b)), running))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// A live progress line over a pass whose work is spread across threads.
pub struct LiveProgress<'a> {
    /// What the pass is, as the first words of the line.
    pub title: &'static str,
    /// What the slow-item hint says it is doing: "measuring", "checking".
    pub verb: &'static str,
    pub tracker: &'a LiveTracker,
    /// Files seen so far under item `i`, for the hint, when the work can count them.
    pub files_of: Option<&'a (dyn Fn(usize) -> u64 + Sync)>,
    /// Run before each redraw, for work that reports its progress in its own counters
    /// rather than by calling [`LiveTracker::end`].
    pub refresh: Option<&'a (dyn Fn() + Sync)>,
}

impl LiveProgress<'_> {
    /// Run `work` with the line drawn under it on stderr.
    ///
    /// `enabled` is the caller's existing spinner switch: `--json`, a pipe and a dumb
    /// terminal all arrive here as `false` and get no line, no thread and no cost. The
    /// line is redrawn from a thread of its own, so a worker stuck in one slow file
    /// system call cannot freeze it, which is the whole point of showing it.
    pub fn run<R>(&self, enabled: bool, work: impl FnOnce() -> R) -> R {
        if !enabled {
            return work();
        }
        use std::sync::atomic::{AtomicBool, Ordering};

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.cyan} {msg}")
                .expect("Invalid progress bar template"),
        );
        pb.enable_steady_tick(Duration::from_millis(80));
        let started = std::time::Instant::now();
        let hint_after = Duration::from_secs(crate::constants::SLOW_ITEM_HINT_SECS);
        let stop = AtomicBool::new(false);

        let result = std::thread::scope(|scope| {
            // A refused thread only means the line keeps its last message: the work still
            // runs and the steady tick still turns the spinner.
            let _ = std::thread::Builder::new()
                .name("devp-progress".to_string())
                .spawn_scoped(scope, || {
                    while !stop.load(Ordering::Relaxed) {
                        pb.set_message(self.message(started.elapsed(), hint_after));
                        std::thread::sleep(Duration::from_millis(
                            crate::constants::PROGRESS_POLL_MILLIS,
                        ));
                    }
                });
            let result = work();
            stop.store(true, Ordering::Relaxed);
            result
        });
        pb.finish_and_clear();
        result
    }

    /// The text of the line at this moment. Separate from the drawing so the thresholds
    /// can be exercised without a terminal.
    fn message(&self, elapsed: Duration, hint_after: Duration) -> String {
        if let Some(refresh) = self.refresh {
            refresh();
        }
        let oldest = self.tracker.oldest_running().map(|(i, running, count)| {
            let (name, detail) = &self.tracker.items[i];
            let files = self.files_of.map(|f| f(i));
            (name.as_str(), detail.as_str(), running, count, files)
        });
        live_message(
            self.title,
            self.verb,
            (self.tracker.done(), self.tracker.len()),
            oldest,
            elapsed,
            hint_after,
        )
    }
}

/// Compose the progress text: the counter, what is being worked on, and after
/// `hint_after` a plain line saying the pass is alive and harmless.
///
/// `oldest` is the item that has been running longest: its name, its location, how long,
/// how many items are running in all, and how many files it has shown so far.
fn live_message(
    title: &str,
    verb: &str,
    (done, total): (usize, usize),
    oldest: Option<(&str, &str, Duration, usize, Option<u64>)>,
    elapsed: Duration,
    hint_after: Duration,
) -> String {
    let mut out = format!("{title}  {done}/{total}  {}", format_elapsed(elapsed));
    let Some((name, detail, running_for, running, files)) = oldest else {
        return out;
    };
    out.push_str(&format!(
        "\n  {name}  {}",
        shorten_middle(detail, crate::constants::PROGRESS_PATH_MAX_CHARS)
    ));
    if running > 1 {
        out.push_str(&format!("  (+{} more)", running - 1));
    }
    if running_for >= hint_after {
        // Spelled out because the person reading it is deciding whether to kill the
        // window, and the two facts that decide it are that nothing is changing and that
        // stopping now loses nothing.
        let progress = files.map_or(String::new(), |n| {
            format!(", {} files so far", format_count_short(n))
        });
        out.push_str(&format!(
            "\n  still {verb} {name}{progress}, {} s. This only reads, nothing is deleted, Ctrl-C is safe.",
            running_for.as_secs()
        ));
    }
    out
}

/// `38s`, or `1m 05s` once it passes a minute.
fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

/// A file count short enough for a status line: `850`, `12k`, `1.2M`.
fn format_count_short(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}k", n / 1_000),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// Shorten `s` to at most `max` characters by eliding the middle, which keeps both the
/// drive or root that says where it is and the last folders that say which one it is.
fn shorten_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max || max < 4 {
        return s.to_string();
    }
    let head = (max - 1) / 3;
    let tail = max - 1 - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// Color the contents of `backtick` spans — commands, flags, filenames — so the part
/// the user is meant to type or look for stands out from the prose around it.
///
/// Pairs only: an odd trailing backtick is left exactly as typed. The backticks
/// themselves are kept, because the `colored` crate emits no escape codes when stdout
/// is not a terminal (or `NO_COLOR` is set), and in that plain rendering the backticks
/// are what marks the span.
fn highlight_code_spans(msg: &str) -> String {
    if !msg.contains('`') {
        return msg.to_string();
    }
    let mut out = String::with_capacity(msg.len() + 16);
    let mut rest = msg;
    while let Some(start) = rest.find('`') {
        let Some(len) = rest[start + 1..].find('`') else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push('`');
        out.push_str(&rest[start + 1..start + 1 + len].cyan().to_string());
        out.push('`');
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// Print a success message (green checkmark)
pub fn print_success(msg: &str) {
    println!("{} {}", "✓".green().bold(), highlight_code_spans(msg));
}

/// Print a warning message (yellow exclamation)
///
/// To stderr, like errors: warnings can fire while stdout is a pipe or holds a pending
/// `--json` document (adapter drift notices, the criterion note), and a warning printed
/// into that stream is either invisible or a parse error.
pub fn print_warning(msg: &str) {
    eprintln!("{} {}", "⚠".yellow().bold(), highlight_code_spans(msg));
}

/// Print an error message (red X)
pub fn print_error(msg: &str) {
    eprintln!("{} {}", "✗".red().bold(), highlight_code_spans(msg));
}

/// Print an info message (dimmed arrow)
///
/// Dimmed rather than coloured on purpose. Info lines are the most common thing this
/// tool prints, and a bold blue marker on every one of them competes with the ✓ and ⚠
/// that actually need to be noticed — blue is also the worst colour to bet on, being
/// close to unreadable against the default background of several popular terminals.
pub fn print_info(msg: &str) {
    println!("{} {}", "→".dimmed(), highlight_code_spans(msg));
}

/// Print a line in the terminal's dimmed style, with no marker glyph.
///
/// For text that belongs to the item above it rather than being an item of its own —
/// the "and 13 more" under a list. A `→` there would announce it as a new point.
pub fn print_dimmed(msg: &str) {
    println!("{}", highlight_code_spans(msg).dimmed());
}

/// Print a notice to stderr.
///
/// For anything the user should see that is *about* the command rather than part of its
/// output — a deprecated flag, say. It has to be stderr: `--json` promises stdout carries
/// one JSON document and nothing else, and a friendly note printed above it is the
/// difference between a parseable contract and a parse error.
pub fn print_notice(msg: &str) {
    eprintln!("{} {}", "→".dimmed(), highlight_code_spans(msg));
}

/// Widest line this tool will print prose at, however wide the terminal is.
///
/// A paragraph set to the full width of a maximised terminal is measurably harder to
/// read than the same paragraph at ninety columns: the eye loses the line it was on
/// when it travels back to the left edge. Tables and paths are exempt — truncating
/// those loses information, whereas wrapping prose loses nothing.
const MAX_PROSE_WIDTH: usize = 90;

/// Print an explanatory paragraph, wrapped to the terminal and indented under `indent`.
///
/// The alternative is what `devp run` did until a machine with twenty-one unreadable
/// repositories showed it: three-line explanations soft-wrapped by the terminal back to
/// column zero, so the continuation of an indented note started further left than the
/// note did and read as a new item.
pub fn print_wrapped(indent: &str, msg: &str) {
    let width = crossterm::terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(MAX_PROSE_WIDTH)
        .min(MAX_PROSE_WIDTH);
    // A terminal narrow enough to make the wrap width zero would loop forever below.
    let room = width.saturating_sub(indent.len()).max(20);

    let mut line = String::new();
    for word in msg.split_whitespace() {
        if !line.is_empty() && line.width() + 1 + word.width() > room {
            println!("{indent}{}", highlight_code_spans(&line));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        println!("{indent}{}", highlight_code_spans(&line));
    }
}

/// Print a section header
///
/// Weight, not colour. A header is structure — the reader finds it by scanning down the
/// left edge, which bold already serves. Colouring and underlining it as well spends
/// two more signals on something that was already unambiguous, and leaves the palette
/// with nothing distinct to say when a line genuinely means "this went wrong".
pub fn print_header(msg: &str) {
    println!("\n{}", msg.bold());
}

/// A heading *inside* a report that already opened with a [`print_header`].
///
/// Same reasoning as `print_header` — weight, not colour — set one indent in, so it reads
/// as a division of the list under it rather than the start of a second report.
pub fn print_section(msg: &str) {
    println!("\n  {}", msg.bold());
}

/// A byte figure styled as "space you got back" — the number this tool exists for.
pub fn format_bytes_styled(bytes: u64) -> String {
    format_bytes(bytes).green().bold().to_string()
}

/// A byte figure styled as "this is the number to look at", saying nothing about whether
/// it is good news.
///
/// [`format_bytes_styled`]'s green means "space you got back". A cache's cost per
/// repository is not that: it is the figure a decision turns on, and green would promise
/// the reader something the number does not mean. Weight rather than colour, the same
/// choice [`print_header`] makes and for the same reason.
pub fn format_bytes_weighted(bytes: u64) -> String {
    format_bytes(bytes).bold().to_string()
}

/// A filesystem path, styled. One place to change if cyan-on-cyan ever clashes.
pub fn styled_path<P: AsRef<Path>>(path: P) -> String {
    clean_path(path).cyan().to_string()
}

/// A package-manager name, deliberately left in the terminal's default colour.
///
/// It used to be magenta, which put a fifth hue on a status row that already carried
/// green, cyan and a state colour — and an adapter name is an identifier, not a status,
/// so the colour was decorating rather than saying anything. Plain text is also what
/// keeps a wall of coloured columns readable: something has to be the resting state.
///
/// Still a function, and still called everywhere an adapter is named, so this stays one
/// decision in one place rather than a hundred call sites to revisit.
pub fn styled_adapter(name: &str) -> String {
    name.to_string()
}

/// Print the dev-prune ASCII art banner, the version, and which channel this copy is on.
///
/// The channel is there because dev-prune ships through eleven of them and nothing stops
/// two from landing a copy on the same machine. Every report that starts "devp still says
/// 1.9.0 after I upgraded" is really the question "which copy are you running, and who
/// owns it" — and that answer now appears in the screenshot before anyone has to ask for
/// it. It costs one lexical look at a path that has already been resolved: no process is
/// spawned, nothing is read from disk, and no package manager has to be installed for its
/// name to be printed.
pub fn print_banner() {
    // Cyan, not a hard-coded RGB. `truecolor` degrades to nothing useful on a 16- or
    // 256-colour terminal, and it ignored the palette the user picked for their own
    // terminal — a named colour honours it and matches the cyan used everywhere else.
    //
    // The channel is dimmed and the version is not: one is the thing people came to read
    // and the other is the footnote that explains it.
    println!();
    for line in banner_art_lines() {
        let (dev, prune) = split_art_line(line);
        println!("{}{}", dev.cyan(), prune.cyan().bold());
    }
    // Right-aligned under the art rather than trailing its last line, so the two subtitle
    // lines below start against a straight left edge instead of under a version string.
    // Padded before colouring: a width in a format spec counts the escape codes the
    // colouring adds and would indent this by however long they happen to be.
    let version = format!("v{}", crate::constants::VERSION);
    let pad = " ".repeat(art_width().saturating_sub(version.len()));
    println!(
        "{pad}{} {}",
        version.cyan().bold(),
        format!("· {}", crate::channel::Channel::detect().badge()).dimmed()
    );
    println!("{}", crate::constants::TAGLINE);
    println!("{}\n", crate::constants::TAGLINE_SAFETY.dimmed());
}

/// The widest line of the art, which the version line is right-aligned against.
///
/// Measured rather than written down: the rows are not all the same length, and a
/// constant would be one more thing to remember the next time the drawing is touched.
fn art_width() -> usize {
    banner_art_lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
}

/// Where `DEV` ends and `PRUNE` begins, in characters.
///
/// A fixed column rather than anything clever, because the art is a fixed drawing. The
/// gutter between the two words is three columns wide on every row, and
/// `the_two_words_split_cleanly` fails the build if an edit ever moves it.
const ART_SPLIT: usize = 26;

fn banner_art_lines() -> std::str::Lines<'static> {
    r#" ___    _____ __     __    ____  ____  _   _ _   _ _____
|  _ \ | ____|\ \   / /   |  _ \|  _ \| | | | \ | | ____|
| | | ||  _|   \ \ / /    | |_) | |_) | | | |  \| |  _|
| |_| || |___   \ V /     |  __/|  _ <| |_| | |\  | |___
|____/ |_____|   \_/      |_|   |_| \_\\___/|_| \_|_____|"#
        .lines()
}

/// Split one line of the art into its `DEV` half and its `PRUNE` half.
///
/// Two weights rather than two colours: the word that says what the command does carries
/// the emphasis, and the palette stays at the one hue the rest of the output uses.
fn split_art_line(line: &str) -> (&str, &str) {
    line.split_at(ART_SPLIT.min(line.len()))
}

/// Print the one-line credit, if anything is going to read it.
///
/// Gated on stdout being a terminal, which is the whole of the logic — a person watching
/// the command run sees it, a pipe, a redirect, a CI log and every `--json` consumer does
/// not. There is no other condition: no build flag, no environment variable, no check
/// that the binary is called `devp`. Forks are welcome to change
/// [`constants::ATTRIBUTION_LINE`] or delete this function, and nothing anywhere will
/// notice or complain.
pub fn print_attribution() {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        println!("{}", crate::constants::ATTRIBUTION_LINE.dimmed());
    }
}

/// Pick the singular or plural form for a count.
///
/// Small, but "Unregistered 1 repositories" is the kind of thing people notice and
/// nothing else in the codebase was doing it consistently.
pub fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// Format bytes into human-readable string (e.g., "1.20 GiB", "450 MiB").
/// Binary units, because that is what `humansize::BINARY` produces — every doc
/// example quoting sizes should say MiB/GiB, not MB/GB.
pub fn format_bytes(bytes: u64) -> String {
    use humansize::{BINARY, format_size};
    format_size(bytes, BINARY)
}

/// A duration in seconds, at the precision a person would actually say it in.
///
/// Deliberately coarse above a minute: an estimate printed as "14m 37s" claims a second
/// of accuracy that a throughput average over a handful of restores does not have, and
/// reads as a measurement rather than as the guess it is.
pub fn format_seconds(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s.div_ceil(60)),
        s => {
            let hours = s / 3600;
            let minutes = (s % 3600) / 60;
            if minutes == 0 {
                format!("{hours}h")
            } else {
                format!("{hours}h {minutes}m")
            }
        }
    }
}

/// The suffix explaining bytes a prune does not free because a package-manager store
/// hardlinks them (pnpm, bun). Empty when there is nothing to explain, so call sites
/// can append it unconditionally.
///
/// This line exists because `du` and Explorer report the *apparent* size: without it,
/// "node_modules (40 MiB)" beside a 2 GiB folder reads as a bug rather than as pnpm
/// working exactly as designed.
pub fn shared_note(shared_bytes: u64, adapter: &str) -> String {
    if shared_bytes == 0 {
        String::new()
    } else {
        format!(
            " (+{} hardlinked into the {adapter} store — not counted, the store keeps them)",
            format_bytes(shared_bytes)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_words_split_cleanly() {
        // `ART_SPLIT` is a column counted off a drawing, so the drawing is the only thing
        // that can invalidate it. An edit that shifts one row by a character would put a
        // bold `|` on the wrong side of the gap, which nothing else would catch — the
        // banner still prints, it just looks wrong to whoever runs it next.
        for line in banner_art_lines() {
            let gutter: String = line.chars().skip(ART_SPLIT - 3).take(3).collect();
            assert_eq!(
                gutter, "   ",
                "the gutter between DEV and PRUNE moved: {line:?}"
            );
            // The other side of the same failure: a split inside a glyph. The two halves
            // have to reassemble into exactly the row drawn above.
            let (dev, prune) = split_art_line(line);
            assert_eq!(format!("{dev}{prune}"), line);
        }
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1 GiB");
    }

    #[test]
    fn code_spans_survive_highlighting_verbatim_when_color_is_off() {
        // The test harness has no TTY, so `colored` emits nothing — which is itself the
        // property under test: piped output must be byte-identical to the input,
        // including the backticks and any odd trailing one.
        colored::control::set_override(false);
        assert_eq!(
            highlight_code_spans("run `devp setup` again"),
            "run `devp setup` again"
        );
        assert_eq!(highlight_code_spans("no spans here"), "no spans here");
        assert_eq!(
            highlight_code_spans("odd `tick remains"),
            "odd `tick remains"
        );
        assert_eq!(
            highlight_code_spans("`a` and `b`, plus `stray"),
            "`a` and `b`, plus `stray"
        );
        colored::control::unset_override();
    }

    #[test]
    fn a_wide_name_is_padded_to_columns_not_to_char_count() {
        // Eight Chinese characters: eight `char`s, sixteen columns. `{:<20}` would add
        // twelve spaces and draw twenty-eight columns wide; this adds four.
        let cjk = "项目目录名称测试";
        assert_eq!(cjk.chars().count(), 8);
        assert_eq!(UnicodeWidthStr::width(cjk), 16);
        let padded = pad_display(cjk, 20);
        assert_eq!(UnicodeWidthStr::width(padded.as_str()), 20);
        assert!(padded.ends_with("    "));
    }

    #[test]
    fn ascii_padding_still_matches_the_format_specifier_it_replaces() {
        assert_eq!(pad_display("repo", 10), format!("{:<10}", "repo"));
        assert_eq!(pad_display("", 3), "   ");
    }

    #[test]
    fn an_overlong_name_is_truncated_rather_than_pushing_the_next_column() {
        let long = "a".repeat(50);
        let out = pad_display(&long, 10);
        assert_eq!(UnicodeWidthStr::width(out.as_str()), 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn a_wide_char_straddling_the_cut_is_dropped_and_the_gap_is_closed() {
        // Budget after the ellipsis is 4 columns; the third character would need
        // columns 5–6, so it is dropped and a space keeps the width exact.
        let out = truncate_display("测试字符", 5);
        assert_eq!(UnicodeWidthStr::width(out.as_str()), 5);
        assert!(out.starts_with("测试"));
    }

    #[test]
    fn an_emoji_path_component_counts_as_two_columns() {
        let s = "🚀repo";
        assert_eq!(UnicodeWidthStr::width(s), 6);
        assert_eq!(UnicodeWidthStr::width(pad_display(s, 12).as_str()), 12);
    }

    #[test]
    fn a_zero_width_column_produces_nothing() {
        assert_eq!(truncate_display("anything", 0), "");
    }

    #[test]
    fn test_clean_path() {
        assert_eq!(clean_path(r"\\?\C:\Users\user"), r"C:\Users\user");
        assert_eq!(
            clean_path(r"\\?\UNC\server\share\repo"),
            r"\\server\share\repo"
        );
        assert_eq!(clean_path(r"/private/var/tmp/repo"), r"/var/tmp/repo");
        // A leading `//` is a network-share spelling and survives; only the doubled
        // separators inside the path collapse.
        assert_eq!(clean_path(r"//server//share//repo"), r"//server/share/repo");
        assert_eq!(clean_path(r"/home//user///repo"), r"/home/user/repo");
    }

    #[test]
    fn short_output_is_relayed_whole() {
        let raw = "npm error code EUSAGE\nnpm error requires an existing package-lock.json";
        assert_eq!(condense_tool_output(raw, 6), raw);
    }

    #[test]
    fn a_usage_screen_is_reduced_to_its_diagnostics() {
        // The shape that motivated this: `npm ci` failed, printed its whole usage
        // screen, and dev-prune relayed all of it into the middle of a prune report.
        let mut raw = String::from("npm error code EUSAGE\nnpm error\n");
        raw.push_str("Usage:\nnpm ci\n");
        for i in 0..120 {
            raw.push_str(&format!("  --flag-{i} <value>\n"));
        }
        raw.push_str("npm error A complete log of this run can be found in: /tmp/log\n");

        let out = condense_tool_output(&raw, 6);
        assert!(out.contains("EUSAGE"), "{out}");
        // The escape hatch survives even though it is the very last line.
        assert!(out.contains("complete log of this run"), "{out}");
        assert!(!out.contains("--flag-50"), "{out}");
        assert!(out.contains("more lines of output"), "{out}");
    }

    #[test]
    fn output_with_no_diagnostics_keeps_the_top_of_it() {
        // Not every tool marks its complaint. Falling back to the first few lines beats
        // dropping everything, and the count still says what was hidden.
        let raw: String = (0..40).map(|i| format!("line {i}\n")).collect();
        let out = condense_tool_output(&raw, 3);
        assert!(out.starts_with("line 0\nline 1\nline 2\n…"), "{out}");
        assert!(out.contains("37 more lines"), "{out}");
    }

    #[test]
    fn the_dropped_count_never_claims_more_than_there_was() {
        // Blank lines are removed before counting, so a command that padded its output
        // must not be reported as having said more than it did.
        let raw = "a\n\n\nb\n\n\nc\n\n\nd\n";
        let out = condense_tool_output(raw, 2);
        assert!(out.contains("2 more lines"), "{out}");
    }

    #[test]
    fn an_estimate_is_stated_at_the_precision_it_has() {
        assert_eq!(format_seconds(45), "45s");
        // Rounded up: "0m" for a 61-second restore reads as instant.
        assert_eq!(format_seconds(61), "2m");
        assert_eq!(format_seconds(3600), "1h");
        assert_eq!(format_seconds(4500), "1h 15m");
    }

    fn tracker_of(names: &[&str]) -> LiveTracker {
        LiveTracker::new(
            names
                .iter()
                .map(|n| ((*n).to_string(), format!("/caches/{n}")))
                .collect(),
        )
    }

    #[test]
    fn the_line_names_what_is_running_and_stays_quiet_before_the_threshold() {
        let tracker = tracker_of(&["go build cache", "npm cache"]);
        tracker.begin(0);
        tracker.begin(1);
        tracker.end(1);
        let progress = LiveProgress {
            title: "Measuring caches",
            verb: "measuring",
            tracker: &tracker,
            files_of: None,
            refresh: None,
        };
        let line = progress.message(Duration::from_secs(3), Duration::from_secs(10));
        assert!(line.starts_with("Measuring caches  1/2  3s"), "{line}");
        assert!(
            line.contains("go build cache  /caches/go build cache"),
            "{line}"
        );
        assert!(!line.contains("still"), "no hint yet: {line}");
    }

    #[test]
    fn a_slow_item_gets_the_still_working_hint_with_a_moving_file_count() {
        let tracker = tracker_of(&["go build cache"]);
        tracker.begin(0);
        let files = |_: usize| 1_234_567u64;
        let progress = LiveProgress {
            title: "Measuring caches",
            verb: "measuring",
            tracker: &tracker,
            files_of: Some(&files),
            refresh: None,
        };
        // A zero threshold stands in for waiting out the real one.
        let line = progress.message(Duration::from_secs(38), Duration::ZERO);
        assert!(
            line.contains(
                "still measuring go build cache, 1.2M files so far, 0 s. This only reads, \
                 nothing is deleted, Ctrl-C is safe."
            ),
            "{line}"
        );
    }

    #[test]
    fn the_hint_and_the_counts_are_plain_text() {
        assert!(
            !live_message(
                "t",
                "measuring",
                (0, 1),
                Some(("x", "/p", Duration::from_secs(99), 3, None)),
                Duration::from_secs(99),
                Duration::from_secs(10),
            )
            .contains('\u{2014}')
        );
        assert_eq!(format_elapsed(Duration::from_secs(38)), "38s");
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1m 05s");
        assert_eq!(format_count_short(850), "850");
        assert_eq!(format_count_short(12_345), "12k");
        assert_eq!(format_count_short(1_200_000), "1.2M");
    }

    #[test]
    fn long_paths_keep_their_root_and_their_last_folders() {
        let long = r"C:\Users\someone\AppData\Local\go-build\deep\er\and\deeper";
        let short = shorten_middle(long, 24);
        assert_eq!(short.chars().count(), 24);
        assert!(short.starts_with("C:\\User"), "{short}");
        assert!(short.ends_with("deeper"), "{short}");
        assert_eq!(shorten_middle("/a/b", 24), "/a/b");
    }

    #[test]
    fn a_disabled_line_still_runs_the_work_and_draws_nothing() {
        let tracker = tracker_of(&["x"]);
        let progress = LiveProgress {
            title: "t",
            verb: "v",
            tracker: &tracker,
            files_of: None,
            refresh: None,
        };
        assert_eq!(progress.run(false, || 7), 7);
    }
}
