// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Zig build cache adapter.
//
// `zig build` keeps its incremental compilation state beside the project: `.zig-cache/`
// since Zig 0.12, `zig-cache/` before that, and `zig-out/` for the installed build
// artifacts. All three are written only by the build system, all three sit beside
// `build.zig`, and all three regenerate from `zig build` — which is why the standard
// Zig `.gitignore` excludes them. `zig-out/` is included deliberately: it is the build
// system's own install prefix, populated entirely by build steps `build.zig` declares,
// not a directory a person authors into.
//
// Opt-in, and held to `build_idle_days`: getting the caches back means recompiling the
// project, not downloading dependencies.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The build script at every Zig project's root.
const MANIFEST: &str = "build.zig";

/// The build system's directories: `.zig-cache/` is 0.12+, `zig-cache/` is older, and
/// `zig-out/` is the default install prefix build steps write into.
const CACHE_DIRS: &[&str] = &[".zig-cache", "zig-cache", "zig-out"];

/// Zig build cache adapter. Opt-in; see the module comment.
pub struct Zig;

impl PackageManager for Zig {
    fn name(&self) -> &'static str {
        "zig"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join(MANIFEST).is_file()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        CACHE_DIRS
            .iter()
            .map(|name| path.join(name))
            .filter(|dir| dir.is_dir())
            .map(|dir| BloatDir {
                name: dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                size_bytes: dir_size(&dir),
                path: dir,
                shared_bytes: 0,
            })
            .collect()
    }

    /// What is checked is that `build.zig` is still readable and still reads as a build
    /// script, because it is what `zig build` is about to execute. Every build script
    /// exports a `pub fn build` entry point — that is the contract the compiler calls
    /// into — and a file without one is whatever it is, not something that can rebuild
    /// these directories. Running `zig build` here to find out would start a compile in
    /// the middle of a delete pass.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let manifest = path.join(MANIFEST);
        let content = fs::read_to_string(&manifest).map_err(|e| {
            anyhow!("`{MANIFEST}` could not be read ({e}): nothing to rebuild the caches from.")
        })?;
        if !content.contains("pub fn build") {
            return Err(anyhow!(
                "`{MANIFEST}` has no `pub fn build` entry point: refusing to treat the \
                 build caches as regenerable from it."
            ));
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!("Zig's caches and outputs regenerate on the next `zig build`");
        Ok(())
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &[MANIFEST]
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A project root holding a plausible `build.zig`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join(MANIFEST),
            "const std = @import(\"std\");\n\npub fn build(b: *std.Build) void {\n    _ = b;\n}\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    fn claimed(project: &Path) -> Vec<String> {
        Zig.bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_the_build_script() {
        let dir = tempdir().unwrap();
        assert!(!Zig.detect(dir.path()));
        project(dir.path());
        assert!(Zig.detect(dir.path()));
    }

    #[test]
    fn caches_and_install_output_are_claimed() {
        // A project that lived through the 0.12 rename can carry both cache spellings,
        // and `zig-out` is the build system's install prefix, not the user's.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join(".zig-cache")).unwrap();
        fs::create_dir(root.join("zig-cache")).unwrap();
        fs::create_dir(root.join("zig-out")).unwrap();

        assert_eq!(claimed(&root), vec![".zig-cache", "zig-cache", "zig-out"]);
    }

    #[test]
    fn nothing_else_beside_the_manifest_is_claimed() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir(root.join("src")).unwrap();

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_missing_or_bogus_build_script_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(Zig.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join(MANIFEST), "hello there").unwrap();
        assert!(Zig.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(Zig.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn zig_is_opt_in() {
        assert!(Zig.opt_in());
    }
}
