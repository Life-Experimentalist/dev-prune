// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

//! Working out where the repositories are, without being told.
//!
//! `devp init <path>` has always registered a whole tree at once, but somebody has to
//! know which tree to name. That is a fine thing to ask of a person setting their own
//! machine up and a bad thing to ask of an assistant driving the tool on their behalf:
//! an agent that has to guess `~/Code` guesses wrong on the machine that keeps its work
//! on `D:\`, and the repositories it never found are the ones that go on wasting disk.
//!
//! So the roots are derived rather than guessed, from two sources that cost nothing:
//!
//! 1. **The neighbourhood.** People keep repositories next to each other. The parent of
//!    every registered repository is therefore a place where more of them probably live,
//!    and rescanning those parents is how registering one project ends up finding the
//!    rest of the workspace around it. This is the source that works on any layout,
//!    including drives and directory names no list could have anticipated.
//! 2. **The working directory.** Bare `devp init` already scans `.`, so the workspace the
//!    command was run from is evidence that costs nothing in surprise — and it is the only
//!    evidence there is on a cold start where the code lives on a second drive.
//! 3. **The conventions.** A machine with an empty registry has no neighbourhood yet, so
//!    [`constants::CODE_ROOT_NAMES`] is probed by name under the home directory. This is
//!    only ever the bootstrap: one registered repository anywhere makes source 1 better
//!    than this one will ever be.
//!
//! Discovery registers. It does not delete, and nothing here shortens the distance
//! between "registered" and "pruned": a discovered repository still has to go idle, still
//! has to have every candidate directory proved recoverable by a lockfile, and still has
//! to clear all seven safety invariants before anything is removed. That is what makes
//! registering by itself a safe thing to do — the worst outcome of a wrong guess is a row
//! in `devp status`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::{Registry, canonical_key};
use crate::{constants, scanner};

/// What one discovery pass found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Repositories that are not in the registry and carry no opt-out.
    pub found: Vec<PathBuf>,
    /// Repositories skipped because they hold an `ignore.devprune.json`.
    ///
    /// Counted rather than listed, and counted rather than dropped silently: a discovery
    /// pass that says "found nothing" on a machine where fifty repositories opted out
    /// reads as a broken scan.
    pub opted_out: usize,
    /// Repositories skipped for being a package manager's disposable checkout.
    pub throwaway: usize,
    /// The directories that were actually walked.
    pub roots: Vec<PathBuf>,
}

/// The workspace the command was run from.
///
/// Bare `devp init` already scans `.`, so including it costs nothing in surprise and
/// closes the cold-start case the other two sources cannot: a machine whose code lives on
/// a second drive has nothing under `~` to probe and no registered repository to work
/// outwards from, and `~/Code` does not exist to be found. Standing in the workspace is
/// the one piece of evidence available at that point, so the *parent* of the enclosing
/// repository is used — that is the workspace, where its siblings are.
fn working_directory_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let root = if scanner::is_git_repo(&cwd) {
        cwd.parent()?.to_path_buf()
    } else {
        cwd
    };
    is_scannable_root(&root).then_some(root)
}

/// The home-relative conventional roots that exist on this machine.
fn conventional_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    constants::CODE_ROOT_NAMES
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect()
}

/// The parent directory of every registered repository, where that parent is deep enough
/// to be a workspace rather than a home directory or a drive root.
pub fn neighbourhood_roots(registry: &Registry) -> Vec<PathBuf> {
    registry
        .repositories
        .keys()
        .filter_map(|repo| repo.parent())
        .filter(|parent| is_scannable_root(parent))
        .map(Path::to_path_buf)
        .collect()
}

/// Whether a directory is specific enough to be walked.
///
/// The depth floor is the guard that matters. A repository cloned straight into `~` has
/// the home directory as its parent, and scanning that means walking every cache and
/// application-support tree on the machine — so one repository in an unusual place would
/// silently turn a cheap pass into a full-disk crawl. A directory that is also the home
/// directory is refused outright for the same reason, however deep it happens to sit.
fn is_scannable_root(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    if dirs::home_dir().is_some_and(|home| home == path) {
        return false;
    }
    path.components().count() > constants::MIN_DISCOVERY_ROOT_DEPTH
}

/// Every root worth walking, with the ones contained in another root removed.
///
/// Without the containment pass, a registry holding `~/Code/api` and `~/Code/api/web`
/// would walk `~/Code` twice over — once per parent — and a machine with fifty
/// repositories in one tree would walk it fifty times.
pub fn candidate_roots(registry: &Registry) -> Vec<PathBuf> {
    let mut roots = neighbourhood_roots(registry);
    roots.extend(conventional_roots());
    roots.extend(working_directory_root());
    dedupe_nested(roots)
}

/// Drop every root that lies inside another root in the same set.
///
/// Canonicalising first is not cosmetic on Windows: registry keys go through
/// [`canonical_key`] and come back in `\\?\` verbatim form, while `dirs::home_dir()` does
/// not, so `\\?\C:\Users\me\Code\api` and `C:\Users\me\Code` share no textual prefix and
/// the tree would be walked twice.
fn dedupe_nested(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    // Sorting first is what makes one pass enough: any container of a path sorts before
    // it, so the shortest enclosing root is always already in `kept` by the time a path
    // inside it is considered.
    let sorted: BTreeSet<PathBuf> = roots.iter().map(|r| canonical_key(r)).collect();
    let mut kept: Vec<PathBuf> = Vec::new();
    for root in sorted {
        if !kept.iter().any(|k| root.starts_with(k)) {
            kept.push(root);
        }
    }
    kept
}

/// Look for repositories this registry does not know about.
///
/// Nothing is registered here and the registry is not written — the caller decides what
/// to do with the result, which is what lets `--dry-run` and the scheduled pass share one
/// implementation.
pub fn discover(registry: &Registry) -> Result<Discovery> {
    discover_in(candidate_roots(registry), registry)
}

/// The body of [`discover`], with the roots supplied rather than derived.
///
/// Separated so tests can walk a temporary directory. A test that called [`discover`]
/// would walk whatever `~/Code` happens to hold on the machine running it, which is both
/// slow and a different answer on every machine.
pub fn discover_in(roots: Vec<PathBuf>, registry: &Registry) -> Result<Discovery> {
    let mut result = Discovery {
        roots: dedupe_nested(roots),
        ..Discovery::default()
    };

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for root in &result.roots {
        // One unreadable root must not sink the pass. A drive that has gone away since it
        // was last registered is the ordinary case, not an exceptional one.
        let Ok(repos) = scanner::scan_for_repos(root) else {
            continue;
        };
        for repo in repos {
            if !seen.insert(repo.clone()) {
                continue;
            }
            // Compared through `canonical_key` because that is the form `add_repo` stores:
            // a root reached through a symlink would otherwise spell an already-registered
            // repository a second way and offer it as a new find on every pass.
            if registry.repositories.contains_key(&canonical_key(&repo)) {
                continue;
            }
            if crate::commands::link::is_throwaway_checkout(root, &repo) {
                result.throwaway += 1;
                continue;
            }
            // The opt-out is honoured *before* registration, not just before deletion.
            // A repository whose owner has said no should not appear in the registry at
            // all — otherwise every `devp status` lists rows the user already declined.
            if repo.join(constants::DEVPRUNE_IGNORE_FILE).exists() {
                result.opted_out += 1;
                continue;
            }
            result.found.push(repo);
        }
    }

    result.found.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_repo(at: &Path) {
        fs::create_dir_all(at.join(".git")).expect("repo");
    }

    /// The neighbourhood is the point: one registered repository should make its
    /// siblings discoverable without anyone naming the directory they share.
    #[test]
    fn a_registered_repository_makes_its_siblings_discoverable() {
        let tmp = TempDir::new().expect("temp");
        let workspace = tmp.path().join("a").join("b").join("workspace");
        let known = workspace.join("known");
        let sibling = workspace.join("sibling");
        make_repo(&known);
        make_repo(&sibling);

        let mut registry = Registry::default();
        registry.add_repo(known.clone());

        let roots = neighbourhood_roots(&registry);
        let found = discover_in(roots, &registry).expect("discovery");
        assert!(
            found.found.iter().any(|p| p.ends_with("sibling")),
            "{:?}",
            found.found
        );
        assert!(
            !found.found.iter().any(|p| p.ends_with("known")),
            "an already registered repository is not a find: {:?}",
            found.found
        );
    }

    /// The opt-out has to be honoured at registration, or declining a repository only
    /// means seeing it in every status listing instead of pruning it.
    #[test]
    fn a_repository_that_opted_out_is_never_offered() {
        let tmp = TempDir::new().expect("temp");
        let workspace = tmp.path().join("a").join("b").join("workspace");
        let known = workspace.join("known");
        let declined = workspace.join("declined");
        make_repo(&known);
        make_repo(&declined);
        fs::write(declined.join(constants::DEVPRUNE_IGNORE_FILE), "{}").expect("opt-out");

        let mut registry = Registry::default();
        registry.add_repo(known);

        let roots = neighbourhood_roots(&registry);
        let found = discover_in(roots, &registry).expect("discovery");
        assert_eq!(found.opted_out, 1);
        assert!(
            !found.found.iter().any(|p| p.ends_with("declined")),
            "{:?}",
            found.found
        );
    }

    /// A repository sitting directly in the home directory must not turn the pass into a
    /// walk of the entire home directory.
    #[test]
    fn the_home_directory_is_never_a_scan_root() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert!(!is_scannable_root(&home));
    }

    /// Roots contained in another root are dropped, so a tree is walked once however
    /// many of its repositories are registered.
    #[test]
    fn a_root_inside_another_root_is_not_walked_twice() {
        let tmp = TempDir::new().expect("temp");
        let outer = tmp.path().join("a").join("b").join("outer");
        let inner = outer.join("nested").join("deeper");
        make_repo(&outer.join("one"));
        make_repo(&inner.join("two"));

        let mut registry = Registry::default();
        registry.add_repo(outer.join("one"));
        registry.add_repo(inner.join("two"));

        let roots = dedupe_nested(neighbourhood_roots(&registry));
        let outer = canonical_key(&outer);
        let under_outer: Vec<_> = roots.iter().filter(|r| r.starts_with(&outer)).collect();
        assert_eq!(under_outer.len(), 1, "{roots:?}");
    }

    /// A shallow directory is refused however real it is, because the floor is what keeps
    /// a drive root from being scanned.
    #[test]
    fn a_directory_too_close_to_the_filesystem_root_is_refused() {
        let shallow = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        assert!(!is_scannable_root(&shallow));
    }
}
