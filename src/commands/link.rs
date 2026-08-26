// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Handlers for `dev-prune link` and `dev-prune unlink` commands.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{Adoption, PerRepoConfig, Registry};
use crate::constants;
use crate::output;
use crate::scanner;

/// Run the `link` command — register a Git repository.
///
/// `quiet` is what the global Git hook passes. In that mode nothing is printed and a
/// repository whose `.devprune.json` sets `disable_hooks` is left unregistered — that
/// flag exists precisely to keep the hook out of a specific workspace.
pub fn run_link(path_str: &str, quiet: bool) -> Result<()> {
    let path = Path::new(path_str)
        .canonicalize()
        .with_context(|| format!("Path not found: {path_str}"))?;

    if !scanner::is_git_repo(&path) {
        if quiet {
            return Ok(());
        }
        // Non-zero, because nothing was linked. The hook path above still exits 0: a
        // commit in a directory dev-prune does not track is not a failed commit.
        anyhow::bail!(
            "`{}` is not a Git repository.\n  \
             Run `git init` there first, then `devp link .` again.",
            output::clean_path(&path)
        );
    }

    // A repository in a scratch location is scratch by definition: a test fixture, a
    // `git clone` into `mktemp -d`, a plugin manager's checkout, a build step. The hook
    // fires on its first commit, the directory is gone minutes later, and the registry
    // keeps an entry that can never be pruned and never be found again. Registering those
    // is how a registry fills with dead paths. An explicit `devp link` still works — this
    // only declines to do it *unasked*.
    if quiet && is_ephemeral_location(&path) {
        return Ok(());
    }

    // A config that does not parse also keeps the hook out. It may well be the file that
    // says `disable_hooks`, and a broken one is not licence to register the repo anyway.
    if quiet
        && !matches!(
            PerRepoConfig::load_with_diagnostics(&path),
            Ok(None)
                | Ok(Some(PerRepoConfig {
                    disable_hooks: false,
                    ..
                }))
        )
    {
        return Ok(());
    }

    let mut registry = Registry::load()?;

    if registry.add_repo(path.clone()) {
        let adoption = registry.adopt_moved_entry(&path, scanner::git::repo_identity(&path));
        registry.last_added_repos = vec![path.clone()];
        registry.save()?;
        if !quiet {
            output::print_success(&format!("Linked: {}", output::clean_path(&path)));
            report_adoption(&adoption);
            if registry.settings.auto_config {
                ensure_default_repo_config(&path);
            }
        }
    } else {
        // Backfill only when it is missing. The global Git hook runs this on every
        // commit, and shelling out to git plus rewriting the registry each time would
        // be a real cost for a value that never changes once written.
        if registry.needs_identity(&path) {
            let adoption = registry.adopt_moved_entry(&path, scanner::git::repo_identity(&path));
            registry.save()?;
            if !quiet {
                report_adoption(&adoption);
            }
        }
        if !quiet {
            output::print_info(&format!("Already linked: {}", output::clean_path(&path)));
        }
    }

    Ok(())
}

/// Say when a registration recognised a repository that had moved.
///
/// Silence here would be worse than noise: the entry the user was staring at in
/// `devp status` as `Path missing` has just disappeared, and its lifetime total has
/// turned up on a different row. That is the right outcome, but only if it is stated.
pub(crate) fn report_adoption(adoption: &Adoption) {
    match adoption {
        Adoption::Nothing => {}
        Adoption::Moved(old) => output::print_info(&format!(
            "  Recognised as the repository registered at {} — that path is gone, so its \
             prune history came with it.",
            output::clean_path(old)
        )),
        Adoption::Ambiguous => output::print_warning(
            "  More than one missing repository shares this root commit, so none was \
             adopted — they are clones, not a move. Clear them with `devp unlink --missing`.",
        ),
    }
}

/// Write a default `.devprune.json` into a newly registered repository, when
/// `auto_config` asks for it.
///
/// Never over an existing file — broken or not, it is the user's — and a write failure
/// is a note rather than a failed registration: the repository is tracked either way,
/// the config was only ever a convenience.
pub(crate) fn ensure_default_repo_config(path: &Path) {
    if path.join(crate::constants::PER_REPO_CONFIG_FILE).exists() {
        return;
    }
    match PerRepoConfig::default().save_to_repo(path) {
        Ok(()) => output::print_info(&format!(
            "auto_config: wrote a default `.devprune.json` in {}",
            output::clean_path(path)
        )),
        Err(e) => output::print_warning(&format!(
            "auto_config: could not write `.devprune.json` in {}: {e}",
            output::clean_path(path)
        )),
    }
}

/// Register the repository the caller is standing in, when nothing has registered it yet.
///
/// Git has no `post-init` hook. The three hooks dev-prune installs — `post-commit`,
/// `post-checkout` and `post-merge` — between them cover every way a repository arrives
/// from somewhere else, and none of them covers one created here: `git init` runs no hook
/// at all, and the first hook a new repository ever sees belongs to its first commit.
/// Until then it is invisible to the mechanism whose whole job was to find it — which is
/// precisely when somebody runs `devp status` to check whether that worked, sees nothing,
/// and concludes the hooks are broken. They are not; there was never a hook to fire.
///
/// So the commands that read the registry check the working directory first. The guards
/// are the hook's guards, deliberately: a throwaway checkout, a repository whose config
/// sets `disable_hooks`, a config that does not parse — every case
/// `devp link . --quiet` declines to register is declined here for the same reason, so
/// this can never track something the hook would have left alone.
///
/// Returns the path when one was added, so the caller can say so. Persisting is the
/// caller's: it holds the registry and already saves for its own reasons.
pub fn adopt_enclosing_repo(registry: &mut Registry) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    adopt_repo_at(registry, &cwd)
}

/// [`adopt_enclosing_repo`], against a named directory rather than the process's own.
///
/// Split out so the guards can be tested without a test changing the working directory,
/// which is process-global and would race every other test in the binary.
pub(crate) fn adopt_repo_at(registry: &mut Registry, start: &Path) -> Option<PathBuf> {
    let path = enclosing_repo(start)?;

    if is_ephemeral_location(&path) {
        return None;
    }

    if !matches!(
        PerRepoConfig::load_with_diagnostics(&path),
        Ok(None)
            | Ok(Some(PerRepoConfig {
                disable_hooks: false,
                ..
            }))
    ) {
        return None;
    }

    if !registry.add_repo(path.clone()) {
        return None;
    }

    registry.adopt_moved_entry(&path, scanner::git::repo_identity(&path));
    Some(path)
}

/// Say that the working directory was just registered, and why nothing had done it.
///
/// The "why" is not padding. Somebody who ran `git init` and then `devp status` has
/// already formed the theory that the hooks are broken, and a bare "Registered ..." line
/// leaves that theory standing. One sentence replaces it with the truth.
pub(crate) fn report_cwd_adoption(path: &Path) {
    output::print_success(&format!("Registered {}", output::clean_path(path)));
    output::print_info(
        "  You are standing in it and nothing had tracked it yet. `git init` runs no Git \
         hook, so a repository created since the last pass stays unseen until its first \
         commit — found here instead.",
    );
}

/// The Git repository `start` is inside, if any.
///
/// Walks up rather than testing `start` alone: `devp status` from `src/` in a new
/// repository is the same question as running it from the root, and answering it only at
/// the root would leave the gap open for everyone who does not happen to be standing
/// there.
fn enclosing_repo(start: &Path) -> Option<PathBuf> {
    start
        .canonicalize()
        .ok()?
        .ancestors()
        .find(|dir| scanner::is_git_repo(dir))
        .map(Path::to_path_buf)
}

/// Is this directory called something only a tool would call a checkout?
///
/// See [`constants::EPHEMERAL_REPO_PREFIXES`] for why the match is a narrow prefix.
fn has_ephemeral_name(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        constants::EPHEMERAL_REPO_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
    })
}

/// Is `path` somewhere a tool keeps disposable checkouts?
///
/// `path` is expected to be canonical already; the temp directory is canonicalised here
/// because macOS reports it as `/var/folders/…`, a symlink to `/private/var/folders/…`.
/// A temp directory that cannot be resolved is treated as no match: declining to register
/// a real workspace would be the worse error of the two.
fn is_ephemeral_location(path: &Path) -> bool {
    if has_ephemeral_name(path) {
        return true;
    }
    let under_temp = std::env::temp_dir()
        .canonicalize()
        .is_ok_and(|tmp| path.starts_with(tmp));
    if under_temp {
        return true;
    }
    is_under_ephemeral_ancestor(path, None)
}

/// The ancestor half of [`is_ephemeral_location`], stopping at `root`.
///
/// `devp init <dir>` names a directory outright, and second-guessing the path somebody
/// typed is not this function's job — `devp init ~/.cache/things` must still find the
/// repositories in it. So when a scan root is given, only the directories *below* it are
/// examined. Without a root, every ancestor is.
fn is_under_ephemeral_ancestor(path: &Path, root: Option<&Path>) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    parent
        .ancestors()
        .take_while(|ancestor| root != Some(*ancestor))
        .any(|ancestor| {
            ancestor.file_name().is_some_and(|name| {
                constants::EPHEMERAL_ANCESTORS.contains(&&*name.to_string_lossy())
            })
        })
}

/// Would registering `repo`, found by scanning `root`, be registering a throwaway?
///
/// The check `devp init` applies. One `devp init` in a home directory added twenty-eight
/// plugin-manager checkouts to a real registry, every one of which was deleted within the
/// week — leaving twenty-eight `Path missing` rows on the dashboard and no way to tell
/// them apart from a workspace that was genuinely lost.
pub(crate) fn is_throwaway_checkout(root: &Path, repo: &Path) -> bool {
    has_ephemeral_name(repo) || is_under_ephemeral_ancestor(repo, Some(root))
}

/// Run `unlink --missing`: drop every registered path that no longer exists.
///
/// Nothing on disk is touched — the directories are already gone. Registries accumulate
/// these from clones that were deleted, drives that were reformatted, and workspaces that
/// were moved; `devp doctor` counts them and sends the user here rather than printing one
/// `devp unlink` line per entry.
pub fn run_unlink_missing() -> Result<()> {
    let mut registry = Registry::load()?;

    let gone: Vec<_> = registry
        .repositories
        .keys()
        .filter(|p| !p.exists())
        .cloned()
        .collect();

    if gone.is_empty() {
        output::print_success("Every registered repository still exists — nothing to remove.");
        return Ok(());
    }

    for path in &gone {
        registry.remove_repo(path);
        output::print_info(&format!("Unlinked: {}", output::clean_path(path)));
    }
    // `undo` reverts the last `init`/`link` by unregistering what it added. A path in that
    // list that no longer exists can never be reverted into anything, so leaving it there
    // only sets `undo` up to report that it removed nothing.
    registry.last_added_repos.retain(|p| p.exists());
    registry.save()?;

    output::print_success(&format!(
        "Removed {} registry {} pointing at directories that no longer exist.",
        gone.len(),
        output::plural(gone.len(), "entry", "entries")
    ));
    Ok(())
}

/// Run the `unlink` command — unregister a repository.
pub fn run_unlink(path_str: &str) -> Result<()> {
    // A deleted directory still has to be removable from the registry, so an
    // uncanonicalisable path falls back to what the user typed rather than failing.
    let path = Path::new(path_str)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(path_str).to_path_buf());

    let mut registry = Registry::load()?;

    if registry.remove_repo(&path) {
        registry.save()?;
        output::print_success(&format!("Unlinked: {}", output::clean_path(&path)));
    } else {
        output::print_warning(&format!("Not in registry: {}", output::clean_path(&path)));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_repository_under_the_temp_directory_is_recognised_as_scratch() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        assert!(
            is_ephemeral_location(&repo),
            "{} should be seen as scratch — TempDir builds under std::env::temp_dir()",
            repo.display()
        );
    }

    #[test]
    fn a_repository_outside_the_temp_directory_is_not() {
        // The crate's own source tree: a real workspace by any definition.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        assert!(!is_ephemeral_location(&here));
    }

    #[test]
    fn a_plugin_managers_checkout_is_recognised_as_scratch() {
        // The shape that filled a real registry: an agent plugin manager clones into
        // `~/.claude/plugins/cache/temp_git_<id>`, nowhere near the OS temp directory.
        let home = Path::new(env!("CARGO_MANIFEST_DIR"));
        let clone = home
            .join(".claude")
            .join("plugins")
            .join("cache")
            .join("temp_git_1787245534782_8o55r2");
        assert!(is_ephemeral_location(&clone));
    }

    #[test]
    fn a_project_of_that_name_is_still_a_project() {
        // Only ancestors are matched. A repository *called* `cache` is somebody's work.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("cache");
        assert!(!is_ephemeral_location(&repo));
    }

    #[test]
    fn a_throwaway_clone_is_recognised_by_its_name_alone() {
        // The registry that motivated this held twenty-eight of these. The prefix has to
        // be enough on its own: not every tool is polite enough to put its scratch
        // checkouts under a directory called `cache`.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("temp_git_1787320293656");
        assert!(is_ephemeral_location(&repo));
        assert!(is_throwaway_checkout(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &repo
        ));
    }

    #[test]
    fn a_repository_merely_named_after_temporary_work_is_not() {
        // A prefix, not a substring, and the underscore-and-git shape is required: these
        // are all somebody's actual work.
        for name in [
            "temporary-fixes",
            "template-git",
            "my-temp-git-notes",
            "tempo",
        ] {
            let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
            assert!(!is_ephemeral_location(&repo), "{name} is a real repository");
        }
    }

    #[test]
    fn the_repository_you_are_standing_in_is_adopted_when_nothing_tracks_it() {
        // The `git init` gap, from the inside: a real repository (this crate's own),
        // a registry that has never heard of it, and a starting directory well below
        // the root — which is where people actually are when they run `devp status`.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let mut registry = Registry::default();

        let adopted = adopt_repo_at(&mut registry, &root.join("src").join("commands"));
        assert_eq!(adopted.as_deref(), Some(root.as_path()));
        assert_eq!(registry.repo_count(), 1);
    }

    #[test]
    fn a_repository_already_registered_is_not_adopted_twice() {
        // Every `devp status` would otherwise report registering the same repository,
        // and the caller would save the registry once per invocation for no change.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let mut registry = Registry::default();

        assert!(adopt_repo_at(&mut registry, &root).is_some());
        assert!(adopt_repo_at(&mut registry, &root).is_none());
        assert_eq!(registry.repo_count(), 1);
    }

    #[test]
    fn adoption_declines_everything_the_hook_declines() {
        // Symmetry is the whole safety argument: this path must never register something
        // `devp link . --quiet` would have left alone. A temp directory is the case that
        // is cheap to build — and the one every test fixture on the machine lives in.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();

        let mut registry = Registry::default();
        assert!(adopt_repo_at(&mut registry, &repo).is_none());
        assert_eq!(registry.repo_count(), 0);
    }

    #[test]
    fn a_directory_in_no_repository_at_all_is_left_alone() {
        // `devp status` from a home directory must not invent a repository, and must not
        // walk to the filesystem root looking for one that is not there.
        let tmp = TempDir::new().unwrap();
        let plain = tmp.path().canonicalize().unwrap();

        let mut registry = Registry::default();
        assert!(adopt_repo_at(&mut registry, &plain).is_none());
    }

    #[test]
    fn init_does_not_second_guess_the_directory_it_was_pointed_at() {
        // `devp init ~/.cache/things` names a directory outright. Refusing to scan it
        // because of its own name would make the command silently do nothing — but a
        // cache directory *below* the root is still a tool's doing.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("cache");
        let inside = root.join("project");
        assert!(!is_throwaway_checkout(&root, &inside));

        let deeper = root.join("nested").join("cache").join("project");
        assert!(is_throwaway_checkout(&root, &deeper));
    }
}
