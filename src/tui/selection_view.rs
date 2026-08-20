// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Interactive TUI candidate selection view.
//
// Provides a terminal UI for users to selectively check/uncheck
// repositories before executing a prune pass.

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::engine::PruneResult;
use crate::output;
use crate::tui::Tui;

/// Candidate item for the selection list.
#[derive(Debug, Clone)]
pub struct SelectableCandidate {
    pub candidate: PruneResult,
    pub selected: bool,
}

/// Renders an interactive TUI list allowing the user to toggle which candidates to prune.
/// Returns the list of candidates that the user selected for deletion.
pub fn select_candidates_tui(candidates: &[PruneResult]) -> Result<Vec<PruneResult>> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let mut items: Vec<SelectableCandidate> = candidates
        .iter()
        .map(|c| SelectableCandidate {
            candidate: c.clone(),
            selected: true, // Default all selected
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(0));

    // The guard owns raw mode, the alternate screen and the panic hook, and puts all
    // three back on every exit path — including the `?` below.
    let mut tui = Tui::new()?;
    tui.drain_stale_input(Duration::from_millis(300));

    let confirmed = run_selection_loop(&mut tui.terminal, &mut items, &mut list_state)?;
    if confirmed {
        let selected_results = items
            .into_iter()
            .filter(|item| item.selected)
            .map(|item| item.candidate)
            .collect();
        Ok(selected_results)
    } else {
        // User cancelled with ESC / q
        Ok(Vec::new())
    }
}

fn run_selection_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    items: &mut [SelectableCandidate],
    list_state: &mut ListState,
) -> Result<bool> {
    loop {
        terminal.draw(|frame| {
            render_ui(frame, items, list_state);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Ignore KeyRelease events (common on Windows console)
                if key.kind == KeyEventKind::Release {
                    continue;
                }

                // Raw mode delivers Ctrl-C as a key event rather than a signal, so
                // without this the one key everybody reaches for to escape does nothing.
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
                {
                    return Ok(false);
                }

                let last = items.len().saturating_sub(1);
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        let i = match list_state.selected() {
                            Some(i) => {
                                if i == 0 {
                                    items.len() - 1
                                } else {
                                    i - 1
                                }
                            }
                            None => 0,
                        };
                        list_state.select(Some(i));
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let i = match list_state.selected() {
                            Some(i) => {
                                if i >= items.len() - 1 {
                                    0
                                } else {
                                    i + 1
                                }
                            }
                            None => 0,
                        };
                        list_state.select(Some(i));
                    }
                    KeyCode::Home | KeyCode::Char('g') => list_state.select(Some(0)),
                    KeyCode::End | KeyCode::Char('G') => list_state.select(Some(last)),
                    KeyCode::PageUp => {
                        let i = list_state.selected().unwrap_or(0).saturating_sub(10);
                        list_state.select(Some(i));
                    }
                    KeyCode::PageDown => {
                        let i = (list_state.selected().unwrap_or(0) + 10).min(last);
                        list_state.select(Some(i));
                    }
                    KeyCode::Char(' ') => {
                        if let Some(i) = list_state.selected() {
                            items[i].selected = !items[i].selected;
                        }
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        let all_selected = items.iter().all(|item| item.selected);
                        for item in items.iter_mut() {
                            item.selected = !all_selected;
                        }
                    }
                    KeyCode::Enter => {
                        return Ok(true);
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        return Ok(false);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn render_ui(frame: &mut Frame, items: &[SelectableCandidate], list_state: &mut ListState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header banner
            Constraint::Min(5),    // Interactive candidate list
            Constraint::Length(4), // Footer & space calculation summary
        ])
        .split(frame.area());

    // 1. Header
    let header_text = Line::from(vec![
        Span::styled(
            " dev-prune ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " Select Repositories to Prune ",
            // Default foreground, not white: this sits on the terminal's own
            // background, and white text vanishes on a light theme.
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("(v{})", crate::constants::VERSION),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let header = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(header, chunks[0]);

    // 2. List items
    // The highlighted row gets a fixed dark-blue background, so its path needs
    // explicitly light text; every other row sits on the terminal's own background,
    // where only the default foreground is readable on both light and dark themes.
    let highlighted = list_state.selected();

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let checkbox = if item.selected {
                Span::styled(
                    "[x] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("[ ] ", Style::default().fg(Color::DarkGray))
            };

            // `clean_path`, not `display()`: every other surface abbreviates the home
            // directory, and a full path here pushes the size and adapter columns off
            // the edge of an ordinary terminal.
            let repo_path = output::clean_path(&item.candidate.repo_path);
            let size_str = output::format_bytes(item.candidate.size_freed);

            let content = Line::from(vec![
                checkbox,
                Span::styled(
                    format!("{:<40}", repo_path),
                    if highlighted == Some(i) {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default()
                    },
                ),
                Span::raw(" → "),
                Span::styled(
                    format!("{:<15}", item.candidate.bloat_dir),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("({:>10}) ", size_str),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("[{}]", item.candidate.adapter_name),
                    Style::default().fg(Color::Magenta),
                ),
            ]);

            ListItem::new(content)
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .title(" Prune Candidates ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 40, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, chunks[1], list_state);

    // 3. Footer summary
    let selected_count = items.iter().filter(|i| i.selected).count();
    let selected_bytes: u64 = items
        .iter()
        .filter(|i| i.selected)
        .map(|i| i.candidate.size_freed)
        .sum();

    let footer_text = vec![
        Line::from(vec![
            Span::styled("Selected: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                // Directories, not repositories: one monorepo contributes a row per
                // ecosystem, so "3 of 5 repos" would be wrong on exactly the layout
                // this tool is built for.
                format!("{} of {} directories", selected_count, items.len()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  "),
            Span::styled("Reclaimable Space: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                output::format_bytes(selected_bytes),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Controls: ", Style::default().fg(Color::DarkGray)),
            Span::styled("[↑/↓/k/j]", Style::default().fg(Color::Cyan)),
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
            Span::styled("[q/Esc/Ctrl-C]", Style::default().fg(Color::Red)),
            Span::raw(" Cancel"),
        ]),
    ];

    let footer = Paragraph::new(footer_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );
    frame.render_widget(footer, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_selectable_candidate_struct() {
        let res = PruneResult {
            repo_path: PathBuf::from("/test/repo"),
            adapter_name: "npm".to_string(),
            bloat_dir: "node_modules".to_string(),
            size_freed: 1024,
            shared_bytes: 0,
            status: crate::engine::PruneStatus::SkippedDryRun,
        };
        let selectable = SelectableCandidate {
            candidate: res.clone(),
            selected: true,
        };
        assert!(selectable.selected);
        assert_eq!(selectable.candidate.size_freed, 1024);
    }
}
