// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Pretty-print helpers for terminal output.
//
// Provides colored, formatted output for CLI commands and terminal spinners.

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;

/// Helper to strip Windows UNC `\\?\` prefix, macOS `/private/` prefix, and collapse double slashes.
pub fn clean_path<P: AsRef<Path>>(path: P) -> String {
    let s = path.as_ref().display().to_string();
    let s = if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        s
    };
    let s = if let Some(stripped) = s.strip_prefix("/private/var/") {
        format!("/var/{stripped}")
    } else if let Some(stripped) = s.strip_prefix("/private/tmp/") {
        format!("/tmp/{stripped}")
    } else {
        s
    };
    s.replace("//", "/")
}

/// Create an animated terminal loading spinner for long-running operations.
pub fn create_spinner(msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .expect("Invalid progress bar template"),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Print a success message (green checkmark)
pub fn print_success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}

/// Print a warning message (yellow exclamation)
pub fn print_warning(msg: &str) {
    println!("{} {}", "⚠".yellow().bold(), msg);
}

/// Print an error message (red X)
pub fn print_error(msg: &str) {
    eprintln!("{} {}", "✗".red().bold(), msg);
}

/// Print an info message (blue arrow)
pub fn print_info(msg: &str) {
    println!("{} {}", "→".blue().bold(), msg);
}

/// Print a notice to stderr.
///
/// For anything the user should see that is *about* the command rather than part of its
/// output — a deprecated flag, say. It has to be stderr: `--json` promises stdout carries
/// one JSON document and nothing else, and a friendly note printed above it is the
/// difference between a parseable contract and a parse error.
pub fn print_notice(msg: &str) {
    eprintln!("{} {}", "→".blue().bold(), msg);
}

/// Print a section header
pub fn print_header(msg: &str) {
    println!("\n{}", msg.bold().underline());
}

/// Print the dev-prune ASCII art banner
pub fn print_banner() {
    let art = format!(
        r#"
 ___    _____ __     __    ____  ____  _   _ _   _ _____
|  _ \ | ____|\ \   / /   |  _ \|  _ \| | | | \ | | ____|
| | | ||  _|   \ \ / /    | |_) | |_) | | | |  \| |  _|
| |_| || |___   \ V /     |  __/|  _ <| |_| | |\  | |___
|____/ |_____|   \_/      |_|   |_| \_\\___/|_| \_|_____| v{}
"#,
        crate::constants::VERSION
    );
    println!("{}", art.truecolor(64, 224, 208).bold());
}

/// Pick the singular or plural form for a count.
///
/// Small, but "Unregistered 1 repositories" is the kind of thing people notice and
/// nothing else in the codebase was doing it consistently.
pub fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// Format bytes into human-readable string (e.g., "1.2 GB", "450 MB")
pub fn format_bytes(bytes: u64) -> String {
    use humansize::{BINARY, format_size};
    format_size(bytes, BINARY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1 GiB");
    }

    #[test]
    fn test_clean_path() {
        assert_eq!(clean_path(r"\\?\C:\Users\krish"), r"C:\Users\krish");
        assert_eq!(clean_path(r"/private/var/tmp/repo"), r"/var/tmp/repo");
        assert_eq!(clean_path(r"//home//user//repo"), r"/home/user/repo");
    }
}
