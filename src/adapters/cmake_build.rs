// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// CMake build-tree adapter.
//
// This is the adapter for the question "is that `build/` yours or CMake's?", and the
// answer is never the directory's name. CMake writes a `CMakeCache.txt` at the top of
// every build tree it configures and nobody writes one by hand; that file records
// `CMAKE_HOME_DIRECTORY`, the source directory it was configured from, precisely so a
// build tree can find its own sources again. So a directory is claimed only when it
// carries a cache file whose recorded source directory still exists, still holds a
// `CMakeLists.txt`, and sits inside this repository. A hand-made `build/` full of
// someone's own artefacts has no cache file and is never touched.
//
// Opt-in, and held to `build_idle_days`: a build tree is object files and linked
// binaries, and `cmake -S . -B <dir> && cmake --build <dir>` puts it back by compiling,
// which for a C++ project of any size is the most expensive rebuild dev-prune can ask
// for.
//
// The search stops descending the moment it finds a cache, so the sub-builds
// `FetchContent` and CPM leave in `build/_deps/` are never claimed separately — they go
// with the tree that owns them. It reaches three levels down rather than one because
// Visual Studio's CMake integration configures into `out/build/<preset>/`, and it only
// steps past a directory that holds a handful of subdirectories and nothing else, which
// is what an out-of-source container looks like and what a dependency tree never does.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

/// The file CMake writes at the top of a build tree. Its presence is the whole proof.
const CMAKE_CACHE: &str = "CMakeCache.txt";

/// The cache entry naming the source tree this build tree was configured from.
const HOME_DIRECTORY_KEY: &str = "CMAKE_HOME_DIRECTORY";

/// How far below the project root a build tree is looked for.
///
/// Three, because `out/build/<preset>/CMakeCache.txt` is what Visual Studio produces.
const MAX_DEPTH: usize = 3;

/// The most entries a directory may hold and still be walked past.
///
/// A container in an out-of-source layout holds `build/`, sometimes `install/`, and
/// nothing else. Anything wider is a real directory of content, and walking into it
/// costs a `read_dir` per child for a build tree that is not there.
const MAX_CONTAINER_ENTRIES: usize = 8;

/// CMake build-tree adapter. Opt-in; see the module comment.
pub struct CmakeBuild;

/// The value of a `KEY:TYPE=VALUE` cache entry, whatever the type turns out to be.
fn cache_entry(cache: &str, key: &str) -> Option<String> {
    cache.lines().find_map(|line| {
        let (name, rest) = line.trim().split_once(':')?;
        if name != key {
            return None;
        }
        rest.split_once('=').map(|(_, v)| v.trim().to_string())
    })
}

/// Whether `candidate` is a build tree configured from somewhere inside `project`.
///
/// Both sides are canonicalised before they are compared: CMake records the source
/// directory with forward slashes on every platform, and on Windows the drive letter and
/// path casing it wrote are not necessarily the ones on disk.
fn belongs_to(candidate: &Path, project: &Path) -> bool {
    let Ok(cache) = fs::read_to_string(candidate.join(CMAKE_CACHE)) else {
        return false;
    };
    let Some(home) = cache_entry(&cache, HOME_DIRECTORY_KEY) else {
        return false;
    };
    let home = PathBuf::from(home);
    // A recorded source directory that is gone, or that no longer holds the file CMake
    // read, cannot rebuild anything — whatever this tree is, it is not recoverable here.
    if !home.join("CMakeLists.txt").is_file() {
        return false;
    }
    match (fs::canonicalize(&home), fs::canonicalize(project)) {
        (Ok(home), Ok(project)) => home.starts_with(&project),
        _ => false,
    }
}

/// Whether the walk should step past `dir` looking for a build tree deeper down.
fn is_container(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0;
    for entry in entries.flatten() {
        count += 1;
        if count > MAX_CONTAINER_ENTRIES || !entry.path().is_dir() {
            return false;
        }
    }
    count > 0
}

/// Build trees under `project`, never `project` itself.
///
/// An in-source build puts `CMakeCache.txt` at the top of the repository, and the
/// repository is not something this adapter can ever be allowed to claim, so the walk
/// starts one level down.
fn find_build_trees(project: &Path, dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if path.join(CMAKE_CACHE).is_file() {
            // Stop here rather than descending: `_deps/` sub-builds belong to the tree
            // that configured them and are deleted with it.
            if belongs_to(&path, project) {
                found.push(path);
            }
            continue;
        }
        if is_container(&path) {
            find_build_trees(project, &path, depth + 1, found);
        }
    }
}

impl PackageManager for CmakeBuild {
    fn name(&self) -> &'static str {
        "cmake_build"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("CMakeLists.txt").is_file()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut found = Vec::new();
        find_build_trees(path, path, 1, &mut found);
        found.sort();
        found
            .into_iter()
            .map(|tree| BloatDir {
                name: tree
                    .strip_prefix(path)
                    .unwrap_or(&tree)
                    .to_string_lossy()
                    .replace('\\', "/"),
                size_bytes: dir_size(&tree),
                path: tree,
                shared_bytes: 0,
            })
            .collect()
    }

    /// The per-directory proof already ran: [`find_build_trees`] claims a directory only
    /// after its own `CMakeCache.txt` names a source tree inside this project. What is
    /// left to check is that the top-level `CMakeLists.txt` is still readable and is
    /// still a CMake script, because that is the file `cmake` is about to be pointed at.
    /// Re-running `cmake` here to find out would configure a build tree in the middle of
    /// a delete pass, which is the opposite of what was asked for.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join("CMakeLists.txt");
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!(
                "`CMakeLists.txt` could not be read ({e}) — nothing to reconfigure the build \
                 tree from."
            )
        })?;
        let lowered = content.to_ascii_lowercase();
        if !lowered.contains("cmake_minimum_required") && !lowered.contains("project(") {
            return Err(anyhow!(
                "`CMakeLists.txt` declares neither `cmake_minimum_required` nor `project()` — \
                 refusing to treat the build tree as reconfigurable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "CMake build tree will regenerate on the next `cmake -S . -B <dir> && cmake --build <dir>`"
        );
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["CMakeLists.txt"]
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A project root with a top-level `CMakeLists.txt`, canonicalised so the paths the
    /// cache files record match what `fs::canonicalize` returns for them on macOS, where
    /// the temp directory is reached through a symlink.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.20)\nproject(demo)\n",
        )
        .unwrap();
        fs::canonicalize(dir).unwrap()
    }

    /// A build tree at `at`, recording `home` as the source directory it came from.
    fn build_tree(at: &Path, home: &Path) {
        fs::create_dir_all(at).unwrap();
        fs::write(
            at.join(CMAKE_CACHE),
            format!(
                "# This is the CMakeCache file.\nCMAKE_BUILD_TYPE:STRING=Debug\n{HOME_DIRECTORY_KEY}:INTERNAL={}\n",
                home.display().to_string().replace('\\', "/")
            ),
        )
        .unwrap();
        fs::write(at.join("build.ninja"), "# generated").unwrap();
    }

    fn claimed(project: &Path) -> Vec<String> {
        CmakeBuild
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_top_level_cmakelists() {
        let dir = tempdir().unwrap();
        assert!(!CmakeBuild.detect(dir.path()));
        project(dir.path());
        assert!(CmakeBuild.detect(dir.path()));
    }

    #[test]
    fn a_cache_file_is_what_separates_cmakes_build_from_yours() {
        // The whole point of the adapter: two directories that both look like output,
        // one configured by CMake and one somebody made, and only the first is claimed.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_tree(&root.join("build"), &root);
        fs::create_dir(root.join("output")).unwrap();
        fs::write(root.join("output").join("notes.txt"), "hand made").unwrap();

        assert_eq!(claimed(&root), vec!["build"]);
    }

    #[test]
    fn a_build_tree_configured_from_somewhere_else_is_refused() {
        // Someone's build tree for another checkout, parked inside this repository. It
        // is recoverable, but not from anything here, so this adapter does not own it.
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        let root = project(dir.path());
        let elsewhere = project(other.path());
        build_tree(&root.join("build"), &elsewhere);

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_cache_pointing_at_a_vanished_source_tree_is_refused() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_tree(&root.join("build"), &root.join("gone"));

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn an_in_source_build_never_claims_the_repository() {
        // `cmake .` writes the cache file at the top of the repository. Deleting that
        // directory would delete the project, so the walk starts one level down.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_tree(&root, &root);

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn the_visual_studio_layout_is_found_three_levels_down() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_tree(&root.join("out").join("build").join("x64-Debug"), &root);

        assert_eq!(claimed(&root), vec!["out/build/x64-Debug"]);
    }

    #[test]
    fn a_wide_directory_is_not_walked_into() {
        // A dependency tree is not an out-of-source container, and walking one costs a
        // `read_dir` per package for a build tree that is not in there.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        let modules = root.join("node_modules");
        for i in 0..MAX_CONTAINER_ENTRIES + 2 {
            fs::create_dir_all(modules.join(format!("pkg{i}"))).unwrap();
        }
        build_tree(&modules.join("pkg0").join("build"), &root);

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_sub_build_goes_with_the_tree_that_configured_it() {
        // `FetchContent` and CPM configure their dependencies inside `build/_deps/`.
        // Claiming those separately would delete parts of a tree that is being deleted
        // anyway, and report the same bytes twice.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_tree(&root.join("build"), &root);
        build_tree(&root.join("build").join("_deps").join("fmt-build"), &root);

        assert_eq!(claimed(&root), vec!["build"]);
    }

    #[test]
    fn every_configured_tree_is_claimed() {
        // Debug and Release side by side is the normal way to work.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_tree(&root.join("cmake-build-debug"), &root);
        build_tree(&root.join("cmake-build-release"), &root);

        assert_eq!(
            claimed(&root),
            vec!["cmake-build-debug", "cmake-build-release"]
        );
    }

    #[test]
    fn a_missing_or_bogus_cmakelists_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(CmakeBuild.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join("CMakeLists.txt"), "hello there").unwrap();
        assert!(CmakeBuild.enforce_lockfile(dir.path(), policy).is_err());
        // CMake commands are case-insensitive, and plenty of older projects shout them.
        fs::write(
            dir.path().join("CMakeLists.txt"),
            "CMAKE_MINIMUM_REQUIRED(VERSION 3.20)\nPROJECT(demo)\n",
        )
        .unwrap();
        assert!(CmakeBuild.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn cache_entries_are_read_whatever_their_type_is() {
        let cache = "CMAKE_HOME_DIRECTORY:INTERNAL=/src/proj\nOTHER:BOOL=ON\n";
        assert_eq!(
            cache_entry(cache, HOME_DIRECTORY_KEY),
            Some("/src/proj".to_string())
        );
        assert_eq!(cache_entry(cache, "MISSING"), None);
        assert_eq!(
            cache_entry("CMAKE_HOME_DIRECTORY\n", HOME_DIRECTORY_KEY),
            None
        );
    }

    #[test]
    fn cmake_build_is_opt_in() {
        assert!(CmakeBuild.opt_in());
    }
}
