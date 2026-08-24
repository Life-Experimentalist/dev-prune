// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Workspace discovery — finds every package-manager project inside a repository.
//
// A repository is not necessarily one project. A monorepo can carry `frontend/` on
// pnpm, `services/api/` on uv, and `cli/` on cargo, or hold all three manifests side
// by side in the root. This module walks the repo once and reports every directory
// where at least one adapter applies, so the engine can prune, verify and restore each
// of them independently.
//
// The walk deliberately never descends into the directories it is looking for. A
// `node_modules` tree contains thousands of nested `package.json` files, each of which
// would otherwise register as its own project.

use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use crate::adapters::{self, PackageManager};

/// The depth [`discover`] uses when no configuration says otherwise.
///
/// Re-exported from [`crate::constants`] so callers that only need the default do not
/// have to reach past this module for it.
pub const MAX_DEPTH: usize = crate::constants::DEFAULT_SCAN_DEPTH;

/// The depth to walk `repo_root` with, given the global setting.
///
/// Resolution order is the same one every other tunable follows: the repository's own
/// `.devprune.json` wins over the global setting, which wins over the default. A config
/// that will not parse is *not* consulted — the caller reports that repository as a
/// `config_error` and never gets here — so this deliberately looks only at a config it
/// could read.
pub fn resolve_depth(repo_root: &Path, global: usize) -> usize {
    let configured = crate::config::PerRepoConfig::load_with_diagnostics(repo_root)
        .ok()
        .flatten()
        .and_then(|c| c.scan_depth)
        .unwrap_or(global);
    clamp_depth(configured)
}

/// Hold a requested depth inside the range the walk can afford.
///
/// A zero would find nothing at all — not even the repository root, which is depth 0 in
/// `WalkDir` terms but only yields projects because the walk includes it — so it is
/// raised to 1 rather than silently pruning nothing. The ceiling keeps a mistyped
/// `scan_depth: 900` from turning a background pass into a full-disk crawl.
pub fn clamp_depth(requested: usize) -> usize {
    requested.clamp(1, crate::constants::MAX_SCAN_DEPTH_LIMIT)
}

/// Directory names that are never descended into.
///
/// Hidden directories, virtual environments and nested repositories are excluded
/// separately in [`is_scannable`] because they cannot be matched by name alone.
/// Several of these hold whole projects of their own: `deps/` is full of Elixir
/// packages with their own `mix.exs`, `.build/` of Swift checkouts with their own
/// `Package.swift`. Descending would register a dependency as a project and offer to
/// prune inside something the parent repository rebuilds wholesale.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "bower_components",
    "__pypackages__",
    "Pods",
    "deps",
    "_build",
    ".build",
];

/// A directory inside a repository that at least one package manager owns.
pub struct Project {
    /// Absolute path to the project directory.
    pub path: PathBuf,
    /// Path relative to the repository root, `/`-separated. `"."` for the root itself.
    pub relative: String,
    /// Adapters that apply here. More than one is normal — cargo and npm in the same
    /// directory own `target` and `node_modules` respectively.
    pub adapters: Vec<Box<dyn PackageManager>>,
}

/// Find every package-manager project in `repo_root`, including the root itself.
///
/// Walks to the default depth. Callers that have the user's settings to hand should use
/// [`discover_to_depth`] with [`resolve_depth`] instead.
///
/// Returns an empty vector when nothing in the tree is recognised.
pub fn discover(repo_root: &Path) -> Vec<Project> {
    discover_to_depth(repo_root, MAX_DEPTH)
}

/// [`discover`], to an explicit depth.
///
/// `depth` is clamped, so a caller cannot hand this an unbounded or useless walk even by
/// reading a hand-edited config straight off disk.
pub fn discover_to_depth(repo_root: &Path, depth: usize) -> Vec<Project> {
    discover_with(repo_root, depth, adapters::detect_adapters)
}

/// [`discover_to_depth`], counting managers the user has switched off for pruning too.
///
/// For the one caller that is asking which package managers a repository *uses* rather
/// than which ones a pass would act on: see [`adapters::detect_all_adapters`].
pub fn discover_all_to_depth(repo_root: &Path, depth: usize) -> Vec<Project> {
    discover_with(repo_root, depth, adapters::detect_all_adapters)
}

/// The walk both discovery functions share, parameterised only by which detector runs
/// at each directory.
fn discover_with(
    repo_root: &Path,
    depth: usize,
    detect: fn(&Path) -> Vec<Box<dyn PackageManager>>,
) -> Vec<Project> {
    WalkDir::new(repo_root)
        .follow_links(false)
        .max_depth(clamp_depth(depth))
        // Directory order otherwise comes from the filesystem, so the same repository
        // lists its projects in a different order on different machines — and so does
        // every prune summary and JSON document built from them.
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || is_scannable(entry))
        .flatten()
        .filter(|entry| entry.file_type().is_dir())
        .filter_map(|entry| {
            let adapters = detect(entry.path());
            if adapters.is_empty() {
                return None;
            }
            Some(Project {
                relative: relative_label(repo_root, entry.path()),
                path: entry.path().to_path_buf(),
                adapters,
            })
        })
        .collect()
}

/// Whether the walk should descend into this entry.
fn is_scannable(entry: &DirEntry) -> bool {
    // Symlinked directories report as symlinks with `follow_links(false)` and are never
    // descended, so only real directories need filtering. Files pass through untouched.
    if !entry.file_type().is_dir() {
        return true;
    }

    let name = entry.file_name().to_string_lossy();

    // `.git`, `.venv`, `.tox`, `.next`, `.turbo`, editor state — none of them hold
    // projects worth pruning, and all of them are expensive to walk.
    if name.starts_with('.') {
        return false;
    }

    if SKIP_DIRS.contains(&name.as_ref()) {
        return false;
    }

    let path = entry.path();

    // A virtual environment can be called anything; `pyvenv.cfg` is the marker. It is
    // pruneable output, not a project, and it contains a full package tree.
    if path.join("pyvenv.cfg").exists() {
        return false;
    }

    // Submodules and nested clones are separate repositories with their own activity
    // history and their own `.devprune.json`. They are registered and pruned in their
    // own right, never as part of their parent.
    if path.join(".git").exists() {
        return false;
    }

    true
}

/// Render `path` relative to `root` with forward slashes; `"."` when they are equal.
///
/// Used for every user-facing directory label so that `frontend/node_modules` reads the
/// same on Windows as on Linux, and so the interactive selector can address a specific
/// nested directory unambiguously.
pub fn relative_label(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create `dir` and drop the given files into it.
    fn project(root: &Path, rel: &str, files: &[&str]) -> PathBuf {
        let dir = if rel == "." {
            root.to_path_buf()
        } else {
            root.join(rel)
        };
        fs::create_dir_all(&dir).unwrap();
        for file in files {
            fs::write(dir.join(file), "{}").unwrap();
        }
        dir
    }

    fn names(projects: &[Project]) -> Vec<(String, Vec<&'static str>)> {
        let mut out: Vec<(String, Vec<&'static str>)> = projects
            .iter()
            .map(|p| {
                let mut adapters: Vec<&'static str> = p.adapters.iter().map(|a| a.name()).collect();
                adapters.sort_unstable();
                (p.relative.clone(), adapters)
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn discovers_nothing_in_an_empty_tree() {
        let tmp = TempDir::new().unwrap();
        assert!(discover(tmp.path()).is_empty());
    }

    #[test]
    fn discovers_three_ecosystems_in_one_root() {
        let tmp = TempDir::new().unwrap();
        project(
            tmp.path(),
            ".",
            &["package.json", "package-lock.json", "uv.lock", "go.mod"],
        );

        assert_eq!(
            names(&discover(tmp.path())),
            vec![(".".to_string(), vec!["go", "npm", "uv"])]
        );
    }

    #[test]
    fn discovers_ecosystems_at_different_depths() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), "frontend", &["pnpm-lock.yaml"]);
        project(tmp.path(), "services/api", &["uv.lock"]);
        project(tmp.path(), "tools/cli", &["go.mod"]);

        assert_eq!(
            names(&discover(tmp.path())),
            vec![
                ("frontend".to_string(), vec!["pnpm"]),
                ("services/api".to_string(), vec!["uv"]),
                ("tools/cli".to_string(), vec!["go"]),
            ]
        );
    }

    #[test]
    fn combines_a_root_project_with_nested_ones() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), ".", &["go.mod"]);
        project(tmp.path(), "web", &["package.json", "package-lock.json"]);

        assert_eq!(
            names(&discover(tmp.path())),
            vec![
                (".".to_string(), vec!["go"]),
                ("web".to_string(), vec!["npm"]),
            ]
        );
    }

    #[test]
    fn never_descends_into_node_modules() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), ".", &["package.json", "package-lock.json"]);
        // A dependency that ships its own lockfile must not become a project.
        project(
            tmp.path(),
            "node_modules/some-dep",
            &["package.json", "package-lock.json"],
        );

        assert_eq!(names(&discover(tmp.path())).len(), 1);
    }

    #[test]
    fn never_descends_into_target_or_vendor() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), ".", &["go.mod"]);
        project(tmp.path(), "target/debug/build/x", &["go.mod"]);
        project(tmp.path(), "vendor/dep", &["go.mod"]);

        assert_eq!(
            names(&discover(tmp.path())),
            vec![(".".to_string(), vec!["go"])]
        );
    }

    #[test]
    fn never_descends_into_a_virtual_environment() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), ".", &["uv.lock"]);
        let venv = project(tmp.path(), "my_env", &["pyvenv.cfg"]);
        project(&venv, "lib/site-packages/dep", &["go.mod"]);

        assert_eq!(
            names(&discover(tmp.path())),
            vec![(".".to_string(), vec!["uv"])]
        );
    }

    #[test]
    fn never_descends_into_a_nested_repository() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), ".", &["go.mod"]);
        let sub = project(tmp.path(), "submodule", &["package-lock.json"]);
        fs::create_dir(sub.join(".git")).unwrap();

        assert_eq!(
            names(&discover(tmp.path())),
            vec![(".".to_string(), vec!["go"])]
        );
    }

    #[test]
    fn never_descends_into_hidden_directories() {
        let tmp = TempDir::new().unwrap();
        project(tmp.path(), ".github/actions/thing", &["package-lock.json"]);
        assert!(discover(tmp.path()).is_empty());
    }

    #[test]
    fn stops_at_the_depth_cap() {
        let tmp = TempDir::new().unwrap();
        let deep = "a/b/c/d/e/f/g/h";
        project(tmp.path(), deep, &["Cargo.toml"]);
        assert!(discover(tmp.path()).is_empty());
    }

    #[test]
    fn relative_label_is_slash_separated() {
        let root = Path::new("/repo");
        assert_eq!(relative_label(root, Path::new("/repo")), ".");
        assert_eq!(
            relative_label(root, Path::new("/repo/a/b/node_modules")),
            "a/b/node_modules"
        );
    }
}
