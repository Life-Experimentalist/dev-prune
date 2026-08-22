// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The interactive configurator, used by the first-run walkthrough and by
// `devp config wizard`.
//
// One view serves both because they are the same question asked at two different
// moments: "here is everything this tool will do to your machine — change any of it
// before it starts." The line-by-line prompt in `commands::config` stays as the fallback
// for terminals this cannot run in, and as the path an agent or a script drives.

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::Tui;

/// The control a setting is edited with.
///
/// Mirrors `commands::config::Kind`, which is private to that module. Kept as its own
/// type so the view depends on nothing but its own inputs, and so a new control can be
/// added here without the settings table knowing how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Flipped in place with Space.
    Toggle,
    /// Typed into an inline field.
    Number,
    /// Opens the adapter checklist.
    Adapters,
}

/// One setting, as the view needs it.
#[derive(Debug, Clone)]
pub struct ConfigRow {
    pub key: &'static str,
    pub help: &'static str,
    pub control: Control,
    /// The value, spelled the way `devp config set` would take it.
    pub value: String,
    /// What it was when the view opened, so the summary shows only real changes.
    pub original: String,
    /// Introduced in a release newer than the one this machine last reviewed at.
    pub is_new: bool,
}

impl ConfigRow {
    pub fn changed(&self) -> bool {
        self.value != self.original
    }
}

/// One line of the declaration shown before anything is configurable.
#[derive(Debug, Clone)]
pub struct DeclarationLine {
    /// `+` for a guarantee or a safe reading, `!` for something widened, `#` for a
    /// section heading, ` ` for a plain fact.
    pub mark: char,
    pub subject: String,
    pub state: String,
}

/// What the user decided.
pub enum Outcome {
    /// Write these values back.
    Save(Vec<ConfigRow>),
    /// Everything stays as it is, and the settings count as reviewed.
    KeepAll,
    /// Escape hatch: change nothing, and do not count as reviewed either.
    Cancelled,
}

/// Everything the view needs that it cannot work out for itself.
pub struct ConfigSession<'a> {
    /// Shown above the settings; in practice the `devp trust` report.
    pub declaration: Vec<DeclarationLine>,
    /// A one-line summary of what has and has not happened yet.
    pub standing: String,
    pub rows: Vec<ConfigRow>,
    /// Every adapter name, in registry order, for the checklist.
    pub adapters: &'a [&'static str],
    /// Adapter names that need their own `enable_*` switch as well.
    pub opt_in_adapters: &'a [&'static str],
    /// Round-trips one value through the setter that owns it. `Err` is shown in place
    /// and the edit is refused, so validation lives in exactly one place.
    pub validate: &'a dyn Fn(&str, &str) -> std::result::Result<(), String>,
    /// Title bar text — the walkthrough and `config wizard` arrive here differently.
    pub title: &'a str,
}

/// Where the cursor starts: the first setting the user has never been shown, when there
/// is one. After an upgrade that setting is the only reason this screen is in front of
/// them, and making them hunt for it down a list of twenty is how it gets skipped.
fn opening_index(rows: &[ConfigRow]) -> usize {
    rows.iter().position(|r| r.is_new).unwrap_or(0)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Screen {
    Declaration,
    Settings,
    Adapters,
    Summary,
}

struct State<'a> {
    session: ConfigSession<'a>,
    screen: Screen,
    list: ListState,
    /// Buffer for an in-progress `Number` edit; `None` when not editing.
    editing: Option<String>,
    /// The last refused edit, shown until the next keypress that changes anything.
    error: Option<String>,
    /// Adapter checklist state: `true` means the adapter stays active.
    picker_active: Vec<bool>,
    picker_list: ListState,
    /// Scroll position of the declaration, which is longer than most terminals are tall.
    decl_list: ListState,
}

/// Run the configurator. Returns what the user decided; writing is the caller's job.
pub fn run(session: ConfigSession<'_>) -> Result<Outcome> {
    if session.rows.is_empty() {
        return Ok(Outcome::KeepAll);
    }

    let mut list = ListState::default();
    list.select(Some(opening_index(&session.rows)));

    let mut picker_list = ListState::default();
    picker_list.select(Some(0));

    let mut decl_list = ListState::default();
    decl_list.select(Some(0));

    let mut state = State {
        picker_active: vec![true; session.adapters.len()],
        session,
        screen: Screen::Declaration,
        list,
        editing: None,
        error: None,
        picker_list,
        decl_list,
    };

    // The guard owns raw mode, the alternate screen and the panic hook, and puts all
    // three back on every exit path — including the `?` below.
    let mut tui = Tui::new()?;
    tui.drain_stale_input(Duration::from_millis(300));
    event_loop(&mut tui.terminal, &mut state)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut State<'_>,
) -> Result<Outcome> {
    loop {
        terminal.draw(|frame| render(frame, state))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        // Windows consoles deliver a release for every press; acting on both would
        // toggle every setting twice.
        if key.kind == KeyEventKind::Release {
            continue;
        }
        // Raw mode delivers Ctrl-C as a key event rather than a signal, so without this
        // the one key everybody reaches for to escape does nothing.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            return Ok(Outcome::Cancelled);
        }

        if let Some(outcome) = handle_key(state, key.code) {
            return Ok(outcome);
        }
    }
}

/// Apply one keypress. `Some` ends the view.
fn handle_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    match state.screen {
        Screen::Declaration => declaration_key(state, code),
        Screen::Settings => settings_key(state, code),
        Screen::Adapters => adapters_key(state, code),
        Screen::Summary => summary_key(state, code),
    }
}

fn declaration_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    let len = state.session.declaration.len().max(1);
    let current = state.decl_list.selected().unwrap_or(0);
    match code {
        // A promise the reader cannot scroll to is not a promise they have been shown.
        KeyCode::Up | KeyCode::Char('k') => {
            state.decl_list.select(Some(current.saturating_sub(1)));
            None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.decl_list.select(Some((current + 1).min(len - 1)));
            None
        }
        // `y` has meant "yes, all of it, carry on" at this prompt since 1.0.0, and it
        // still does — this screen must not turn a habit into a detour.
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Outcome::KeepAll),
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('c') | KeyCode::Char('C') => {
            state.screen = Screen::Settings;
            None
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => Some(Outcome::Cancelled),
        _ => None,
    }
}

fn settings_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    // An in-progress number edit owns the keyboard until it is committed or abandoned.
    if state.editing.is_some() {
        return number_edit_key(state, code);
    }

    let len = state.session.rows.len();
    let current = state.list.selected().unwrap_or(0);
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.error = None;
            state
                .list
                .select(Some(if current == 0 { len - 1 } else { current - 1 }));
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.error = None;
            state
                .list
                .select(Some(if current + 1 >= len { 0 } else { current + 1 }));
        }
        KeyCode::Home | KeyCode::Char('g') => state.list.select(Some(0)),
        KeyCode::End | KeyCode::Char('G') => state.list.select(Some(len - 1)),
        KeyCode::Char(' ') | KeyCode::Enter => activate(state, current),
        KeyCode::Char('r') | KeyCode::Char('R') => {
            state.error = None;
            let row = &mut state.session.rows[current];
            row.value = row.original.clone();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('s') | KeyCode::Char('S') => {
            state.screen = Screen::Summary;
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => return Some(Outcome::Cancelled),
        _ => {}
    }
    None
}

/// Space or Enter on a row: flip it, open its editor, or open its checklist.
fn activate(state: &mut State<'_>, index: usize) {
    state.error = None;
    match state.session.rows[index].control {
        Control::Toggle => {
            let row = &mut state.session.rows[index];
            row.value = if row.value == "true" {
                "false".to_string()
            } else {
                "true".to_string()
            };
        }
        Control::Number => state.editing = Some(state.session.rows[index].value.clone()),
        Control::Adapters => {
            let disabled = parse_list(&state.session.rows[index].value);
            state.picker_active = state
                .session
                .adapters
                .iter()
                .map(|name| !disabled.iter().any(|d| d == name))
                .collect();
            state.picker_list.select(Some(0));
            state.screen = Screen::Adapters;
        }
    }
}

fn number_edit_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    let index = state.list.selected().unwrap_or(0);
    match code {
        KeyCode::Char(c) if c.is_ascii_digit() => {
            if let Some(buf) = state.editing.as_mut() {
                buf.push(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(buf) = state.editing.as_mut() {
                buf.pop();
            }
        }
        KeyCode::Enter => {
            let typed = state.editing.clone().unwrap_or_default();
            let key = state.session.rows[index].key;
            match (state.session.validate)(key, typed.trim()) {
                Ok(()) => {
                    state.session.rows[index].value = typed.trim().to_string();
                    state.editing = None;
                    state.error = None;
                }
                // Refused in place rather than accepted and rejected on save: the
                // reason belongs next to the field that caused it.
                Err(why) => state.error = Some(why),
            }
        }
        KeyCode::Esc => {
            state.editing = None;
            state.error = None;
        }
        _ => {}
    }
    None
}

fn adapters_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    let len = state.session.adapters.len();
    let current = state.picker_list.selected().unwrap_or(0);
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            state
                .picker_list
                .select(Some(if current == 0 { len - 1 } else { current - 1 }));
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state
                .picker_list
                .select(Some(if current + 1 >= len { 0 } else { current + 1 }));
        }
        KeyCode::Char(' ') => state.picker_active[current] = !state.picker_active[current],
        KeyCode::Char('a') | KeyCode::Char('A') => state.picker_active.fill(true),
        KeyCode::Char('n') | KeyCode::Char('N') => state.picker_active.fill(false),
        KeyCode::Enter => {
            let disabled: Vec<&str> = state
                .session
                .adapters
                .iter()
                .zip(state.picker_active.iter())
                .filter(|(_, active)| !**active)
                .map(|(name, _)| *name)
                .collect();
            let index = state.list.selected().unwrap_or(0);
            // `(none)` rather than an empty string, so what the row shows is exactly
            // what `devp config get disabled_adapters` prints.
            state.session.rows[index].value = if disabled.is_empty() {
                "(none)".to_string()
            } else {
                disabled.join(",")
            };
            state.screen = Screen::Settings;
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => state.screen = Screen::Settings,
        _ => {}
    }
    None
}

fn summary_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    match code {
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('s') => {
            let changed: Vec<ConfigRow> = state
                .session
                .rows
                .iter()
                .filter(|r| r.changed())
                .cloned()
                .collect();
            if changed.is_empty() {
                Some(Outcome::KeepAll)
            } else {
                Some(Outcome::Save(changed))
            }
        }
        KeyCode::Esc | KeyCode::Backspace => {
            state.screen = Screen::Settings;
            None
        }
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(Outcome::Cancelled),
        _ => None,
    }
}

/// Split a stored deny-list back into names. `(none)` is the empty list.
fn parse_list(value: &str) -> Vec<String> {
    if value.trim().eq_ignore_ascii_case("(none)") {
        return Vec::new();
    }
    value
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(frame: &mut Frame, state: &mut State<'_>) {
    match state.screen {
        Screen::Declaration => render_declaration(frame, state),
        Screen::Settings => render_settings(frame, state),
        Screen::Adapters => render_adapters(frame, state),
        Screen::Summary => render_summary(frame, state),
    }
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn header(title: &str, subtitle: &str) -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(subtitle.to_string(), dim())),
    ])
}

fn footer(keys: &[(&str, &str)]) -> Paragraph<'static> {
    let mut spans = Vec::new();
    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", dim()));
        }
        spans.push(Span::styled(
            key.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {what}"), dim()));
    }
    Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP).border_style(dim()))
}

fn render_declaration(frame: &mut Frame, state: &mut State<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(frame.area());

    frame.render_widget(
        header(
            state.session.title,
            "What this tool is allowed to do on this machine, before it does any of it.",
        ),
        chunks[0],
    );

    let items: Vec<ListItem> = state
        .session
        .declaration
        .iter()
        .map(|d| {
            if d.mark == '#' {
                return ListItem::new(Line::from(Span::styled(
                    format!(" {}", d.subject),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            let (mark_style, symbol) = match d.mark {
                '!' => (Style::default().fg(Color::Yellow), "!"),
                '+' => (Style::default().fg(Color::Green), "✓"),
                _ => (dim(), " "),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {symbol} "), mark_style),
                Span::styled(crate::output::pad_display(&d.subject, 26), Style::default()),
                Span::styled(d.state.clone(), dim()),
            ]))
        })
        .collect();

    // A `List` rather than a `Paragraph` only for the scrolling: no highlight symbol and
    // no highlight style, because nothing on this screen is selectable.
    frame.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(" Declaration ")
                .borders(Borders::ALL)
                .border_style(dim()),
        ),
        chunks[1],
        &mut state.decl_list,
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", state.session.standing),
            Style::default().fg(Color::Green),
        )))
        .block(Block::default().borders(Borders::ALL).border_style(dim())),
        chunks[2],
    );

    frame.render_widget(
        footer(&[
            ("↑↓", "read"),
            ("y", "keep all defaults and go"),
            ("Enter", "configure"),
            ("q", "cancel"),
        ]),
        chunks[3],
    );
}

fn render_settings(frame: &mut Frame, state: &mut State<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(4),
        Constraint::Length(2),
    ])
    .split(frame.area());

    let changed = state.session.rows.iter().filter(|r| r.changed()).count();
    let new = state.session.rows.iter().filter(|r| r.is_new).count();
    let subtitle = match (changed, new) {
        (0, 0) => "Nothing changed yet.".to_string(),
        (c, 0) => format!("{c} changed."),
        (0, n) => format!("{n} new in this version."),
        (c, n) => format!("{c} changed, {n} new in this version."),
    };
    frame.render_widget(header(state.session.title, &subtitle), chunks[0]);

    let selected = state.list.selected();
    let items: Vec<ListItem> = state
        .session
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let control = match row.control {
                Control::Toggle if row.value == "true" => Span::styled(
                    "[x] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Control::Toggle => Span::styled("[ ] ", dim()),
                Control::Number => Span::styled("123 ", dim()),
                Control::Adapters => Span::styled("••• ", dim()),
            };

            let shown = if state.editing.is_some() && selected == Some(i) {
                format!("{}_", state.editing.clone().unwrap_or_default())
            } else {
                row.value.clone()
            };

            let mut spans = vec![
                control,
                Span::styled(
                    crate::output::pad_display(row.key, 28),
                    if selected == Some(i) {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    crate::output::pad_display(&shown, 20),
                    if row.changed() {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
            ];
            if row.is_new {
                spans.push(Span::styled(
                    "NEW ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if row.changed() {
                spans.push(Span::styled(format!("was {}", row.original), dim()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Settings ")
                .borders(Borders::ALL)
                .border_style(dim()),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 40, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[1], &mut state.list);

    // The help for the highlighted row, and any refusal, in the same place: a message
    // about a field belongs next to the field.
    let index = selected.unwrap_or(0);
    let row = &state.session.rows[index];
    let mut detail = vec![Line::from(Span::styled(format!("  {}", row.help), dim()))];
    if row.is_new {
        detail.push(Line::from(Span::styled(
            "  New in this version — it has been applying its default since the upgrade.",
            Style::default().fg(Color::Magenta),
        )));
    }
    if let Some(why) = &state.error {
        detail.push(Line::from(Span::styled(
            format!("  {why}"),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).border_style(dim())),
        chunks[2],
    );

    let keys: &[(&str, &str)] = if state.editing.is_some() {
        &[("digits", "type"), ("Enter", "accept"), ("Esc", "abandon")]
    } else {
        &[
            ("↑↓", "move"),
            ("Space", "change"),
            ("r", "reset"),
            ("y", "done"),
            ("q", "cancel"),
        ]
    };
    frame.render_widget(footer(keys), chunks[3]);
}

fn render_adapters(frame: &mut Frame, state: &mut State<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .split(frame.area());

    let off = state.picker_active.iter().filter(|a| !**a).count();
    frame.render_widget(
        header(
            "Adapters",
            &format!(
                "Unchecked adapters are left alone entirely — not scanned, not counted, \
                 not pruned. {off} off.",
            ),
        ),
        chunks[0],
    );

    let items: Vec<ListItem> = state
        .session
        .adapters
        .iter()
        .zip(state.picker_active.iter())
        .map(|(name, active)| {
            let mut spans = vec![
                if *active {
                    Span::styled(
                        "[x] ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("[ ] ", dim())
                },
                Span::styled(crate::output::pad_display(name, 16), Style::default()),
            ];
            if state.session.opt_in_adapters.contains(name) {
                // Two switches govern these, and someone who ticks this box and sees
                // nothing happen deserves to know which other one to look at.
                spans.push(Span::styled(
                    format!("opt-in — also needs enable_{name}"),
                    dim(),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Checked adapters stay active ")
                .borders(Borders::ALL)
                .border_style(dim()),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 40, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[1], &mut state.picker_list);

    frame.render_widget(
        footer(&[
            ("↑↓", "move"),
            ("Space", "toggle"),
            ("a", "all on"),
            ("n", "all off"),
            ("Enter", "accept"),
            ("Esc", "back"),
        ]),
        chunks[2],
    );
}

fn render_summary(frame: &mut Frame, state: &State<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .split(frame.area());

    let changed: Vec<&ConfigRow> = state.session.rows.iter().filter(|r| r.changed()).collect();
    frame.render_widget(
        header(
            "Summary",
            if changed.is_empty() {
                "Nothing changed. The defaults stay in place."
            } else {
                "These are the only values that will be written."
            },
        ),
        chunks[0],
    );

    let mut lines: Vec<Line> = changed
        .iter()
        .map(|row| {
            Line::from(vec![
                Span::styled(
                    format!("  {}", crate::output::pad_display(row.key, 28)),
                    Style::default(),
                ),
                Span::styled(row.original.clone(), dim()),
                Span::styled(" → ", dim()),
                Span::styled(
                    row.value.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Every setting is still at the value it had when this opened.",
            dim(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  {}", state.session.standing),
        Style::default().fg(Color::Green),
    )));

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" About to be saved ")
                .borders(Borders::ALL)
                .border_style(dim()),
        ),
        chunks[1],
    );

    frame.render_widget(
        footer(&[
            ("Enter", "save"),
            ("Esc", "back"),
            ("q", "discard everything"),
        ]),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &'static str, control: Control, value: &str) -> ConfigRow {
        ConfigRow {
            key,
            help: "help",
            control,
            value: value.to_string(),
            original: value.to_string(),
            is_new: false,
        }
    }

    fn session<'a>(rows: Vec<ConfigRow>, adapters: &'a [&'static str]) -> ConfigSession<'a> {
        ConfigSession {
            declaration: Vec::new(),
            standing: String::new(),
            rows,
            adapters,
            opt_in_adapters: &[],
            validate: &|_, v| {
                v.parse::<u64>()
                    .map(|_| ())
                    .map_err(|_| "not a number".to_string())
            },
            title: "test",
        }
    }

    fn state<'a>(s: ConfigSession<'a>) -> State<'a> {
        let mut list = ListState::default();
        list.select(Some(0));
        let mut picker_list = ListState::default();
        picker_list.select(Some(0));
        State {
            picker_active: vec![true; s.adapters.len()],
            session: s,
            screen: Screen::Settings,
            list,
            editing: None,
            error: None,
            picker_list,
            decl_list: ListState::default(),
        }
    }

    /// Draw one screen into an off-screen buffer and return it as text.
    ///
    /// The layouts are the one part of this file a keypress test cannot reach, and a
    /// constraint that does not fit its area panics rather than clipping.
    fn screenshot(st: &mut State<'_>, screen: Screen) -> String {
        st.screen = screen;
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(100, 30)).expect("test backend");
        terminal.draw(|frame| render(frame, st)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn every_screen_draws() {
        let adapters: &[&'static str] = &["npm", "cargo"];
        let mut st = state(session(
            vec![
                row("idle_days", Control::Number, "14"),
                row("disabled_adapters", Control::Adapters, "(none)"),
            ],
            adapters,
        ));
        st.session.declaration.push(DeclarationLine {
            mark: '+',
            subject: "Lockfile verification".to_string(),
            state: "Required before every delete".to_string(),
        });
        st.session.standing = "Nothing has been deleted.".to_string();

        let decl = screenshot(&mut st, Screen::Declaration);
        assert!(decl.contains("Lockfile verification"));
        assert!(decl.contains("Nothing has been deleted."));

        let settings = screenshot(&mut st, Screen::Settings);
        assert!(settings.contains("idle_days"));

        let picker = screenshot(&mut st, Screen::Adapters);
        assert!(picker.contains("cargo"));

        // The summary must say so when there is nothing to say, rather than draw an
        // empty box that reads as a rendering failure.
        let summary = screenshot(&mut st, Screen::Summary);
        assert!(summary.contains("still at the value"));
    }

    #[test]
    fn y_on_the_declaration_still_means_yes_to_everything() {
        // The prompt this replaced was `Keep all of these? [Y/n]`. Anyone who has typed
        // `y` at it once will type `y` at this, and must get the same result.
        let mut st = state(session(vec![row("idle_days", Control::Number, "14")], &[]));
        st.screen = Screen::Declaration;
        assert!(matches!(
            handle_key(&mut st, KeyCode::Char('y')),
            Some(Outcome::KeepAll)
        ));
    }

    #[test]
    fn a_refused_value_is_not_stored() {
        let mut st = state(session(vec![row("idle_days", Control::Number, "14")], &[]));
        handle_key(&mut st, KeyCode::Enter); // open the editor
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Backspace); // buffer now empty, which will not parse
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[0].value, "14");
        assert!(st.error.is_some(), "the reason was not shown");
        assert!(st.editing.is_some(), "the editor closed on a refusal");
    }

    #[test]
    fn an_accepted_value_replaces_the_old_one() {
        let mut st = state(session(vec![row("idle_days", Control::Number, "14")], &[]));
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Char('3'));
        handle_key(&mut st, KeyCode::Char('0'));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[0].value, "30");
        assert!(st.session.rows[0].changed());
    }

    #[test]
    fn unchecking_an_adapter_writes_it_to_the_deny_list() {
        let adapters: &[&'static str] = &["npm", "cargo", "go"];
        let mut st = state(session(
            vec![row("disabled_adapters", Control::Adapters, "(none)")],
            adapters,
        ));
        handle_key(&mut st, KeyCode::Enter); // open the checklist
        assert_eq!(st.screen, Screen::Adapters);
        handle_key(&mut st, KeyCode::Down); // cargo
        handle_key(&mut st, KeyCode::Char(' '));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[0].value, "cargo");
        assert_eq!(st.screen, Screen::Settings);
    }

    #[test]
    fn the_checklist_opens_showing_what_is_already_disabled() {
        // Opening with everything ticked would silently re-enable an adapter the user
        // turned off, the first time they visited the screen for any other reason.
        let adapters: &[&'static str] = &["npm", "cargo", "go"];
        let mut st = state(session(
            vec![row("disabled_adapters", Control::Adapters, "go")],
            adapters,
        ));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.picker_active, vec![true, true, false]);
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[0].value, "go");
    }

    #[test]
    fn cancelling_reports_cancelled_rather_than_an_empty_save() {
        // The difference matters: `KeepAll` marks the settings reviewed and `Cancelled`
        // does not, so an escape must not be mistaken for an answer.
        let mut st = state(session(
            vec![row("auto_update", Control::Toggle, "false")],
            &[],
        ));
        assert!(matches!(
            handle_key(&mut st, KeyCode::Char('q')),
            Some(Outcome::Cancelled)
        ));
    }

    #[test]
    fn only_changed_rows_are_saved() {
        let mut st = state(session(
            vec![
                row("auto_update", Control::Toggle, "false"),
                row("auto_config", Control::Toggle, "false"),
            ],
            &[],
        ));
        handle_key(&mut st, KeyCode::Char(' ')); // flip the first
        handle_key(&mut st, KeyCode::Char('y')); // to the summary
        let Some(Outcome::Save(changed)) = handle_key(&mut st, KeyCode::Enter) else {
            panic!("expected a save");
        };
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].key, "auto_update");
        assert_eq!(changed[0].value, "true");
    }

    #[test]
    fn reset_puts_a_row_back_without_touching_the_others() {
        let mut st = state(session(
            vec![
                row("auto_update", Control::Toggle, "false"),
                row("auto_config", Control::Toggle, "true"),
            ],
            &[],
        ));
        handle_key(&mut st, KeyCode::Char(' '));
        assert!(st.session.rows[0].changed());
        handle_key(&mut st, KeyCode::Char('r'));
        assert!(!st.session.rows[0].changed());
        assert_eq!(st.session.rows[1].value, "true");
    }

    #[test]
    fn the_view_opens_on_the_first_setting_the_user_has_never_seen() {
        let mut rows = [
            row("idle_days", Control::Number, "14"),
            row("auto_update", Control::Toggle, "false"),
            row("auto_config", Control::Toggle, "false"),
        ];
        assert_eq!(
            opening_index(&rows),
            0,
            "with nothing new, start at the top"
        );
        rows[2].is_new = true;
        assert_eq!(opening_index(&rows), 2);
    }
}
