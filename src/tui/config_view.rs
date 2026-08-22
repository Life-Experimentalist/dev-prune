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
    /// Opens the same adapter checklist, on the idle-window column.
    ///
    /// Which adapters run and how long each one waits are one decision made twice, so
    /// they are edited on one screen. The row exists separately only because the
    /// settings table stores them as two keys.
    AdapterDays,
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
    /// The language groups the adapters are shown under, in display order. Anything
    /// not named by a group is collected under a trailing "Other".
    pub groups: &'a [(&'static str, &'static [&'static str])],
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

/// One drawn line of the adapter checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerEntry {
    /// A language heading, carrying the indices of every adapter under it so that one
    /// keypress on the heading reaches all of them.
    Group {
        label: &'static str,
        members: Vec<usize>,
    },
    /// An adapter, by its index into `session.adapters`.
    Adapter(usize),
}

/// Lay the adapters out under their language headings.
///
/// Order comes from the group table rather than the adapter registry: someone looking
/// for "the Python ones" is looking for a heading, not for four names that happen to be
/// adjacent. An adapter no group claims still has to appear — a checklist that silently
/// omits an adapter is a checklist that cannot turn it off.
fn build_entries(
    adapters: &[&'static str],
    groups: &[(&'static str, &'static [&'static str])],
) -> Vec<PickerEntry> {
    let mut entries = Vec::new();
    let mut placed = vec![false; adapters.len()];

    for (label, names) in groups {
        let members: Vec<usize> = names
            .iter()
            .filter_map(|name| adapters.iter().position(|a| a == name))
            .collect();
        if members.is_empty() {
            continue;
        }
        for &i in &members {
            placed[i] = true;
        }
        entries.push(PickerEntry::Group {
            label,
            members: members.clone(),
        });
        entries.extend(members.into_iter().map(PickerEntry::Adapter));
    }

    let rest: Vec<usize> = (0..adapters.len()).filter(|&i| !placed[i]).collect();
    if !rest.is_empty() {
        entries.push(PickerEntry::Group {
            label: "Other",
            members: rest.clone(),
        });
        entries.extend(rest.into_iter().map(PickerEntry::Adapter));
    }
    entries
}

/// The value of one row, by key.
fn row_value(rows: &[ConfigRow], key: &str) -> Option<String> {
    rows.iter().find(|r| r.key == key).map(|r| r.value.clone())
}

/// Write one row by key, ignoring a key the settings table does not carry.
fn set_row(rows: &mut [ConfigRow], key: &str, value: String) {
    if let Some(row) = rows.iter_mut().find(|r| r.key == key) {
        row.value = value;
    }
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
    /// Per-adapter idle window in days, `None` when the adapter follows the global one.
    picker_days: Vec<Option<u64>>,
    /// The checklist as it is drawn: group headings interleaved with their adapters.
    /// Rebuilt when the screen opens, because it depends on nothing that changes while
    /// it is open.
    picker_entries: Vec<PickerEntry>,
    /// Buffer for an in-progress idle-window edit on the checklist.
    picker_editing: Option<String>,
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
        picker_days: vec![None; session.adapters.len()],
        picker_entries: Vec::new(),
        picker_editing: None,
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
        Control::Adapters | Control::AdapterDays => open_picker(state),
    }
}

/// Seed the checklist from the rows it will write back to.
///
/// Opening with everything ticked would silently re-enable an adapter the user turned
/// off, the first time they visited this screen for any other reason — so all three
/// rows that govern an adapter are read back here, not just the deny-list.
fn open_picker(state: &mut State<'_>) {
    let rows = &state.session.rows;
    let disabled = parse_list(&row_value(rows, "disabled_adapters").unwrap_or_default());
    let days = parse_days(&row_value(rows, "adapter_idle_days").unwrap_or_default());

    state.picker_active = state
        .session
        .adapters
        .iter()
        .map(|name| {
            if disabled.iter().any(|d| d == name) {
                return false;
            }
            // An opt-in adapter is active only if its own switch is on: it is off by
            // default and absent from the deny-list, and showing it ticked would
            // promise a prune that never happens.
            if state.session.opt_in_adapters.contains(name) {
                return row_value(rows, &format!("enable_{name}")).as_deref() == Some("true");
            }
            true
        })
        .collect();
    state.picker_days = state
        .session
        .adapters
        .iter()
        .map(|name| days.iter().find(|(n, _)| n == name).map(|(_, d)| *d))
        .collect();

    state.picker_entries = build_entries(state.session.adapters, state.session.groups);
    state.picker_editing = None;
    state.picker_list.select(Some(0));
    state.screen = Screen::Adapters;
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
    if state.picker_editing.is_some() {
        return picker_days_key(state, code);
    }

    let len = state.picker_entries.len().max(1);
    let current = state.picker_list.selected().unwrap_or(0);
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.error = None;
            state
                .picker_list
                .select(Some(if current == 0 { len - 1 } else { current - 1 }));
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.error = None;
            state
                .picker_list
                .select(Some(if current + 1 >= len { 0 } else { current + 1 }));
        }
        // On a heading, one keypress governs the whole language: off if any of them is
        // on, so "turn Python off" never needs four presses and a count.
        KeyCode::Char(' ') => match state.picker_entries.get(current).cloned() {
            Some(PickerEntry::Adapter(i)) => state.picker_active[i] = !state.picker_active[i],
            Some(PickerEntry::Group { members, .. }) => {
                let target = !members.iter().any(|&i| state.picker_active[i]);
                for i in members {
                    state.picker_active[i] = target;
                }
            }
            None => {}
        },
        KeyCode::Char('d') | KeyCode::Char('D') => {
            state.error = None;
            let seed = match state.picker_entries.get(current) {
                Some(PickerEntry::Adapter(i)) => state.picker_days[*i],
                // A group seeds from the window its members already share; a group of
                // disagreeing values seeds empty rather than picking one of them.
                Some(PickerEntry::Group { members, .. }) => {
                    let first = members.first().and_then(|&i| state.picker_days[i]);
                    if members.iter().all(|&i| state.picker_days[i] == first) {
                        first
                    } else {
                        None
                    }
                }
                None => None,
            };
            state.picker_editing = Some(seed.map(|d| d.to_string()).unwrap_or_default());
        }
        KeyCode::Char('a') | KeyCode::Char('A') => state.picker_active.fill(true),
        KeyCode::Char('n') | KeyCode::Char('N') => state.picker_active.fill(false),
        KeyCode::Enter => commit_picker(state),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => state.screen = Screen::Settings,
        _ => {}
    }
    None
}

/// The inline idle-window editor on the checklist. An empty buffer clears the window,
/// which is the only way back to "follow the global one" once a number is set.
fn picker_days_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    let current = state.picker_list.selected().unwrap_or(0);
    match code {
        KeyCode::Char(c) if c.is_ascii_digit() => {
            if let Some(buf) = state.picker_editing.as_mut() {
                buf.push(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(buf) = state.picker_editing.as_mut() {
                buf.pop();
            }
        }
        KeyCode::Enter => {
            let typed = state.picker_editing.clone().unwrap_or_default();
            let typed = typed.trim().to_string();
            let targets: Vec<usize> = match state.picker_entries.get(current) {
                Some(PickerEntry::Adapter(i)) => vec![*i],
                Some(PickerEntry::Group { members, .. }) => members.clone(),
                None => Vec::new(),
            };
            let value = if typed.is_empty() {
                None
            } else {
                let Some(&first) = targets.first() else {
                    state.picker_editing = None;
                    return None;
                };
                let probe = format!("{}={typed}", state.session.adapters[first]);
                // Through the real setter, so the checklist cannot store a number
                // `devp config set` would refuse.
                if let Err(why) = (state.session.validate)("adapter_idle_days", &probe) {
                    state.error = Some(why);
                    return None;
                }
                typed.parse::<u64>().ok()
            };
            for i in targets {
                state.picker_days[i] = value;
            }
            state.picker_editing = None;
            state.error = None;
        }
        KeyCode::Esc => {
            state.picker_editing = None;
            state.error = None;
        }
        _ => {}
    }
    None
}

/// Fold the checklist back into the rows that store it.
///
/// An opt-in adapter is governed by its own `enable_*` switch rather than by the
/// deny-list: two ways to say the same "off" would leave the settings screen showing a
/// contradiction, and unticking it here should read back there as the switch being off.
fn commit_picker(state: &mut State<'_>) {
    let adapters = state.session.adapters;
    let opt_in = state.session.opt_in_adapters;

    let disabled: Vec<&str> = adapters
        .iter()
        .enumerate()
        .filter(|(i, name)| !state.picker_active[*i] && !opt_in.contains(name))
        .map(|(_, name)| *name)
        .collect();
    // `(none)` rather than an empty string, so what the row shows is exactly what
    // `devp config get disabled_adapters` prints.
    let disabled = if disabled.is_empty() {
        "(none)".to_string()
    } else {
        disabled.join(",")
    };

    let days: Vec<String> = adapters
        .iter()
        .enumerate()
        .filter_map(|(i, name)| state.picker_days[i].map(|d| format!("{name}={d}")))
        .collect();
    let days = if days.is_empty() {
        "(none)".to_string()
    } else {
        days.join(",")
    };

    let switches: Vec<(String, String)> = adapters
        .iter()
        .enumerate()
        .filter(|(_, name)| opt_in.contains(name))
        .map(|(i, name)| (format!("enable_{name}"), state.picker_active[i].to_string()))
        .collect();

    let rows = &mut state.session.rows;
    set_row(rows, "disabled_adapters", disabled);
    set_row(rows, "adapter_idle_days", days);
    for (key, value) in switches {
        set_row(rows, &key, value);
    }
    state.screen = Screen::Settings;
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

/// Split a stored `name=days` map back into pairs. `(none)` is the empty map.
///
/// Anything malformed is dropped rather than refused: this parses a value the setter
/// already accepted, and a checklist that will not open is worse than one that opens
/// with a window missing.
fn parse_days(value: &str) -> Vec<(String, u64)> {
    if value.trim().eq_ignore_ascii_case("(none)") {
        return Vec::new();
    }
    value
        .split(',')
        .filter_map(|entry| {
            let (name, days) = entry.trim().split_once('=')?;
            Some((name.trim().to_lowercase(), days.trim().parse().ok()?))
        })
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
                Control::Adapters | Control::AdapterDays => Span::styled("••• ", dim()),
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
        Constraint::Length(4),
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

    let selected = state.picker_list.selected();
    let items: Vec<ListItem> = state
        .picker_entries
        .iter()
        .enumerate()
        .map(|(line, entry)| match entry {
            PickerEntry::Group { label, members } => {
                let on = members.iter().filter(|&&i| state.picker_active[i]).count();
                let mark = if on == members.len() {
                    "[x]"
                } else if on == 0 {
                    "[ ]"
                } else {
                    // A language half on is neither, and drawing it as either is how
                    // one Space press silently turns three adapters back on.
                    "[-]"
                };
                let shared = members.first().and_then(|&i| state.picker_days[i]);
                // A heading is an editing target like any adapter row, so it has to show
                // the buffer being typed into it — otherwise the keys land silently.
                let window = if state.picker_editing.is_some() && selected == Some(line) {
                    format!("{}_", state.picker_editing.clone().unwrap_or_default())
                } else if members.iter().all(|&i| state.picker_days[i] == shared) {
                    shared.map(|d| format!("{d}d")).unwrap_or_default()
                } else {
                    "mixed".to_string()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{mark} {}", crate::output::pad_display(label, 22)),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        crate::output::pad_display(&format!("{on}/{}", members.len()), 8),
                        dim(),
                    ),
                    Span::styled(window, dim()),
                ]))
            }
            PickerEntry::Adapter(i) => {
                let name = state.session.adapters[*i];
                let shown = if state.picker_editing.is_some() && selected == Some(line) {
                    format!("{}_", state.picker_editing.clone().unwrap_or_default())
                } else {
                    state.picker_days[*i]
                        .map(|d| format!("{d}d"))
                        .unwrap_or_else(|| "default".to_string())
                };
                let mut spans = vec![
                    if state.picker_active[*i] {
                        Span::styled(
                            "  [x] ",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::styled("  [ ] ", dim())
                    },
                    Span::styled(crate::output::pad_display(name, 18), Style::default()),
                    Span::styled(
                        crate::output::pad_display(&shown, 10),
                        if state.picker_days[*i].is_some() {
                            Style::default().fg(Color::Yellow)
                        } else {
                            dim()
                        },
                    ),
                ];
                if state.session.opt_in_adapters.contains(&name) {
                    // Naming the cost is the whole argument for the switch: these come
                    // back by recompiling, and nobody should turn one on without being
                    // told that is what "restore" means here.
                    spans.push(Span::styled("opt-in — rebuilt, not downloaded", dim()));
                }
                ListItem::new(Line::from(spans))
            }
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

    let mut detail = vec![Line::from(Span::styled(
        "  Space toggles one adapter, or a whole language from its heading. d sets how \
         many days that adapter — or that language — must be idle first; an empty value \
         puts it back on the global window.",
        dim(),
    ))];
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

    let keys: &[(&str, &str)] = if state.picker_editing.is_some() {
        &[
            ("digits", "days"),
            ("Enter", "accept"),
            ("empty", "use the global window"),
            ("Esc", "abandon"),
        ]
    } else {
        &[
            ("↑↓", "move"),
            ("Space", "toggle"),
            ("d", "idle days"),
            ("a", "all on"),
            ("n", "all off"),
            ("Enter", "accept"),
            ("Esc", "back"),
        ]
    };
    frame.render_widget(footer(keys), chunks[3]);
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
            groups: &[("Test", &["npm", "cargo", "go"])],
            validate: &|key, v| {
                // Stands in for the real setters: the same shapes accepted, so a test
                // that types a value the checklist stores is a test the wizard passes.
                let number = if key == "adapter_idle_days" {
                    v.split_once('=').map(|(_, d)| d).unwrap_or("")
                } else {
                    v
                };
                number
                    .parse::<u64>()
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
            picker_days: vec![None; s.adapters.len()],
            picker_entries: build_entries(s.adapters, s.groups),
            picker_editing: None,
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

        st.picker_entries = build_entries(st.session.adapters, st.session.groups);
        st.picker_days[1] = Some(45);
        let picker = screenshot(&mut st, Screen::Adapters);
        assert!(picker.contains("cargo"));
        assert!(picker.contains("Test"), "the language heading is missing");
        assert!(picker.contains("45d"), "the idle window is missing");

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
        handle_key(&mut st, KeyCode::Down); // past the heading, onto npm
        handle_key(&mut st, KeyCode::Down); // cargo
        handle_key(&mut st, KeyCode::Char(' '));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[0].value, "cargo");
        assert_eq!(st.screen, Screen::Settings);
    }

    #[test]
    fn every_adapter_appears_under_exactly_one_heading() {
        // An adapter no group claims still has to be listed: a checklist that silently
        // omits an adapter is a checklist that cannot turn it off.
        let adapters: &[&'static str] = &["npm", "cargo", "mystery"];
        let groups: &[(&'static str, &'static [&'static str])] =
            &[("JavaScript", &["npm"]), ("Rust", &["cargo"])];
        let entries = build_entries(adapters, groups);
        let headings: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                PickerEntry::Group { label, .. } => Some(*label),
                PickerEntry::Adapter(_) => None,
            })
            .collect();
        assert_eq!(headings, vec!["JavaScript", "Rust", "Other"]);

        let mut listed: Vec<usize> = entries
            .iter()
            .filter_map(|e| match e {
                PickerEntry::Adapter(i) => Some(*i),
                PickerEntry::Group { .. } => None,
            })
            .collect();
        listed.sort_unstable();
        assert_eq!(
            listed,
            vec![0, 1, 2],
            "an adapter was dropped from the list"
        );
    }

    #[test]
    fn a_heading_turns_its_whole_language_off_in_one_press() {
        let adapters: &[&'static str] = &["npm", "pnpm", "cargo"];
        let mut st = state(session(
            vec![row("disabled_adapters", Control::Adapters, "(none)")],
            adapters,
        ));
        st.session.groups = &[("JavaScript", &["npm", "pnpm"]), ("Rust", &["cargo"])];
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Char(' ')); // on the JavaScript heading
        assert_eq!(st.picker_active, vec![false, false, true]);
        // And back on again: a heading that only ever turned things off would leave the
        // user unable to undo their own keypress.
        handle_key(&mut st, KeyCode::Char(' '));
        assert_eq!(st.picker_active, vec![true, true, true]);
    }

    #[test]
    fn an_idle_window_typed_on_a_heading_reaches_every_adapter_under_it() {
        let adapters: &[&'static str] = &["npm", "pnpm", "cargo"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("adapter_idle_days", Control::AdapterDays, "(none)"),
            ],
            adapters,
        ));
        st.session.groups = &[("JavaScript", &["npm", "pnpm"]), ("Rust", &["cargo"])];
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Char('d')); // on the JavaScript heading
        handle_key(&mut st, KeyCode::Char('3'));
        handle_key(&mut st, KeyCode::Char('0'));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.picker_days, vec![Some(30), Some(30), None]);

        handle_key(&mut st, KeyCode::Enter); // accept the checklist
        assert_eq!(st.session.rows[1].value, "npm=30,pnpm=30");

        // Clearing is how a window goes back to following the global one, and there is
        // no other way to spell it.
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Char('d'));
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[1].value, "(none)");
    }

    #[test]
    fn an_opt_in_adapter_is_governed_by_its_own_switch_not_the_deny_list() {
        // Two ways to spell the same "off" would leave the settings screen showing a
        // contradiction: ticking cargo here has to read back there as enable_cargo.
        let adapters: &[&'static str] = &["npm", "cargo"];
        let opt_in: &[&'static str] = &["cargo"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("enable_cargo", Control::Toggle, "false"),
            ],
            adapters,
        ));
        st.session.opt_in_adapters = opt_in;
        st.session.groups = &[("JavaScript", &["npm"]), ("Rust", &["cargo"])];

        handle_key(&mut st, KeyCode::Enter);
        // Off by default and absent from the deny-list: showing it ticked would promise
        // a prune that never happens.
        assert_eq!(st.picker_active, vec![true, false]);
        handle_key(&mut st, KeyCode::Down); // JavaScript heading -> npm
        handle_key(&mut st, KeyCode::Down); // Rust heading
        handle_key(&mut st, KeyCode::Down); // cargo
        handle_key(&mut st, KeyCode::Char(' '));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[0].value, "(none)");
        assert_eq!(st.session.rows[1].value, "true");
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
