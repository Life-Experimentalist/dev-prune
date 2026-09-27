// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Python bytecode and tool-cache adapter.
//
// Not opt-in: unlike `target/` or `build/`, none of these come back by recompiling.
// `__pycache__` refills the moment the module is imported again; `.pytest_cache`,
// `.mypy_cache` and `.ruff_cache` refill on the next run of their tool. They are pure
// caches in the same sense `node_modules` is, just far smaller individually: the space
// is in how many of them a large tree accumulates, not in any one of them.
//
// What it claims, anywhere under the repository: `__pycache__` (name alone is proof,
// since CPython reserves it), and `.pytest_cache` / `.mypy_cache` / `.ruff_cache` when
// they carry the `CACHEDIR.TAG` the cachedir spec defines, so a same-named directory a
// user created for something else is left alone. `.hypothesis` is deliberately not
// claimed: it holds regression examples a rebuild cannot reconstruct. `.tox` and `.nox`
// are not claimed either, since they are environments, not caches.
//
// A repository with no `.py` file anywhere has nothing to regenerate a bytecode or tool
// cache from, so `enforce_lockfile` refuses exactly like Gradle and Maven do: no command
// run, just the same structural proof the rest of the tree already relies on.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// The signature the [cachedir spec](https://bford.info/cachedir/) defines. A directory
/// carrying a `CACHEDIR.TAG` that starts with these bytes is safe for a backup tool, an
/// indexer, or this adapter to skip and delete without asking what's inside.
const CACHEDIR_TAG_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Whether `dir` carries a `CACHEDIR.TAG` matching the spec's signature.
fn has_cachedir_tag(dir: &Path) -> bool {
    std::fs::read(dir.join("CACHEDIR.TAG"))
        .is_ok_and(|bytes| bytes.starts_with(CACHEDIR_TAG_SIGNATURE))
}

/// Whether `entry` is one of the four caches this adapter claims.
///
/// `__pycache__` is proof by name alone: CPython reserves it for exactly one purpose.
/// The three dot-named tool caches also need the `CACHEDIR.TAG` confirmation, because
/// nothing stops a project from naming its own directory `.mypy_cache`.
fn is_target_cache_dir(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    match name.as_ref() {
        "__pycache__" => true,
        ".pytest_cache" | ".mypy_cache" | ".ruff_cache" => has_cachedir_tag(entry.path()),
        _ => false,
    }
}

/// The result of walking a repository once for every cache this adapter claims.
struct Scan {
    /// Every matching cache directory found, in walk order.
    caches: Vec<PathBuf>,
    /// Whether at least one `.py` file exists anywhere under the repository.
    has_py: bool,
}

/// Walk `repo_root` once, collecting every cache directory this adapter claims and
/// noting whether any Python source exists to regenerate them from.
///
/// Deliberately not `filter_entry`: a matching cache directory must still be *yielded*
/// (so it can be collected) while not being descended into, which `filter_entry` cannot
/// express. A cache-name match is checked before consulting
/// [`crate::workspace::is_scannable`], so this never descends into a `.venv`'s
/// `site-packages`, `node_modules`, `target/`, or a nested repository, the same rule
/// the general workspace walk already applies, reused rather than duplicated.
fn scan(repo_root: &Path) -> Scan {
    let mut caches = Vec::new();
    let mut has_py = false;
    let mut walk = WalkDir::new(repo_root).follow_links(false).into_iter();

    while let Some(entry) = walk.next() {
        let Ok(entry) = entry else { continue };

        // The repository root itself is never a cache directory or a source file.
        if entry.depth() == 0 {
            continue;
        }

        if entry.file_type().is_dir() {
            if is_target_cache_dir(&entry) {
                caches.push(entry.path().to_path_buf());
                walk.skip_current_dir();
            } else if !crate::workspace::is_scannable(&entry) {
                walk.skip_current_dir();
            }
            continue;
        }

        if !has_py
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
        {
            has_py = true;
        }
    }

    Scan { caches, has_py }
}

/// Adapter for Python bytecode and tool caches. Always on; see the module comment.
pub struct Pycache;

impl PackageManager for Pycache {
    fn name(&self) -> &'static str {
        "pycache"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join(".git").exists() && !scan(path).caches.is_empty()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        scan(path)
            .caches
            .into_iter()
            .map(|dir| {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                BloatDir {
                    name,
                    size_bytes: dir_size(&dir),
                    path: dir,
                    shared_bytes: 0,
                }
            })
            .collect()
    }

    /// Gradle's and Maven's proof, adapted: no source to compile from means no reason
    /// these caches exist at all. Runs nothing: the walk `scan` already did is enough.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        if !scan(path).has_py {
            return Err(anyhow!(
                "no `.py` file anywhere under the repository: nothing to regenerate a \
                 Python bytecode or tool cache from."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            "Python caches regenerate on their own: __pycache__ the next time these \
             modules are imported, .pytest_cache/.mypy_cache/.ruff_cache the next time \
             pytest, mypy or ruff runs"
        );
        Ok(())
    }

    fn restore_named(
        &self,
        _path: &Path,
        dir_name: &str,
        _runtime: Option<&str>,
        _timeout: std::time::Duration,
    ) -> Result<()> {
        let hint = match Path::new(dir_name).file_name().and_then(|n| n.to_str()) {
            Some("__pycache__") => "Python regenerates it the next time these modules are imported",
            Some(".pytest_cache") => "pytest regenerates it on its next run",
            Some(".mypy_cache") => "mypy regenerates it on its next run",
            Some(".ruff_cache") => "ruff regenerates it on its next run",
            _ => "it regenerates the next time the tool that made it runs again",
        };
        println!("{dir_name} will come back: {hint}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn tag(dir: &Path) {
        fs::write(
            dir.join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
    }

    #[test]
    fn test_name() {
        assert_eq!(Pycache.name(), "pycache");
    }

    #[test]
    fn pycache_is_not_opt_in() {
        assert!(!Pycache.opt_in());
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir(dir.path().join("__pycache__")).unwrap();
        assert!(Pycache.detect(dir.path()));
    }

    #[test]
    fn detect_requires_the_repository_root_not_just_a_cache() {
        // A nested directory that is not itself the repo root never detects, even if it
        // holds a cache; only `bloat_dirs` called at the true root finds it.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("__pycache__")).unwrap();
        assert!(!Pycache.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(!Pycache.detect(dir.path()));
    }

    #[test]
    fn claims_pycache_by_name_alone() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("pkg/__pycache__")).unwrap();

        let dirs = Pycache.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "__pycache__");
    }

    #[test]
    fn a_dot_mypy_cache_without_the_tag_is_left_alone() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        // A project's own directory that happens to share the name, with no tag inside.
        fs::create_dir(dir.path().join(".mypy_cache")).unwrap();

        assert!(Pycache.bloat_dirs(dir.path()).is_empty());
    }

    #[test]
    fn a_tagged_dot_mypy_cache_is_claimed() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let cache = dir.path().join(".mypy_cache");
        fs::create_dir(&cache).unwrap();
        tag(&cache);

        let dirs = Pycache.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".mypy_cache");
    }

    #[test]
    fn finds_every_cache_kind_anywhere_in_the_tree() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src/pkg/__pycache__")).unwrap();
        fs::create_dir_all(dir.path().join("tests/__pycache__")).unwrap();
        let pytest = dir.path().join(".pytest_cache");
        fs::create_dir(&pytest).unwrap();
        tag(&pytest);
        let ruff = dir.path().join(".ruff_cache");
        fs::create_dir(&ruff).unwrap();
        tag(&ruff);

        let mut names: Vec<String> = Pycache
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![".pytest_cache", ".ruff_cache", "__pycache__", "__pycache__"]
        );
    }

    #[test]
    fn never_descends_into_a_virtual_environment() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let venv = dir.path().join(".venv");
        fs::create_dir_all(venv.join("lib/site-packages/dep/__pycache__")).unwrap();
        fs::write(venv.join("pyvenv.cfg"), "").unwrap();

        // `.venv` is hidden, so it is already excluded before the site-packages cache
        // inside it is ever reached.
        assert!(Pycache.bloat_dirs(dir.path()).is_empty());
    }

    #[test]
    fn never_descends_into_node_modules_or_target() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/some-native-dep/__pycache__")).unwrap();
        fs::create_dir_all(dir.path().join("target/debug/__pycache__")).unwrap();

        assert!(Pycache.bloat_dirs(dir.path()).is_empty());
    }

    #[test]
    fn never_descends_into_a_nested_repository() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let nested = dir.path().join("vendored-tool");
        fs::create_dir_all(nested.join("__pycache__")).unwrap();
        fs::create_dir(nested.join(".git")).unwrap();

        assert!(Pycache.bloat_dirs(dir.path()).is_empty());
    }

    #[test]
    fn enforce_lockfile_refuses_with_no_python_source_anywhere() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir(dir.path().join("__pycache__")).unwrap();

        assert!(
            Pycache
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );

        fs::write(dir.path().join("main.py"), "").unwrap();
        assert!(
            Pycache
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn restore_named_matches_by_basename_not_the_whole_relative_path() {
        let dir = tempdir().unwrap();
        // The recorded name is repo-relative and slash-separated, e.g.
        // "src/pkg/__pycache__", so matching must key off the final component.
        assert!(
            Pycache
                .restore_named(
                    dir.path(),
                    "src/pkg/__pycache__",
                    None,
                    std::time::Duration::from_secs(1),
                )
                .is_ok()
        );
    }
}
