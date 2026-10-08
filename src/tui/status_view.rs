// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Terminal UI for the `dev-prune status` command.
//
// Renders a full interactive, scrollable table of all registered repositories
// showing: status, reason skipped, last activity, last pruned, bloat size,
// and adapters. Users can also select candidates and trigger a prune pass
// directly from this view.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::constants;
use crate::engine::{RepoStatusEntry, SkipReason};
use crate::output::format_bytes;
use crate::tui::Tui;

/// Mode the status view is in.
enum ViewMode {
    /// Browsing the table — 'p' enters PruneSelect mode.
    Browse,
    /// User is selecting candidates to prune.
    PruneSelect,
}

/// Which column the table is ordered by.
///
/// `Default` is the order [`crate::engine::get_full_status`] produced — actionable
/// first, then everything merely present, then everything gone. It is first in the
/// cycle and it is where the view starts, because that ordering is the one answer to
/// "what should I look at" and no sort the user picks should be hard to get back to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Default,
    /// Largest reclaim first.
    Size,
    /// Least recently touched first — the repositories nobody has opened in months.
    Activity,
    /// By path, A to Z.
    Name,
}

impl SortKey {
    fn next(self) -> Self {
        match self {
            SortKey::Default => SortKey::Size,
            SortKey::Size => SortKey::Activity,
            SortKey::Activity => SortKey::Name,
            SortKey::Name => SortKey::Default,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortKey::Default => "relevance",
            SortKey::Size => "size ↓",
            SortKey::Activity => "idle longest ↓",
            SortKey::Name => "name ↑",
        }
    }
}

/// Which rows the table shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    /// Only what a prune would act on.
    Candidates,
    /// Anything holding reclaimable bytes, candidate or not — including the repositories
    /// still active, which is the list you want before deciding to wait.
    WithBloat,
    /// The rows that need a decision rather than a prune: a path that is gone, a config
    /// file that does not parse.
    Problems,
}

impl Filter {
    fn next(self) -> Self {
        match self {
            Filter::All => Filter::Candidates,
            Filter::Candidates => Filter::WithBloat,
            Filter::WithBloat => Filter::Problems,
            Filter::Problems => Filter::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Candidates => "candidates",
            Filter::WithBloat => "has bloat",
            Filter::Problems => "problems",
        }
    }

    fn accepts(self, repo: &RepoStatusEntry) -> bool {
        match self {
            Filter::All => true,
            Filter::Candidates => matches!(repo.reason, SkipReason::Candidate),
            Filter::WithBloat => repo.reclaimable_bytes > 0,
            Filter::Problems => matches!(
                repo.reason,
                SkipReason::PathMissing | SkipReason::ConfigError(_)
            ),
        }
    }
}

struct StatusApp<'a> {
    repos: &'a [RepoStatusEntry],
    /// Indices into `repos`, filtered and sorted — the rows actually on screen.
    ///
    /// The table addresses this, everything else addresses `repos`. Keeping the two
    /// apart is what lets a filter hide a row without disturbing which repositories are
    /// checked for pruning, and what stops the confirmed selection from meaning
    /// something different depending on how the table happened to be sorted.
    view: Vec<usize>,
    table_state: TableState,
    /// Which rows are checked for prune (indexed to `repos`).
    selected: Vec<bool>,
    mode: ViewMode,
    sort: SortKey,
    filter: Filter,
    /// Case-insensitive substring matched against the path and the adapter names.
    search: String,
    /// True while keystrokes are going into `search` rather than being commands.
    searching: bool,
    /// If `Some`, the user confirmed and we return these indices.
    confirmed_indices: Option<Vec<usize>>,
    /// One line explaining the last refused keypress, shown in the prune-mode footer.
    notice: Option<String>,
    /// Set to true when the user toggles ignore config in .devprune.json or presence of ignore.devprune.json so caller can reload.
    pub should_reload: bool,
    /// On by default because the table only has room for one number per repository, and
    /// "why is this one so big" needs the breakdown. Tab hides it for anyone who wants the
    /// full-width table back.
    pub show_inspector: bool,
    /// Whether the last frame had room to draw the inspector. `show_inspector` is only
    /// the wish; on a small terminal nobody would see what the worker collected.
    inspector_drawn: bool,
    /// What the inspector shows beyond the status scan, per index into `repos`. Filled
    /// lazily by a worker thread, because a commit histogram and a walk of every bloat
    /// directory cost far too much to collect for eighty repositories up front.
    details: HashMap<usize, InspectorDetail>,
    /// The repository whose details a worker is collecting now. One at a time: holding
    /// the arrow key down the list would otherwise start a disk walk per row passed.
    inflight: Option<usize>,
    detail_tx: mpsc::Sender<(usize, InspectorDetail)>,
    detail_rx: mpsc::Receiver<(usize, InspectorDetail)>,
    /// A pending `i`: the repository and whether the keypress would ignore it (`true`) or
    /// stop ignoring it. `i` changes what every later pass is allowed to delete, and it
    /// sits one key away from `j` and `k`, so it waits for a `y`.
    pending_ignore: Option<(usize, bool)>,
}

/// How many weeks of commits the inspector's histogram covers: half a year shows whether
/// a repository was abandoned or is just quiet this month.
const HISTOGRAM_WEEKS: usize = 26;

/// How many of the largest entries inside the bloat directories the inspector lists.
const LARGEST_ENTRIES: usize = 5;

/// The inspector's figures that the status scan does not collect.
struct InspectorDetail {
    /// Commits per week, oldest first. `None` when git would not read the history.
    weekly_commits: Option<Vec<u32>>,
    /// The largest top-level entries inside the bloat directories, largest first, each
    /// named `<bloat dir>/<entry>`.
    largest: Vec<(String, u64)>,
}

/// Why this repository cannot be checked for pruning, or `None` if it can be.
///
/// Selectable is deliberately wider than `SkipReason::Candidate`. An *active*
/// repository is one the engine would skip on the idle threshold alone, and the
/// dashboard's own executor already runs with `force: true` — so offering it here
/// promises nothing the prune cannot deliver. On a machine whose repositories are all
/// worked in daily, candidate-only selection left the mode with nothing to select at
/// all, which is indistinguishable from a dead feature.
///
/// Everything else stays refused because pruning it genuinely would not work:
/// verification still applies to every row this function accepts.
fn refusal(repo: &RepoStatusEntry) -> Option<String> {
    match &repo.reason {
        SkipReason::Candidate | SkipReason::Active if repo.reclaimable_bytes > 0 => None,
        SkipReason::Candidate | SkipReason::Active | SkipReason::NoBloat => {
            Some("nothing to reclaim here".to_string())
        }
        SkipReason::Ignored => Some("ignored — press [i] to un-ignore it".to_string()),
        SkipReason::PathMissing => {
            Some("path is missing — `devp unlink --missing` clears it".to_string())
        }
        SkipReason::ConfigError(e) => Some(format!("config error: {e}")),
    }
}

impl<'a> StatusApp<'a> {
    fn new(repos: &'a [RepoStatusEntry]) -> Self {
        let selected = vec![false; repos.len()];
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        let (detail_tx, detail_rx) = mpsc::channel();
        let mut app = Self {
            repos,
            view: Vec::new(),
            table_state,
            selected,
            mode: ViewMode::Browse,
            sort: SortKey::Default,
            filter: Filter::All,
            search: String::new(),
            searching: false,
            confirmed_indices: None,
            notice: None,
            should_reload: false,
            show_inspector: true,
            inspector_drawn: false,
            details: HashMap::new(),
            inflight: None,
            detail_tx,
            detail_rx,
            pending_ignore: None,
        };
        app.rebuild_view();
        app
    }

    /// Recompute the visible rows after a change to the sort, the filter or the query.
    ///
    /// Keeps the cursor on the same *repository* rather than the same row number. A
    /// filter that removes four rows above the cursor would otherwise slide the
    /// selection onto a different repository under the user's hands — and `i` and the
    /// prune toggle both act on whatever is selected.
    fn rebuild_view(&mut self) {
        let anchor = self.cursor_repo();
        let needle = self.search.to_lowercase();

        let mut view: Vec<usize> = (0..self.repos.len())
            .filter(|&i| {
                let repo = &self.repos[i];
                if !self.filter.accepts(repo) {
                    return false;
                }
                if needle.is_empty() {
                    return true;
                }
                // Adapters as well as the path, so `/uv` finds every Python project
                // without anyone having to remember where they live.
                repo.path.to_string_lossy().to_lowercase().contains(&needle)
                    || repo
                        .adapters
                        .iter()
                        .any(|a| a.to_lowercase().contains(&needle))
            })
            .collect();

        match self.sort {
            // Already in this order: `get_full_status` sorted the slice before it got
            // here, and `view` was built by walking it forwards.
            SortKey::Default => {}
            SortKey::Size => view.sort_by(|&a, &b| {
                self.repos[b]
                    .reclaimable_bytes
                    .cmp(&self.repos[a].reclaimable_bytes)
                    .then_with(|| self.repos[a].path.cmp(&self.repos[b].path))
            }),
            // `None` is a repository whose activity could not be read at all, which is
            // not evidence of being idle — so it sorts last rather than first.
            SortKey::Activity => view.sort_by(|&a, &b| {
                let key = |i: usize| {
                    self.repos[i]
                        .last_activity
                        .map(|t| (0, t))
                        .unwrap_or((1, chrono::DateTime::<chrono::Utc>::MIN_UTC))
                };
                key(a)
                    .cmp(&key(b))
                    .then_with(|| self.repos[a].path.cmp(&self.repos[b].path))
            }),
            SortKey::Name => view.sort_by(|&a, &b| self.repos[a].path.cmp(&self.repos[b].path)),
        }

        self.view = view;
        let row = anchor
            .and_then(|repo_idx| self.view.iter().position(|&i| i == repo_idx))
            .unwrap_or(0);
        // `then`, not `then_some`: the argument to `then_some` is evaluated whether the
        // condition holds or not, and on an empty view that subtraction underflows.
        self.table_state
            .select((!self.view.is_empty()).then(|| row.min(self.view.len() - 1)));
    }

    /// Index into `repos` of the highlighted row, or `None` when the view is empty.
    fn cursor_repo(&self) -> Option<usize> {
        self.view.get(self.table_state.selected()?).copied()
    }

    fn move_up(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.view.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn move_down(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.view.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn toggle_current(&mut self) {
        let Some(i) = self.cursor_repo() else {
            return;
        };
        match refusal(&self.repos[i]) {
            // Silently refusing every keypress is what made this mode look broken: a
            // machine whose repositories are all worked-in daily has no candidates, so
            // Space did nothing on every row and there was no way to find out why.
            Some(why) => self.notice = Some(why),
            None => {
                self.selected[i] = !self.selected[i];
                self.notice = None;
            }
        }
    }

    /// Check, or uncheck, every candidate currently on screen.
    ///
    /// Deliberately scoped to the view: with a filter or a search active, "all" has to
    /// mean the rows the user can see. Selecting thirty repositories they filtered out
    /// on a keypress labelled *Toggle All* is how a prune becomes a surprise.
    ///
    /// Candidates only, even though Space can now check an active repository too.
    /// Enter prunes immediately — there is no second confirmation — so one stray `a`
    /// must never arm every repository that merely has something to reclaim, including
    /// the one the user is working in right now. Actives are opted into a row at a time.
    fn toggle_all_candidates(&mut self) {
        let visible: Vec<usize> = self
            .view
            .iter()
            .copied()
            .filter(|&i| matches!(self.repos[i].reason, SkipReason::Candidate))
            .collect();
        let any_selected = visible.iter().any(|&i| self.selected[i]);
        for i in visible {
            self.selected[i] = !any_selected;
        }
    }

    fn confirm_prune(&mut self) {
        let indices: Vec<usize> = self
            .selected
            .iter()
            .enumerate()
            .filter(|&(_, s)| *s)
            .map(|(i, _)| i)
            .collect();
        self.confirmed_indices = Some(indices);
    }

    fn selected_bytes(&self) -> u64 {
        self.repos
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i])
            .map(|(_, r)| r.reclaimable_bytes)
            .sum()
    }

    fn selected_count(&self) -> usize {
        self.selected.iter().filter(|&&s| s).count()
    }

    fn candidate_count(&self) -> usize {
        self.repos
            .iter()
            .filter(|r| matches!(r.reason, SkipReason::Candidate))
            .count()
    }

    /// How many rows Space would accept — the honest denominator for the footer.
    fn selectable_count(&self) -> usize {
        self.repos.iter().filter(|r| refusal(r).is_none()).count()
    }

    /// Store whatever the worker finished, and start one for the highlighted repository
    /// if nothing is running and its details are not known yet. Called once per loop
    /// turn, so the inspector fills in on the next redraw without blocking a keypress.
    fn pump_details(&mut self) {
        while let Ok((idx, detail)) = self.detail_rx.try_recv() {
            self.details.insert(idx, detail);
            if self.inflight == Some(idx) {
                self.inflight = None;
            }
        }
        if !self.inspector_drawn || self.inflight.is_some() {
            return;
        }
        let Some(idx) = self.cursor_repo() else {
            return;
        };
        let repo = &self.repos[idx];
        if self.details.contains_key(&idx) || matches!(repo.reason, SkipReason::PathMissing) {
            return;
        }
        let path = repo.path.clone();
        let dirs: Vec<(String, PathBuf)> = repo
            .bloat_dirs
            .iter()
            .map(|b| (b.name.clone(), b.path.clone()))
            .collect();
        let tx = self.detail_tx.clone();
        self.inflight = Some(idx);
        std::thread::spawn(move || {
            let detail = InspectorDetail {
                weekly_commits: weekly_commits(&path),
                largest: largest_entries(&dirs, LARGEST_ENTRIES),
            };
            // The receiver is gone once the user quits or the view reloads; the
            // answer is simply no longer wanted.
            let _ = tx.send((idx, detail));
        });
    }
}

/// Commits per week over the last [`HISTOGRAM_WEEKS`] weeks, oldest first, or `None` when
/// git will not read the repository.
fn weekly_commits(repo: &Path) -> Option<Vec<u32>> {
    let output = crate::scanner::git::git_in(repo)
        .args([
            "log",
            &format!("--since={HISTOGRAM_WEEKS}.weeks"),
            "--format=%ct",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stamps: Vec<i64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    Some(bucket_by_week(
        &stamps,
        chrono::Utc::now().timestamp(),
        HISTOGRAM_WEEKS,
    ))
}

/// Count `stamps` (Unix seconds) into `weeks` week-long buckets ending at `now`, oldest
/// bucket first. Anything outside the window is dropped, including a commit dated in the
/// future by a skewed clock.
fn bucket_by_week(stamps: &[i64], now: i64, weeks: usize) -> Vec<u32> {
    const WEEK: i64 = 7 * 24 * 60 * 60;
    let mut buckets = vec![0u32; weeks];
    for &t in stamps {
        if t > now {
            continue;
        }
        let age = ((now - t) / WEEK) as usize;
        if age < weeks {
            buckets[weeks - 1 - age] += 1;
        }
    }
    buckets
}

/// One character per bucket, scaled to the busiest one. An empty week is a dot rather
/// than the lowest bar, so a quiet week cannot be mistaken for a week with one commit.
fn sparkline(counts: &[u32]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = counts.iter().copied().max().unwrap_or(0);
    counts
        .iter()
        .map(|&c| {
            if c == 0 {
                '·'
            } else {
                let step = ((c as u64 * (BARS.len() as u64 - 1)) / max as u64) as usize;
                BARS[step]
            }
        })
        .collect()
}

/// The `n` largest top-level entries across `dirs`, largest first. Top level only: inside
/// `node_modules` that is one line per package, which is the level a person can act on.
/// Symlinked entries are not followed, the same as every other size in this tool.
fn largest_entries(dirs: &[(String, PathBuf)], n: usize) -> Vec<(String, u64)> {
    let mut entries: Vec<(String, u64)> = Vec::new();
    for (name, dir) in dirs {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in read.flatten() {
            let size = match entry.file_type() {
                Ok(t) if t.is_dir() => crate::adapters::dir_size(&entry.path()),
                Ok(t) if t.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
                _ => 0,
            };
            entries.push((
                format!("{name}/{}", entry.file_name().to_string_lossy()),
                size,
            ));
        }
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.truncate(n);
    entries
}

/// Split the reclaimable total into what a prune would free *now* and everything.
///
/// Two very different numbers that were being shown as one. The grand total counts the
/// dependency directories in every registered repository, including the ones worked in
/// this morning — a figure nothing is going to act on. What people are reading the
/// dashboard for is the other one: the repositories that have gone idle long enough to
/// be candidates, whose bytes `devp run` would reclaim on its next pass.
///
/// Always over the whole registry, never over the filtered view. A header that shrank
/// when a filter was applied would make the machine look tidier than it is.
fn reclaimable_split(repos: &[RepoStatusEntry]) -> (u64, u64) {
    let ready = repos
        .iter()
        .filter(|r| matches!(r.reason, SkipReason::Candidate))
        .map(|r| r.reclaimable_bytes)
        .sum();
    let total = repos.iter().map(|r| r.reclaimable_bytes).sum();
    (ready, total)
}

/// Total bytes and repository count per tool name (graphify, understand-anything),
/// across every repository passed in, in the order the tools were first seen. Report-
/// only figures: never added to [`reclaimable_split`], never a prune candidate.
fn tools_summary(repos: &[RepoStatusEntry]) -> Vec<(&'static str, u64, usize)> {
    let mut totals: Vec<(&'static str, u64, usize)> = Vec::new();
    for repo in repos {
        for tool in &repo.tools {
            match totals.iter_mut().find(|(name, _, _)| *name == tool.name) {
                Some((_, bytes, count)) => {
                    *bytes += tool.size_bytes;
                    *count += 1;
                }
                None => totals.push((tool.name, tool.size_bytes, 1)),
            }
        }
    }
    totals
}

/// Render the full interactive status view.
///
/// Returns `Some` with the selected repositories' paths if the user confirmed a
/// prune, or `None` if they just quit. Paths, not indices: every `i` toggle
/// reloads the list, and an ignored repository entering or leaving it renumbers
/// everything — indices handed to the caller would address the list it loaded
/// before any of that happened.
///
/// Re-runs automatically when the user toggles ignore config in `devprune.json` so the
/// status reflects the change immediately.
pub fn render_status_tui(
    repos_loader: &dyn Fn() -> Vec<RepoStatusEntry>,
) -> Result<Option<Vec<PathBuf>>> {
    loop {
        let repos = repos_loader();

        if repos.is_empty() {
            return Ok(None);
        }

        let mut app = StatusApp::new(&repos);
        {
            // Scoped so the terminal is restored before anything below prints, and on
            // every exit path from the loop — return, error, or panic.
            let mut tui = Tui::new()?;
            tui.drain_stale_input(Duration::from_millis(100));
            run_status_loop(&mut tui.terminal, &mut app)?;
        }

        if app.should_reload {
            // User toggled ignore — reload and re-render
            continue;
        }

        // Resolved against `repos` — the list this iteration actually displayed —
        // while it is still in scope, so a reload can never desynchronise them.
        return Ok(app
            .confirmed_indices
            .map(|indices| indices.into_iter().map(|i| repos[i].path.clone()).collect()));
    }
}

fn run_status_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut StatusApp,
) -> Result<()> {
    loop {
        app.pump_details();
        terminal.draw(|frame| render_ui(frame, app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            // Raw mode delivers Ctrl-C as a key event rather than a signal, so
            // without this the one key everybody reaches for to escape does nothing.
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            {
                return Ok(());
            }
            // Only `y` or Enter confirms. Every other key cancels, so a stray keypress can
            // never be the one that changes what a pass may delete.
            if let Some((idx, would_ignore)) = app.pending_ignore.take() {
                if matches!(
                    key.code,
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
                ) {
                    apply_ignore_toggle(&app.repos[idx], would_ignore)?;
                    app.should_reload = true;
                    return Ok(());
                }
                continue;
            }
            // While the query line is open every printable key is text, not a command.
            // Checked before the command table below, because otherwise typing `q` in a
            // search would quit and typing `p` would arm a prune.
            if app.searching {
                match key.code {
                    KeyCode::Char(c) => {
                        app.search.push(c);
                        app.rebuild_view();
                    }
                    KeyCode::Backspace => {
                        app.search.pop();
                        app.rebuild_view();
                    }
                    // Enter keeps the filter and hands the keyboard back; Esc abandons
                    // it. A query you cannot undo in one key is a query people restart
                    // the whole command to escape.
                    KeyCode::Enter => app.searching = false,
                    KeyCode::Esc => {
                        app.searching = false;
                        app.search.clear();
                        app.rebuild_view();
                    }
                    _ => {}
                }
                continue;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::Home | KeyCode::Char('g') => app.table_state.select(Some(0)),
                KeyCode::End | KeyCode::Char('G') => {
                    app.table_state
                        .select(Some(app.view.len().saturating_sub(1)));
                }
                KeyCode::PageUp => {
                    let i = app.table_state.selected().unwrap_or(0).saturating_sub(10);
                    app.table_state.select(Some(i));
                }
                KeyCode::PageDown => {
                    let i = (app.table_state.selected().unwrap_or(0) + 10)
                        .min(app.view.len().saturating_sub(1));
                    app.table_state.select(Some(i));
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    app.sort = app.sort.next();
                    app.rebuild_view();
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    app.filter = app.filter.next();
                    app.rebuild_view();
                }
                KeyCode::Char('/') => app.searching = true,
                KeyCode::Char(' ') => {
                    if matches!(app.mode, ViewMode::PruneSelect) {
                        app.toggle_current();
                    }
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    if matches!(app.mode, ViewMode::PruneSelect) {
                        app.toggle_all_candidates();
                    }
                }
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    app.mode = ViewMode::PruneSelect;
                    // Auto-select the candidates on screen — not every candidate in the
                    // registry. Someone who filtered or searched their way down to four
                    // repositories asked for those four.
                    //
                    // Candidates only: an active repository is selectable but never
                    // pre-checked, because entering a mode must not arm a repository
                    // the user is still working in.
                    let mut armed = 0;
                    for i in app.view.clone() {
                        if matches!(app.repos[i].reason, SkipReason::Candidate) {
                            app.selected[i] = true;
                            armed += 1;
                        }
                    }
                    // Arming nothing used to be silent, and looked exactly like a mode
                    // that had failed to open. Say which it is.
                    app.notice = (armed == 0).then(|| {
                        if app.view.iter().any(|&i| refusal(&app.repos[i]).is_none()) {
                            "No repository is past the idle threshold — press [Space] \
                             to select one you are still working in."
                                .to_string()
                        } else {
                            "Nothing here can be pruned — press [f] to widen the filter."
                                .to_string()
                        }
                    });
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    // Asks first; the write happens on `y`, in the branch above.
                    if let Some(idx) = app.cursor_repo()
                        && let Some(would_ignore) = ignore_toggle_for(&app.repos[idx])?
                    {
                        app.pending_ignore = Some((idx, would_ignore));
                    }
                }
                KeyCode::Enter => {
                    if matches!(app.mode, ViewMode::PruneSelect) && app.selected_count() > 0 {
                        app.confirm_prune();
                        return Ok(());
                    }
                }
                KeyCode::Esc => {
                    if matches!(app.mode, ViewMode::PruneSelect) {
                        // Exit prune-select mode, go back to browse
                        app.mode = ViewMode::Browse;
                        app.selected.fill(false);
                        app.notice = None;
                    } else {
                        return Ok(());
                    }
                }
                KeyCode::Tab => {
                    app.show_inspector = !app.show_inspector;
                }
                KeyCode::Char('q') => return Ok(()),
                _ => {}
            }
        }
    }
}

/// What `i` would do to `repo`: `Some(true)` to ignore it, `Some(false)` to stop ignoring
/// it, or `None` when the key has nothing it can honestly change.
fn ignore_toggle_for(repo: &RepoStatusEntry) -> Result<Option<bool>> {
    // A missing repository has no directory to write the config into; propagating that
    // write error would tear the whole TUI down over a row that says "Path Missing"
    // right on it.
    if matches!(repo.reason, SkipReason::PathMissing) {
        return Ok(None);
    }
    let (layers, per_repo) = load_personal_config(repo)?;
    // A committed `project.devprune.json` that names `ignore` wins over anything this key
    // could write, so writing it anyway would leave a personal file disagreeing with the
    // row it did not change. The help pane says which file to edit instead.
    if layers.source_of("ignore") == crate::config::ConfigSource::Project {
        return Ok(None);
    }
    // The legacy marker file still counts as "ignored". Reading only the config key made
    // `i` on such a row write `ignore: true` and delete the marker, leaving it ignored.
    let legacy = repo.path.join(crate::constants::DEVPRUNE_IGNORE_FILE);
    Ok(Some(!(per_repo.ignore || legacy.exists())))
}

/// Write the answer to a confirmed `i`.
fn apply_ignore_toggle(repo: &RepoStatusEntry, ignore: bool) -> Result<()> {
    let (_, mut per_repo) = load_personal_config(repo)?;
    per_repo.ignore = ignore;
    // Both writes are propagated rather than swallowed. A silent failure here redraws the
    // table unchanged, which reads as a dead key; worse, a repository the user just
    // un-ignored would still be pruned on the next pass.
    per_repo.save_to_repo(&repo.path).with_context(|| {
        format!(
            "Could not write the config for {}",
            crate::output::clean_path(&repo.path)
        )
    })?;
    // The marker goes either way, so the config key is the one place the answer lives.
    let legacy_ignore = repo.path.join(crate::constants::DEVPRUNE_IGNORE_FILE);
    if legacy_ignore.exists() {
        std::fs::remove_file(&legacy_ignore).with_context(|| {
            format!(
                "Could not remove {}",
                crate::output::clean_path(&legacy_ignore)
            )
        })?;
    }
    Ok(())
}

/// The repository's config layers and its personal layer, defaults when it has none.
///
/// Refuses a config that does not parse. Starting from the defaults would write a fresh
/// file over the broken one, discarding every other override it held, and the dashboard
/// already shows such a repository as `config_error`.
fn load_personal_config(
    repo: &RepoStatusEntry,
) -> Result<(
    crate::config::RepoConfigLayers,
    crate::config::PerRepoConfig,
)> {
    let layers = crate::config::RepoConfigLayers::load(&repo.path)
        .map_err(|e| anyhow::anyhow!(e))
        .with_context(|| {
            format!(
                "Could not toggle ignore for {}",
                crate::output::clean_path(&repo.path)
            )
        })?;
    let per_repo = layers.personal_config().cloned().unwrap_or_default();
    Ok((layers, per_repo))
}

/// Three tiers, not six colours: green is actionable, red and yellow are broken, and
/// everything that is merely normal is the terminal's own colour or grey. A distinct hue
/// per variant looks like a legend the reader has to learn, and it spends the loud
/// colours on rows that need nothing.
fn reason_color(reason: &SkipReason) -> Color {
    match reason {
        SkipReason::Candidate => Color::Green,
        SkipReason::Active => Color::Reset,
        SkipReason::Ignored | SkipReason::NoBloat => Color::DarkGray,
        SkipReason::PathMissing => Color::Red,
        // Actionable rather than broken: the repo is fine, its config file is not.
        SkipReason::ConfigError(_) => Color::Yellow,
    }
}

fn render_ui(frame: &mut Frame, app: &mut StatusApp) {
    let is_prune_mode = matches!(app.mode, ViewMode::PruneSelect);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // table
            // Five content lines plus the border: the sort/filter/search line, the
            // keybindings, the mode-specific line, and the credit line.
            Constraint::Length(7), // footer
        ])
        .split(frame.area());

    // ── Header ───────────────────────────────────────────────────────────────
    let mode_label = if is_prune_mode {
        Span::styled(
            " PRUNE-SELECT MODE ",
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " BROWSE MODE ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    };

    let (ready, total) = reclaimable_split(app.repos);
    let tools = tools_summary(app.repos);
    let mut header_spans = vec![
        Span::styled(
            " dev-prune ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        mode_label,
        Span::styled(
            format!(
                "  {}  |  {} candidates  |  ",
                // "showing 4 of 80" only when they differ — on an unfiltered view the
                // qualifier is noise, and it is the filtered view that needs to say
                // plainly that the numbers beside it are not the whole machine.
                if app.view.len() == app.repos.len() {
                    format!("{} repos", app.repos.len())
                } else {
                    format!("showing {} of {} repos", app.view.len(), app.repos.len())
                },
                app.candidate_count(),
            ),
            Style::default().fg(Color::DarkGray),
        ),
        // The actionable figure, and the only coloured one in the header: this is what
        // pressing `p` right now would free.
        Span::styled(
            format!("{} ready now", format_bytes(ready)),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  |  {} reclaimable in all", format_bytes(total)),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    // Report-only: never folded into `ready`/`total` above, and omitted entirely when
    // no repository has a graphify or understand-anything output directory.
    if !tools.is_empty() {
        let summary = tools
            .iter()
            .map(|(name, bytes, count)| {
                format!(
                    "{} {} in {} repo{}",
                    name,
                    format_bytes(*bytes),
                    count,
                    if *count == 1 { "" } else { "s" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        header_spans.push(Span::styled(
            format!("  |  Tools: {summary}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    let header_line = Line::from(header_spans);

    let header_widget =
        Paragraph::new(header_line).block(Block::default().borders(Borders::ALL).border_style(
            Style::default().fg(if is_prune_mode {
                Color::Yellow
            } else {
                Color::Cyan
            }),
        ));
    frame.render_widget(header_widget, outer[0]);

    // ── Table ─────────────────────────────────────────────────────────────────
    let col_headers = Row::new(vec![
        Cell::from(if is_prune_mode { "Sel" } else { "#" })
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Repository").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status / Reason").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Adapters").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Bloat").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Last Activity").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Last Pruned").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .height(1)
    .bottom_margin(1)
    // The bar's background is a fixed dark navy, so the foreground must be fixed
    // too: on a light-theme terminal the default foreground is near-black and
    // vanishes into it.
    .style(Style::default().bg(Color::Rgb(20, 25, 40)).fg(Color::White));

    // Rows sitting on one of the fixed dark backgrounds (selection green, highlight
    // blue) need explicitly light text; everywhere else the terminal's own default
    // foreground is the only colour guaranteed readable on both light and dark themes.
    let highlighted_row = app.table_state.selected();

    // Once per frame, not once per row — display names are relative to each other,
    // so the row closure below needs the full list, but rebuilding it n times made
    // every redraw O(n²) in path clones.
    let all_paths: Vec<_> = app.repos.iter().map(|r| r.path.clone()).collect();

    let rows: Vec<Row> = app
        .view
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let repo = &app.repos[i];
            let is_selected = app.selected[i];
            let color = reason_color(&repo.reason);

            let sel_cell = if is_prune_mode {
                if matches!(repo.reason, SkipReason::Candidate) {
                    if is_selected {
                        Cell::from("[x]").style(
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Cell::from("[ ]").style(Style::default().fg(Color::DarkGray))
                    }
                } else {
                    Cell::from(" — ").style(Style::default().fg(Color::DarkGray))
                }
            } else {
                Cell::from(format!("{}", row + 1)).style(Style::default().fg(Color::DarkGray))
            };

            let path_str = crate::engine::compute_display_name(&repo.path, &all_paths);

            let reason_str = repo.reason.to_string();
            let adapters_str = if repo.adapters.is_empty() {
                "—".to_string()
            } else {
                repo.adapters.join(", ")
            };
            let bloat_str = if repo.reclaimable_bytes > 0 {
                format_bytes(repo.reclaimable_bytes)
            } else {
                "—".to_string()
            };
            // Green means "you can have these bytes back" everywhere in this tool, so a
            // dash — nothing to reclaim — must not borrow the same colour.
            let bloat_color = if repo.reclaimable_bytes > 0 {
                Color::Green
            } else {
                Color::DarkGray
            };
            let activity_str = repo
                .last_activity
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "—".to_string());
            let pruned_str = repo
                .entry
                .last_pruned_at
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "Never".to_string());

            let row_style = if is_selected {
                Style::default().bg(Color::Rgb(20, 50, 30))
            } else {
                Style::default()
            };

            // `row`, not `i`: the highlight is a position in the view, and comparing it to
            // a repository index lit the wrong row whenever a sort or filter was active.
            let on_dark_bg = is_selected || highlighted_row == Some(row);
            let path_style = if on_dark_bg {
                Style::default().fg(Color::White)
            } else {
                Style::default()
            };
            let date_color = if on_dark_bg {
                Color::Gray
            } else {
                Color::DarkGray
            };

            Row::new(vec![
                sel_cell,
                Cell::from(path_str).style(path_style),
                Cell::from(reason_str).style(Style::default().fg(color)),
                Cell::from(adapters_str),
                Cell::from(bloat_str).style(Style::default().fg(bloat_color)),
                Cell::from(activity_str).style(Style::default().fg(date_color)),
                Cell::from(pruned_str).style(Style::default().fg(date_color)),
            ])
            .style(row_style)
        })
        .collect();

    // Beside the table when there is room for both, below it on a narrow but tall
    // terminal, and not at all when neither fits. Hiding it outright below 110 columns
    // meant a split pane or a laptop screen never saw it, with nothing saying why.
    let (table_area, inspector_area, compact) = if !app.show_inspector {
        (outer[1], None, false)
    } else if frame.area().width >= 110 {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(outer[1]);
        (horizontal[0], Some(horizontal[1]), true)
    } else if outer[1].height >= 24 {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(outer[1]);
        (vertical[0], Some(vertical[1]), false)
    } else {
        (outer[1], None, false)
    };
    app.inspector_drawn = inspector_area.is_some();

    let table_constraints = if compact {
        vec![
            Constraint::Length(4),  // sel/#
            Constraint::Min(16),    // path
            Constraint::Length(18), // reason
            Constraint::Length(12), // adapters
            Constraint::Length(10), // bloat
            Constraint::Length(0),  // last activity
            Constraint::Length(0),  // last pruned
        ]
    } else {
        vec![
            Constraint::Length(4),  // sel/#
            Constraint::Min(24),    // path
            Constraint::Length(22), // reason
            Constraint::Length(16), // adapters
            Constraint::Length(11), // bloat
            Constraint::Length(13), // last activity
            Constraint::Length(13), // last pruned
        ]
    };

    let table = Table::new(rows, table_constraints)
        .header(col_headers)
        .block(
            Block::default()
                .title(" Registered Repositories ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 40, 70))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    // The real state, not a clone: ratatui stores the computed scroll offset back into
    // it, and rendering into a throwaway copy pins the viewport to the top — the
    // moment the selection moved below the visible rows it simply left the screen.
    frame.render_stateful_widget(table, table_area, &mut app.table_state);

    if let Some(insp_area) = inspector_area {
        render_inspector(frame, insp_area, app);
    }

    // ── Footer ────────────────────────────────────────────────────────────────
    let mut footer_lines = if is_prune_mode {
        vec![
            Line::from(vec![
                Span::styled("Selected: ", Style::default().fg(Color::DarkGray)),
                // Bold carries the count; green carries the bytes, the same as it does
                // in every other view. The whole line used to be yellow, which is the
                // colour this screen already uses for "you are in the destructive mode"
                // — saying it twice made neither reading land.
                Span::styled(
                    // "of N candidates" undercounted the moment Space started accepting
                    // active repositories, and on a machine with no idle repository at
                    // all it read "0 of 0" while rows were plainly selectable.
                    format!(
                        "{} of {} selectable  ",
                        app.selected_count(),
                        app.selectable_count()
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({})", format_bytes(app.selected_bytes())),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("[↑/↓/j/k]", Style::default().fg(Color::Cyan)),
                Span::raw(" Navigate  "),
                Span::styled("[PgUp/PgDn/g/G]", Style::default().fg(Color::Cyan)),
                Span::raw(" Jump  "),
                Span::styled("[Space]", Style::default().fg(Color::Cyan)),
                Span::raw(" Toggle  "),
                Span::styled("[a]", Style::default().fg(Color::Cyan)),
                Span::raw(" Toggle All  "),
                Span::styled(
                    "[Enter]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Prune Selected  "),
                Span::styled("[Esc]", Style::default().fg(Color::Cyan)),
                Span::raw(" Back to Browse  "),
                Span::styled("[q]", Style::default().fg(Color::Cyan)),
                Span::raw(" Quit"),
            ]),
            // Reuses the blank line the footer already reserved, so the notice costs no
            // height against the fixed footer constraint.
            Line::from(match &app.notice {
                Some(msg) => vec![Span::styled(
                    format!("⚠ {msg}"),
                    Style::default().fg(Color::Yellow),
                )],
                None => vec![],
            }),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("Legend: ", Style::default().fg(Color::DarkGray)),
                Span::styled("■ Candidate", Style::default().fg(Color::Green)),
                Span::raw("  "),
                Span::styled("■ Active", Style::default().fg(Color::Reset)),
                Span::raw("  "),
                // One swatch, because these two share a colour: neither needs anything
                // from you, and a legend with two identical squares is worse than one.
                Span::styled("■ Ignored / No Bloat", Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled("■ Path Missing", Style::default().fg(Color::Red)),
            ]),
            Line::from(vec![
                Span::styled("[↑/↓/j/k]", Style::default().fg(Color::Cyan)),
                Span::raw(" Navigate  "),
                Span::styled("[PgUp/PgDn/g/G]", Style::default().fg(Color::Cyan)),
                Span::raw(" Jump  "),
                // Every key is cyan; the two that change what the view does are also
                // bold. They used to be yellow and magenta, which said "warning" and
                // "nothing" respectively, and red is needed for real failures rather
                // than for the key that closes a screen.
                Span::styled(
                    "[p]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Prune-Select Mode  "),
                Span::styled(
                    "[Tab]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Inspector  "),
                Span::styled(
                    "[i]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Toggle Ignore  "),
                Span::styled("[q/Esc/Ctrl-C]", Style::default().fg(Color::Cyan)),
                Span::raw(" Quit"),
            ]),
            Line::from(vec![
                Span::styled(
                    "[i] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "asks, then toggles `ignore` in `.devprune.json` (kept out of `git status` via `.git/info/exclude`). Inert where a committed `project.devprune.json` sets `ignore`; edit that file instead.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ]
    };

    // The state of the view, on both footers. Without it a filtered or searched table is
    // indistinguishable from a machine that simply has fewer repositories on it — and
    // the count in the header would then read as a fact about the machine.
    let mut state_line = vec![
        Span::styled(
            "[s]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Sort: "),
        Span::styled(app.sort.label(), Style::default().fg(Color::Green)),
        Span::raw("   "),
        Span::styled(
            "[f]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Filter: "),
        Span::styled(app.filter.label(), Style::default().fg(Color::Green)),
        Span::raw("   "),
        Span::styled(
            "[/]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Search: "),
    ];
    if app.searching {
        // A block for the caret, because raw mode gives the terminal's own cursor
        // nowhere useful to sit — the table has it.
        state_line.push(Span::styled(
            format!("{}█", app.search),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        state_line.push(Span::styled(
            "  (Enter to keep, Esc to clear)",
            Style::default().fg(Color::DarkGray),
        ));
    } else if app.search.is_empty() {
        state_line.push(Span::styled("—", Style::default().fg(Color::DarkGray)));
    } else {
        state_line.push(Span::styled(
            app.search.clone(),
            Style::default().fg(Color::Yellow),
        ));
    }
    if app.view.is_empty() {
        // The one state where the table itself says nothing at all. Silence here reads
        // as a crash, so the footer has to explain what hid the rows.
        state_line.push(Span::styled(
            "   no repositories match",
            Style::default().fg(Color::Red),
        ));
    }
    footer_lines.push(Line::from(state_line));

    // The credit, on both footers, in the dimmest colour the theme has. It is one
    // constant and one push — a fork that does not want it deletes these two lines and
    // nothing else in the binary cares.
    footer_lines.push(Line::from(Span::styled(
        constants::ATTRIBUTION_LINE,
        Style::default().fg(Color::DarkGray),
    )));

    let footer =
        Paragraph::new(footer_lines).block(Block::default().borders(Borders::ALL).border_style(
            Style::default().fg(if is_prune_mode {
                Color::Yellow
            } else {
                Color::Green
            }),
        ));
    frame.render_widget(footer, outer[2]);

    if let Some((idx, would_ignore)) = app.pending_ignore {
        let repo = &app.repos[idx];
        let name = crate::engine::compute_display_name(&repo.path, &all_paths);
        let (title, question) = if would_ignore {
            (
                " Ignore repository ",
                format!("Ignore {name}? dev-prune leaves it alone until you un-ignore it."),
            )
        } else {
            (
                " Stop ignoring ",
                format!(
                    "Stop ignoring {name}? It can be pruned again once it has been idle {} days.",
                    repo.idle_days
                ),
            )
        };
        let text = vec![
            Line::raw(question),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    "[y]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(if would_ignore {
                    " Ignore  "
                } else {
                    " Stop ignoring  "
                }),
                Span::styled("[n/Esc]", Style::default().fg(Color::Cyan)),
                Span::raw(" Cancel"),
            ]),
        ];
        let area = centered(frame.area(), 64, 7);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
            area,
        );
    }
}

/// A `width` by `height` rectangle in the middle of `area`, shrunk to fit it.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// The highlighted repository in full: what the table's one number per row is made of,
/// and the history that says whether the repository is abandoned or only quiet. Those
/// two questions decide whether to prune, and neither fits in a table column.
fn render_inspector(frame: &mut Frame, area: Rect, app: &StatusApp) {
    let highlighted_row = app.table_state.selected();
    let selected_repo = highlighted_row
        .and_then(|row| app.view.get(row))
        .map(|&idx| (idx, &app.repos[idx]));

    let Some((idx, repo)) = selected_repo else {
        let empty = Paragraph::new("  No repository selected.").block(
            Block::default()
                .title(" Repository Inspector [Tab] ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(empty, area);
        return;
    };
    // `None` until the worker reports back; a missing path never gets a worker at all.
    let detail = app.details.get(&idx);
    let pending = detail.is_none() && !matches!(repo.reason, SkipReason::PathMissing);

    let mut lines = Vec::new();

    // ── Repository Header ──────────────────────────────────────────────
    lines.push(Line::from(vec![
        Span::styled(" Path: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            crate::output::clean_path(&repo.path),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(ref id) = repo.entry.identity {
        let short = if id.len() > 10 {
            &id[..10]
        } else {
            id.as_str()
        };
        lines.push(Line::from(vec![
            Span::styled(" Git Identity: ", Style::default().fg(Color::DarkGray)),
            Span::styled(short, Style::default().fg(Color::Cyan)),
        ]));
    }

    lines.push(Line::raw(""));

    // ── Status & Reason ────────────────────────────────────────────────
    let color = reason_color(&repo.reason);
    lines.push(Line::from(vec![
        Span::styled(" Status: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            repo.reason.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled(" Idle Threshold: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} days", repo.idle_days),
            Style::default().fg(Color::White),
        ),
    ]));

    let activity_str = repo
        .last_activity
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "Never".to_string());
    lines.push(Line::from(vec![
        Span::styled(" Last Activity: ", Style::default().fg(Color::DarkGray)),
        Span::styled(activity_str, Style::default().fg(Color::White)),
    ]));

    let pruned_str = repo
        .entry
        .last_pruned_at
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "Never".to_string());
    lines.push(Line::from(vec![
        Span::styled(" Last Pruned: ", Style::default().fg(Color::DarkGray)),
        Span::styled(pruned_str, Style::default().fg(Color::White)),
    ]));

    if repo.entry.total_freed_bytes > 0 {
        lines.push(Line::from(vec![
            Span::styled(" Lifetime Saved: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_bytes(repo.entry.total_freed_bytes),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Last Activity is one date; this shows whether that date was the tail of steady work
    // or a single commit in an otherwise dead repository.
    let commits: Vec<Span> = match detail.map(|d| &d.weekly_commits) {
        Some(Some(weeks)) => vec![
            Span::styled(sparkline(weeks), Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("  {} commits", weeks.iter().sum::<u32>()),
                Style::default().fg(Color::DarkGray),
            ),
        ],
        Some(None) => vec![Span::styled(
            "git would not read this history",
            Style::default().fg(Color::DarkGray),
        )],
        None if pending => vec![Span::styled(
            "reading git log…",
            Style::default().fg(Color::DarkGray),
        )],
        None => vec![Span::styled("—", Style::default().fg(Color::DarkGray))],
    };
    lines.push(Line::styled(
        format!(" Commits, last {HISTOGRAM_WEEKS} weeks:"),
        Style::default().fg(Color::DarkGray),
    ));
    let mut commit_line = vec![Span::raw("   ")];
    commit_line.extend(commits);
    lines.push(Line::from(commit_line));

    lines.push(Line::raw(""));

    // ── Detected Adapters ──────────────────────────────────────────────
    let adapters_str = if repo.adapters.is_empty() {
        "—".to_string()
    } else {
        repo.adapters.join(", ")
    };
    lines.push(Line::from(vec![
        Span::styled(" Adapters: ", Style::default().fg(Color::DarkGray)),
        Span::styled(adapters_str, Style::default().fg(Color::Yellow)),
    ]));

    lines.push(Line::raw(""));

    // ── Bloat Breakdown ────────────────────────────────────────────────
    lines.push(Line::from(vec![Span::styled(
        " Bloat Breakdown: ",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::UNDERLINED),
    )]));

    if repo.bloat_dirs.is_empty() {
        lines.push(Line::styled(
            "   (no bloat directories found)",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        for b in &repo.bloat_dirs {
            let shared_note = if b.shared_bytes > 0 {
                format!(" ({} store-shared)", format_bytes(b.shared_bytes))
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   • {:<16} ", b.name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format_bytes(b.size_bytes),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(shared_note, Style::default().fg(Color::DarkGray)),
            ]));
        }

        // One directory's total says how much; this says which package is to blame,
        // which is what decides between pruning and fixing a dependency.
        lines.push(Line::styled(
            " Largest inside:",
            Style::default().fg(Color::DarkGray),
        ));
        match detail {
            Some(d) if !d.largest.is_empty() => {
                for (name, size) in &d.largest {
                    lines.push(Line::from(vec![
                        Span::raw(format!("   {name}  ")),
                        Span::styled(format_bytes(*size), Style::default().fg(Color::Green)),
                    ]));
                }
            }
            Some(_) => lines.push(Line::styled(
                "   (empty)",
                Style::default().fg(Color::DarkGray),
            )),
            None => lines.push(Line::styled(
                "   measuring…",
                Style::default().fg(Color::DarkGray),
            )),
        }
    }

    if !repo.tools.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            " Tool Footprints: ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::UNDERLINED),
        )]));
        for t in &repo.tools {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   • {:<16} ", t.name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format_bytes(t.size_bytes),
                    Style::default().fg(Color::Magenta),
                ),
            ]));
        }
    }

    let block = Block::default()
        .title(" Repository Inspector [Tab] ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Wrapped, because below the table the inspector gets the terminal's full width but
    // beside it only 40%, and a long path or package name was simply cut off.
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, area);
}

// ── Plain-text fallback ──────────────────────────────────────────────────────

/// Plain text fallback rendering for non-TUI environments.
pub fn render_status_plain(repos: &[RepoStatusEntry]) {
    use crate::output;
    use colored::Colorize;

    output::print_header("dev-prune status");
    // The repository column is padded in terminal columns, not `char`s: it is the only
    // column whose contents a user names, so it is the only one that can hold wide
    // characters. The rule spans the whole row — 3+35+22+12+12+13+13 plus six 2-space
    // gaps — which is 122, not the 118 it used to draw.
    println!(
        "\n  {:>3}  {}  {:<22}  {:<12}  {:<12}  {:<13}  {:<13}",
        "#",
        output::pad_display("Repository", 35),
        "Status / Reason",
        "Adapters",
        "Bloat",
        "Last Activity",
        "Last Pruned"
    );
    println!("  {}", "─".repeat(122));

    let all_paths: Vec<_> = repos.iter().map(|r| r.path.clone()).collect();
    for (i, repo) in repos.iter().enumerate() {
        let path_str = crate::engine::compute_display_name(&repo.path, &all_paths);
        let reason = repo.reason.to_string();
        let adapters = if repo.adapters.is_empty() {
            "—".to_string()
        } else {
            repo.adapters.join("+")
        };
        let bloat = if repo.reclaimable_bytes > 0 {
            format_bytes(repo.reclaimable_bytes)
        } else {
            "—".to_string()
        };
        let activity = repo
            .last_activity
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "—".to_string());
        let pruned = repo
            .entry
            .last_pruned_at
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "Never".to_string());

        // Padded *before* colouring: ANSI escapes count toward `{:<22}`-style format
        // widths, so colouring inside the format string would shear every column that
        // follows. `colored` emits nothing when stdout is a pipe, so the plain widths
        // survive redirection untouched.
        //
        // `Colorize::` spelled out, not `cell.green()`: this module imports
        // `ratatui::prelude::*`, which brings `ratatui::style::Stylize` into scope with a
        // `green()` of its own that takes `self` by value. It wins the method probe over
        // `colored`'s, which needs an autoref, so `cell.green()` silently built a ratatui
        // `Span` and `.to_string()` handed back the text with no escapes at all. This
        // table printed in plain white for every release up to 1.3.0 and nothing failed.
        // Three tiers, not six colours. Green marks the rows you can act on, red and
        // yellow the two that are actually broken, and everything that is merely normal
        // — in use, nothing to reclaim, deliberately ignored — is default or dim. A
        // colour per enum variant reads as decoration and leaves nothing louder to say
        // when a repository really is misconfigured.
        let reason_cell = format!("{reason:<22}");
        let reason_cell = match &repo.reason {
            SkipReason::Candidate => Colorize::green(reason_cell.as_str()).to_string(),
            SkipReason::Active => reason_cell,
            SkipReason::Ignored | SkipReason::NoBloat => {
                Colorize::dimmed(reason_cell.as_str()).to_string()
            }
            SkipReason::PathMissing => Colorize::red(reason_cell.as_str()).to_string(),
            SkipReason::ConfigError(_) => Colorize::yellow(reason_cell.as_str()).to_string(),
        };
        // Green, not bold green: this figure repeats on every row, and bolding all of
        // them means none of them stands out. The bold copy is the grand total below.
        let bloat_cell = if repo.reclaimable_bytes > 0 {
            Colorize::green(format!("{bloat:<12}").as_str()).to_string()
        } else {
            format!("{bloat:<12}")
        };
        println!(
            "  {:>3}  {}  {}  {:<12}  {}  {:<13}  {:<13}",
            i + 1,
            output::pad_display(&path_str, 35),
            reason_cell,
            adapters,
            bloat_cell,
            activity,
            pruned
        );
    }
    println!("  {}", "─".repeat(122));

    let (ready, total) = reclaimable_split(repos);
    let candidates = repos
        .iter()
        .filter(|r| matches!(r.reason, SkipReason::Candidate))
        .count();
    // The one bold figure on the screen — and it is the *actionable* one, not the grand
    // total. Each row's own bloat is plain green; bolding what a prune would free right
    // now is what makes it the line the eye lands on.
    output::print_info(&format!(
        "Total: {} repos  |  {} candidates  |  {} ready now  |  {} reclaimable in all",
        repos.len(),
        candidates,
        output::format_bytes_styled(ready),
        output::format_bytes(total)
    ));

    // Reclaimable is what a prune actually frees, which for pnpm and bun is less than
    // the folder's apparent size — the rest is hardlinked into the manager's store.
    // Said once here rather than per row so the table stays scannable.
    let shared: u64 = repos
        .iter()
        .flat_map(|r| &r.bloat_dirs)
        .map(|b| b.shared_bytes)
        .sum();
    if shared > 0 {
        output::print_info(&format!(
            "Excluded: {} hardlinked into package-manager stores (pnpm/bun) — deleting \
             node_modules does not free those bytes, the store keeps them.",
            format_bytes(shared)
        ));
    }

    // AI-assistant tool output (graphify, understand-anything) is never a prune
    // candidate: nothing here has a lockfile proving it can be rebuilt, so `devp run`
    // does not touch it. Reported once, in its own block, so its size is not a mystery.
    let all_paths: Vec<_> = repos.iter().map(|r| r.path.clone()).collect();
    let with_tools: Vec<_> = repos.iter().filter(|r| !r.tools.is_empty()).collect();
    if !with_tools.is_empty() {
        println!();
        output::print_info("Tools (report-only, never pruned):");
        for repo in with_tools {
            let path_str = crate::engine::compute_display_name(&repo.path, &all_paths);
            for tool in &repo.tools {
                println!(
                    "  {}  {:<20}  {}",
                    output::pad_display(&path_str, 35),
                    tool.name,
                    format_bytes(tool.size_bytes)
                );
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use crate::engine::SkipReason;

    use super::*;
    use crate::engine::RepoStatusEntry;

    #[test]
    fn test_row_style_logic() {
        assert_eq!(reason_color(&SkipReason::Candidate), Color::Green);
        assert_eq!(reason_color(&SkipReason::Active), Color::Reset);
        assert_eq!(reason_color(&SkipReason::Ignored), Color::DarkGray); // merged Disabled+Ignored
        assert_eq!(reason_color(&SkipReason::NoBloat), Color::DarkGray);
        assert_eq!(reason_color(&SkipReason::PathMissing), Color::Red);
        assert_eq!(
            reason_color(&SkipReason::ConfigError(String::new())),
            Color::Yellow
        );
    }

    /// One entry, with only the fields the view actually orders and filters on.
    fn entry(path: &str, reason: SkipReason, bytes: u64, days_idle: i64) -> RepoStatusEntry {
        RepoStatusEntry {
            path: std::path::PathBuf::from(path),
            entry: crate::config::RepoEntry::new(),
            reason,
            adapters: vec!["uv".to_string()],
            bloat_dirs: Vec::new(),
            reclaimable_bytes: bytes,
            reclaimable_by_adapter: Vec::new(),
            last_activity: Some(chrono::Utc::now() - chrono::Duration::days(days_idle)),
            idle_days: 30,
            tools: Vec::new(),
        }
    }

    fn sample() -> Vec<RepoStatusEntry> {
        vec![
            entry("/code/alpha", SkipReason::Candidate, 500, 90),
            entry("/code/beta", SkipReason::Active, 9_000, 1),
            entry("/code/gamma", SkipReason::PathMissing, 0, 400),
            entry("/code/delta", SkipReason::Candidate, 2_000, 40),
        ]
    }

    #[test]
    fn the_default_view_is_every_row_in_the_order_it_arrived() {
        let repos = sample();
        let app = StatusApp::new(&repos);
        assert_eq!(app.view, vec![0, 1, 2, 3]);
    }

    #[test]
    fn sorting_by_size_puts_the_biggest_reclaim_first() {
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.sort = SortKey::Size;
        app.rebuild_view();
        assert_eq!(app.view, vec![1, 3, 0, 2]);
    }

    #[test]
    fn sorting_by_activity_puts_the_longest_untouched_first() {
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.sort = SortKey::Activity;
        app.rebuild_view();
        assert_eq!(app.view, vec![2, 0, 3, 1]);
    }

    #[test]
    fn filters_narrow_the_view_without_disturbing_the_selection() {
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.mode = ViewMode::PruneSelect;
        app.selected[3] = true;

        app.filter = Filter::Candidates;
        app.rebuild_view();
        assert_eq!(app.view, vec![0, 3]);

        app.filter = Filter::Problems;
        app.rebuild_view();
        assert_eq!(app.view, vec![2]);

        // The checked repository is filtered off screen, and stays checked: a filter is
        // a way to look, not a way to silently un-arm a prune.
        assert!(app.selected[3]);
        assert_eq!(app.selected_count(), 1);
    }

    #[test]
    fn a_search_matches_the_path_and_the_adapters() {
        let repos = sample();
        let mut app = StatusApp::new(&repos);

        app.search = "ELT".to_string();
        app.rebuild_view();
        assert_eq!(app.view, vec![3], "the match is case-insensitive");

        app.search = "uv".to_string();
        app.rebuild_view();
        assert_eq!(app.view, vec![0, 1, 2, 3], "every sample repo reports uv");

        app.search = "nothing-matches-this".to_string();
        app.rebuild_view();
        assert!(app.view.is_empty());
        // Nothing on screen means nothing highlighted — the `i` and prune keys read the
        // cursor, and a cursor pointing into an empty view would index out of bounds.
        assert_eq!(app.cursor_repo(), None);
    }

    #[test]
    fn the_cursor_follows_its_repository_through_a_filter() {
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.table_state.select(Some(3)); // `/code/delta`
        assert_eq!(app.cursor_repo(), Some(3));

        app.filter = Filter::Candidates;
        app.rebuild_view();
        // Row 1 of the two-row view — a different row number, the same repository.
        assert_eq!(app.cursor_repo(), Some(3));
    }

    #[test]
    fn toggle_all_is_scoped_to_what_is_on_screen() {
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.search = "alpha".to_string();
        app.rebuild_view();
        app.toggle_all_candidates();

        assert!(app.selected[0]);
        assert!(
            !app.selected[3],
            "delta was filtered out and must not be armed"
        );
    }

    #[test]
    fn the_header_separates_what_is_prunable_now_from_everything() {
        // The whole point of the split: `beta` is 9 KB of dependencies in a repository
        // somebody worked in yesterday. It counts towards the machine's footprint and
        // towards nothing a prune would do today.
        let repos = sample();
        let (ready, total) = reclaimable_split(&repos);
        assert_eq!(ready, 2_500, "alpha + delta, the two candidates");
        assert_eq!(total, 11_500, "every registered repository");
    }

    #[test]
    fn a_filter_never_changes_the_header_totals() {
        // A dashboard that shrank its own totals when a filter was applied would make
        // the machine look tidier than it is.
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        let before = reclaimable_split(app.repos);

        app.filter = Filter::Problems;
        app.rebuild_view();
        assert_eq!(app.view.len(), 1);
        assert_eq!(reclaimable_split(app.repos), before);
    }

    #[test]
    fn space_checks_a_repository_that_is_only_too_recently_worked_in() {
        // The bug this covers: every repository on a machine used daily is `Active`,
        // so a candidate-only rule left prune-select with nothing it would accept and
        // no way to tell that apart from a mode that had failed to open.
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.mode = ViewMode::PruneSelect;
        app.table_state.select(Some(1)); // `/code/beta` — active, 9 KB to reclaim

        app.toggle_current();
        assert!(
            app.selected[1],
            "an active repository is the user's to pick"
        );
        assert!(app.notice.is_none());

        app.toggle_current();
        assert!(!app.selected[1], "and the same key unchecks it");
    }

    #[test]
    fn space_refuses_what_a_prune_could_not_deliver_and_says_why() {
        let repos = vec![
            entry("/code/ignored", SkipReason::Ignored, 500, 90),
            entry("/code/empty", SkipReason::NoBloat, 0, 90),
            entry("/code/gone", SkipReason::PathMissing, 0, 90),
            entry(
                "/code/broken",
                SkipReason::ConfigError("bad key".into()),
                0,
                90,
            ),
        ];
        let mut app = StatusApp::new(&repos);
        app.mode = ViewMode::PruneSelect;

        for i in 0..repos.len() {
            app.table_state.select(Some(i));
            app.toggle_current();
            assert!(!app.selected[i], "row {i} must stay unchecked");
            assert!(
                app.notice.is_some(),
                "row {i} refused a keypress without explaining itself"
            );
        }

        // The un-ignore affordance is on this screen; the refusal points at it.
        app.table_state.select(Some(0));
        app.toggle_current();
        assert!(app.notice.as_deref().unwrap().contains("[i]"));
    }

    #[test]
    fn an_active_repository_with_nothing_to_reclaim_is_still_refused() {
        // Selectable is "the idle threshold is the only thing in the way", not "active".
        let repos = vec![entry("/code/bare", SkipReason::Active, 0, 1)];
        let mut app = StatusApp::new(&repos);
        app.mode = ViewMode::PruneSelect;
        app.table_state.select(Some(0));

        app.toggle_current();
        assert!(!app.selected[0]);
    }

    #[test]
    fn the_footer_counts_every_row_space_would_accept() {
        // Not `candidate_count`: it read "0 of 0" on a machine where rows were plainly
        // selectable, which is the number that made the mode look broken.
        let repos = sample();
        let app = StatusApp::new(&repos);
        assert_eq!(app.candidate_count(), 2, "alpha + delta");
        assert_eq!(
            app.selectable_count(),
            3,
            "alpha + delta + beta, the active one with bloat"
        );
    }

    #[test]
    fn toggle_all_leaves_active_repositories_alone() {
        // Enter prunes with no second confirmation, so `a` must never arm a repository
        // somebody is working in today.
        let repos = sample();
        let mut app = StatusApp::new(&repos);
        app.toggle_all_candidates();

        assert!(app.selected[0], "alpha is a candidate");
        assert!(app.selected[3], "delta is a candidate");
        assert!(!app.selected[1], "beta is active and must be opted into");
    }

    #[test]
    fn weeks_are_bucketed_oldest_first_and_the_window_is_strict() {
        const WEEK: i64 = 7 * 24 * 60 * 60;
        let now = 100 * WEEK;
        let stamps = [
            now,            // this week
            now - 10,       // this week
            now - WEEK - 1, // last week
            now - 3 * WEEK, // outside a 3-week window
            now + 60,       // a skewed clock in the future
        ];
        assert_eq!(bucket_by_week(&stamps, now, 3), vec![0, 1, 2]);
    }

    #[test]
    fn an_empty_week_is_a_dot_and_the_busiest_is_a_full_bar() {
        let line = sparkline(&[0, 1, 8, 4]);
        let chars: Vec<char> = line.chars().collect();
        assert_eq!(chars.len(), 4);
        assert_eq!(chars[0], '·');
        assert_ne!(chars[1], '·', "one commit must not look like none");
        assert_eq!(chars[2], '█');
        assert_eq!(sparkline(&[0, 0]), "··");
    }

    #[test]
    fn largest_entries_are_sorted_labelled_and_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let modules = dir.path().join("node_modules");
        std::fs::create_dir_all(modules.join("big")).unwrap();
        std::fs::write(modules.join("big").join("a.bin"), vec![0u8; 4096]).unwrap();
        std::fs::write(modules.join("small.txt"), vec![0u8; 10]).unwrap();
        std::fs::write(modules.join("mid.txt"), vec![0u8; 100]).unwrap();

        let dirs = vec![("node_modules".to_string(), modules)];
        let top = largest_entries(&dirs, 2);
        let names: Vec<&str> = top.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["node_modules/big", "node_modules/mid.txt"]);
        assert!(top[0].1 >= 4096);
    }

    #[test]
    fn i_offers_to_stop_ignoring_a_repository_held_by_the_legacy_marker() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = entry("/unused", SkipReason::Ignored, 0, 90);
        repo.path = dir.path().to_path_buf();

        assert_eq!(ignore_toggle_for(&repo).unwrap(), Some(true));

        std::fs::write(
            dir.path().join(crate::constants::DEVPRUNE_IGNORE_FILE),
            "{}",
        )
        .unwrap();
        assert_eq!(
            ignore_toggle_for(&repo).unwrap(),
            Some(false),
            "the marker means it is already ignored, so `i` must un-ignore"
        );

        apply_ignore_toggle(&repo, false).unwrap();
        assert!(
            !dir.path()
                .join(crate::constants::DEVPRUNE_IGNORE_FILE)
                .exists()
        );
        assert_eq!(ignore_toggle_for(&repo).unwrap(), Some(true));
    }

    #[test]
    fn i_has_nothing_to_offer_on_a_missing_path() {
        let repo = entry("/code/gone", SkipReason::PathMissing, 0, 90);
        assert_eq!(ignore_toggle_for(&repo).unwrap(), None);
    }
}
