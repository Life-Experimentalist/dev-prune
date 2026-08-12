// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Terminal UI components for dev-prune.

use std::io::{Stdout, stdout};
use std::panic::PanicHookInfo;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::cursor::Show;
use crossterm::event;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub mod selection_view;
pub mod status_view;

/// Put the terminal back the way it was found.
///
/// Every step is best-effort and independent: if leaving the alternate screen fails there
/// is still a raw-mode flag to clear, and a terminal left in raw mode with a hidden cursor
/// is a terminal the user has to close and reopen.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = stdout().execute(LeaveAlternateScreen);
    let _ = stdout().execute(Show);
}

/// An entered full-screen terminal session that always exits cleanly.
///
/// The three ways out of a TUI are a normal return, an error, and a panic. Before this
/// guard existed each view handled the first by hand and leaked the terminal on the other
/// two — `?` between "raw mode on" and the restore call would return with the screen still
/// swapped and echo still off, which reads to the user as a hung shell.
pub(crate) struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    prior_hook: Arc<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>,
}

impl Tui {
    /// Enter raw mode and the alternate screen, and arm the restore paths.
    pub fn new() -> Result<Self> {
        let prior_hook: Arc<dyn Fn(&PanicHookInfo<'_>) + Sync + Send> =
            Arc::from(std::panic::take_hook());

        // Restore first, then let the previous hook print: a panic message rendered into
        // the alternate screen vanishes the moment the screen is dropped.
        let hook_for_panic = Arc::clone(&prior_hook);
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            hook_for_panic(info);
        }));

        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;

        match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => Ok(Self {
                terminal,
                prior_hook,
            }),
            Err(e) => {
                // Constructing the backend failed *after* the screen was swapped. `Drop`
                // never runs for a `Self` that was never built, so both the screen and
                // the panic hook have to be put back by hand here. Note this is a
                // `set_hook`, not a `take_hook`: taking would install std's default and
                // silently discard whatever hook the caller had before.
                restore_terminal();
                std::panic::set_hook(Box::new(move |info| prior_hook(info)));
                Err(e.into())
            }
        }
    }

    /// Discard input that arrived before the view was ready for it.
    ///
    /// The Enter keypress that launched the command is still queued when the loop starts,
    /// and on Windows its KeyPress/KeyRelease pair arrives inside the loop and confirms
    /// the selection instantly. The sleep gives the console time to deliver it so that the
    /// drain below has something to drain.
    pub fn drain_stale_input(&self, settle: Duration) {
        std::thread::sleep(settle);
        // Deliberately not `?`: a failure to drain must never abort the view.
        while matches!(event::poll(Duration::from_millis(50)), Ok(true)) {
            let _ = event::read();
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        restore_terminal();
        let _ = self.terminal.show_cursor();
        // Hand the panic hook back to whoever owned it, rather than to std's default.
        let prior = Arc::clone(&self.prior_hook);
        std::panic::set_hook(Box::new(move |info| prior(info)));
    }
}
