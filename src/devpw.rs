// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The windowless twin: the same CLI, built for the GUI subsystem.
//
// The subsystem is a single `u16` in the PE optional header, and it is the only
// difference between this binary and `dev-prune.exe` — the `python.exe`/`pythonw.exe`
// relationship. A GUI-subsystem process is never given a console, so the scheduled task
// that runs it cannot flash a black window at whoever is logged in, while the pipes and
// files it writes still work exactly as they do from a terminal.
//
// This used to be produced at runtime: the binary read its own image, rewrote that field
// and wrote the result out as `devpw.exe`. That is a program writing a modified copy of
// its own executable to disk and then registering it for persistence, which is the
// textbook description of a dropper — Sophos quarantined the binary on that profile and
// WinGet's dynamic validation failed it. Asking the linker for the subsystem is what the
// attribute below does, so the file ships in the archives, `cargo install` places it, and
// nothing has to be generated on a user's machine.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    dev_prune::run_cli();
}

// Cargo cannot declare a `[[bin]]` for one platform, so this target is built wherever
// dev-prune is. Only Windows has a console to suppress, and only the Windows archives
// ship the file — but `cargo install` places every binary a crate defines, so on Linux
// and macOS this would otherwise put a third seven-megabyte copy of the whole CLI on
// PATH under a name nobody has heard of. A stub that says what it is costs kilobytes and
// answers the question instead.
#[cfg(not(windows))]
fn main() {
    eprintln!(
        "devpw is the Windows-only windowless build of dev-prune; it exists so a \
         scheduled task can run without flashing a console. On this platform there is \
         nothing for it to do — use `devp`."
    );
    // 2 is this CLI's usage-error code, and being asked to run at all is the error.
    std::process::exit(2);
}
