// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Terminal UI for the `dev-prune status` command.
//
// Renders a full interactive, scrollable table of all registered repositories
// showing: status, reason skipped, last activity, last pruned, bloat size,
// and adapters. Users can also select candidates and trigger a prune pass
// directly from this view.

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

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

struct StatusApp<'a> {
    repos: &'a [RepoStatusEntry],
    table_state: TableState,
    /// Which rows are checked for prune (indexed to `repos`).
    selected: Vec<bool>,
    mode: ViewMode,
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
        Self {
            repos,
            table_state,
            selected,
            mode: ViewMode::Browse,
            confirmed_indices: None,
            should_reload: false,
        }
    }

    fn move_up(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.repos.len().saturating_sub(1)
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
                if i >= self.repos.len().saturating_sub(1) {
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
        if let Some(i) = self.table_state.selected() {
            // Only allow toggling candidate repos for pruning
            if matches!(self.repos[i].reason, SkipReason::Candidate) {
                self.selected[i] = !self.selected[i];
            }
        }
    }

    fn toggle_all_candidates(&mut self) {
        let any_candidate_selected = self
            .repos
            .iter()
            .enumerate()
            .any(|(i, r)| matches!(r.reason, SkipReason::Candidate) && self.selected[i]);

        for (i, repo) in self.repos.iter().enumerate() {
            if matches!(repo.reason, SkipReason::Candidate) {
                self.selected[i] = !any_candidate_selected;
            }
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

/// Render the full interactive status view.
///
/// Returns `Some(Vec<usize>)` of selected repo indices if the user confirmed
/// a prune, or `None` if they just quit.
///
/// Re-runs automatically when the user toggles ignore config in `devprune.json` so the
/// status reflects the change immediately.
pub fn render_status_tui(
    repos_loader: &dyn Fn() -> Vec<RepoStatusEntry>,
) -> Result<Option<Vec<usize>>> {
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

        return Ok(app.confirmed_indices);
    }
}

fn run_status_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut StatusApp,
) -> Result<()> {
    loop {
        terminal.draw(|frame| render_ui(frame, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
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
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    KeyCode::Home | KeyCode::Char('g') => app.table_state.select(Some(0)),
                    KeyCode::End | KeyCode::Char('G') => {
                        app.table_state
                            .select(Some(app.repos.len().saturating_sub(1)));
                    }
                    KeyCode::PageUp => {
                        let i = app.table_state.selected().unwrap_or(0).saturating_sub(10);
                        app.table_state.select(Some(i));
                    }
                    KeyCode::PageDown => {
                        let i = (app.table_state.selected().unwrap_or(0) + 10)
                            .min(app.repos.len().saturating_sub(1));
                        app.table_state.select(Some(i));
                    }
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
                        // Auto-select all candidates
                        for (i, repo) in app.repos.iter().enumerate() {
                            if matches!(repo.reason, SkipReason::Candidate) {
                                app.selected[i] = true;
                            }
                        }
                    }
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        // Toggle ignore in .devprune.json on the current repo
                        if let Some(idx) = app.table_state.selected() {
                            let repo = &app.repos[idx];
                            // Refuses a config that does not parse. Starting from the
                            // defaults would have written a fresh file over the broken
                            // one, discarding every other override it held — and the
                            // dashboard already shows such a repo as `config_error`.
                            let mut per_repo =
                                crate::config::PerRepoConfig::load_with_diagnostics(&repo.path)
                                    .map_err(|e| anyhow::anyhow!(e))
                                    .with_context(|| {
                                        format!(
                                            "Could not toggle ignore for {}",
                                            crate::output::clean_path(&repo.path)
                                        )
                                    })?
                                    .unwrap_or_default();
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
                            let legacy_ignore =
                                repo.path.join(crate::constants::DEVPRUNE_IGNORE_FILE);
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
                            for s in &mut app.selected {
                                *s = false;
                            }
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
}

fn reason_color(reason: &SkipReason) -> Color {
    match reason {
        SkipReason::Candidate => Color::Green,
        SkipReason::Active => Color::Cyan,
        SkipReason::Ignored => Color::DarkGray,
        SkipReason::NoBloat => Color::Blue,
        SkipReason::PathMissing => Color::Red,
        // Actionable rather than broken: the repo is fine, its config file is not.
        SkipReason::ConfigError(_) => Color::Yellow,
    }
}

fn render_ui(frame: &mut Frame, app: &StatusApp) {
    let is_prune_mode = matches!(app.mode, ViewMode::PruneSelect);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // table
            Constraint::Length(5), // footer
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
                "  {} repos  |  {} candidates  |  {} reclaimable",
                app.repos.len(),
                app.candidate_count(),
                format_bytes(app.repos.iter().map(|r| r.reclaimable_bytes).sum::<u64>())
            ),
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
    .style(Style::default().bg(Color::Rgb(20, 25, 40)));

    let rows: Vec<Row> = app
        .repos
        .iter()
        .enumerate()
        .map(|(i, repo)| {
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
                Cell::from(format!("{}", i + 1)).style(Style::default().fg(Color::DarkGray))
            };

            let all_paths: Vec<_> = app.repos.iter().map(|r| r.path.clone()).collect();
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

            Row::new(vec![
                sel_cell,
                Cell::from(path_str).style(Style::default().fg(Color::White)),
                Cell::from(reason_str).style(Style::default().fg(color)),
                Cell::from(adapters_str).style(Style::default().fg(Color::Magenta)),
                Cell::from(bloat_str).style(Style::default().fg(Color::Cyan)),
                Cell::from(activity_str).style(Style::default().fg(Color::Gray)),
                Cell::from(pruned_str).style(Style::default().fg(Color::Gray)),
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
            .border_style(Style::default().fg(Color::Gray)),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::Rgb(30, 40, 70))
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, outer[1], &mut app.table_state.clone());

    // ── Footer ────────────────────────────────────────────────────────────────
    let footer_lines = if is_prune_mode {
        vec![
            Line::from(vec![
                Span::styled("Selected: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!(
                        "{} of {} candidates  ({})",
                        app.selected_count(),
                        app.candidate_count(),
                        format_bytes(app.selected_bytes())
                    ),
                    Style::default()
                        .fg(Color::Yellow)
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
                Span::styled("[Esc]", Style::default().fg(Color::Yellow)),
                Span::raw(" Back to Browse  "),
                Span::styled("[q]", Style::default().fg(Color::Red)),
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
                Span::styled("■ Active", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("■ No Bloat", Style::default().fg(Color::Blue)),
                Span::raw("  "),
                Span::styled("■ Ignored", Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled("■ Path Missing", Style::default().fg(Color::Red)),
            ]),
            Line::from(vec![
                Span::styled("[↑/↓/j/k]", Style::default().fg(Color::Cyan)),
                Span::raw(" Navigate  "),
                Span::styled("[PgUp/PgDn/g/G]", Style::default().fg(Color::Cyan)),
                Span::raw(" Jump  "),
                Span::styled(
                    "[p]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Prune-Select Mode  "),
                Span::styled(
                    "[i]",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Toggle Ignore  "),
                Span::styled("[q/Esc/Ctrl-C]", Style::default().fg(Color::Red)),
                Span::raw(" Quit"),
            ]),
            Line::from(vec![
                Span::styled(
                    "[i] ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "toggles `ignore` in `.devprune.json` and updates `.gitignore` — refreshes instantly.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ]
    };

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

    output::print_header("dev-prune status");
    println!(
        "\n  {:>3}  {:<35}  {:<22}  {:<12}  {:<12}  {:<13}  {:<13}",
        "#", "Repository", "Status / Reason", "Adapters", "Bloat", "Last Activity", "Last Pruned"
    );
    println!("  {}", "─".repeat(118));

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

        println!(
            "  {:>3}  {:<35}  {:<22}  {:<12}  {:<12}  {:<13}  {:<13}",
            i + 1,
            path_str,
            reason,
            adapters,
            bloat,
            activity,
            pruned
        );
    }
    println!("  {}", "─".repeat(118));

    let total: u64 = repos.iter().map(|r| r.reclaimable_bytes).sum();
    let candidates = repos
        .iter()
        .filter(|r| matches!(r.reason, SkipReason::Candidate))
        .count();
    output::print_info(&format!(
        "Total: {} repos  |  {} candidates  |  {} reclaimable",
        repos.len(),
        candidates,
        format_bytes(total)
    ));
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use crate::engine::SkipReason;

    use super::reason_color;

    #[test]
    fn test_row_style_logic() {
        assert_eq!(reason_color(&SkipReason::Candidate), Color::Green);
        assert_eq!(reason_color(&SkipReason::Active), Color::Cyan);
        assert_eq!(reason_color(&SkipReason::Ignored), Color::DarkGray); // merged Disabled+Ignored
        assert_eq!(reason_color(&SkipReason::NoBloat), Color::Blue);
        assert_eq!(reason_color(&SkipReason::PathMissing), Color::Red);
    }
}
