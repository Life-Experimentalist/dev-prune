// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Terminal UI for the `dev-prune status` command.
//
// Renders a full interactive, scrollable table of all registered repositories
// showing: status, reason skipped, last activity, last pruned, bloat size,
// and adapters. Users can also select candidates and trigger a prune pass
// directly from this view.

use std::io;
use std::path::PathBuf;
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
    /// Set to true when the user toggles ignore config in .devprune.json or presence of ignore.devprune.json so caller can reload.
    pub should_reload: bool,
}

impl<'a> StatusApp<'a> {
    fn new(repos: &'a [RepoStatusEntry]) -> Self {
        let selected = vec![false; repos.len()];
        let mut table_state = TableState::default();
        table_state.select(Some(0));
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
            should_reload: false,
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
        if let Some(i) = self.cursor_repo() {
            // Only allow toggling candidate repos for pruning
            if matches!(self.repos[i].reason, SkipReason::Candidate) {
                self.selected[i] = !self.selected[i];
            }
        }
    }

    /// Check, or uncheck, every candidate currently on screen.
    ///
    /// Deliberately scoped to the view: with a filter or a search active, "all" has to
    /// mean the rows the user can see. Selecting thirty repositories they filtered out
    /// on a keypress labelled *Toggle All* is how a prune becomes a surprise.
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
                    for i in app.view.clone() {
                        if matches!(app.repos[i].reason, SkipReason::Candidate) {
                            app.selected[i] = true;
                        }
                    }
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    // Toggle ignore in .devprune.json on the current repo
                    if let Some(idx) = app.cursor_repo() {
                        let repo = &app.repos[idx];
                        // A missing repository has no directory to write the config
                        // into; propagating that write error would tear the whole TUI
                        // down over a row that says "Path Missing" right on it.
                        if matches!(repo.reason, SkipReason::PathMissing) {
                            continue;
                        }
                        // Refuses a config that does not parse. Starting from the
                        // defaults would have written a fresh file over the broken
                        // one, discarding every other override it held — and the
                        // dashboard already shows such a repo as `config_error`.
                        let layers = crate::config::RepoConfigLayers::load(&repo.path)
                            .map_err(|e| anyhow::anyhow!(e))
                            .with_context(|| {
                                format!(
                                    "Could not toggle ignore for {}",
                                    crate::output::clean_path(&repo.path)
                                )
                            })?;
                        // A committed `project.devprune.json` that names `ignore` wins over
                        // anything this key could write, so writing it anyway would leave
                        // a personal file disagreeing with the row it did not change. Skip
                        // it the same way a missing path is skipped; the help pane says
                        // which file to edit instead.
                        if layers.source_of("ignore") == crate::config::ConfigSource::Project {
                            continue;
                        }
                        let mut per_repo = layers.personal_config().cloned().unwrap_or_default();
                        per_repo.ignore = !per_repo.ignore;
                        // Both writes are propagated rather than swallowed. A silent
                        // failure here redraws the table unchanged, which reads as a
                        // dead key; worse, a repository the user just un-ignored would
                        // still be pruned on the next pass.
                        per_repo.save_to_repo(&repo.path).with_context(|| {
                            format!(
                                "Could not write the config for {}",
                                crate::output::clean_path(&repo.path)
                            )
                        })?;

                        // The legacy marker file still counts as "ignored", so it has
                        // to go for the toggle to mean anything.
                        let legacy_ignore = repo.path.join(crate::constants::DEVPRUNE_IGNORE_FILE);
                        if legacy_ignore.exists() {
                            std::fs::remove_file(&legacy_ignore).with_context(|| {
                                format!(
                                    "Could not remove {}",
                                    crate::output::clean_path(&legacy_ignore)
                                )
                            })?;
                        }

                        // Signal caller to reload status
                        app.should_reload = true;
                        return Ok(());
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
                    } else {
                        return Ok(());
                    }
                }
                KeyCode::Char('q') => return Ok(()),
                _ => {}
            }
        }
    }
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
            // Four content lines plus the border: the keybindings, the mode-specific
            // line, and the credit line that closes both footers.
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
    let header_line = Line::from(vec![
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
    ]);

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

            let on_dark_bg = is_selected || highlighted_row == Some(i);
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

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),  // sel/#
            Constraint::Min(24),    // path
            Constraint::Length(22), // reason
            Constraint::Length(16), // adapters
            Constraint::Length(11), // bloat
            Constraint::Length(13), // last activity
            Constraint::Length(13), // last pruned
        ],
    )
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
    frame.render_stateful_widget(table, outer[1], &mut app.table_state);

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
                    format!(
                        "{} of {} candidates  ",
                        app.selected_count(),
                        app.candidate_count()
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
            Line::from(vec![]),
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
                    "toggles `ignore` in `.devprune.json` (kept out of `git status` via `.git/info/exclude`) — refreshes instantly. Inert where a committed `project.devprune.json` sets `ignore`; edit that file instead.",
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
}
