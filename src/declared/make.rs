// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! The rebuild check for `make`.
//!
//! Every package manager's rebuild check lives beside its adapter in `crate::adapters`.
//! `make` has no adapter: it is a build tool, not a package manager, and finding a
//! `Makefile` proves nothing about what a lockfile could rebuild. Its check lives here
//! instead, next to the dispatch that calls it.

use std::fs;
use std::path::Path;

use super::{Gap, RebuildCheck, relative_parts, split_relative};

/// The check for `make` and its common spellings.
pub(crate) struct MakeTargets;

impl RebuildCheck for MakeTargets {
    fn tools(&self) -> &'static [&'static str] {
        &["make", "gmake", "mingw32-make"]
    }

    fn gap(&self, repo_path: &Path, _tool: &str, args: &[&str]) -> Option<Gap> {
        make_target_gap(repo_path, args)
    }
}

/// A `make` target the makefile does not define.
fn make_target_gap(repo_path: &Path, args: &[&str]) -> Option<Gap> {
    let mut dir = None;
    let mut file = None;
    let mut target = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        if arg == "--" {
            return None;
        }
        if let Some(value) = arg.strip_prefix("--directory=") {
            dir = Some(value.to_string());
        } else if let Some(value) = arg
            .strip_prefix("--file=")
            .or_else(|| arg.strip_prefix("--makefile="))
        {
            file = Some(value.to_string());
        } else if arg == "-C" || arg == "--directory" {
            dir = Some((*args.get(i + 1)?).to_string());
            i += 1;
        } else if arg == "-f" || arg == "--file" || arg == "--makefile" {
            file = Some((*args.get(i + 1)?).to_string());
            i += 1;
        } else if let Some(value) = arg.strip_prefix("-C").filter(|v| !v.is_empty()) {
            dir = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("-f").filter(|v| !v.is_empty()) {
            file = Some(value.to_string());
        } else if arg.starts_with('-') {
            return None;
        } else if !arg.contains('=') {
            // `VAR=value` is an override, not a goal; the first word that is not one is.
            target = Some(arg.to_string());
            break;
        }
        i += 1;
    }
    // Bare `make` builds the default goal — whichever rule comes first, which is not a
    // name to look up.
    let target = target?;
    let base = relative_parts(dir.as_deref())?;
    let candidates: Vec<Vec<String>> = match &file {
        Some(name) => vec![split_relative(name).ok()?],
        None => ["Makefile", "makefile", "GNUmakefile"]
            .iter()
            .map(|name| vec![(*name).to_string()])
            .collect(),
    };
    let (parts, content) = candidates.into_iter().find_map(|name| {
        let mut parts = base.clone();
        parts.extend(name);
        let full = parts
            .iter()
            .fold(repo_path.to_path_buf(), |acc, part| acc.join(part));
        fs::read_to_string(full).ok().map(|text| (parts, text))
    })?;
    if make_targets(&content)?.contains(&target) {
        return None;
    }
    let manifest = parts.join("/");
    Some(Gap {
        what: format!("`{manifest}` defines no `{target}` target"),
        fix: format!("Add a `{target}` target to `{manifest}`, or fix the command."),
    })
}

/// Every target name a makefile states outright, or `None` when some are not in the text.
///
/// Line-based, the way the poetry adapter reads `pyproject.toml`: this crate has no
/// makefile parser and the question is a membership test. An `include`, a pattern rule
/// or a target built out of a variable each mean the file names targets this cannot see,
/// so those give up entirely rather than answer from a partial list.
fn make_targets(content: &str) -> Option<Vec<String>> {
    let mut targets = Vec::new();
    for raw in content.lines() {
        if raw.starts_with('\t') {
            continue;
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("include")
            || line.starts_with("-include")
            || line.starts_with("sinclude")
        {
            return None;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let rest = &line[colon..];
        if rest.starts_with(":=") || rest.starts_with("::=") {
            continue;
        }
        let head = &line[..colon];
        if head.contains('=') {
            continue;
        }
        if head.contains('%') || head.contains("$(") || head.contains("${") {
            return None;
        }
        targets.extend(head.split_whitespace().map(str::to_string));
        // `.PHONY: build test` names targets on the right of the colon, and a phony
        // target with no recipe of its own is still one `make` accepts.
        if head.split_whitespace().any(|word| word == ".PHONY") {
            targets.extend(rest[1..].split_whitespace().map(str::to_string));
        }
    }
    if targets.is_empty() {
        None
    } else {
        Some(targets)
    }
}
