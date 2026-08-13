// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! Handler for `dev-prune caches`.
//!
//! Every package manager keeps a machine-wide download cache outside any repository:
//! npm's `_cacache`, pnpm's content-addressable store, the Go module cache, cargo's
//! registry. They are frequently the largest reclaimable thing on a developer's disk and
//! nobody notices, because nothing ever mentions them — a 4 GiB `GOMODCACHE` looks like
//! free space that simply went missing.
//!
//! This command finds them, sizes them, and prints the command that clears each one.
//!
//! **It deletes nothing, ever.** That is the entire design. A cache is shared by every
//! project on the machine, so its contents are not something dev-prune can prove is
//! recoverable for any one repository — which is the bar every deletion in this tool has
//! to clear. It is also the thing that makes `devp restore` fast: clearing a cache turns
//! the next reinstall into a download. Reporting is most of the value and none of the
//! risk, so the clear commands are printed for a human to run deliberately.
//!
//! Each manager is asked where its own cache lives rather than being assumed — a
//! `CARGO_HOME`, a `--cache-dir`, a corporate `.npmrc` all move it. Every one of those
//! queries is read-only, and a manager that is not installed falls back to the
//! conventional location, so a cache left behind by an uninstalled manager still shows up.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::adapters;
use crate::constants;
use crate::json;
use crate::output;

/// One cache directory that exists on this machine.
pub struct CacheReport {
    /// The package manager that owns it.
    pub manager: &'static str,
    /// Which of that manager's caches this is, when it keeps more than one.
    pub kind: &'static str,
    /// Where it actually is, as resolved on this machine.
    pub path: PathBuf,
    /// Total size on disk.
    pub bytes: u64,
    /// The command that empties it. Printed, never run.
    pub clear_command: &'static str,
    /// What the user gives up by running that command, when it is more than time.
    pub note: Option<&'static str>,
}

/// How to find one cache.
struct Probe {
    manager: &'static str,
    kind: &'static str,
    /// The manager's own answer to "where is it?", as `(program, args)`.
    ///
    /// All of these print a path and exit; none of them writes anything or creates the
    /// directory. `None` means the ecosystem has no such query and only the conventional
    /// locations are available.
    query: Option<(&'static str, &'static [&'static str])>,
    clear_command: &'static str,
    note: Option<&'static str>,
}

/// cargo ships no cache subcommand, so the only honest "how do I clear this" is the
/// deletion itself. `cargo build` re-downloads and re-extracts what it needs.
#[cfg(windows)]
const CARGO_CACHE_CLEAR: &str =
    r"Remove-Item -Recurse -Force $env:USERPROFILE\.cargo\registry\cache";
#[cfg(not(windows))]
const CARGO_CACHE_CLEAR: &str = "rm -rf ~/.cargo/registry/cache";

#[cfg(windows)]
const CARGO_SRC_CLEAR: &str = r"Remove-Item -Recurse -Force $env:USERPROFILE\.cargo\registry\src";
#[cfg(not(windows))]
const CARGO_SRC_CLEAR: &str = "rm -rf ~/.cargo/registry/src";

const PROBES: &[Probe] = &[
    Probe {
        manager: "npm",
        kind: "cache",
        query: Some(("npm", &["config", "get", "cache"])),
        clear_command: "npm cache clean --force",
        note: None,
    },
    Probe {
        manager: "pnpm",
        kind: "store",
        query: Some(("pnpm", &["store", "path"])),
        clear_command: "pnpm store prune",
        note: Some(
            "hardlinked into every node_modules on the machine; emptying it is what makes \
             the next pnpm install a download",
        ),
    },
    Probe {
        manager: "yarn",
        kind: "cache",
        query: Some(("yarn", &["cache", "dir"])),
        clear_command: "yarn cache clean",
        note: None,
    },
    Probe {
        manager: "bun",
        kind: "cache",
        query: Some(("bun", &["pm", "cache"])),
        clear_command: "bun pm cache rm",
        note: None,
    },
    Probe {
        manager: "uv",
        kind: "cache",
        query: Some(("uv", &["cache", "dir"])),
        // `prune` drops what nothing can use again and keeps the rest; `uv cache clean`
        // is the sledgehammer, and is not what most people mean by "clear the cache".
        clear_command: "uv cache prune",
        note: None,
    },
    Probe {
        manager: "pip",
        kind: "cache",
        query: Some(("pip", &["cache", "dir"])),
        clear_command: "pip cache purge",
        note: None,
    },
    Probe {
        manager: "cargo",
        kind: "registry cache",
        query: None,
        clear_command: CARGO_CACHE_CLEAR,
        note: Some("the downloaded .crate archives; clearing them means downloading again"),
    },
    Probe {
        manager: "cargo",
        kind: "registry sources",
        query: None,
        clear_command: CARGO_SRC_CLEAR,
        note: Some("unpacked copies of the archives above; cargo re-extracts these offline"),
    },
    Probe {
        manager: "go",
        kind: "module cache",
        query: Some(("go", &["env", "GOMODCACHE"])),
        clear_command: "go clean -modcache",
        note: None,
    },
    Probe {
        manager: "go",
        kind: "build cache",
        query: Some(("go", &["env", "GOCACHE"])),
        clear_command: "go clean -cache",
        note: Some("compiled build artifacts; clearing them means the next build is a cold one"),
    },
];

/// Run the `caches` command.
pub fn run(json_output: bool) -> Result<()> {
    let reports = collect(!json_output);

    if json_output {
        return json::emit(&json::caches_document(&reports));
    }

    print_report(&reports);
    Ok(())
}

/// Find and size every cache on this machine, largest first.
fn collect(spinner: bool) -> Vec<CacheReport> {
    let pb = spinner.then(|| output::create_spinner("Measuring package manager caches..."));
    let from = query_dir();

    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut reports = Vec::new();

    for probe in PROBES {
        let Some(path) = locate(probe, &from) else {
            continue;
        };
        // Canonical, because two probes can land on the same directory — `GOCACHE` and
        // `GOMODCACHE` are both under `~/.cache` on Linux, and a machine can be
        // configured to share them. Counting one twice would inflate the total, which is
        // the one number this command exists to get right. It also settles the spelling:
        // a manager answers in whatever case and separators it likes, and two rows
        // disagreeing about how to write `C:\Users` reads like a bug.
        let path = path.canonicalize().unwrap_or(path);
        if !seen.insert(path.clone()) {
            continue;
        }
        reports.push(CacheReport {
            manager: probe.manager,
            kind: probe.kind,
            bytes: adapters::dir_size(&path),
            path,
            clear_command: probe.clear_command,
            note: probe.note,
        });
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    reports.sort_by_key(|r| std::cmp::Reverse(r.bytes));
    reports
}

/// Where to run the "where is your cache?" queries from.
///
/// The home directory, not the current one. A project's `.npmrc` or `.cargo/config.toml`
/// can move the cache for that project alone, and answering with it would report a
/// directory that is not the machine's actual cache. Falling back to the current
/// directory is only for the case where there is no home directory at all.
fn query_dir() -> PathBuf {
    dirs::home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve one probe to a directory that exists, or nothing.
fn locate(probe: &Probe, from: &Path) -> Option<PathBuf> {
    if let Some((program, args)) = probe.query {
        if adapters::binary_available(program) {
            let answered = adapters::capture_command_with_timeout(
                program,
                args,
                from,
                std::time::Duration::from_secs(constants::CACHE_QUERY_TIMEOUT_SECS),
            )
            .ok()
            .and_then(|raw| path_from_output(&raw))
            .filter(|p| p.is_dir());
            if answered.is_some() {
                return answered;
            }
        }
    }

    // Either the manager is not installed, or it is and its cache has never been
    // populated. The conventional location is still worth checking: an uninstalled
    // manager leaves its cache behind, and that is exactly the multi-gigabyte directory
    // nobody remembers.
    fallbacks(probe.manager, probe.kind)
        .into_iter()
        .find(|p| p.is_dir())
}

/// Read a path out of a manager's answer.
///
/// The last non-empty line, because some managers print a notice first, and quotes are
/// stripped because `go env` quotes paths containing spaces on Windows.
fn path_from_output(raw: &str) -> Option<PathBuf> {
    let line = raw.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let line = line.trim_matches('"');
    // npm answers `undefined` for a config key it does not have, and a manager that
    // errored can print anything at all. A relative path is never a machine-wide cache.
    if line.is_empty() || line == "undefined" || !Path::new(line).is_absolute() {
        return None;
    }
    Some(PathBuf::from(line))
}

/// Conventional locations for a cache, most likely first.
fn fallbacks(manager: &str, kind: &str) -> Vec<PathBuf> {
    let home = dirs::home_dir();
    let local = dirs::data_local_dir();
    let cache = dirs::cache_dir();
    // `rel` is split rather than joined whole so a Windows path never comes out as
    // `C:\Users\dev\go\pkg/mod`. `Path::join` accepts the forward slashes, it just keeps
    // them, and a report that spells the same drive two ways reads like a bug.
    let under = |base: &Option<PathBuf>, rel: &str| {
        base.as_ref()
            .map(|b| rel.split('/').fold(b.clone(), |p, seg| p.join(seg)))
    };

    let candidates = match (manager, kind) {
        // `npm config get cache` answers `~/.npm` on Unix and `%LocalAppData%\npm-cache`
        // on Windows; the payload lives in `_cacache` underneath either one.
        ("npm", _) => vec![under(&local, "npm-cache"), under(&home, ".npm")],
        ("pnpm", _) => vec![
            under(&local, "pnpm/store"),
            under(&home, ".local/share/pnpm/store"),
            under(&home, "Library/pnpm/store"),
            under(&home, ".pnpm-store"),
        ],
        ("yarn", _) => vec![
            under(&home, ".yarn/berry/cache"),
            under(&local, "Yarn/Cache"),
            under(&cache, "yarn"),
        ],
        ("bun", _) => vec![under(&home, ".bun/install/cache")],
        ("uv", _) => vec![under(&cache, "uv"), under(&local, "uv/cache")],
        ("pip", _) => vec![under(&cache, "pip"), under(&local, "pip/Cache")],
        ("cargo", "registry cache") => vec![Some(cargo_home().join("registry").join("cache"))],
        ("cargo", _) => vec![Some(cargo_home().join("registry").join("src"))],
        ("go", "module cache") => vec![
            std::env::var_os("GOMODCACHE").map(PathBuf::from),
            std::env::var_os("GOPATH").map(|p| PathBuf::from(p).join("pkg").join("mod")),
            under(&home, "go/pkg/mod"),
        ],
        ("go", _) => vec![
            std::env::var_os("GOCACHE").map(PathBuf::from),
            under(&cache, "go-build"),
            under(&local, "go-build"),
        ],
        _ => vec![],
    };

    candidates.into_iter().flatten().collect()
}

/// `CARGO_HOME`, or the default cargo puts it in.
fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
}

fn print_report(reports: &[CacheReport]) {
    output::print_header("Package manager caches");

    if reports.is_empty() {
        println!();
        output::print_info("No package manager caches found on this machine.");
        return;
    }

    println!();
    for r in reports {
        let label = format!("{} {}", r.manager, r.kind);
        println!(
            "  {:<22} {:>10}  {}",
            label,
            output::format_bytes(r.bytes),
            output::clean_path(&r.path)
        );
        println!("  {:<22} {:>10}  clear: {}", "", "", r.clear_command);
        if let Some(note) = r.note {
            println!("  {:<22} {:>10}  {}", "", "", note);
        }
        println!();
    }

    let total: u64 = reports.iter().map(|r| r.bytes).sum();
    println!(
        "  {:<22} {:>10}  across {} {}",
        "Total",
        output::format_bytes(total),
        reports.len(),
        output::plural(reports.len(), "cache", "caches")
    );

    println!();
    output::print_info(
        "Nothing above was deleted, and dev-prune never deletes any of it. A cache is \
         shared by every project on the machine, so no single repository's lockfile can \
         prove it is recoverable — and it is what makes `devp restore` fast. Run a clear \
         command yourself when you want the space more than the speed.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_probe_can_be_found_without_its_manager_installed() {
        // A probe with no query and no fallbacks is a row that can never appear, which
        // is a silent hole in the report rather than a test failure anywhere else.
        for probe in PROBES {
            assert!(
                !fallbacks(probe.manager, probe.kind).is_empty(),
                "{} {} has no conventional location",
                probe.manager,
                probe.kind
            );
        }
    }

    #[test]
    fn every_probe_names_the_command_that_clears_it() {
        for probe in PROBES {
            assert!(
                !probe.clear_command.trim().is_empty(),
                "{} {} reports a size with no way to act on it",
                probe.manager,
                probe.kind
            );
        }
    }

    #[test]
    fn no_two_probes_describe_the_same_cache() {
        let mut keys: Vec<(&str, &str)> = PROBES.iter().map(|p| (p.manager, p.kind)).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two probes share a manager and kind");
    }

    #[test]
    fn a_managers_answer_is_read_off_the_last_line() {
        // npm prints notices before the value it was asked for.
        let raw = if cfg!(windows) {
            "npm warn config global deprecated\nC:\\Users\\dev\\AppData\\Local\\npm-cache\n"
        } else {
            "npm warn config global deprecated\n/home/dev/.npm\n"
        };
        assert!(path_from_output(raw).is_some());
    }

    #[test]
    fn quoted_paths_lose_their_quotes() {
        let raw = if cfg!(windows) {
            "\"C:\\Program Files\\go\\pkg\\mod\"\n"
        } else {
            "\"/opt/go path/pkg/mod\"\n"
        };
        let path = path_from_output(raw).expect("a quoted path is still a path");
        assert!(!path.to_string_lossy().contains('"'));
    }

    #[test]
    fn a_non_answer_is_not_mistaken_for_a_path() {
        // Each of these has been an actual answer from a package manager at some point,
        // and treating any of them as a directory would size the wrong thing.
        for raw in [
            "",
            "\n \n",
            "undefined\n",
            "not a command\n",
            "./relative\n",
        ] {
            assert!(
                path_from_output(raw).is_none(),
                "{raw:?} was accepted as a cache path"
            );
        }
    }

    #[test]
    fn the_cargo_rows_point_inside_the_registry() {
        // Both cargo rows are fallback-only — cargo has no "where is your cache" query —
        // so a wrong path here is a row that silently reports 0 B forever.
        for kind in ["registry cache", "registry sources"] {
            let path = fallbacks("cargo", kind).remove(0);
            assert!(
                path.starts_with(cargo_home().join("registry")),
                "{kind} resolved outside the cargo registry: {}",
                path.display()
            );
        }
    }

    #[test]
    fn the_report_is_ordered_by_what_is_worth_clearing() {
        let mut reports = [
            CacheReport {
                manager: "npm",
                kind: "cache",
                path: PathBuf::from("/a"),
                bytes: 10,
                clear_command: "x",
                note: None,
            },
            CacheReport {
                manager: "go",
                kind: "module cache",
                path: PathBuf::from("/b"),
                bytes: 4_000,
                clear_command: "y",
                note: None,
            },
        ];
        reports.sort_by_key(|r| std::cmp::Reverse(r.bytes));
        assert_eq!(reports[0].manager, "go");
    }
}
