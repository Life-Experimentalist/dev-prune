// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune completions`.
//
// The script is generated from the same clap definition the binary parses arguments
// with, so a flag cannot exist in one and be missing from the other. That is the whole
// reason this is a subcommand rather than five checked-in files that go stale.

use std::path::PathBuf;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell;

use crate::Cli;
use crate::constants;

/// Print a completion script for `shell` to stdout.
///
/// The script is written for whichever of the two names invoked it — `devp completions`
/// completes `devp`, `dev-prune completions` completes `dev-prune`. They are the same
/// executable, but a completion script is registered against a command *name*, so one
/// script cannot serve both and guessing would leave half the users without completion.
///
/// Nothing else is printed. Not a header, not the credit line, not a "now add this to
/// your profile" hint — the output is piped into a file or `eval`'d, and anything extra
/// in it is a shell error on every new terminal.
pub fn run(shell: Shell) -> Result<()> {
    let mut command = Cli::command();
    let bin_name = invoked_as();
    clap_complete::generate(shell, &mut command, bin_name, &mut std::io::stdout());
    Ok(())
}

/// The name this process was launched under, without any `.exe`.
///
/// Falls back to the canonical name when `argv[0]` is missing or empty, which is not a
/// thing a shell does but is a thing an embedder can do.
fn invoked_as() -> String {
    std::env::args_os()
        .next()
        .map(PathBuf::from)
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| constants::APP_NAME.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shell_produces_a_script() {
        // A generator that panics or emits nothing would only be discovered by a user
        // sourcing the output, at which point their shell is the error message.
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Elvish,
        ] {
            let mut command = Cli::command();
            let mut buffer: Vec<u8> = Vec::new();
            clap_complete::generate(shell, &mut command, "devp", &mut buffer);
            let script = String::from_utf8(buffer).expect("completion script is UTF-8");

            assert!(!script.is_empty(), "{shell} produced nothing");
            assert!(
                script.contains("devp"),
                "{shell} script does not name the binary"
            );
            assert!(
                script.contains("stats"),
                "{shell} script is missing a subcommand"
            );
        }
    }

    #[test]
    fn the_binary_name_falls_back_to_the_canonical_one() {
        // Not empty, whatever the test harness passes as argv[0].
        assert!(!invoked_as().is_empty());
    }
}
