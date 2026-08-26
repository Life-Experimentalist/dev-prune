// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handler for `dev-prune hook` subcommands.
//
// Manages non-blocking global Git hooks (`post-commit`, `post-checkout`, `post-merge`)
// to automatically track Git repositories as you clone them and work in them.
//
// `git init` is the one arrival these cannot see: Git runs no hook for it, and none of
// the three fires until the new repository's first commit. That gap is closed from the
// other side, in `commands::link::adopt_enclosing_repo`.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Registry;
use crate::output;

/// The hooks dev-prune installs. All three fire *after* the operation completes, so
/// none of them can fail a commit, and each is a moment a repository can first appear
/// on disk or first be worked in: `post-checkout` also runs after `git clone`.
///
/// What they cannot cover is `git init`. Git has no `post-init` hook to install, and a
/// repository created here fires none of these until its first commit — so between
/// `git init` and that commit it is genuinely invisible here. Widening the set does not
/// help: the only name that fires in that window is `post-index-change`, which also
/// fires on every routine index refresh (see [`NO_SHADOW_SHIM`]). The window is closed
/// in `commands::link::adopt_enclosing_repo` instead, where reading the registry costs
/// nothing extra.
const HOOKS: [&str; 3] = ["post-commit", "post-checkout", "post-merge"];

/// Every hook name Git will look for inside `core.hooksPath`.
///
/// Two uses. It decides which files in *another* tool's hooks directory are hooks worth
/// forwarding — without it a chained install would happily shim husky's `_/` helper
/// directory, its `.gitignore` and its README. And it is the set of shims an unchained
/// install writes, so that setting `core.hooksPath` does not silently disable every
/// repository's own `.git/hooks`; see [`SHADOWING_HOOKS`].
const GIT_HOOK_NAMES: [&str; 28] = [
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// Hook names an unchained install does *not* shim, despite Git looking for them there.
///
/// Setting `core.hooksPath` makes Git ignore `.git/hooks` in every repository on the
/// machine, so dev-prune writes a passthrough for each name to put that behaviour back —
/// but not for these. `reference-transaction` fires once per ref per transaction, which
/// on a fetch of a busy remote is hundreds of times; `post-index-change` fires on
/// routine index refreshes. Neither is a hook anyone reaches for in a working
/// repository, and spawning a shell that many times to discover there is nothing to run
/// is a cost users would feel and never attribute to this.
const NO_SHADOW_SHIM: [&str; 2] = ["reference-transaction", "post-index-change"];

/// The names an unchained install writes: everything Git looks for, minus the two above.
fn shadowing_hooks() -> Vec<&'static str> {
    GIT_HOOK_NAMES
        .iter()
        .copied()
        .filter(|name| !NO_SHADOW_SHIM.contains(name))
        .collect()
}

/// Marker file recording the `core.hooksPath` a chained install displaced.
///
/// It lives in the hooks directory rather than in the registry because it is state, not
/// preference: it describes what is currently installed on this machine, and it has to
/// stay next to the shims that depend on it. Git never runs it — it is not a hook name.
const CHAIN_MARKER: &str = ".chain-target";

/// Return path to the dev-prune hooks directory (`~/.config/dev-prune/hooks/`).
pub fn hooks_dir() -> Result<PathBuf> {
    let base = Registry::config_dir()?;
    Ok(base.join("hooks"))
}

/// The hooks directory a chained install is forwarding to, if we are chaining.
pub fn chain_target() -> Option<PathBuf> {
    let marker = hooks_dir().ok()?.join(CHAIN_MARKER);
    let raw = fs::read_to_string(marker).ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Advice printed when `git` cannot be found.
pub const GIT_MISSING_HELP: &str = "`git` was not found on your PATH.\n\
     dev-prune identifies repositories with Git and installs its hooks through \
     `git config --global`, so it can do neither without it.\n\
     Install Git from https://git-scm.com/downloads (or your package manager), confirm \
     that `git --version` works in a new terminal, then run `devp setup` again.";

/// Whether a usable `git` is on PATH.
///
/// Everything dev-prune does is scoped to Git repositories, so this is not a degraded
/// mode to work around — it is a stop, with an instruction attached.
pub fn git_available() -> bool {
    crate::spawn::command("git")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Where the global hook installation currently stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookState {
    /// `core.hooksPath` points at our directory and every hook file is present.
    Active,
    /// Nothing of ours is installed and the single global slot is free.
    Absent,
    /// `core.hooksPath` belongs to another tool; taking it would disable that tool
    /// in every repository on the machine.
    Foreign(String),
    /// Ours, forwarding every hook on to the directory it displaced.
    Chained {
        /// Where the shims hand off to.
        previous: String,
        /// Hook names present over there with no shim here, so currently not running.
        /// Non-empty means the other tool added a hook after the chain was built.
        drifted: Vec<String>,
    },
}

/// Classify the current global hook installation.
pub fn state() -> Result<HookState> {
    let dir = hooks_dir()?;
    match global_hooks_path() {
        Some(existing) if Path::new(&existing) != dir => Ok(HookState::Foreign(existing)),
        Some(_) if HOOKS.iter().all(|hook| dir.join(hook).exists()) => match chain_target() {
            Some(previous) => Ok(HookState::Chained {
                drifted: chain_drift(&dir, &previous),
                previous: output::clean_path(&previous),
            }),
            None => Ok(HookState::Active),
        },
        // Pointed here with files missing, or not pointed anywhere: either way the fix
        // is the same install, so both are "absent".
        _ => Ok(HookState::Absent),
    }
}

/// Hook names the chained-to directory has that we do not shim.
///
/// A chain is built from a snapshot of the other tool's directory, and that tool can add
/// a hook afterwards — husky's whole workflow is `husky add`. Without this check the new
/// hook would simply stop running, with nothing anywhere saying so.
fn chain_drift(ours: &Path, theirs: &Path) -> Vec<String> {
    hook_names_in(theirs)
        .into_iter()
        .filter(|name| !ours.join(name).exists())
        .collect()
}

/// The Git hook files present in a directory, in a stable order.
fn hook_names_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let present: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    GIT_HOOK_NAMES
        .iter()
        .filter(|name| present.iter().any(|p| p == *name))
        .map(|name| name.to_string())
        .collect()
}

/// Whether an unchained install is missing shims it should have.
///
/// Every install before 1.4.0 wrote three files and left `core.hooksPath` shadowing each
/// repository's own `.git/hooks`. Upgrading has to repair that, and the upgrade path only
/// reinstalls when something tells it to — [`state`] reports such an install as perfectly
/// `Active`, because as far as registration goes it is.
///
/// Only meaningful for an unchained install: a chained one writes exactly the names the
/// other tool has, which is a different and correct set. `chain_drift` covers that case.
pub fn shims_incomplete() -> bool {
    let Ok(dir) = hooks_dir() else {
        return false;
    };
    if chain_target().is_some() {
        return false;
    }
    shims_missing_in(&dir)
}

/// The directory-inspecting half of [`shims_incomplete`], split out so the rule can be
/// tested against a scratch directory rather than the machine's real config directory.
fn shims_missing_in(dir: &Path) -> bool {
    shadowing_hooks()
        .into_iter()
        .any(|name| !dir.join(name).exists())
}

/// Read the current global `core.hooksPath`, if any.
fn global_hooks_path() -> Option<String> {
    let out = crate::spawn::command("git")
        .args(["config", "--global", "core.hooksPath"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Build the hook script that registers the current repository in the background.
///
/// The executable path is single-quoted, not double-quoted: inside double quotes `sh`
/// still expands `$` and backticks, and both are legal in a Windows install path
/// (`C:\Users\a$b\...`), where the hook runs under Git for Windows' bundled `sh`.
/// Single quotes are literal all the way through, so only an embedded single quote
/// needs handling — done the POSIX way, by closing and reopening the string.
fn build_hook_script(exe: &str, hook: &str, register: bool) -> String {
    let registration = if register {
        format!("('{}' link . --quiet >/dev/null 2>&1 &)\n", sq(exe))
    } else {
        String::new()
    };
    format!(
        r#"#!/usr/bin/env sh
# dev-prune hook shim. Rebuild with `devp hook install`.
{registration}{}"#,
        local_passthrough(hook)
    )
}

/// The tail every unchained shim ends with: run this repository's own hook, if it has
/// one, exactly as Git would have.
///
/// `core.hooksPath` replaces `.git/hooks` rather than adding to it, so without this a
/// machine-wide install silently stops every repo-local `pre-commit`, `commit-msg` and
/// `pre-push` on the machine — lint gates, secret scanners, conventional-commit checks —
/// and nothing reports it. That is not a trade dev-prune gets to make on someone's
/// behalf for the sake of registering a workspace.
///
/// `--git-common-dir` and not `--git-path hooks/<name>`, which looks like the obvious
/// call and is a trap: `--git-path` resolves hook paths *through* `core.hooksPath`, so it
/// hands back this very shim and the script execs itself until the machine gives up. It
/// is also not simply `$GIT_DIR/hooks`, because in a linked worktree `$GIT_DIR` is
/// `.git/worktrees/<name>` while the hooks live in the common directory. The `$GIT_DIR`
/// arm is the fallback for the case where `git` is somehow not on the hook's own PATH.
///
/// `exec`, so the real hook inherits stdin — `pre-push` reads its refs from it — and its
/// exit code is the hook's exit code, with no wrapper left to swallow a rejection.
fn local_passthrough(hook: &str) -> String {
    format!(
        r#"common=$(git rev-parse --git-common-dir 2>/dev/null) || common="${{GIT_DIR:-.git}}"
next="$common/hooks/{0}"
if [ -x "$next" ]; then exec "$next" "$@"; fi
if [ -f "$next" ]; then exec sh "$next" "$@"; fi
exit 0
"#,
        hook
    )
}

/// Build a hook that forwards to the same hook in the directory dev-prune displaced.
///
/// `register` adds the background registration on top; the other names are pure
/// passthrough, present only so the other tool keeps working.
///
/// `exec` rather than a call: it replaces this process, so the real hook inherits stdin
/// (which `pre-push` and `pre-receive` read their refs from) and its exit code is the
/// hook's exit code, with no chance of a wrapper swallowing a rejection.
fn build_chained_hook_script(exe: &str, previous: &Path, hook: &str, register: bool) -> String {
    let registration = if register {
        format!("('{}' link . --quiet >/dev/null 2>&1 &)\n", sq(exe))
    } else {
        String::new()
    };
    let target = previous.join(hook);
    format!(
        r#"#!/usr/bin/env sh
# dev-prune hook shim — chained. Rebuild with `devp hook install --chain`.
{registration}next='{}'
if [ -x "$next" ]; then exec "$next" "$@"; fi
if [ -f "$next" ]; then exec sh "$next" "$@"; fi
exit 0
"#,
        sq(&target.to_string_lossy())
    )
}

/// Escape a value for a POSIX single-quoted string, the only quoting `sh` does not
/// expand: `$`, backticks and backslashes are all legal in a Windows install path.
fn sq(value: &str) -> String {
    value.replace('\'', r"'\''")
}

/// Recover the binary path out of a hook script this module wrote.
///
/// Both templates start the registration line with `('<exe>' link . --quiet`, and the
/// path is single-quoted, so the closing quote is the one immediately before ` link`.
/// Searching for that rather than the first `'` is what keeps a path containing an
/// escaped quote intact.
fn parse_hook_exe(script: &str) -> Option<PathBuf> {
    let start = script.find("('")? + 2;
    let end = start + script[start..].find("' link . --quiet")?;
    let exe = script[start..end].replace(r"'\''", "'");
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

/// The binary the installed hooks will actually run, if there are hooks to read.
///
/// `None` means "could not determine", not "not installed" — [`state`] answers that.
/// A hook is deliberately silent (it backgrounds itself and discards its output), so a
/// script left pointing at a deleted directory never reports anything; this is what lets
/// `devp doctor` say so.
pub fn registered_exe_path() -> Option<PathBuf> {
    let script = fs::read_to_string(hooks_dir().ok()?.join(HOOKS[0])).ok()?;
    parse_hook_exe(&script)
}

/// Install the hooks, printing the result and the caveats.
pub fn run_install(chain: bool) -> Result<()> {
    let dir = hooks_dir()?;
    install_with(chain)?;

    output::print_header("dev-prune Non-Blocking Git Hooks");
    output::print_success(&format!(
        "Installed global Git hooks in `{}`",
        output::clean_path(&dir)
    ));
    println!("  Hooks Active:        {}", HOOKS.join(", "));
    println!("  Execution Mode:      Asynchronous / Non-blocking (0ms commit impact)");

    match chain_target() {
        Some(previous) => {
            let forwarded = hook_names_in(&previous);
            println!("  Chained To:          {}", output::clean_path(&previous));
            println!(
                "  Forwarded Hooks:     {}",
                if forwarded.is_empty() {
                    "none found (the directory is empty)".to_string()
                } else {
                    forwarded.join(", ")
                }
            );
            println!();
            output::print_info("How to manage hook settings:");
            println!("  Restore Previous:    devp hook uninstall");
            println!("  Rebuild The Chain:   devp hook install --chain");
            println!();
            output::print_warning(
                "The chain is a snapshot. If that tool adds a hook later, re-run \
                 `devp hook install --chain` — `devp hook status` reports the drift.",
            );
        }
        None => {
            println!();
            output::print_info("How to manage hook settings:");
            println!("  Disable Globally:    devp hook uninstall");
            println!("  Disable Per-Repo:    git config core.hooksPath \"\" (inside project root)");
            println!("  Re-enable Globally:  devp hook install");
            println!();
            output::print_warning(
                "While this is active, per-repo `.git/hooks` are ignored in every repository on \
                 this machine — that includes husky, pre-commit and lefthook.",
            );
        }
    }

    Ok(())
}

/// Install non-blocking global Git hooks (`post-commit`, `post-checkout`, `post-merge`).
///
/// Silent, so the automatic setup pass can call it and report in its own format.
pub fn install() -> Result<()> {
    install_with(false)
}

/// Install the hooks, optionally forwarding to the hooks directory already configured.
///
/// `chain` is the answer to the single-slot problem. Git has one `core.hooksPath` and no
/// way to list two, but nothing says the directory in that slot has to hold the *final*
/// hooks: ours can register the repository and then `exec` the hook of the same name from
/// the directory it displaced. The other tool keeps working, we get our registration, and
/// `devp hook uninstall` puts the original value back.
pub fn install_with(chain: bool) -> Result<()> {
    let dir = hooks_dir()?;

    // Git is not an optional dependency here: it is both what dev-prune detects
    // repositories with and the mechanism that installs these hooks.
    if !git_available() {
        anyhow::bail!("{GIT_MISSING_HELP}");
    }

    // `core.hooksPath` is a single global slot. Overwriting someone else's value
    // disables husky / pre-commit / lefthook in *every* repository on the machine.
    // Refuse rather than break their setup — unless asked to chain, which preserves it.
    let previous = match global_hooks_path() {
        Some(existing) if Path::new(&existing) != dir => {
            if !chain {
                anyhow::bail!(
                    "`core.hooksPath` is already set globally to `{existing}`.\n\
                     Git only supports one hooks directory, so installing here would disable \
                     those hooks in every repo on this machine.\n\
                     Run `devp hook install --chain` to install in front of it instead: \
                     dev-prune registers the repo, then hands every hook on to `{existing}`.\n\
                     Or unset it first:\n    git config --global --unset core.hooksPath\n\
                     Or do nothing — `devp link .` in new repos does the same job by hand."
                );
            }
            let path = PathBuf::from(&existing);
            // A relative `core.hooksPath` resolves against each repository's own root, so
            // there is no single directory to forward to and the shim would point at
            // whichever repo happened to be current when we installed.
            if path.is_relative() {
                anyhow::bail!(
                    "`core.hooksPath` is set to the relative path `{existing}`, which Git \
                     resolves separately inside every repository. There is no one directory \
                     to chain to.\n\
                     Set it to an absolute path first, or leave it alone and use `devp link .`."
                );
            }
            Some(path)
        }
        // Already ours: keep whatever chain is in place, so a plain `devp hook install`
        // re-run repairs the shims instead of silently unchaining the other tool.
        _ => chain.then(chain_target).flatten(),
    };

    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create hooks directory at {}", dir.display()))?;

    // Resolve the binary by absolute path. Git runs hooks with a minimal environment,
    // and a bare `devp` is frequently not on the PATH it sees — which turns the hook
    // into a silent no-op, since it discards its own output by design.
    //
    // A *durable* absolute path, not `current_exe()`: these scripts stay on disk long
    // after the process that wrote them, so `npx dev-prune link .` must not bake in a
    // path inside npm's cache. See `setup::stable_exe_path`.
    let exe = crate::setup::stable_exe_path()
        .to_string_lossy()
        .into_owned();

    match &previous {
        None => {
            let names = shadowing_hooks();
            for hook in &names {
                let content = build_hook_script(&exe, hook, HOOKS.contains(hook));
                write_hook(&dir.join(hook), &content)?;
            }
            // Left over from a chained install, or from a Git version that had a name
            // this one does not. Either way they are shims for hooks nothing runs.
            for stale in hook_names_in(&dir) {
                if !names.contains(&stale.as_str()) {
                    let _ = fs::remove_file(dir.join(&stale));
                }
            }
            // Any stale chain is gone now, and a marker left behind would make
            // `devp hook uninstall` restore a path nothing forwards to any more.
            let _ = fs::remove_file(dir.join(CHAIN_MARKER));
        }
        Some(prev) => {
            // Ours first, then every hook the other tool actually has. A name it does not
            // have gets no shim: `reference-transaction` fires several times per Git
            // operation, and a shell that only exits 0 is not free.
            let mut names: Vec<String> = HOOKS.iter().map(|h| h.to_string()).collect();
            for name in hook_names_in(prev) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            // Drop shims for hooks the other tool has since removed, otherwise they
            // linger as no-ops for hooks nobody installed.
            for stale in hook_names_in(&dir) {
                if !names.contains(&stale) {
                    let _ = fs::remove_file(dir.join(&stale));
                }
            }
            for name in &names {
                let register = HOOKS.contains(&name.as_str());
                let content = build_chained_hook_script(&exe, prev, name, register);
                write_hook(&dir.join(name), &content)?;
            }
            fs::write(dir.join(CHAIN_MARKER), format!("{}\n", prev.display())).with_context(
                || "Failed to record the chained hooks path; refusing a chain we cannot undo",
            )?;
        }
    }

    // Set global git config core.hooksPath
    let status = crate::spawn::command("git")
        .args([
            "config",
            "--global",
            "core.hooksPath",
            &dir.to_string_lossy(),
        ])
        .status()
        .with_context(|| "Failed to execute `git config --global core.hooksPath`")?;

    if !status.success() {
        anyhow::bail!("Failed to update git global configuration.");
    }

    Ok(())
}

/// Write one hook file, executable on the platforms that care.
fn write_hook(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).with_context(|| format!("Failed to write hook {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Delete the hook scripts and chain marker from dev-prune's own hooks directory.
///
/// Called once `core.hooksPath` no longer points here. Left behind, the dead scripts
/// make `devp hook status` warn "files exist but never run" forever, and a stale
/// `.chain-target` would make the *next* uninstall "restore" a path nothing forwards
/// to any more.
fn remove_hook_files(dir: &Path) {
    let _ = fs::remove_file(dir.join(CHAIN_MARKER));
    for name in hook_names_in(dir) {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// Uninstall non-blocking global Git hooks.
pub fn run_uninstall() -> Result<()> {
    // Only clear the setting if it still points at us. Blindly unsetting would delete
    // a value the user set for something else entirely.
    let dir = hooks_dir()?;

    // A chained install borrowed the slot from another tool. Handing it back is the
    // whole reason the chain was allowed in the first place — unsetting would leave
    // that tool's hooks configured nowhere and silently dead.
    if let Some(previous) = chain_target()
        && global_hooks_path().is_some_and(|c| Path::new(&c) == dir)
    {
        let restored = crate::spawn::command("git")
            .args([
                "config",
                "--global",
                "core.hooksPath",
                &previous.to_string_lossy(),
            ])
            .status();
        match restored {
            Ok(status) if status.success() => {
                remove_hook_files(&dir);
                output::print_success(&format!(
                    "Restored `core.hooksPath` to `{}`.",
                    output::clean_path(&previous)
                ));
                return Ok(());
            }
            Ok(status) => anyhow::bail!(
                "`git config --global core.hooksPath` exited with {status} while restoring \
                     `{}`. Set it by hand to bring those hooks back.",
                output::clean_path(&previous)
            ),
            Err(e) => anyhow::bail!("Could not run `git config --global core.hooksPath`: {e}"),
        }
    }

    match global_hooks_path() {
        Some(current) if Path::new(&current) != dir => {
            output::print_info(&format!(
                "`core.hooksPath` is set to `{current}`, which is not dev-prune's — leaving it alone."
            ));
            // Our own directory can still hold dead scripts (and a stale chain marker)
            // from an earlier install — the setting was changed out from under them.
            if !hook_names_in(&dir).is_empty() || dir.join(CHAIN_MARKER).exists() {
                remove_hook_files(&dir);
                output::print_info("Removed dev-prune's leftover hook scripts.");
            }
            return Ok(());
        }
        None => {
            if !hook_names_in(&dir).is_empty() || dir.join(CHAIN_MARKER).exists() {
                remove_hook_files(&dir);
                output::print_success(
                    "`core.hooksPath` was not set globally; removed dev-prune's leftover \
                     hook scripts.",
                );
            } else {
                output::print_info("`core.hooksPath` is not set globally — nothing to remove.");
            }
            return Ok(());
        }
        Some(_) => {}
    }

    // Reported honestly: a failed unset leaves `core.hooksPath` pointing at a directory
    // whose hook files may already be gone, which is the one state that silently breaks
    // Git hooks machine-wide. Saying "removed" when it was not would hide exactly that.
    let unset = crate::spawn::command("git")
        .args(["config", "--global", "--unset", "core.hooksPath"])
        .status();
    match unset {
        Ok(status) if status.success() => {
            remove_hook_files(&dir);
            output::print_success(
                "Removed global Git hook configuration (`git config --global --unset core.hooksPath`).",
            );
            Ok(())
        }
        Ok(status) => anyhow::bail!(
            "`git config --global --unset core.hooksPath` exited with {status}. \
             Run it by hand to finish removing the hooks."
        ),
        Err(e) => anyhow::bail!("Could not run `git config --global --unset core.hooksPath`: {e}"),
    }
}

/// Show status of global Git hooks.
pub fn run_status() -> Result<()> {
    let dir = hooks_dir()?;

    if !git_available() {
        output::print_header("dev-prune Git Hooks Status");
        output::print_error(GIT_MISSING_HELP);
        return Ok(());
    }

    let configured = global_hooks_path();
    let on_disk = HOOKS.iter().all(|hook| dir.join(hook).exists());

    // Both halves must hold. Hook files with `core.hooksPath` pointing elsewhere are
    // dead files, and a `core.hooksPath` merely *containing* the string "dev-prune"
    // could just as easily be some other tool living under a `dev-prune` directory.
    let points_at_us = configured
        .as_deref()
        .is_some_and(|current| Path::new(current) == dir);

    output::print_header("dev-prune Git Hooks Status");
    println!(
        "  Configured core.hooksPath: {}",
        configured.as_deref().unwrap_or("Not set")
    );
    println!("  DevPrune Hooks Directory:  {}", dir.display());
    println!(
        "  Hooks Installed on Disk:   {}",
        if on_disk {
            format!("Yes ({})", HOOKS.join(", "))
        } else {
            "No".to_string()
        }
    );
    if let Some(previous) = chain_target() {
        println!(
            "  Chained To:                {}",
            output::clean_path(&previous)
        );
        let forwarded = hook_names_in(&previous);
        println!(
            "  Forwarded Hooks:           {}",
            if forwarded.is_empty() {
                "none".to_string()
            } else {
                forwarded.join(", ")
            }
        );
        let drifted = chain_drift(&dir, &previous);
        if !drifted.is_empty() {
            println!();
            output::print_warning(&format!(
                "`{}` now has hooks the chain does not forward: {}.\n  \
                 They are not running. Rebuild with `devp hook install --chain`.",
                output::clean_path(&previous),
                drifted.join(", ")
            ));
        }
    }
    println!();
    match (points_at_us, on_disk) {
        (true, true) => output::print_success("Global background auto-registration is ACTIVE."),
        (true, false) => output::print_warning(
            "`core.hooksPath` points here but the hook files are missing. \
             Re-run `devp hook install`.",
        ),
        (false, true) => output::print_warning(
            "Hook files exist but `core.hooksPath` points elsewhere — they never run. \
             Re-run `devp hook install`, or delete the directory.",
        ),
        (false, false) => output::print_info(
            "Global background hook is inactive. Run `devp hook install` to enable.",
        ),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_script_single_quotes_the_executable_path() {
        let script = build_hook_script("/usr/local/bin/dev-prune", "post-commit", true);
        assert!(script.contains("('/usr/local/bin/dev-prune' link . --quiet"));
    }

    #[test]
    fn hook_script_neutralises_shell_metacharacters_in_the_path() {
        // All legal in a Windows path, and all live inside sh double quotes.
        let script = build_hook_script(r"C:\Users\a$b\`whoami`\dev-prune.exe", "post-commit", true);
        assert!(script.contains(r"('C:\Users\a$b\`whoami`\dev-prune.exe' link ."));
    }

    #[test]
    fn hook_script_escapes_an_embedded_single_quote() {
        let script = build_hook_script("/home/o'brien/dev-prune", "post-commit", true);
        assert!(script.contains(r"('/home/o'\''brien/dev-prune' link ."));
    }

    #[test]
    fn hook_script_starts_with_a_shebang_and_backgrounds_the_call() {
        let script = build_hook_script("devp", "post-commit", true);
        assert!(script.starts_with("#!/usr/bin/env sh\n"));
        // Backgrounded in a subshell so a commit never waits on registration.
        assert!(script.contains(">/dev/null 2>&1 &)"));
    }

    #[test]
    fn a_hooks_path_git_reported_with_forward_slashes_is_still_ours() {
        // `state()` and `run_uninstall()` both decide whether the global hooks directory
        // belongs to dev-prune by comparing `Path`s, not strings. Git for Windows hands
        // config values back with forward slashes, so a string compare would classify our
        // own directory as Foreign — refusing to install, and refusing to clean up.
        // `Path` compares by component, which is why this holds.
        #[cfg(windows)]
        assert_eq!(
            Path::new("C:/Users/dev/AppData/Roaming/dev-prune/hooks"),
            Path::new(r"C:\Users\dev\AppData\Roaming\dev-prune\hooks")
        );

        // And the negative case, on every platform: a different directory is Foreign.
        assert_ne!(
            Path::new("/home/dev/.config/dev-prune/hooks"),
            Path::new("/home/dev/.config/husky/hooks")
        );
    }

    #[test]
    fn a_chained_hook_execs_the_hook_it_displaced() {
        let script = build_chained_hook_script(
            "/usr/local/bin/dev-prune",
            Path::new("/home/dev/.husky"),
            "post-commit",
            true,
        );
        assert!(script.contains("('/usr/local/bin/dev-prune' link . --quiet"));
        // `exec`, so the real hook keeps stdin and owns the exit code.
        assert!(script.contains(r#"exec "$next" "$@""#));
        assert!(script.contains("post-commit'"));
        // A missing target is not a failure — the other tool simply has no such hook.
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn the_binary_is_recoverable_from_a_plain_hook() {
        let script = build_hook_script("/usr/local/bin/dev-prune", "post-commit", true);
        assert_eq!(
            parse_hook_exe(&script),
            Some(PathBuf::from("/usr/local/bin/dev-prune"))
        );
    }

    #[test]
    fn the_binary_is_recoverable_from_a_chained_hook() {
        let script = build_chained_hook_script(
            "C:\\Users\\a\\AppData\\Roaming\\dev-prune\\bin\\dev-prune.exe",
            Path::new("/home/dev/.husky"),
            "post-commit",
            true,
        );
        assert_eq!(
            parse_hook_exe(&script),
            Some(PathBuf::from(
                "C:\\Users\\a\\AppData\\Roaming\\dev-prune\\bin\\dev-prune.exe"
            ))
        );
    }

    #[test]
    fn a_quote_in_the_path_survives_the_round_trip() {
        // `sq` escapes it as `'\''`; splitting on the first quote instead of the one
        // before ` link` would cut the path in half here.
        let script = build_hook_script("/home/o'brien/dev-prune", "post-commit", true);
        assert_eq!(
            parse_hook_exe(&script),
            Some(PathBuf::from("/home/o'brien/dev-prune"))
        );
    }

    #[test]
    fn a_script_that_is_not_ours_answers_nothing() {
        assert!(parse_hook_exe("#!/bin/sh\nnpm test\n").is_none());
    }

    #[test]
    fn a_forwarded_hook_we_do_not_own_only_forwards() {
        let script = build_chained_hook_script(
            "/usr/local/bin/dev-prune",
            Path::new("/home/dev/.husky"),
            "pre-commit",
            false,
        );
        // Registering on `pre-commit` would put dev-prune in front of a hook that can
        // reject the commit, for no benefit: `post-commit` already covers the repo.
        assert!(!script.contains("link ."));
        assert!(script.contains(r#"exec "$next" "$@""#));
    }

    #[test]
    fn chaining_never_shims_a_file_that_is_not_a_git_hook() {
        let tmp = tempfile::TempDir::new().unwrap();
        // What a husky directory actually looks like.
        fs::write(tmp.path().join("pre-commit"), "#!/bin/sh\nnpm test\n").unwrap();
        fs::write(tmp.path().join("commit-msg"), "#!/bin/sh\ncommitlint\n").unwrap();
        fs::write(tmp.path().join(".gitignore"), "_\n").unwrap();
        fs::write(tmp.path().join("README.md"), "hooks\n").unwrap();
        fs::create_dir(tmp.path().join("_")).unwrap();

        let found = hook_names_in(tmp.path());
        assert_eq!(
            found,
            vec!["pre-commit".to_string(), "commit-msg".to_string()]
        );
    }

    #[test]
    fn drift_is_a_hook_the_other_tool_added_after_the_chain_was_built() {
        let ours = tempfile::TempDir::new().unwrap();
        let theirs = tempfile::TempDir::new().unwrap();
        fs::write(theirs.path().join("pre-commit"), "x").unwrap();
        fs::write(theirs.path().join("pre-push"), "x").unwrap();
        fs::write(ours.path().join("pre-commit"), "shim").unwrap();

        assert_eq!(
            chain_drift(ours.path(), theirs.path()),
            vec!["pre-push".to_string()]
        );
    }

    #[test]
    fn every_installed_hook_runs_after_the_operation_it_follows() {
        // A pre-* hook can abort a commit; none of ours may.
        assert!(HOOKS.iter().all(|hook| hook.starts_with("post-")));
        assert_eq!(HOOKS.len(), 3);
    }

    #[test]
    fn every_shim_hands_control_back_to_the_repositorys_own_hook() {
        // `core.hooksPath` replaces `.git/hooks` rather than adding to it. Without this
        // line an install silently kills every repo-local pre-commit on the machine.
        for hook in shadowing_hooks() {
            let script = build_hook_script("/usr/local/bin/dev-prune", hook, false);
            assert!(
                script.contains(&format!("hooks/{hook}")),
                "{hook} does not forward to the repository's own hook"
            );
            assert!(script.contains("exec"), "{hook} must exec, not call");
            // `--git-path hooks/<name>` resolves through `core.hooksPath` and so points
            // back at this shim: the script would exec itself forever.
            assert!(
                !script.contains("--git-path"),
                "{hook} must not resolve its target through core.hooksPath"
            );
            assert!(
                script.trim_end().ends_with("exit 0"),
                "{hook} must succeed when the repository has no hook of that name"
            );
        }
    }

    #[test]
    fn only_the_three_registration_hooks_register() {
        // A shim for `pre-push` exists to stop dev-prune breaking someone's push, not to
        // run `devp link` on every push.
        let registering = build_hook_script("/bin/devp", "post-commit", true);
        assert!(registering.contains("link . --quiet"));
        let passthrough = build_hook_script("/bin/devp", "pre-push", false);
        assert!(!passthrough.contains("link . --quiet"));
    }

    #[test]
    fn the_high_frequency_hooks_are_left_alone() {
        // `reference-transaction` fires once per ref per transaction. Shimming it means
        // spawning a shell hundreds of times on a single fetch, to discover there is
        // nothing to run.
        let names = shadowing_hooks();
        assert!(!names.contains(&"reference-transaction"));
        assert!(!names.contains(&"post-index-change"));
        // Everything anyone actually writes by hand is still covered.
        for expected in ["pre-commit", "commit-msg", "pre-push", "prepare-commit-msg"] {
            assert!(names.contains(&expected), "{expected} must be shimmed");
        }
    }

    #[test]
    fn a_pre_1_4_0_hook_set_is_recognised_as_incomplete() {
        // Exactly what an install from 1.3.1 left behind: three registration hooks and
        // nothing forwarding, which `state()` still reports as a healthy `Active`.
        let tmp = tempfile::tempdir().unwrap();
        for name in HOOKS {
            std::fs::write(
                tmp.path().join(name),
                "#!/bin/sh
",
            )
            .unwrap();
        }
        assert!(
            shims_missing_in(tmp.path()),
            "a three-file hooks directory must be reported as needing repair"
        );
    }

    #[test]
    fn a_full_shim_set_needs_no_repair() {
        let tmp = tempfile::tempdir().unwrap();
        for name in shadowing_hooks() {
            std::fs::write(
                tmp.path().join(name),
                "#!/bin/sh
",
            )
            .unwrap();
        }
        assert!(!shims_missing_in(tmp.path()));
        // And one missing name is enough to bring it back.
        std::fs::remove_file(tmp.path().join("pre-commit")).unwrap();
        assert!(shims_missing_in(tmp.path()));
    }
}
