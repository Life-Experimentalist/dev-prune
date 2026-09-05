// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! Directories a project declares prunable, and the checks that let dev-prune act on
//! one.
//!
//! Every adapter in this tool earns the right to delete a directory the same way: it
//! finds a lockfile, verifies the lockfile can rebuild what is about to go, and only
//! then deletes. A declaration is that same bargain written by hand, for the tree no
//! adapter can recognise — a generated fixture set, a vendored toolchain, a scratch
//! cache with a `make` target behind it.
//!
//! What makes that safe is *not* trust. `project.devprune.json` is committed, so a
//! cloned repository can declare anything it likes, and `devp run` may be running from
//! a scheduler with nobody watching. So a declaration is treated as a claim to be
//! checked rather than an instruction to be followed: it names a directory, and this
//! module proves the directory is inside the repository, holds nothing Git is tracking,
//! has a rebuild command whose tool this machine actually has, and — where a manifest
//! already in the tree can answer — that the command's target is one that manifest
//! defines. A claim that fails any of those is reported, in full, and nothing is
//! deleted.
//!
//! None of those checks runs the rebuild command, or any part of it. Every one is a
//! read.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{DeclaredDir, Prunable};
use crate::scanner::git;

/// A declared directory that passed every check, ready to be treated as bloat.
#[derive(Debug, Clone)]
pub struct Target {
    /// Repository-relative, `/`-separated — the same label shape adapters report.
    pub label: String,
    /// Where it actually is on this machine.
    pub path: PathBuf,
    /// The command the project says rebuilds it. Shown to the user, never run.
    pub rebuild: String,
    /// The project's own reason, if it gave one.
    pub why: Option<String>,
    /// Bytes deleting it would give back.
    pub size_bytes: u64,
}

/// What became of one entry in `prunable.directories`.
#[derive(Debug, Clone)]
pub enum Declaration {
    /// Checked out, and safe to delete on the usual terms.
    Prunable(Box<Target>),
    /// Something about the claim did not hold. The reason is the user-facing sentence.
    Refused { label: String, reason: String },
}

/// Commands that are not programs on disk anywhere.
///
/// `"rebuild": "echo not needed"` is the deliberate escape hatch for a directory that
/// genuinely needs nothing to come back — a scratch area some tool refills on demand.
/// It has to keep working, and on Windows there is no `echo.exe`: `echo` is a shell
/// builtin in both `cmd` and PowerShell, so a plain `PATH` search finds nothing and the
/// documented answer would be refused on the one platform most of this project's users
/// are on.
const SHELL_BUILTINS: &[&str] = &["echo", "true", ":"];

/// Builtins that mark the command as shell-shaped rather than a tool invocation.
///
/// `cd docs && npm run build` runs fine pasted into a shell, but its first word proves
/// nothing about what this machine can rebuild — and macOS ships a `/usr/bin/cd` shim,
/// so a plain `PATH` search would accept there what Windows refuses. Refused
/// everywhere, with the rewrite in the message, so a committed declaration means one
/// thing on every clone.
const SHELL_ONLY: &[&str] = &["cd", "pushd", "source", ".", "export", "set"];

/// Check every declaration in a repository, in the order the file lists them.
///
/// Directories that simply are not there are dropped rather than reported: a declared
/// directory that does not exist is a declaration that has already been honoured, and
/// a repository that declares four caches and currently has one should not print three
/// lines about the other three on every single pass.
///
/// So is anything `prunable.exclude` names, and for the same reason. Whoever wrote the
/// exclusion has already answered every question this module would ask about that
/// directory — including whether to keep saying that it cannot be honoured.
pub fn resolve(repo_path: &Path, declared: &Prunable) -> Vec<Declaration> {
    let excluded: Vec<String> = declared.exclude.iter().map(|raw| key(raw)).collect();
    let mut out = Vec::new();
    for entry in &declared.directories {
        if excluded.contains(&key(&entry.path)) {
            continue;
        }
        match check(repo_path, entry) {
            Ok(Some(target)) => out.push(Declaration::Prunable(Box::new(target))),
            Ok(None) => {}
            Err(reason) => out.push(Declaration::Refused {
                label: entry.path.clone(),
                reason,
            }),
        }
    }
    out
}

/// The comparable spelling of a declared or excluded path.
///
/// Both sides go through the same splitter, so `dist`, `dist/`, `./dist` and `dist\`
/// are one path: an exclusion that missed on a trailing slash would delete the exact
/// directory it was written to keep. A path the splitter rejects has no normal form, so
/// its own text is all it can match on — which costs nothing, because a declaration of
/// that shape is refused rather than deleted anyway.
pub(crate) fn key(raw: &str) -> String {
    split_relative(raw).map_or_else(|_| raw.trim().to_string(), |parts| parts.join("/"))
}

/// One declaration: `Ok(Some)` to delete, `Ok(None)` for absent, `Err` for refused.
fn check(repo_path: &Path, entry: &DeclaredDir) -> Result<Option<Target>, String> {
    let parts = split_relative(&entry.path)?;
    let label = parts.join("/");
    let path = parts.iter().fold(repo_path.to_path_buf(), |p, s| p.join(s));

    if !path.exists() {
        return Ok(None);
    }
    if !path.is_dir() {
        return Err(format!(
            "`{label}` is declared prunable but is a file, not a directory — \
             dev-prune only deletes whole directories. Left alone."
        ));
    }

    // Guards against a symlinked *ancestor*, which is the one way a path with no `..`
    // in it can still land outside the repository. The leaf being a symlink is caught
    // later, by the same check every adapter's directories go through.
    let (Ok(real), Ok(root)) = (path.canonicalize(), repo_path.canonicalize()) else {
        return Err(format!(
            "`{label}` is declared prunable but could not be resolved on this machine — \
             refusing to delete a path dev-prune cannot pin down."
        ));
    };
    if !real.starts_with(&root) {
        return Err(format!(
            "`{label}` is declared prunable but resolves to `{}`, outside the \
             repository. Left alone.",
            crate::output::clean_path(&real)
        ));
    }

    if let Some(tracked) = first_tracked_file(repo_path, &label)? {
        return Err(format!(
            "`{label}` is declared prunable but Git is tracking `{tracked}` inside it — \
             refusing. A lockfile cannot rebuild a file that is in the repository \
             itself. Remove the declaration, or stop tracking those files."
        ));
    }

    let rebuild = entry.rebuild.trim();
    if rebuild.is_empty() {
        return Err(format!(
            "`{label}` is declared prunable with an empty `rebuild` command — refusing. \
             Say what puts it back, or use `\"rebuild\": \"echo not needed\"` if nothing \
             does."
        ));
    }
    let tool = first_word(rebuild);
    if SHELL_ONLY.contains(&tool) {
        return Err(format!(
            "`{label}` is declared prunable, rebuilt by `{rebuild}`, but `{tool}` is a \
             shell builtin, not a program this machine can be checked for — put the \
             tool first, e.g. `npm --prefix docs run build` rather than \
             `cd docs && npm run build`."
        ));
    }
    if !SHELL_BUILTINS.contains(&tool) && !on_path(tool) {
        return Err(format!(
            "`{label}` is declared prunable, rebuilt by `{rebuild}`, but `{tool}` is not \
             on this machine — refusing to delete something this machine cannot put \
             back. Install `{tool}` first."
        ));
    }
    if let Some(gap) = rebuild_gap(repo_path, rebuild) {
        return Err(format!(
            "`{label}` is declared prunable, rebuilt by `{rebuild}`, but {} — refusing to \
             delete something that command cannot put back. {}",
            gap.what, gap.fix
        ));
    }

    Ok(Some(Target {
        size_bytes: crate::adapters::dir_size(&path),
        label,
        path,
        rebuild: rebuild.to_string(),
        why: entry.why.clone(),
    }))
}

/// Split a declared path into components, refusing anything that could point outward.
///
/// Deliberately not `Path::components`: this string is read on every platform from a
/// file written on one of them, and `Path` disagrees with itself across platforms about
/// what `C:\x` and `a\b` even are. Splitting on both separators by hand means a
/// declaration that is refused on Windows is refused on Linux too, which is the whole
/// value of the file being committed.
fn split_relative(raw: &str) -> Result<Vec<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("An entry in `prunable.directories` has an empty `path`.".to_string());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(format!(
            "`{trimmed}` is declared prunable but is an absolute path — declarations are \
             relative to the repository root. Left alone."
        ));
    }
    let mut parts = Vec::new();
    for part in trimmed.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(format!(
                "`{trimmed}` is declared prunable but climbs out of the repository with \
                 `..` — refusing. Left alone."
            ));
        }
        if part.contains(':') {
            return Err(format!(
                "`{trimmed}` is declared prunable but names a drive or stream — \
                 declarations are relative to the repository root. Left alone."
            ));
        }
        if part.eq_ignore_ascii_case(".git") {
            return Err(format!(
                "`{trimmed}` is declared prunable but is inside `.git` — the one \
                 directory dev-prune never crosses. Left alone."
            ));
        }
        parts.push(part.to_string());
    }
    if parts.is_empty() {
        return Err(format!(
            "`{trimmed}` is declared prunable but resolves to the repository root \
             itself — refusing. Left alone."
        ));
    }
    Ok(parts)
}

/// The first Git-tracked file inside `label`, if there is one.
///
/// The check that makes a *committed* declaration safe to honour. dev-prune's promise
/// is that everything it deletes can be rebuilt from something that stays behind, and
/// the one thing no lockfile can rebuild is the repository's own content. A hostile —
/// or merely careless — `project.devprune.json` declaring `src` therefore gets refused
/// on the same grounds as everything else, without dev-prune having to guess intent.
///
/// A `git` that cannot answer is an error rather than a shrug: "I could not check" is
/// not "there is nothing there".
fn first_tracked_file(repo_path: &Path, label: &str) -> Result<Option<String>, String> {
    let output = git::git_in(repo_path)
        .args(["ls-files", "--", label])
        .output()
        .map_err(|e| {
            format!(
                "`{label}` is declared prunable, but `git ls-files` could not run ({e}) — \
                 refusing to delete without knowing whether it holds tracked files."
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "`{label}` is declared prunable, but `git ls-files` failed — refusing to \
             delete without knowing whether it holds tracked files."
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_string))
}

/// The program a rebuild command starts with, unquoted.
fn first_word(command: &str) -> &str {
    command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(['"', '\''])
}

/// Is `program` something this machine could actually run?
///
/// Presence on `PATH`, not a `--version` probe. A rebuild command can start with
/// anything — `make`, `./scripts/gen.sh`, a project's own tool — and most of those have
/// no version flag, so probing would refuse commands that work perfectly well.
fn on_path(program: &str) -> bool {
    let named = Path::new(program);
    if named.components().count() > 1 {
        return named.is_file();
    }
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    // `CreateProcess` only ever appends `.exe`, but a shell resolves the rest, and a
    // rebuild command is run by a person in a shell.
    let exts: &[&str] = if cfg!(windows) {
        &["", "exe", "cmd", "bat", "com", "ps1"]
    } else {
        &[""]
    };
    std::env::split_paths(&path_var).any(|dir| {
        exts.iter().any(|ext| {
            if ext.is_empty() {
                dir.join(program).is_file()
            } else {
                dir.join(format!("{program}.{ext}")).is_file()
            }
        })
    })
}

/// Something a rebuild command names that the manifest it would read does not define.
struct Gap {
    /// What was looked for and where, as a clause: "`package.json` defines no `build`
    /// script".
    what: String,
    /// What to do about it.
    fix: String,
}

/// Characters that make a rebuild command a shell script rather than one invocation.
///
/// `:` is deliberately absent — `npm run build:prod` is an ordinary script name.
const SHELL_METACHARACTERS: &[char] = &[
    '&', '|', ';', '$', '`', '>', '<', '(', ')', '*', '?', '%', '{', '}', '#',
];

/// Cargo's own subcommands, which never need a plugin behind them.
///
/// Only used to *stop* asking: a name on this list is accepted immediately. A name that
/// is not on it goes on to the `PATH` and alias checks rather than being refused, so a
/// list that falls behind cargo makes this slower, never wrong.
const CARGO_BUILTINS: &[&str] = &[
    "add",
    "b",
    "bench",
    "build",
    "c",
    "check",
    "clean",
    "clippy",
    "config",
    "d",
    "doc",
    "fetch",
    "fix",
    "fmt",
    "generate-lockfile",
    "help",
    "info",
    "init",
    "install",
    "locate-project",
    "login",
    "logout",
    "metadata",
    "miri",
    "new",
    "owner",
    "package",
    "pkgid",
    "publish",
    "r",
    "read-manifest",
    "remove",
    "report",
    "run",
    "rustc",
    "rustdoc",
    "search",
    "t",
    "test",
    "tree",
    "uninstall",
    "unpublish",
    "update",
    "vendor",
    "verify-project",
    "version",
    "yank",
];

/// Subcommands of `pnpm` and `yarn`, which both also run a script from a bare word.
///
/// `pnpm build` runs the `build` script, but `pnpm install` does not — so the bare form
/// can only be resolved against a list of the tool's own verbs. Deliberately generous,
/// and shared between the two tools: a word wrongly on this list is a script this
/// module declines to check, which is the failure this file prefers.
const NODE_SUBCOMMANDS: &[&str] = &[
    "add",
    "audit",
    "bin",
    "cache",
    "config",
    "create",
    "dedupe",
    "deploy",
    "dlx",
    "doctor",
    "env",
    "exec",
    "fetch",
    "get",
    "global",
    "help",
    "i",
    "import",
    "info",
    "init",
    "install",
    "licenses",
    "link",
    "list",
    "login",
    "logout",
    "ls",
    "node",
    "outdated",
    "pack",
    "patch",
    "policies",
    "prune",
    "publish",
    "rebuild",
    "remove",
    "restart",
    "rm",
    "root",
    "server",
    "set",
    "setup",
    "start",
    "stop",
    "store",
    "test",
    "un",
    "uninstall",
    "unlink",
    "up",
    "update",
    "upgrade",
    "version",
    "whoami",
    "why",
    "workspace",
    "workspaces",
];

/// What the rebuild command's target is missing, when a manifest in the tree can say.
///
/// The `PATH` check above proves the *tool* is installed. It proves nothing about what
/// the tool was asked to do: `"rebuild": "npm run build"` passes it on any machine with
/// node on it, including one whose `package.json` has no `build` script at all. The user
/// is then told the directory is recoverable, dev-prune deletes it, and the command that
/// was supposed to put it back fails — silent data loss, from a check that stopped one
/// word too early. This closes that, by reading the manifest the tool itself would read.
///
/// **Anything this cannot answer returns `None`, which allows the prune.** An
/// unrecognised tool, a shell pipeline, a variable in place of the target, a flag whose
/// meaning would have to be guessed, a manifest that is absent or unparseable — every
/// one of those falls through to the `PATH` check alone. That asymmetry is deliberate: a
/// false refusal blocks a prune that was safe, in a committed file the user cannot
/// easily debug, and is a worse bug than the gap being closed here. Only a manifest that
/// positively does not define the named target produces a refusal.
///
/// Nothing in here runs the rebuild command, or any part of it. Every check is a read.
fn rebuild_gap(repo_path: &Path, rebuild: &str) -> Option<Gap> {
    let words = command_words(rebuild)?;
    let (tool, rest) = words.split_first()?;
    let args: Vec<&str> = rest.iter().map(String::as_str).collect();
    match tool_name(tool) {
        node @ ("npm" | "pnpm" | "yarn") => node_script_gap(repo_path, node, &args),
        "make" | "gmake" | "mingw32-make" => make_target_gap(repo_path, &args),
        "uv" => uv_script_gap(repo_path, &args),
        "cargo" => cargo_subcommand_gap(repo_path, &args),
        _ => None,
    }
}

/// The words of a rebuild command, or `None` when it is not a single invocation.
///
/// A shell metacharacter anywhere means the string is a script — a pipeline, a
/// substitution, a glob — and the words around it do not mean what they look like.
fn command_words(rebuild: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    for raw in rebuild.split_whitespace() {
        if raw.contains(SHELL_METACHARACTERS) {
            return None;
        }
        words.push(raw.trim_matches(['"', '\'']).to_string());
    }
    Some(words)
}

/// A tool's name with the executable extension a Windows declaration might carry.
fn tool_name(word: &str) -> &str {
    if word.contains(['/', '\\']) {
        return word;
    }
    for ext in [".cmd", ".exe", ".bat", ".ps1"] {
        if let Some(stem) = word.strip_suffix(ext) {
            return stem;
        }
    }
    word
}

/// A `--prefix`-style directory as repository-relative components.
///
/// Run through the same splitter declarations are. This only ever reads a manifest, but
/// it should only ever read one out of the tree it was asked about.
fn relative_parts(dir: Option<&str>) -> Option<Vec<String>> {
    match dir.map(str::trim) {
        None | Some("") | Some(".") => Some(Vec::new()),
        Some(raw) => split_relative(raw).ok(),
    }
}

/// Where a manifest sits on disk, given repository-relative components.
fn path_of(repo_path: &Path, parts: &[String], file: &str) -> PathBuf {
    parts
        .iter()
        .fold(repo_path.to_path_buf(), |acc, part| acc.join(part))
        .join(file)
}

/// How that manifest is named back to the user: repository-relative, `/`-separated.
fn label_of(parts: &[String], file: &str) -> String {
    if parts.is_empty() {
        file.to_string()
    } else {
        format!("{}/{file}", parts.join("/"))
    }
}

/// A `package.json` script the command names and the file does not define.
fn node_script_gap(repo_path: &Path, tool: &str, args: &[&str]) -> Option<Gap> {
    let (script, prefix) = node_script_and_prefix(tool, args)?;
    let parts = relative_parts(prefix.as_deref())?;
    let manifest = label_of(&parts, "package.json");
    let content = fs::read_to_string(path_of(repo_path, &parts, "package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let defined = match json.get("scripts") {
        // No `scripts` table at all: there is nothing `run` could resolve against.
        None => false,
        // Present but not a table — a shape this cannot read.
        Some(value) => value.as_object()?.contains_key(script.as_str()),
    };
    if defined {
        return None;
    }
    Some(Gap {
        what: format!("`{manifest}` defines no `{script}` script"),
        fix: format!("Add a `{script}` script to `{manifest}`, or fix the command."),
    })
}

/// The script an `npm`/`pnpm`/`yarn` command runs, and the directory it runs it in.
///
/// The prefix flags matter because the refusal one check above actively recommends
/// `npm --prefix docs run build`: following that advice must not then land on the wrong
/// `package.json`. Resolution stops at the named directory rather than walking upward
/// the way npm does — a parent manifest could be outside the repository, and "somewhere
/// above here" is not an answer this module is willing to refuse on.
fn node_script_and_prefix(tool: &str, args: &[&str]) -> Option<(String, Option<String>)> {
    let mut prefix = None;
    let mut saw_run = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        if arg == "--" {
            return None;
        }
        if let Some(value) = arg
            .strip_prefix("--prefix=")
            .or_else(|| arg.strip_prefix("--dir="))
            .or_else(|| arg.strip_prefix("--cwd="))
        {
            prefix = Some(value.to_string());
        } else if matches!(arg, "--prefix" | "--dir" | "-C" | "--cwd") {
            prefix = Some((*args.get(i + 1)?).to_string());
            i += 1;
        } else if arg == "run" || arg == "run-script" {
            saw_run = true;
        } else if arg.starts_with('-') {
            // A flag this does not model. Whatever follows it might be its argument, and
            // reading that as the script name is exactly the guess to avoid.
            return None;
        } else if saw_run {
            return Some((arg.to_string(), prefix));
        } else if tool == "npm" || NODE_SUBCOMMANDS.contains(&arg) {
            // npm has no bare-script shorthand, and the pnpm/yarn one does not apply to
            // the tool's own verbs.
            return None;
        } else {
            return Some((arg.to_string(), prefix));
        }
        i += 1;
    }
    None
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

/// What `pyproject.toml` had to say about the name `uv run` was given.
struct PyprojectLookup {
    /// `[project.scripts]` — the one table uv resolves entry points from.
    defined: bool,
    /// The name appears as a requirement, so a dependency's console script provides it.
    from_dependency: bool,
    /// `[tool.uv.scripts]` names it. That table is not one uv reads.
    in_tool_uv_scripts: bool,
}

/// A `uv run` target that `pyproject.toml` does not provide.
fn uv_script_gap(repo_path: &Path, args: &[&str]) -> Option<Gap> {
    let mut dir = None;
    let mut saw_run = false;
    let mut script = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        if arg == "--" {
            return None;
        }
        if let Some(value) = arg
            .strip_prefix("--directory=")
            .or_else(|| arg.strip_prefix("--project="))
        {
            dir = Some(value.to_string());
        } else if arg == "--directory" || arg == "--project" {
            dir = Some((*args.get(i + 1)?).to_string());
            i += 1;
        } else if arg == "run" {
            saw_run = true;
        } else if arg.starts_with('-') {
            return None;
        } else if saw_run {
            script = Some(arg.to_string());
            break;
        } else {
            // `uv sync`, `uv pip install …` — not a script invocation at all.
            return None;
        }
        i += 1;
    }
    let script = script?;
    // `uv run ./tools/gen.py` runs a file. Whether that file is there is not a question
    // `pyproject.toml` answers.
    if script.contains(['/', '\\']) || script.ends_with(".py") {
        return None;
    }
    // `uv run` also falls back to anything already on `PATH`.
    if on_path(&script) {
        return None;
    }
    let parts = relative_parts(dir.as_deref())?;
    let manifest = label_of(&parts, "pyproject.toml");
    let content = fs::read_to_string(path_of(repo_path, &parts, "pyproject.toml")).ok()?;
    let found = pyproject_lookup(&content, &script)?;
    if found.defined || found.from_dependency {
        return None;
    }
    // A dependency's console script is not named in `pyproject.toml` at all when the
    // requirement is only pinned in the lockfile.
    if lockfile_records(&path_of(repo_path, &parts, "uv.lock"), &script) {
        return None;
    }
    let fix = if found.in_tool_uv_scripts {
        format!(
            "`[tool.uv.scripts]` is not a table uv reads — it is silently ignored. Move \
             `{script}` to `[project.scripts]` in `{manifest}`, or fix the command."
        )
    } else {
        format!("Add `{script}` to `[project.scripts]` in `{manifest}`, or fix the command.")
    };
    Some(Gap {
        what: format!("`{manifest}` defines no `{script}` entry point"),
        fix,
    })
}

/// Where `pyproject.toml` does and does not mention one name.
///
/// Line-based for the same reason the poetry adapter's read of this file is: there is no
/// TOML dependency in this crate, and table headers only ever start a line. Any shape
/// the scan cannot see through returns `None`, which allows the prune.
fn pyproject_lookup(content: &str, wanted: &str) -> Option<PyprojectLookup> {
    let mut found = PyprojectLookup {
        defined: false,
        from_dependency: false,
        in_tool_uv_scripts: false,
    };
    let mut table = "";
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            table = line;
            continue;
        }
        // A dotted key puts entry points somewhere this scan does not look.
        if line.starts_with("project.scripts") {
            return None;
        }
        // Any quoted requirement anywhere in the file — `"pytest>=8"` in a dependency
        // array, wherever that array happens to be written. `uv run pytest` runs a
        // console script that a dependency installed, and no table here lists it.
        if quoted_strings(line).any(|value| same_name(requirement_head(value), wanted)) {
            found.from_dependency = true;
        }
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches(['"', '\'']);
        match table {
            // An inline `scripts = { regen = "…" }` is a shape this cannot read.
            "[project]" if key == "scripts" => return None,
            "[project.scripts]" | "[project.gui-scripts]" if same_name(key, wanted) => {
                found.defined = true;
            }
            "[tool.uv.scripts]" if same_name(key, wanted) => found.in_tool_uv_scripts = true,
            _ => {}
        }
    }
    Some(found)
}

/// The double-quoted runs of a line.
fn quoted_strings(line: &str) -> impl Iterator<Item = &str> {
    line.split('"').skip(1).step_by(2)
}

/// The distribution name at the front of a requirement string like `pytest>=8,<9`.
fn requirement_head(raw: &str) -> &str {
    raw.trim()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .next()
        .unwrap_or("")
}

/// Python treats `-` and `_` in a distribution or entry-point name as the same character.
fn same_name(a: &str, b: &str) -> bool {
    a.replace('_', "-")
        .eq_ignore_ascii_case(&b.replace('_', "-"))
}

/// Does a `uv.lock` record a package under this name?
fn lockfile_records(lockfile: &Path, wanted: &str) -> bool {
    let Ok(content) = fs::read_to_string(lockfile) else {
        return false;
    };
    content.lines().any(|raw| {
        raw.trim()
            .strip_prefix("name")
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .is_some_and(|value| same_name(value.trim().trim_matches('"'), wanted))
    })
}

/// A `cargo` subcommand nothing on this machine provides.
///
/// Deliberately not an attempt to enumerate cargo plugins — there is no list of those.
/// It only rules out the names that are definitively absent: not one of cargo's own, no
/// `cargo-<name>` program on `PATH`, and no `[alias]` table anywhere cargo reads one.
fn cargo_subcommand_gap(repo_path: &Path, args: &[&str]) -> Option<Gap> {
    let mut i = 0;
    // `cargo +nightly build` picks a toolchain before naming the subcommand.
    while args.get(i).is_some_and(|arg| arg.starts_with('+')) {
        i += 1;
    }
    let sub = *args.get(i)?;
    if sub.starts_with('-') || CARGO_BUILTINS.contains(&sub) {
        return None;
    }
    // Asking `PATH` the same question cargo itself asks when it meets a name it does not
    // know. An installed plugin is found here.
    if on_path(&format!("cargo-{sub}")) {
        return None;
    }
    if cargo_aliases_exist(repo_path) {
        return None;
    }
    Some(Gap {
        what: format!("`{sub}` is not a cargo subcommand and no `cargo-{sub}` is on this machine"),
        fix: format!("Install whatever provides `cargo {sub}`, or fix the command."),
    })
}

/// Whether any config cargo would read declares an `[alias]` table.
///
/// Its mere presence is enough to stop asking. An alias can name anything, and reading
/// one machine's table to decide whether a committed declaration is honoured is exactly
/// the kind of answer this module would rather not give.
fn cargo_aliases_exist(repo_path: &Path) -> bool {
    let mut candidates = vec![
        repo_path.join(".cargo").join("config.toml"),
        repo_path.join(".cargo").join("config"),
    ];
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|dir| dir.join(".cargo")));
    if let Some(home) = home {
        candidates.push(home.join("config.toml"));
        candidates.push(home.join("config"));
    }
    candidates.iter().any(|path| {
        fs::read_to_string(path)
            .map(|text| text.lines().any(|l| l.trim_start().starts_with("[alias")))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn declared(path: &str, rebuild: &str) -> DeclaredDir {
        DeclaredDir {
            path: path.to_string(),
            rebuild: rebuild.to_string(),
            why: None,
        }
    }

    /// One declaration and nothing excluded — the shape most of these tests want.
    fn one(entry: DeclaredDir) -> Prunable {
        Prunable {
            directories: vec![entry],
            exclude: Vec::new(),
        }
    }

    /// A repository with one commit, so `git ls-files` has an index to answer from.
    fn repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(path)
                .output()
                .unwrap();
        }
        tmp
    }

    fn refusal(repo_path: &Path, entry: DeclaredDir) -> String {
        match resolve(repo_path, &one(entry)).pop() {
            Some(Declaration::Refused { reason, .. }) => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_declaration_that_holds_up_is_prunable_with_its_reason_carried_along() {
        let tmp = repo();
        let path = tmp.path();
        fs::create_dir_all(path.join("build/fixtures")).unwrap();
        fs::write(path.join("build/fixtures/a.bin"), vec![0u8; 4096]).unwrap();

        let mut entry = declared("build/fixtures", "echo not needed");
        entry.why = Some("regenerated by the test suite".into());
        let Some(Declaration::Prunable(target)) = resolve(path, &one(entry)).pop() else {
            panic!("a declaration nothing is wrong with must be prunable");
        };
        assert_eq!(target.label, "build/fixtures");
        assert_eq!(target.why.as_deref(), Some("regenerated by the test suite"));
        assert!(target.size_bytes >= 4096);
    }

    #[test]
    fn the_documented_escape_hatch_works_on_every_platform() {
        // `echo` is a shell builtin, not a program, and on Windows there is no
        // `echo.exe` at all. The one rebuild command the docs hand people has to pass.
        let tmp = repo();
        fs::create_dir_all(tmp.path().join("scratch")).unwrap();
        assert!(matches!(
            resolve(tmp.path(), &one(declared("scratch", "echo not needed"))).pop(),
            Some(Declaration::Prunable(_))
        ));
    }

    #[test]
    fn a_declaration_covering_tracked_files_is_refused() {
        // The check that makes a committed file safe to honour: a repository that
        // declares its own source is refused without dev-prune having to guess why.
        let tmp = repo();
        let path = tmp.path();
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("src/main.rs"), "fn main() {}").unwrap();
        Command::new("git")
            .args(["add", "src/main.rs"])
            .current_dir(path)
            .output()
            .unwrap();

        let reason = refusal(path, declared("src", "echo not needed"));
        assert!(reason.contains("Git is tracking"), "{reason}");
        assert!(path.join("src/main.rs").exists());
    }

    #[test]
    fn a_declaration_whose_rebuild_tool_is_absent_is_refused() {
        let tmp = repo();
        fs::create_dir_all(tmp.path().join("vendor")).unwrap();
        let reason = refusal(
            tmp.path(),
            declared("vendor", "definitely-not-a-real-tool-xyz build"),
        );
        assert!(reason.contains("is not on this machine"), "{reason}");
    }

    #[test]
    fn a_rebuild_starting_with_cd_gets_the_rewrite_not_install_advice() {
        let tmp = repo();
        fs::create_dir_all(tmp.path().join("docs/out")).unwrap();
        let reason = refusal(tmp.path(), declared("docs/out", "cd docs && npm run build"));
        assert!(reason.contains("shell builtin"), "{reason}");
        assert!(reason.contains("--prefix"), "{reason}");
        assert!(!reason.contains("Install"), "{reason}");
    }

    #[test]
    fn an_empty_rebuild_is_refused_and_says_what_to_write_instead() {
        let tmp = repo();
        fs::create_dir_all(tmp.path().join("vendor")).unwrap();
        let reason = refusal(tmp.path(), declared("vendor", "   "));
        assert!(reason.contains("echo not needed"), "{reason}");
    }

    #[test]
    fn paths_that_could_point_outside_the_repository_never_get_that_far() {
        // Refused on their shape alone, before anything touches the disk — so the
        // answer is the same on Windows and Linux, which matters for a file that is
        // committed once and cloned everywhere.
        for (raw, expected) in [
            ("../secrets", "climbs out of the repository"),
            ("/etc", "absolute path"),
            ("C:/Windows", "names a drive"),
            (".git/objects", "inside `.git`"),
            (".", "the repository root itself"),
        ] {
            let err = split_relative(raw).unwrap_err();
            assert!(err.contains(expected), "{raw}: {err}");
        }
    }

    #[test]
    fn a_declared_directory_that_is_not_there_says_nothing_at_all() {
        // Otherwise a repository declaring four caches prints three "missing" lines on
        // every pass, for three directories that are already in the state asked for.
        let tmp = repo();
        assert!(
            resolve(
                tmp.path(),
                &one(declared("never/existed", "echo not needed"))
            )
            .is_empty()
        );
    }

    #[test]
    fn an_exclusion_takes_a_declaration_out_of_play_however_it_is_spelled() {
        // The committed file is the team's; the exclusion is one machine's answer to it.
        // It has to survive the spellings a person actually types, because the failure
        // mode is deleting the directory it was written to keep.
        let tmp = repo();
        let path = tmp.path();
        fs::create_dir_all(path.join("scratch")).unwrap();

        for spelling in ["scratch", "scratch/", "./scratch", r"scratch\"] {
            let prunable = Prunable {
                directories: vec![declared("scratch", "echo not needed")],
                exclude: vec![spelling.to_string()],
            };
            assert!(
                resolve(path, &prunable).is_empty(),
                "`{spelling}` did not exclude `scratch`"
            );
        }

        // And it takes only what it names.
        fs::create_dir_all(path.join("vendor")).unwrap();
        let prunable = Prunable {
            directories: vec![
                declared("scratch", "echo not needed"),
                declared("vendor", "echo not needed"),
            ],
            exclude: vec!["scratch".to_string()],
        };
        let left: Vec<String> = resolve(path, &prunable)
            .into_iter()
            .map(|d| match d {
                Declaration::Prunable(t) => t.label,
                Declaration::Refused { label, .. } => label,
            })
            .collect();
        assert_eq!(left, ["vendor"]);
    }

    #[test]
    fn an_exclusion_silences_the_refusal_too_not_only_the_delete() {
        // A refusal is a standing complaint printed on every pass. Somebody who has said
        // this directory is not dev-prune's business has answered that as well.
        let tmp = repo();
        let path = tmp.path();
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("src/main.rs"), "fn main() {}").unwrap();
        Command::new("git")
            .args(["add", "src/main.rs"])
            .current_dir(path)
            .output()
            .unwrap();

        assert!(
            !resolve(path, &one(declared("src", "echo not needed"))).is_empty(),
            "this repository is supposed to produce a refusal"
        );
        let prunable = Prunable {
            directories: vec![declared("src", "echo not needed")],
            exclude: vec!["src".to_string()],
        };
        assert!(resolve(path, &prunable).is_empty());
    }

    #[test]
    fn a_backslash_declaration_reads_the_same_as_a_forward_slash_one() {
        assert_eq!(
            split_relative(r"build\fixtures").unwrap(),
            split_relative("build/fixtures").unwrap()
        );
    }

    /// The rebuild-target checks are exercised through `rebuild_gap` rather than
    /// `resolve`, because the `PATH` check runs first: on a machine without `make` or
    /// `uv` the refusal would be the "not on this machine" one instead, and these have
    /// to mean the same thing on every platform CI runs. The end-to-end composition is
    /// covered separately, below.
    fn gap(repo_path: &Path, rebuild: &str) -> Gap {
        match rebuild_gap(repo_path, rebuild) {
            Some(gap) => gap,
            None => panic!("`{rebuild}` should have been refused"),
        }
    }

    #[test]
    fn a_rebuild_naming_a_script_the_package_json_does_not_have_is_refused() {
        // The gap this whole section exists to close: `npm` being installed says nothing
        // about whether `npm run build` would do anything.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();

        let gap = gap(tmp.path(), "npm run build");
        assert!(gap.what.contains("package.json"), "{}", gap.what);
        assert!(gap.what.contains("`build`"), "{}", gap.what);
        assert!(gap.fix.contains("Add a `build` script"), "{}", gap.fix);
    }

    #[test]
    fn a_rebuild_naming_a_script_that_is_there_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"build":"tsc -p .","test":"vitest"}}"#,
        )
        .unwrap();

        for rebuild in [
            "npm run build",
            "pnpm run build",
            "yarn run build",
            "pnpm build",
            "yarn build",
            "npm.cmd run build",
        ] {
            assert!(
                rebuild_gap(tmp.path(), rebuild).is_none(),
                "`{rebuild}` names a script that is right there"
            );
        }
    }

    #[test]
    fn the_prefix_flag_decides_which_package_json_is_read() {
        // The refusal one check earlier hands people `npm --prefix docs run build`.
        // Following that advice has to land on `docs/package.json`, not the root one.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path();
        fs::write(
            path.join("package.json"),
            r#"{"scripts":{"lint":"eslint"}}"#,
        )
        .unwrap();
        fs::create_dir_all(path.join("docs")).unwrap();
        fs::write(
            path.join("docs/package.json"),
            r#"{"scripts":{"build":"astro build"}}"#,
        )
        .unwrap();

        for rebuild in [
            "npm --prefix docs run build",
            "npm --prefix=docs run build",
            "pnpm -C docs run build",
            "pnpm --dir docs build",
            "yarn --cwd docs build",
        ] {
            assert!(
                rebuild_gap(path, rebuild).is_none(),
                "`{rebuild}` should have read docs/package.json"
            );
        }

        // The root manifest is the one without a `build` script, and saying which file
        // was read is the difference between a useful refusal and a confusing one.
        assert!(gap(path, "npm run build").what.contains("`package.json`"));
        let elsewhere = gap(path, "npm --prefix docs run missing");
        assert!(
            elsewhere.what.contains("`docs/package.json`"),
            "{}",
            elsewhere.what
        );
    }

    #[test]
    fn a_make_target_the_makefile_does_not_define_is_refused() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Makefile"),
            "CACHE := .cache\n\n.PHONY: clean\n\nvendor: tools/manifest.toml\n\tgo mod vendor\n",
        )
        .unwrap();

        assert!(rebuild_gap(tmp.path(), "make vendor").is_none());
        assert!(rebuild_gap(tmp.path(), "make clean").is_none());
        assert!(rebuild_gap(tmp.path(), "make CACHE=x vendor").is_none());

        let gap = gap(tmp.path(), "make fixtures");
        assert!(gap.what.contains("`Makefile`"), "{}", gap.what);
        assert!(gap.what.contains("`fixtures`"), "{}", gap.what);
    }

    #[test]
    fn a_uv_script_declared_in_tool_uv_scripts_is_refused_because_uv_never_reads_it() {
        // `[tool.uv.scripts]` is not a uv field. It parses, it looks right, and it does
        // nothing — so a directory declared behind one is a directory nothing rebuilds.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path();
        fs::write(
            path.join("pyproject.toml"),
            "[project]\nname = \"proj\"\n\n[tool.uv.scripts]\nregen-fixtures = \"tools.gen:main\"\n",
        )
        .unwrap();

        let gap = gap(path, "uv run regen-fixtures");
        assert!(gap.what.contains("pyproject.toml"), "{}", gap.what);
        assert!(gap.fix.contains("[project.scripts]"), "{}", gap.fix);

        // Moved to the table uv actually reads, the same declaration passes.
        fs::write(
            path.join("pyproject.toml"),
            "[project]\nname = \"proj\"\n\n[project.scripts]\nregen-fixtures = \"tools.gen:main\"\n",
        )
        .unwrap();
        assert!(rebuild_gap(path, "uv run regen-fixtures").is_none());

        // And a console script a dependency brings in is not in either table.
        fs::write(
            path.join("pyproject.toml"),
            "[project]\nname = \"proj\"\ndependencies = [\n  \"pytest>=8\",\n]\n",
        )
        .unwrap();
        assert!(rebuild_gap(path, "uv run pytest").is_none());
    }

    #[test]
    fn a_command_shape_this_cannot_read_is_allowed_rather_than_guessed_at() {
        // The asymmetry the whole check is built on. A false refusal blocks a prune that
        // was safe, in a committed file that is awkward to debug — worse than the gap.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path();
        fs::write(
            path.join("package.json"),
            r#"{"scripts":{"lint":"eslint"}}"#,
        )
        .unwrap();
        fs::write(path.join("Makefile"), "vendor:\n\tgo mod vendor\n").unwrap();

        for rebuild in [
            "definitely-not-a-real-tool-xyz build", // a tool with no manifest to read
            "npm run build && npm run docs",        // a chain, not one invocation
            "npm run $TARGET",                      // the target is a variable
            "npm run build | tee log",              // a pipeline
            "npm ci",                               // not a script invocation at all
            "npm --workspace api run build",        // a flag this does not model
            "npm run -- build",                     // everything after `--` is opaque
            "make",                                 // the default goal has no name
            "make -j4 vendor",                      // a flag this does not model
            "uv sync",                              // not a script invocation
            "echo not needed",                      // the documented escape hatch
            "./scripts/gen.sh",                     // a program, not a subcommand
        ] {
            assert!(
                rebuild_gap(path, rebuild).is_none(),
                "`{rebuild}` should have been allowed through, not refused"
            );
        }

        // A manifest that is not there, or that does not parse, is also "cannot tell".
        let bare = TempDir::new().unwrap();
        assert!(rebuild_gap(bare.path(), "npm run build").is_none());
        assert!(rebuild_gap(bare.path(), "make vendor").is_none());
        assert!(rebuild_gap(bare.path(), "uv run regen").is_none());
        fs::write(bare.path().join("package.json"), "{ not json").unwrap();
        assert!(rebuild_gap(bare.path(), "npm run build").is_none());
    }

    #[test]
    fn a_makefile_this_cannot_see_all_of_is_not_answered_from_the_part_it_can() {
        // An `include`, a pattern rule or a variable in the target position each mean
        // the file names targets that are not in its text.
        let tmp = TempDir::new().unwrap();
        for makefile in [
            "include common.mk\n\nvendor:\n\tgo mod vendor\n",
            "%.pb.go: %.proto\n\tprotoc $<\n",
            "$(GENERATED): schema.json\n\tgen\n",
        ] {
            fs::write(tmp.path().join("Makefile"), makefile).unwrap();
            assert!(
                rebuild_gap(tmp.path(), "make fixtures").is_none(),
                "a makefile with hidden targets must not produce a refusal"
            );
        }
    }

    #[test]
    fn the_refusal_reads_like_every_other_one_in_this_module() {
        // Composition, end to end. Guarded because the `PATH` check runs first: without
        // node the refusal is the "not on this machine" one, which is correct there.
        if !on_path("npm") {
            return;
        }
        let tmp = repo();
        let path = tmp.path();
        fs::create_dir_all(path.join("site/dist")).unwrap();
        fs::write(
            path.join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();

        let reason = refusal(path, declared("site/dist", "npm run build"));
        assert!(
            reason.contains("`site/dist` is declared prunable, rebuilt by `npm run build`"),
            "{reason}"
        );
        assert!(reason.contains("defines no `build` script"), "{reason}");
        assert!(reason.contains("cannot put back"), "{reason}");
        assert!(path.join("site/dist").exists());
    }
}
