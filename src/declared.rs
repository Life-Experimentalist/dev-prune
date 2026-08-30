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
//! and has a rebuild command whose tool this machine actually has. A claim that fails
//! any of those is reported, in full, and nothing is deleted.

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
    if !SHELL_BUILTINS.contains(&tool) && !on_path(tool) {
        return Err(format!(
            "`{label}` is declared prunable, rebuilt by `{rebuild}`, but `{tool}` is not \
             on this machine — refusing to delete something this machine cannot put \
             back. Install `{tool}` first."
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
}
