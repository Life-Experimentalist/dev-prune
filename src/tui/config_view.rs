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
    /// Cycled in place with Space, one `(value, label)` pair at a time.
    ///
    /// A toggle with more than two positions. Carries its own options because the label
    /// is the only part a reader can act on: `te` is not a word, and a picker that shows
    /// only the stored value is a picker for people who already knew the answer.
    Choice(&'static [(&'static str, &'static str)]),
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
    /// Opens the same adapter checklist, on the cache-cap column.
    ///
    /// Third column of the same table for the same reason the second one is there: how
    /// big npm's cache may get is a decision about npm, and the screen where npm is a
    /// row is where it belongs.
    CacheCaps,
}

/// Which column of the adapter checklist an inline edit is landing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerField {
    /// `adapter_idle_days`, in days.
    Days,
    /// `cache_max_gb`, in gibibytes.
    Cap,
}

/// One setting, as the view needs it.
#[derive(Debug, Clone)]
pub struct ConfigRow {
    pub key: &'static str,
    /// The heading this row is drawn under. Rows sharing one are drawn together, in the
    /// order they arrive; the caller owns which group a setting is in.
    pub category: &'static str,
    pub help: &'static str,
    /// The same setting said again without jargon, shown under `help` rather than
    /// instead of it. Someone who knows what a build tree is skips the second line;
    /// someone who does not was going to guess, and guessing is how a setting gets
    /// turned on for the wrong reason.
    pub plain: &'static str,
    pub control: Control,
    /// The value, spelled the way `devp config set` would take it.
    pub value: String,
    /// What it was when the view opened, so the summary shows only real changes.
    pub original: String,
    /// What a fresh install would hold, spelled the same way as `value`.
    ///
    /// Not the same question as `original`, which is what this machine happens to hold.
    /// Somebody looking at a setting they have never touched cannot tell those apart,
    /// and the one they need in order to decide whether to touch it is this one.
    pub default: String,
    /// The value the first-run screen suggests, if this setting is suggested at all.
    ///
    /// A recommendation, never a requirement: everything here works with all of them
    /// declined. It is shown on every visit rather than only on the first run, because
    /// the screen that suggested it appears once and the settings list is forever.
    pub recommended: Option<&'static str>,
    /// Introduced in a release newer than the one this machine last reviewed at.
    pub is_new: bool,
}

impl ConfigRow {
    pub fn changed(&self) -> bool {
        self.value != self.original
    }

    /// Whether this row already holds what is recommended for it.
    ///
    /// `None` when nothing is recommended, which is a different answer from "no" and is
    /// why the badge has three states rather than two.
    pub fn takes_advice(&self) -> Option<bool> {
        self.recommended.map(|r| self.value == r)
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

/// One entry on the first-run suggestions screen.
///
/// Only the *first* run gets this screen. Everything on it is also on the settings list
/// two keystrokes later, so this is not the only way to reach any of it — it exists
/// because a list of twenty-four settings, shown to somebody who has had this tool
/// installed for nine seconds, is a list nobody reads.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub key: &'static str,
    /// Three or four words naming what it turns on.
    pub label: &'static str,
    /// The official one-liner — the setting's own `help`.
    pub help: &'static str,
    /// The same thing without jargon.
    pub plain: &'static str,
    /// Why it is being suggested at all, which neither of the other two answers.
    pub why: &'static str,
    /// The value accepting it sets.
    pub value: &'static str,
    /// The second tier: worth turning on, with something specific to know first.
    ///
    /// Kept apart rather than mixed in with a warning glyph, because "recommended" and
    /// "recommended once you know what it does" are different claims and one button
    /// must not be able to accept both at once.
    pub cautious: bool,
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
    /// Settings worth turning on, shown once before the full list. Empty on every run
    /// but the first, which is the only time this screen appears at all.
    pub suggestions: Vec<Suggestion>,
    /// Every adapter name, in registry order, for the checklist.
    pub adapters: &'a [&'static str],
    /// Adapter names that need their own `enable_*` switch as well.
    pub opt_in_adapters: &'a [&'static str],
    /// Adapter names that are also the name of a cache `devp caches` knows, and so can
    /// carry a `cache_max_gb` entry.
    ///
    /// Identity only, never a guess: npm's cache is npm's. The caches with no adapter
    /// of the same name — `pip`, `nuget`, `conan`, `conda`, `vcpkg`, `hex` — have no
    /// row here to sit on and are capped with `devp config set cache_max_gb` instead,
    /// which the footer says. Inventing a row for them, or pointing `poetry` at pip's
    /// cache, would be the checklist claiming a relationship dev-prune has not
    /// verified.
    pub capped_adapters: &'a [&'static str],
    /// The language groups the adapters are shown under, in display order. Anything
    /// not named by a group is collected under a trailing "Other".
    pub groups: &'a [(&'static str, &'static [&'static str])],
    /// Round-trips one value through the setter that owns it. `Err` is shown in place
    /// and the edit is refused, so validation lives in exactly one place.
    pub validate: &'a dyn Fn(&str, &str) -> std::result::Result<(), String>,
    /// Title bar text — the walkthrough and `config wizard` arrive here differently.
    pub title: &'a str,
    /// Why this opened, when nobody pointed a command at it.
    ///
    /// `None` for `devp config wizard`, which was typed on purpose. `Some(_)` on the
    /// first run and after an upgrade that added a setting — the two times this takes
    /// a terminal in the middle of a command somebody typed for another reason, and so
    /// the two times it owes them a reason before it asks for anything.
    pub uninvited: Option<&'a str>,
}

/// The settings list as it is drawn: category headings interleaved with their settings.
///
/// The same shape as [`PickerEntry`] on the adapter checklist, for the same reason — a
/// column of thirty keys is a list nobody reads to the end of. Unlike a group
/// there, a heading here has nothing to toggle, so [`step`] walks past it: a cursor
/// that can rest on a line where no key does anything reads as a broken cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SettingEntry {
    Heading(&'static str),
    Row(usize),
    /// The last line of the list: where the walk ends and the summary begins. A cursor
    /// stop rather than a line in the footer, because "press some key when you are
    /// done" is the part of a configurator people report as having no way out of.
    Finish,
}

/// Interleave headings, keeping the caller's order within each group.
///
/// A heading is emitted whenever the category changes, not once per distinct category,
/// so a caller that interleaves groups gets what it asked for rather than a silent
/// regrouping.
fn settings_entries(rows: &[ConfigRow]) -> Vec<SettingEntry> {
    let mut entries = Vec::with_capacity(rows.len() + 8);
    let mut current: Option<&str> = None;
    for (i, row) in rows.iter().enumerate() {
        if current != Some(row.category) {
            entries.push(SettingEntry::Heading(row.category));
            current = Some(row.category);
        }
        entries.push(SettingEntry::Row(i));
    }
    entries.push(SettingEntry::Finish);
    entries
}

/// Which [`ConfigRow`] the cursor is on.
fn selected_row(state: &State<'_>) -> usize {
    let at = state.list.selected().unwrap_or(0);
    match state.setting_entries.get(at) {
        Some(SettingEntry::Row(i)) => *i,
        // Unreachable while every move goes through `step`, which never stops on a
        // heading. Falling forward to the first real row beats panicking mid-redraw.
        _ => first_row(&state.setting_entries).map_or(0, |at| match state.setting_entries[at] {
            SettingEntry::Row(i) => i,
            SettingEntry::Heading(_) | SettingEntry::Finish => 0,
        }),
    }
}

/// The next entry the cursor may rest on, wrapping at both ends.
fn step(entries: &[SettingEntry], from: usize, forward: bool) -> usize {
    let len = entries.len();
    let mut at = from;
    for _ in 0..len {
        at = if forward {
            if at + 1 >= len { 0 } else { at + 1 }
        } else if at == 0 {
            len - 1
        } else {
            at - 1
        };
        if matches!(entries[at], SettingEntry::Row(_) | SettingEntry::Finish) {
            return at;
        }
    }
    from
}

fn first_row(entries: &[SettingEntry]) -> Option<usize> {
    entries
        .iter()
        .position(|e| matches!(e, SettingEntry::Row(_)))
}

/// The last entry the cursor may rest on, which is the finish line rather than a row.
fn last_stop(entries: &[SettingEntry]) -> Option<usize> {
    entries
        .iter()
        .rposition(|e| matches!(e, SettingEntry::Row(_) | SettingEntry::Finish))
}

/// Where the cursor starts: the first setting the user has never been shown, when there
/// is one. After an upgrade that setting is the only reason this screen is in front of
/// them, and making them hunt for it down a list of twenty is how it gets skipped.
///
/// An index into the drawn entries, not into `rows`: the two stopped being the same
/// thing when headings joined the list.
fn opening_index(entries: &[SettingEntry], rows: &[ConfigRow]) -> usize {
    entries
        .iter()
        .position(|e| matches!(e, SettingEntry::Row(i) if rows[*i].is_new))
        .or_else(|| first_row(entries))
        .unwrap_or(0)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Screen {
    Declaration,
    Suggestions,
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
    /// Per-adapter cache cap in gibibytes, `None` when that cache has no cap. Always
    /// `None` for an adapter that is not in `capped_adapters`.
    picker_caps: Vec<Option<u64>>,
    /// The checklist as it is drawn: group headings interleaved with their adapters.
    /// Rebuilt when the screen opens, because it depends on nothing that changes while
    /// it is open.
    picker_entries: Vec<PickerEntry>,
    /// The settings list as it is drawn: category headings interleaved with their rows.
    /// Built once, because it depends on nothing that changes while the view is open.
    setting_entries: Vec<SettingEntry>,
    /// Buffer for an in-progress number edit on the checklist.
    picker_editing: Option<String>,
    /// Which column [`State::picker_editing`] is being typed into.
    picker_field: PickerField,
    picker_list: ListState,
    /// Scroll position of the declaration, which is longer than most terminals are tall.
    decl_list: ListState,
    /// Cursor on the first-run suggestions screen.
    sugg_list: ListState,
    /// Whether the last key was the first Enter of the two-press finish. Any other key
    /// clears it, so it can only ever describe the keypress immediately before this one.
    enter_armed: bool,
}

/// Run the configurator. Returns what the user decided; writing is the caller's job.
pub fn run(session: ConfigSession<'_>) -> Result<Outcome> {
    if session.rows.is_empty() {
        return Ok(Outcome::KeepAll);
    }

    let setting_entries = settings_entries(&session.rows);
    let mut list = ListState::default();
    list.select(Some(opening_index(&setting_entries, &session.rows)));

    let mut picker_list = ListState::default();
    picker_list.select(Some(0));

    let mut decl_list = ListState::default();
    decl_list.select(Some(0));

    let mut sugg_list = ListState::default();
    sugg_list.select(Some(0));

    let mut state = State {
        picker_active: vec![true; session.adapters.len()],
        picker_days: vec![None; session.adapters.len()],
        picker_caps: vec![None; session.adapters.len()],
        picker_entries: Vec::new(),
        setting_entries,
        picker_editing: None,
        picker_field: PickerField::Days,
        session,
        screen: Screen::Declaration,
        list,
        editing: None,
        error: None,
        picker_list,
        decl_list,
        sugg_list,
        enter_armed: false,
    };

    preaccept_recommended(&mut state);

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
    // "Twice" means twice in a row. Anything in between disarms, so an Enter pressed
    // minutes later cannot finish a gesture nobody remembers starting.
    if code != KeyCode::Enter {
        state.enter_armed = false;
    }
    match state.screen {
        Screen::Declaration => declaration_key(state, code),
        Screen::Suggestions => suggestions_key(state, code),
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
        // No `y` here any more. It used to mean "keep everything and go", which was
        // the one exit that never showed what was about to be written. Every exit goes
        // through the summary now, so the key that skipped it is gone rather than
        // rebound to something else.
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('c') | KeyCode::Char('C') => {
            state.screen = if state.session.suggestions.is_empty() {
                Screen::Settings
            } else {
                Screen::Suggestions
            };
            None
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => Some(Outcome::Cancelled),
        _ => None,
    }
}

/// Arrive with the safe recommendations already accepted.
///
/// This screen used to open with every box empty, on the reasoning that a pre-ticked
/// box teaches people to tick boxes. That reasoning is sound about consent and wrong
/// about this list: everything on the safe tier is a build directory a build command
/// puts back, under a 45-day idle window, and leaving them off by default meant the
/// common outcome of the first run was a tool that had been installed and configured to
/// reclaim almost nothing. The honest version of a default is not an empty one, it is a
/// visible one — so the header says what is accepted and which key clears the lot, and
/// `r` undoes all of it in one keystroke.
///
/// The cautious tier is deliberately untouched. `allow_manifest_rewrite` can leave a
/// change in `git status`, and the tier exists precisely because that is a thing to be
/// told before rather than after.
fn preaccept_recommended(state: &mut State<'_>) {
    for i in 0..state.session.suggestions.len() {
        if !state.session.suggestions[i].cautious {
            apply_suggestion(state, i, true);
        }
    }
}

/// Whether a suggestion is currently accepted: its setting already holds the value the
/// suggestion would set.
///
/// Derived rather than stored. The settings list two screens on can change the same
/// value, and a remembered "accepted" flag would then disagree with the setting it
/// claims to describe — the summary reads the settings, so the settings are the truth.
fn accepted(state: &State<'_>, index: usize) -> bool {
    let s = &state.session.suggestions[index];
    row_value(&state.session.rows, s.key).as_deref() == Some(s.value)
}

/// Accept or undo one suggestion. Undoing restores what the setting had when the view
/// opened, not a hard-coded default: the recommendation is the only thing being
/// withdrawn, and anything the user had already chosen is not this screen's to discard.
fn apply_suggestion(state: &mut State<'_>, index: usize, accept: bool) {
    let (key, value) = {
        let s = &state.session.suggestions[index];
        (s.key, s.value)
    };
    let restore = state
        .session
        .rows
        .iter()
        .find(|r| r.key == key)
        .map(|r| r.original.clone());
    let Some(restore) = restore else { return };
    let next = if accept { value.to_string() } else { restore };
    set_row(&mut state.session.rows, key, next);
}

fn suggestions_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    let len = state.session.suggestions.len();
    let current = state.sugg_list.selected().unwrap_or(0);
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            state
                .sugg_list
                .select(Some(if current == 0 { len - 1 } else { current - 1 }))
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state
                .sugg_list
                .select(Some(if current + 1 >= len { 0 } else { current + 1 }))
        }
        KeyCode::Char(' ') => {
            let now = accepted(state, current);
            apply_suggestion(state, current, !now);
        }
        // One key for the whole first tier, which is the point of the screen. It
        // deliberately does not reach the cautious tier: a button that accepts the thing
        // you were told to read about first is not a shortcut, it is a trap.
        KeyCode::Char('a') | KeyCode::Char('A') => {
            for i in 0..len {
                if !state.session.suggestions[i].cautious {
                    apply_suggestion(state, i, true);
                }
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            for i in 0..len {
                apply_suggestion(state, i, false);
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.screen = Screen::Settings;
        }
        // Straight to the summary: someone who took the suggestions and wants nothing
        // else should not have to walk the full list to get out. Twice, because one
        // Enter is what a person presses to dismiss a screen they have stopped reading,
        // and this one leaves the rest of the settings unvisited.
        KeyCode::Enter => {
            if state.enter_armed {
                state.enter_armed = false;
                state.screen = Screen::Summary;
            } else {
                state.enter_armed = true;
            }
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => return Some(Outcome::Cancelled),
        _ => {}
    }
    None
}

fn settings_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    // An in-progress number edit owns the keyboard until it is committed or abandoned.
    if state.editing.is_some() {
        return number_edit_key(state, code);
    }

    let at = state.list.selected().unwrap_or(0);
    let current = selected_row(state);
    // `selected_row` falls forward to the first row when the cursor is not on one, so
    // every arm that touches `current` has to know when the cursor is on the finish
    // line instead — otherwise Space down there would silently flip the top of the list.
    let on_finish = matches!(state.setting_entries.get(at), Some(SettingEntry::Finish));
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.error = None;
            let to = step(&state.setting_entries, at, false);
            state.list.select(Some(to));
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.error = None;
            let to = step(&state.setting_entries, at, true);
            state.list.select(Some(to));
        }
        KeyCode::Home | KeyCode::Char('g') => state.list.select(first_row(&state.setting_entries)),
        KeyCode::End | KeyCode::Char('G') => state.list.select(last_stop(&state.setting_entries)),
        KeyCode::Enter if on_finish => {
            state.error = None;
            if state.enter_armed {
                state.enter_armed = false;
                state.screen = Screen::Summary;
            } else {
                state.enter_armed = true;
            }
        }
        KeyCode::Char(' ') | KeyCode::Enter if !on_finish => activate(state, current),
        KeyCode::Char('r') | KeyCode::Char('R') if !on_finish => {
            state.error = None;
            let row = &mut state.session.rows[current];
            row.value = row.original.clone();
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
        Control::Choice(options) => {
            let row = &mut state.session.rows[index];
            // A value the list does not contain lands on the first option rather than
            // sticking: the row has to be able to leave a state the binary no longer
            // supports, which is what a config written by a newer version looks like.
            let next = options
                .iter()
                .position(|(value, _)| *value == row.value)
                .map_or(0, |at| (at + 1) % options.len());
            row.value = options[next].0.to_string();
        }
        Control::Number => state.editing = Some(state.session.rows[index].value.clone()),
        Control::Adapters | Control::AdapterDays | Control::CacheCaps => open_picker(state),
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
    let caps = parse_days(&row_value(rows, "cache_max_gb").unwrap_or_default());

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

    state.picker_caps = state
        .session
        .adapters
        .iter()
        .map(|name| {
            if !state.session.capped_adapters.contains(name) {
                return None;
            }
            caps.iter().find(|(n, _)| n == name).map(|(_, g)| *g)
        })
        .collect();

    state.picker_entries = build_entries(state.session.adapters, state.session.groups);
    state.picker_editing = None;
    state.picker_field = PickerField::Days;
    state.picker_list.select(Some(0));
    state.screen = Screen::Adapters;
}

fn number_edit_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    let index = selected_row(state);
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
        return picker_number_key(state, code);
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
            state.picker_field = PickerField::Days;
            state.picker_editing = Some(seed.map(|d| d.to_string()).unwrap_or_default());
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.error = None;
            // Nothing to type into: the adapter has no cache of its own name, so a cap
            // typed here would be stored against a manager that does not exist. Saying
            // so beats an editor that accepts a number and drops it.
            let targets = capped_targets(state, current);
            if targets.is_empty() {
                state.error = Some(
                    "No cache of that name for dev-prune to size. `devp caches` lists the ones \
                     there are; `devp config set cache_max_gb` caps them."
                        .to_string(),
                );
                return None;
            }
            let first = targets.first().and_then(|&i| state.picker_caps[i]);
            let seed = if targets.iter().all(|&i| state.picker_caps[i] == first) {
                first
            } else {
                None
            };
            state.picker_field = PickerField::Cap;
            state.picker_editing = Some(seed.map(|g| g.to_string()).unwrap_or_default());
        }
        KeyCode::Char('a') | KeyCode::Char('A') => state.picker_active.fill(true),
        KeyCode::Char('n') | KeyCode::Char('N') => state.picker_active.fill(false),
        KeyCode::Enter => commit_picker(state),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => state.screen = Screen::Settings,
        _ => {}
    }
    None
}

/// The adapters a cap typed at line `line` should land on: those under it that have a
/// cache of their own name.
///
/// A language heading types into every capped adapter beneath it at once and silently
/// skips the rest — "cap the JavaScript caches at 10" is one sentence, and the four
/// managers it reaches are exactly the four that have one.
fn capped_targets(state: &State<'_>, line: usize) -> Vec<usize> {
    let members: Vec<usize> = match state.picker_entries.get(line) {
        Some(PickerEntry::Adapter(i)) => vec![*i],
        Some(PickerEntry::Group { members, .. }) => members.clone(),
        None => Vec::new(),
    };
    members
        .into_iter()
        .filter(|&i| {
            state
                .session
                .capped_adapters
                .contains(&state.session.adapters[i])
        })
        .collect()
}

/// The inline number editor on the checklist, for whichever column
/// [`State::picker_field`] names. An empty buffer clears the value, which is the only
/// way back to "no window of its own" or "no cap" once a number is set.
fn picker_number_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
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
            let key = match state.picker_field {
                PickerField::Days => "adapter_idle_days",
                PickerField::Cap => "cache_max_gb",
            };
            let targets: Vec<usize> = match state.picker_field {
                PickerField::Days => match state.picker_entries.get(current) {
                    Some(PickerEntry::Adapter(i)) => vec![*i],
                    Some(PickerEntry::Group { members, .. }) => members.clone(),
                    None => Vec::new(),
                },
                PickerField::Cap => capped_targets(state, current),
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
                if let Err(why) = (state.session.validate)(key, &probe) {
                    state.error = Some(why);
                    return None;
                }
                typed.parse::<u64>().ok()
            };
            for i in targets {
                match state.picker_field {
                    PickerField::Days => state.picker_days[i] = value,
                    PickerField::Cap => state.picker_caps[i] = value,
                }
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

    let mut days: Vec<String> = adapters
        .iter()
        .enumerate()
        .filter_map(|(i, name)| state.picker_days[i].map(|d| format!("{name}={d}")))
        .collect();
    // Sorted for the same reason the caps below are: `config get adapter_idle_days`
    // prints a `BTreeMap`, and this row is compared against that. Assembling it in
    // adapter order instead would report an untouched setting as changed.
    days.sort_unstable();
    let days = if days.is_empty() {
        "(none)".to_string()
    } else {
        days.join(",")
    };

    // A cap on a cache with no adapter of its own name — `pip`, `nuget`, `conan`,
    // `conda`, `vcpkg`, `hex` — has no row on this screen to be edited from, and a
    // screen that writes back only what it can draw would delete it the first time
    // anyone opened the checklist for any other reason.
    let existing = parse_days(&row_value(&state.session.rows, "cache_max_gb").unwrap_or_default());
    let mut caps: Vec<String> = existing
        .iter()
        .filter(|(name, _)| !adapters.iter().any(|a| a == name))
        .map(|(name, gb)| format!("{name}={gb}"))
        .collect();
    caps.extend(
        adapters
            .iter()
            .enumerate()
            .filter_map(|(i, name)| state.picker_caps[i].map(|g| format!("{name}={g}"))),
    );
    caps.sort_unstable();
    let caps = if caps.is_empty() {
        "(none)".to_string()
    } else {
        caps.join(",")
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
    set_row(rows, "cache_max_gb", caps);
    for (key, value) in switches {
        set_row(rows, &key, value);
    }
    state.screen = Screen::Settings;
}

fn summary_key(state: &mut State<'_>, code: KeyCode) -> Option<Outcome> {
    match code {
        KeyCode::Enter => {
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
        Screen::Suggestions => render_suggestions(frame, state),
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
    // The reason block is in the layout only when there is a reason to give, rather than
    // always present and sometimes empty: an empty bordered box on the screen somebody
    // meets this tool on reads as something having failed to load.
    let notice = state.session.uninvited;
    let mut constraints = vec![Constraint::Length(3)];
    if notice.is_some() {
        constraints.push(Constraint::Length(6));
    }
    constraints.extend([
        Constraint::Min(5),
        // Four rather than three: two borders and two lines. What is true right now, and
        // what is true about the licence the whole screen is offered under.
        Constraint::Length(4),
        Constraint::Length(2),
    ]);
    let chunks = Layout::vertical(constraints).split(frame.area());
    let mut at = 0;

    frame.render_widget(
        header(
            state.session.title,
            "What this tool is allowed to do on this machine, before it does any of it.",
        ),
        chunks[at],
    );
    at += 1;

    if let Some(why) = notice {
        frame.render_widget(
            Paragraph::new(why)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .title(" Why this opened on its own ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
            chunks[at],
        );
        at += 1;
    }

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
        chunks[at],
        &mut state.decl_list,
    );
    at += 1;

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!("  {}", state.session.standing),
                Style::default().fg(Color::Green),
            )),
            // Dim, and under the green line rather than over it. The promise is the
            // reason to keep reading; the licence is the terms that promise is made on,
            // and putting the terms first is how a screen becomes one nobody finishes.
            Line::from(Span::styled(
                format!("  {}", crate::constants::LICENCE_NOTICE),
                dim(),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(dim())),
        chunks[at],
    );
    at += 1;

    frame.render_widget(
        footer(&[("↑↓", "read"), ("Enter", "configure"), ("q", "cancel")]),
        chunks[at],
    );
}

/// The first-run suggestions: a short list, in two tiers, with the selected one
/// explained twice underneath.
///
/// Two lines of explanation per setting rather than one, and only for the setting under
/// the cursor. Printing all of it at once is how a screen becomes a wall nobody reads,
/// which is the failure this screen exists to fix.
fn render_suggestions(frame: &mut Frame, state: &mut State<'_>) {
    // Only reachable with entries, and indexing on a drawn frame is not the place to be
    // sure of that: a panic here takes the terminal down with the alternate screen on.
    if state.session.suggestions.is_empty() {
        state.screen = Screen::Settings;
        return render_settings(frame, state);
    }
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(7),
        Constraint::Length(2),
    ])
    .split(frame.area());

    let on = (0..state.session.suggestions.len())
        .filter(|&i| accepted(state, i))
        .count();
    frame.render_widget(
        header(
            "Suggested settings",
            // Naming the arrow keys here rather than only in the footer: the first
            // reaction to a list of nine settings is to accept or skip the lot, and
            // nobody arrows through an unfamiliar list to find out whether anything
            // appears elsewhere on screen. The panel below is the point of the screen.
            &format!(
                "{} of {} accepted \u{2014} press \u{2191}\u{2193} to read what each one does. \
                 The safe ones start accepted; `r` turns every one of them back off.",
                on,
                state.session.suggestions.len()
            ),
        ),
        chunks[0],
    );

    let selected = state
        .sugg_list
        .selected()
        .unwrap_or(0)
        .min(state.session.suggestions.len() - 1);
    let mut items: Vec<ListItem> = Vec::new();
    let mut tier_shown = false;
    for (i, s) in state.session.suggestions.iter().enumerate() {
        // The tier heading is drawn as part of the first cautious entry rather than as an
        // entry of its own: a heading in the list would be a line the cursor can land on
        // and Space cannot do anything to.
        let mut lines = Vec::new();
        if s.cautious && !tier_shown {
            tier_shown = true;
            lines.push(Line::from(Span::styled(
                "  Worth turning on once you know what it does",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        let mark = if accepted(state, i) {
            Span::styled(
                "[x] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("[ ] ", dim())
        };
        lines.push(Line::from(vec![
            mark,
            Span::styled(
                crate::output::pad_display(s.label, 28),
                if selected == i {
                    Style::default().fg(Color::White)
                } else {
                    Style::default()
                },
            ),
            Span::styled(s.key.to_string(), dim()),
        ]));
        items.push(ListItem::new(lines));
    }

    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .title(" Suggested ")
                    .borders(Borders::ALL)
                    .border_style(dim()),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(30, 40, 60))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("\u{25b6} "),
        chunks[1],
        &mut state.sugg_list,
    );

    let s = &state.session.suggestions[selected];
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("  {}", s.help), Style::default())),
            Line::from(""),
            Line::from(vec![
                Span::styled("  In plain words  ", dim()),
                Span::styled(s.plain.to_string(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Why we suggest it  ", dim()),
                Span::styled(s.why.to_string(), Style::default().fg(Color::Green)),
            ]),
        ])
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).border_style(dim())),
        chunks[2],
    );

    frame.render_widget(
        footer(&[
            ("\u{2191}\u{2193}", "read"),
            ("Space", "accept one"),
            ("a", "accept all suggested"),
            ("r", "undo"),
            ("c", "all settings"),
            ("Enter Enter", "review and finish"),
        ]),
        chunks[3],
    );
}

fn render_settings(frame: &mut Frame, state: &mut State<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        // Seven rather than six: the pane gained the line that says what a fresh install
        // would hold, and losing a list row to it is the cheaper of the two trades.
        Constraint::Length(7),
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
        .setting_entries
        .iter()
        .enumerate()
        .map(|(at, entry)| {
            // Styled the way the declaration screen styles a `'#'` line and the checklist
            // styles a group label: one program, one way of saying "heading".
            let row = match entry {
                SettingEntry::Heading(title) => {
                    return ListItem::new(Line::from(Span::styled(
                        format!(" {title}"),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                SettingEntry::Row(i) => &state.session.rows[*i],
                SettingEntry::Finish => {
                    return ListItem::new(Line::from(vec![
                        Span::styled(
                            " Finish — review the changes  ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        if state.enter_armed {
                            Span::styled(
                                "Press Enter again for the summary",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::styled("Press Enter twice when you are done", dim())
                        },
                    ]));
                }
            };
            let control = match row.control {
                Control::Toggle if row.value == "true" => Span::styled(
                    "[x] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Control::Toggle => Span::styled("[ ] ", dim()),
                Control::Choice(_) => Span::styled("(o) ", dim()),
                Control::Number => Span::styled("123 ", dim()),
                Control::Adapters | Control::AdapterDays | Control::CacheCaps => {
                    Span::styled("••• ", dim())
                }
            };

            let shown = if state.editing.is_some() && selected == Some(at) {
                format!("{}_", state.editing.clone().unwrap_or_default())
            } else if let Control::Choice(options) = row.control {
                // The stored value *and* what it means. `en` alone would make this row
                // unreadable to the one person it exists for.
                options
                    .iter()
                    .find(|(value, _)| *value == row.value)
                    .map_or_else(
                        || row.value.clone(),
                        |(value, label)| format!("{value} {label}"),
                    )
            } else {
                row.value.clone()
            };

            let mut spans = vec![
                control,
                Span::styled(
                    crate::output::pad_display(row.key, 28),
                    if selected == Some(at) {
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
            // Green for "already what is suggested", yellow for "suggested, and this is
            // not it". Both are drawn, because a badge that disappears once taken tells
            // you nothing about the row you are looking at — only about the row you are
            // not.
            match row.takes_advice() {
                Some(true) => spans.push(Span::styled("REC ", Style::default().fg(Color::Green))),
                Some(false) => spans.push(Span::styled("REC ", Style::default().fg(Color::Yellow))),
                None => {}
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

    // The finish line has no row behind it, so it gets a pane of its own: what has
    // changed so far, and the fact that none of it has been written.
    if matches!(
        state
            .setting_entries
            .get(state.list.selected().unwrap_or(0)),
        Some(SettingEntry::Finish)
    ) {
        let mut detail = vec![
            Line::from(Span::styled(
                "  Two presses of Enter open a summary of every change. \
                 Nothing has been written yet.",
                Style::default(),
            )),
            Line::from(vec![
                Span::styled("  Changed so far  ", dim()),
                Span::styled(
                    match changed {
                        0 => "nothing".to_string(),
                        1 => "1 setting".to_string(),
                        n => format!("{n} settings"),
                    },
                    Style::default().fg(Color::Cyan),
                ),
            ]),
        ];
        if state.enter_armed {
            detail.push(Line::from(Span::styled(
                "  Press Enter again for the summary.",
                Style::default().fg(Color::Green),
            )));
        }
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).border_style(dim())),
            chunks[2],
        );
        frame.render_widget(
            footer(&[
                ("↑↓", "move"),
                ("Enter Enter", "review and finish"),
                ("q", "cancel"),
            ]),
            chunks[3],
        );
        return;
    }

    // The help for the highlighted row, and any refusal, in the same place: a message
    // about a field belongs next to the field.
    let row = &state.session.rows[selected_row(state)];
    let mut detail = vec![
        Line::from(Span::styled(format!("  {}", row.help), Style::default())),
        Line::from(vec![
            Span::styled("  In plain words  ", dim()),
            Span::styled(row.plain.to_string(), Style::default().fg(Color::Cyan)),
        ]),
    ];
    // The two questions a row cannot answer about itself: what it would be if nobody had
    // ever touched it, and what it is suggested to be. Neither is what it currently is,
    // which is the only one the list column shows.
    let mut facts = vec![
        Span::styled("  Default  ", dim()),
        Span::styled(
            crate::output::pad_display(&row.default, 12),
            Style::default(),
        ),
    ];
    if let Some(rec) = row.recommended {
        facts.push(Span::styled("Recommended  ", dim()));
        facts.push(Span::styled(
            crate::output::pad_display(rec, 12),
            Style::default().fg(Color::Green),
        ));
        facts.push(Span::styled(
            if row.takes_advice() == Some(true) {
                "— already set"
            } else {
                "— suggested, not required; everything works without it"
            },
            dim(),
        ));
    }
    detail.push(Line::from(facts));
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
            ("End", "finish"),
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
                let editing_here = state.picker_editing.is_some() && selected == Some(line);
                let shared = members.first().and_then(|&i| state.picker_days[i]);
                // A heading is an editing target like any adapter row, so it has to show
                // the buffer being typed into it — otherwise the keys land silently.
                let window = if editing_here && state.picker_field == PickerField::Days {
                    format!("{}_", state.picker_editing.clone().unwrap_or_default())
                } else if members.iter().all(|&i| state.picker_days[i] == shared) {
                    shared.map(|d| format!("{d}d")).unwrap_or_default()
                } else {
                    "mixed".to_string()
                };
                let capped: Vec<usize> = members
                    .iter()
                    .copied()
                    .filter(|&i| {
                        state
                            .session
                            .capped_adapters
                            .contains(&state.session.adapters[i])
                    })
                    .collect();
                let shared_cap = capped.first().and_then(|&i| state.picker_caps[i]);
                let cap = if editing_here && state.picker_field == PickerField::Cap {
                    format!("{}_", state.picker_editing.clone().unwrap_or_default())
                } else if capped.is_empty() {
                    String::new()
                } else if capped.iter().all(|&i| state.picker_caps[i] == shared_cap) {
                    shared_cap.map(|g| format!("{g}G")).unwrap_or_default()
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
                    Span::styled(crate::output::pad_display(&window, 10), dim()),
                    Span::styled(cap, dim()),
                ]))
            }
            PickerEntry::Adapter(i) => {
                let name = state.session.adapters[*i];
                let editing_here = state.picker_editing.is_some() && selected == Some(line);
                let shown = if editing_here && state.picker_field == PickerField::Days {
                    format!("{}_", state.picker_editing.clone().unwrap_or_default())
                } else {
                    state.picker_days[*i]
                        .map(|d| format!("{d}d"))
                        .unwrap_or_else(|| "default".to_string())
                };
                // Blank, not "no cap": there is no cache of this name for a cap to be
                // about, and an empty cell is the only honest way to draw a column that
                // does not apply to this row.
                let cap = if editing_here && state.picker_field == PickerField::Cap {
                    format!("{}_", state.picker_editing.clone().unwrap_or_default())
                } else if !state.session.capped_adapters.contains(&name) {
                    String::new()
                } else {
                    state.picker_caps[*i]
                        .map(|g| format!("{g}G"))
                        .unwrap_or_else(|| "no cap".to_string())
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
                    Span::styled(
                        crate::output::pad_display(&cap, 10),
                        if state.picker_caps[*i].is_some() {
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
                .title(" Checked adapters stay active      idle      cache cap ")
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
         puts it back on the global window. c caps that ecosystem's download cache in \
         GiB — reported by `devp caches`, and emptied only when you run \
         `devp caches clear --over-cap`, never on a schedule.",
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

    let keys: &[(&str, &str)] = match (state.picker_editing.is_some(), state.picker_field) {
        (true, PickerField::Days) => &[
            ("digits", "days"),
            ("Enter", "accept"),
            ("empty", "use the global window"),
            ("Esc", "abandon"),
        ],
        (true, PickerField::Cap) => &[
            ("digits", "GiB"),
            ("Enter", "accept"),
            ("empty", "no cap"),
            ("Esc", "abandon"),
        ],
        (false, _) => &[
            ("↑↓", "move"),
            ("Space", "toggle"),
            ("d", "idle days"),
            ("c", "cache cap"),
            ("a", "all on"),
            ("n", "all off"),
            ("Enter", "accept"),
            ("Esc", "back"),
        ],
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
        categorised_row(key, "Settings", control, value)
    }

    /// A row in a named group, for the tests that are about the grouping itself.
    fn categorised_row(
        key: &'static str,
        category: &'static str,
        control: Control,
        value: &str,
    ) -> ConfigRow {
        ConfigRow {
            key,
            category,
            help: "help",
            plain: "plain",
            control,
            value: value.to_string(),
            original: value.to_string(),
            default: value.to_string(),
            recommended: None,
            is_new: false,
        }
    }

    fn session<'a>(rows: Vec<ConfigRow>, adapters: &'a [&'static str]) -> ConfigSession<'a> {
        ConfigSession {
            declaration: Vec::new(),
            standing: String::new(),
            suggestions: Vec::new(),
            rows,
            adapters,
            opt_in_adapters: &[],
            capped_adapters: &["npm", "pnpm", "cargo", "go"],
            groups: &[("Test", &["npm", "cargo", "go"])],
            validate: &|key, v| {
                // Stands in for the real setters: the same shapes accepted, so a test
                // that types a value the checklist stores is a test the wizard passes.
                let number = if key == "adapter_idle_days" || key == "cache_max_gb" {
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
            uninvited: None,
        }
    }

    fn state<'a>(s: ConfigSession<'a>) -> State<'a> {
        let setting_entries = settings_entries(&s.rows);
        let mut list = ListState::default();
        // The first row, not entry 0 — entry 0 is a heading, which is the one
        // place the cursor is never allowed to be.
        list.select(first_row(&setting_entries));
        let mut picker_list = ListState::default();
        picker_list.select(Some(0));
        State {
            picker_active: vec![true; s.adapters.len()],
            picker_days: vec![None; s.adapters.len()],
            picker_caps: vec![None; s.adapters.len()],
            picker_entries: build_entries(s.adapters, s.groups),
            setting_entries,
            picker_editing: None,
            picker_field: PickerField::Days,
            session: s,
            screen: Screen::Settings,
            list,
            editing: None,
            error: None,
            picker_list,
            decl_list: ListState::default(),
            sugg_list: ListState::default(),
            enter_armed: false,
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
    fn the_declaration_no_longer_leaves_on_y() {
        // `y` used to mean "keep everything and go", and it was the one exit that never
        // showed what was about to be written. Rebinding it to something else would be
        // worse than dropping it: the habit would then do a different thing silently.
        let mut st = state(session(vec![row("idle_days", Control::Number, "14")], &[]));
        st.screen = Screen::Declaration;
        assert!(handle_key(&mut st, KeyCode::Char('y')).is_none());
        assert_eq!(st.screen, Screen::Declaration);
    }

    #[test]
    fn finishing_takes_two_presses_of_enter() {
        // One Enter is what people press to dismiss a screen they have stopped reading.
        // Two is a decision, and the second one opens the summary rather than saving.
        let mut st = state(session(
            vec![row("auto_update", Control::Toggle, "false")],
            &[],
        ));
        handle_key(&mut st, KeyCode::End);
        assert!(handle_key(&mut st, KeyCode::Enter).is_none());
        assert_eq!(st.screen, Screen::Settings, "one press must not leave");
        assert!(st.enter_armed);
        assert!(handle_key(&mut st, KeyCode::Enter).is_none());
        assert_eq!(st.screen, Screen::Summary);

        // And the two have to be consecutive.
        st.screen = Screen::Settings;
        handle_key(&mut st, KeyCode::End);
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Up);
        handle_key(&mut st, KeyCode::End);
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(
            st.screen,
            Screen::Settings,
            "a keypress in between must disarm the first Enter"
        );
    }

    #[test]
    fn the_finish_line_is_not_a_setting() {
        // It shares the list with the rows, and `selected_row` answers with the first
        // row when the cursor is not on one. Space there must do nothing at all rather
        // than reach past the cursor and flip the top of the list.
        let mut st = state(session(
            vec![row("auto_update", Control::Toggle, "false")],
            &[],
        ));
        handle_key(&mut st, KeyCode::End);
        handle_key(&mut st, KeyCode::Char(' '));
        handle_key(&mut st, KeyCode::Char('r'));
        assert_eq!(st.session.rows[0].value, "false");
        assert!(!st.session.rows[0].changed());
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
    fn a_cache_cap_typed_on_a_heading_reaches_only_the_adapters_that_have_a_cache() {
        // The two lists overlap without either containing the other, so a heading has to
        // skip the members dev-prune knows no cache for rather than store a cap against
        // a manager name that does not exist.
        let adapters: &[&'static str] = &["npm", "pnpm", "venv"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("cache_max_gb", Control::CacheCaps, "(none)"),
            ],
            adapters,
        ));
        st.session.groups = &[("JavaScript", &["npm", "pnpm"]), ("Python", &["venv"])];
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Char('c')); // on the JavaScript heading
        handle_key(&mut st, KeyCode::Char('1'));
        handle_key(&mut st, KeyCode::Char('0'));
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.picker_caps, vec![Some(10), Some(10), None]);

        handle_key(&mut st, KeyCode::Enter); // accept the checklist
        assert_eq!(st.session.rows[1].value, "npm=10,pnpm=10");
    }

    #[test]
    fn a_cache_cap_is_cleared_by_emptying_it() {
        let adapters: &[&'static str] = &["npm"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("cache_max_gb", Control::CacheCaps, "npm=10"),
            ],
            adapters,
        ));
        st.session.groups = &[("JavaScript", &["npm"])];
        handle_key(&mut st, KeyCode::Enter);
        // Opening shows the cap that is already set, or accepting the screen for any
        // other reason would quietly drop it.
        assert_eq!(st.picker_caps, vec![Some(10)]);
        handle_key(&mut st, KeyCode::Down); // heading -> npm
        handle_key(&mut st, KeyCode::Char('c'));
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Backspace);
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[1].value, "(none)");
    }

    #[test]
    fn a_cap_on_a_cache_with_no_adapter_survives_the_checklist() {
        // `pip`, `nuget`, `conan`, `conda`, `vcpkg` and `hex` are caches no adapter is
        // named after, so they have no row here to be edited from. The screen writes
        // back the whole setting, and without this it would delete them the first time
        // anyone opened the checklist for any other reason.
        let adapters: &[&'static str] = &["npm"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("cache_max_gb", Control::CacheCaps, "npm=10,pip=20"),
            ],
            adapters,
        ));
        st.session.groups = &[("JavaScript", &["npm"])];
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Enter);
        assert_eq!(st.session.rows[1].value, "npm=10,pip=20");
    }

    #[test]
    fn typing_a_cap_where_there_is_no_cache_says_so_instead_of_dropping_it() {
        let adapters: &[&'static str] = &["venv"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("cache_max_gb", Control::CacheCaps, "(none)"),
            ],
            adapters,
        ));
        st.session.groups = &[("Python", &["venv"])];
        handle_key(&mut st, KeyCode::Enter);
        handle_key(&mut st, KeyCode::Down); // heading -> venv
        handle_key(&mut st, KeyCode::Char('c'));
        assert!(st.picker_editing.is_none(), "no editor opened");
        let err = st.error.clone().expect("the refusal is explained");
        assert!(err.contains("devp caches"), "{err}");
    }

    #[test]
    fn the_checklist_draws_the_cache_cap_beside_the_idle_window() {
        // Both settings are per adapter, and the whole point of the third column is that
        // one screen answers "what is on, for how long, and how big".
        let adapters: &[&'static str] = &["npm", "venv"];
        let mut st = state(session(
            vec![
                row("disabled_adapters", Control::Adapters, "(none)"),
                row("adapter_idle_days", Control::AdapterDays, "npm=30"),
                row("cache_max_gb", Control::CacheCaps, "npm=10"),
            ],
            adapters,
        ));
        st.session.groups = &[("JavaScript", &["npm"]), ("Python", &["venv"])];
        handle_key(&mut st, KeyCode::Enter);
        let picker = screenshot(&mut st, Screen::Adapters);
        assert!(picker.contains("30d"), "the idle window is drawn");
        assert!(picker.contains("10G"), "the cap is drawn");
        // `venv` has no cache, so its cell is blank rather than "no cap" — there is
        // nothing there for a cap to be about.
        assert!(picker.contains("no cap") || picker.contains("10G"));
        assert!(picker.contains("cache cap"), "the column is labelled");
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
        handle_key(&mut st, KeyCode::End); // onto the finish line
        handle_key(&mut st, KeyCode::Enter); // arm
        handle_key(&mut st, KeyCode::Enter); // to the summary
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
            categorised_row("idle_days", "Scope", Control::Number, "14"),
            categorised_row("auto_update", "Updates", Control::Toggle, "false"),
            categorised_row("auto_config", "Updates", Control::Toggle, "false"),
        ];
        let entries = settings_entries(&rows);
        // Heading, idle_days, heading, auto_update, auto_config, finish.
        assert_eq!(entries.len(), 6);
        assert_eq!(
            opening_index(&entries, &rows),
            1,
            "with nothing new, start at the first row — never on a heading"
        );
        rows[2].is_new = true;
        assert_eq!(
            opening_index(&entries, &rows),
            4,
            "an index into the drawn list, not into the rows"
        );
    }

    #[test]
    fn the_cursor_never_lands_on_a_heading() {
        let rows = vec![
            categorised_row("idle_days", "Scope", Control::Number, "14"),
            categorised_row("auto_update", "Updates", Control::Toggle, "false"),
        ];
        let entries = settings_entries(&rows);
        assert_eq!(entries.len(), 5, "two rows, two headings, one finish line");

        // Every stop, in both directions and all the way round, is a row.
        for forward in [true, false] {
            let mut at = first_row(&entries).expect("a row");
            for _ in 0..entries.len() * 2 {
                at = step(&entries, at, forward);
                assert!(
                    matches!(entries[at], SettingEntry::Row(_) | SettingEntry::Finish),
                    "stopped on entry {at}, which is a heading"
                );
            }
        }

        // And it wraps between the ends rather than sticking on the last heading.
        let last = last_stop(&entries).expect("a stop");
        assert_eq!(step(&entries, last, true), first_row(&entries).unwrap());
        assert_eq!(step(&entries, first_row(&entries).unwrap(), false), last);
    }

    #[test]
    fn a_run_of_one_category_gets_one_heading() {
        let rows = vec![
            categorised_row("a", "Scope", Control::Toggle, "false"),
            categorised_row("b", "Scope", Control::Toggle, "false"),
            categorised_row("c", "Scope", Control::Toggle, "false"),
        ];
        // Three rows, one heading, one finish line.
        assert_eq!(settings_entries(&rows).len(), 5);
    }

    fn suggestion(key: &'static str, cautious: bool) -> Suggestion {
        Suggestion {
            key,
            label: "label",
            help: "help",
            plain: "plain",
            why: "why",
            value: "true",
            cautious,
        }
    }

    #[test]
    fn the_safe_tier_arrives_accepted_and_the_cautious_one_does_not() {
        // The setting that reads best on this screen is the one nobody has to press a
        // key for. The setting that reads worst is the one that edits a tracked file
        // and was accepted by a screen the user had not finished reading.
        let adapters: &[&str] = &["npm"];
        let mut s = session(
            vec![
                row("enable_cargo", Control::Toggle, "false"),
                row("allow_manifest_rewrite", Control::Toggle, "false"),
            ],
            adapters,
        );
        s.suggestions = vec![
            suggestion("enable_cargo", false),
            suggestion("allow_manifest_rewrite", true),
        ];
        let mut st = state(s);
        preaccept_recommended(&mut st);

        assert_eq!(
            row_value(&st.session.rows, "enable_cargo").as_deref(),
            Some("true"),
            "the safe tier should already be on"
        );
        assert_eq!(
            row_value(&st.session.rows, "allow_manifest_rewrite").as_deref(),
            Some("false"),
            "the cautious tier must still be a deliberate choice"
        );

        // And `r` still means what the footer says it means: one keystroke back to
        // exactly what the machine held before this screen opened.
        st.screen = Screen::Suggestions;
        st.sugg_list.select(Some(0));
        assert!(suggestions_key(&mut st, KeyCode::Char('r')).is_none());
        assert_eq!(
            row_value(&st.session.rows, "enable_cargo").as_deref(),
            Some("false")
        );
    }

    #[test]
    fn a_recommended_row_says_so_and_names_the_fresh_default() {
        let adapters: &[&str] = &["npm"];
        let mut rows = vec![row("enable_cargo", Control::Toggle, "false")];
        rows[0].recommended = Some("true");
        let mut st = state(session(rows, adapters));

        let shot = screenshot(&mut st, Screen::Settings);
        assert!(
            shot.contains("REC"),
            "the badge is the only thing on the row \
                                       that says a recommendation exists"
        );
        assert!(
            shot.contains("Default"),
            "a value nobody chose is unreadable without the one they would have got"
        );
        // Worded as advice. A configurator that says "required" about a setting the
        // tool runs perfectly well without has spent the word it needs for the ones
        // that are.
        assert!(shot.contains("not required"));
    }

    #[test]
    fn accept_all_stops_at_the_cautious_tier() {
        // The whole reason the second tier exists. A single key that also accepted the
        // setting the screen just told you to think about would make the warning
        // decorative.
        let adapters: &[&str] = &["npm"];
        let mut s = session(
            vec![
                row("enable_cargo", Control::Toggle, "false"),
                row("allow_manifest_rewrite", Control::Toggle, "false"),
            ],
            adapters,
        );
        s.suggestions = vec![
            suggestion("enable_cargo", false),
            suggestion("allow_manifest_rewrite", true),
        ];
        let mut st = state(s);
        st.screen = Screen::Suggestions;
        st.sugg_list.select(Some(0));

        assert!(suggestions_key(&mut st, KeyCode::Char('a')).is_none());
        assert_eq!(
            row_value(&st.session.rows, "enable_cargo").as_deref(),
            Some("true")
        );
        assert_eq!(
            row_value(&st.session.rows, "allow_manifest_rewrite").as_deref(),
            Some("false")
        );

        // Reachable, just not by the one key: Space on the row itself still takes it.
        st.sugg_list.select(Some(1));
        suggestions_key(&mut st, KeyCode::Char(' '));
        assert_eq!(
            row_value(&st.session.rows, "allow_manifest_rewrite").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn undoing_a_suggestion_puts_back_what_the_setting_had() {
        let adapters: &[&str] = &["npm"];
        let mut s = session(
            vec![row("enable_cargo", Control::Toggle, "false")],
            adapters,
        );
        s.suggestions = vec![suggestion("enable_cargo", false)];
        let mut st = state(s);
        st.screen = Screen::Suggestions;
        st.sugg_list.select(Some(0));

        suggestions_key(&mut st, KeyCode::Char(' '));
        assert!(accepted(&st, 0));
        suggestions_key(&mut st, KeyCode::Char(' '));
        assert!(!accepted(&st, 0));
        assert_eq!(
            row_value(&st.session.rows, "enable_cargo").as_deref(),
            Some("false")
        );
        // And the summary must not offer to save a value that never changed.
        assert!(!st.session.rows[0].changed());
    }

    #[test]
    fn the_suggestions_screen_is_skipped_when_there_is_nothing_to_suggest() {
        // Every run but the first: `first_run_suggestions` returns nothing, and Enter on
        // the declaration must go straight to the settings rather than to a blank screen.
        let adapters: &[&str] = &["npm"];
        let s = session(vec![row("idle_days", Control::Number, "30")], adapters);
        let mut st = state(s);
        st.screen = Screen::Declaration;
        assert!(declaration_key(&mut st, KeyCode::Enter).is_none());
        assert_eq!(st.screen, Screen::Settings);
    }

    #[test]
    fn the_first_run_reaches_the_suggestions_first() {
        let adapters: &[&str] = &["npm"];
        let mut s = session(
            vec![row("enable_cargo", Control::Toggle, "false")],
            adapters,
        );
        s.suggestions = vec![suggestion("enable_cargo", false)];
        let mut st = state(s);
        st.screen = Screen::Declaration;
        assert!(declaration_key(&mut st, KeyCode::Enter).is_none());
        assert_eq!(st.screen, Screen::Suggestions);
    }
}
