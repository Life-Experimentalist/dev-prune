// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// .NET build-output adapter.
//
// This is the adapter for the question "is that `bin/` yours or MSBuild's?", and the
// answer is never the directory's name — plenty of repositories commit a `bin/` full of
// scripts. The proof is NuGet's: restore writes `obj/project.assets.json` and nobody
// writes one by hand, and that file records the path of the project it was restored
// for, precisely so the build can find its dependency graph again. So `obj/` is claimed
// only when it carries an assets file naming a project file that is still sitting in
// this directory, and `bin/` is claimed only alongside a proven `obj/` — and only while
// everything in it is a build-configuration directory (`Debug`, `Release`). One
// committed script inside `bin/` refuses the whole directory.
//
// Deliberately missed, and fine to miss: custom configuration names (`bin/Staging`),
// the `UseArtifactsOutput` layout (which moves output to `artifacts/`, out of these
// directories entirely), and packages.config-era projects, which never write an assets
// file. All of them stay untouched, which is the failure direction this tool prefers.
//
// Opt-in, and held to `build_idle_days`: `bin/` and `obj/` are compiled output, and
// `dotnet build` puts them back by restoring packages and compiling again.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

/// The file NuGet's restore writes into `obj/`. Its contents are the whole proof.
const ASSETS_FILE: &str = "project.assets.json";

/// The JSON pointer to the project the assets file was restored for.
const PROJECT_PATH_POINTER: &str = "/project/restore/projectPath";

/// The MSBuild project file extensions this adapter detects on.
const PROJECT_EXTENSIONS: &[&str] = &["csproj", "fsproj", "vbproj"];

/// The only names `bin/` may contain and still be claimed.
const CONFIG_DIRS: &[&str] = &["Debug", "Release"];

/// .NET build-output adapter. Opt-in; see the module comment.
pub struct DotnetBuild;

/// The MSBuild project files sitting directly in `dir`, sorted.
fn project_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| PROJECT_EXTENSIONS.contains(&e))
        })
        .collect();
    found.sort();
    found
}

/// Whether `project`'s `obj/` was written by a NuGet restore of a project still here.
///
/// The recorded path is compared by file name only: the repository may have been moved
/// or cloned somewhere else since the restore ran, and the assets file still proves what
/// wrote the directory even when the absolute path it recorded no longer exists. The
/// comparison ignores ASCII case because the file name came through a Windows
/// filesystem at least once.
fn obj_is_nugets(project: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(project.join("obj").join(ASSETS_FILE)) else {
        return false;
    };
    let Ok(assets) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let Some(recorded) = assets
        .pointer(PROJECT_PATH_POINTER)
        .and_then(|v| v.as_str())
    else {
        return false;
    };
    // The assets file records the path with whatever separators the restoring OS used.
    let Some(recorded_name) = recorded.rsplit(['/', '\\']).next() else {
        return false;
    };
    project_files(project).iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(recorded_name))
    })
}

/// Whether `bin/` holds build configurations and nothing else.
fn bin_is_only_build_output(bin: &Path) -> bool {
    let Ok(entries) = fs::read_dir(bin) else {
        return false;
    };
    let mut any = false;
    for entry in entries.flatten() {
        let is_config_dir = entry.path().is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| CONFIG_DIRS.iter().any(|c| n.eq_ignore_ascii_case(c)));
        if !is_config_dir {
            return false;
        }
        any = true;
    }
    any
}

impl PackageManager for DotnetBuild {
    fn name(&self) -> &'static str {
        "dotnet_build"
    }

    fn detect(&self, path: &Path) -> bool {
        !project_files(path).is_empty()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        // No proven `obj/` means no claim on anything: `bin/` on its own has nothing
        // machine-written in it to say whose output it is.
        if !obj_is_nugets(path) {
            return Vec::new();
        }
        let mut dirs = Vec::new();
        let bin = path.join("bin");
        if bin.is_dir() && bin_is_only_build_output(&bin) {
            dirs.push(BloatDir {
                name: "bin".to_string(),
                size_bytes: dir_size(&bin),
                path: bin,
                shared_bytes: 0,
            });
        }
        let obj = path.join("obj");
        dirs.push(BloatDir {
            name: "obj".to_string(),
            size_bytes: dir_size(&obj),
            path: obj,
            shared_bytes: 0,
        });
        dirs
    }

    /// The per-directory proof already ran: [`obj_is_nugets`] claimed the output only
    /// after the assets file named a project still in this directory. What is left to
    /// check is that the project files themselves still read as MSBuild XML, because
    /// they are what `dotnet build` is about to be pointed at. Running `dotnet` here to
    /// find out would start a restore in the middle of a delete pass, which is the
    /// opposite of what was asked for.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let projects = project_files(path);
        if projects.is_empty() {
            return Err(anyhow!(
                "no MSBuild project file (*.csproj, *.fsproj, *.vbproj) left in `{}` — \
                 nothing for `dotnet build` to rebuild from.",
                path.display()
            ));
        }
        for project in &projects {
            let name = project.file_name().unwrap_or_default().to_string_lossy();
            let content = fs::read_to_string(project).map_err(|e| {
                anyhow!("`{name}` could not be read ({e}) — nothing to rebuild the output from.")
            })?;
            if !content.contains("<Project") {
                return Err(anyhow!(
                    "`{name}` has no `<Project` root — refusing to treat the build output \
                     as rebuildable from it."
                ));
            }
        }
        Ok(())
    }

    fn restore(&self, _path: &Path, _timeout: std::time::Duration) -> Result<()> {
        println!(
            ".NET build output will come back on the next `dotnet build` — it restores \
             packages and recompiles in one step"
        );
        Ok(())
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A project directory holding `App.csproj`.
    fn project(dir: &Path) -> PathBuf {
        fs::write(
            dir.join("App.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n",
        )
        .unwrap();
        dir.to_path_buf()
    }

    /// An `obj/` written the way NuGet's restore writes one, recording `for_project`.
    fn restored_obj(dir: &Path, for_project: &str) {
        let obj = dir.join("obj");
        fs::create_dir_all(&obj).unwrap();
        // Escaped the way a JSON writer would have: the assets file is parsed, not
        // pattern-matched, so the fixture has to be valid JSON.
        let recorded = for_project.replace('\\', "\\\\");
        fs::write(
            obj.join(ASSETS_FILE),
            format!(
                "{{\"version\":3,\"project\":{{\"restore\":{{\"projectPath\":\"{recorded}\"}}}}}}"
            ),
        )
        .unwrap();
        fs::write(obj.join("App.csproj.nuget.g.props"), "<Project />").unwrap();
    }

    fn build_config(dir: &Path, name: &str) {
        let config = dir.join("bin").join(name);
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("App.dll"), "compiled").unwrap();
    }

    fn claimed(project: &Path) -> Vec<String> {
        DotnetBuild
            .bloat_dirs(project)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn detects_on_a_project_file_in_the_directory() {
        let dir = tempdir().unwrap();
        assert!(!DotnetBuild.detect(dir.path()));
        project(dir.path());
        assert!(DotnetBuild.detect(dir.path()));
    }

    #[test]
    fn the_assets_file_is_what_separates_nugets_obj_from_yours() {
        // The whole point of the adapter: `obj/` full of build state is claimed, and an
        // `obj/` somebody made by hand — no assets file — is never touched.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        fs::create_dir_all(root.join("obj")).unwrap();
        fs::write(root.join("obj").join("notes.txt"), "hand made").unwrap();
        assert!(claimed(&root).is_empty());

        restored_obj(&root, "C:\\src\\App\\App.csproj");
        assert_eq!(claimed(&root), vec!["obj"]);
    }

    #[test]
    fn an_assets_file_for_someone_elses_project_is_refused() {
        // An `obj/` copied in from another project records that project's file, which is
        // not here — whatever this directory is, `dotnet build` here did not write it.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        restored_obj(&root, "/src/other/Other.csproj");

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn a_moved_repository_still_proves_its_own_obj() {
        // The recorded path is from wherever the restore ran; only the file name has to
        // still match, so a cloned or moved checkout keeps its claim.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        restored_obj(&root, "/home/somebody/else/entirely/App.csproj");

        assert_eq!(claimed(&root), vec!["obj"]);
    }

    #[test]
    fn bin_is_only_claimed_alongside_a_proven_obj() {
        // `bin/Debug` with no restored `obj/` has nothing machine-written to say whose
        // output it is, so nothing at all is claimed.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        build_config(&root, "Debug");

        assert!(claimed(&root).is_empty());
    }

    #[test]
    fn one_committed_file_in_bin_refuses_the_whole_directory() {
        // The generic name is the hazard: a repository can commit `bin/run.sh` next to
        // where MSBuild writes `bin/Debug`. The build configurations could be claimed
        // alone, but a `bin/` that is partly someone's is left entirely alone.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        restored_obj(&root, "App.csproj");
        build_config(&root, "Debug");
        fs::write(root.join("bin").join("run.sh"), "#!/bin/sh").unwrap();

        assert_eq!(claimed(&root), vec!["obj"]);
    }

    #[test]
    fn a_directory_of_committed_tools_in_bin_refuses_it_too() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        restored_obj(&root, "App.csproj");
        build_config(&root, "Release");
        fs::create_dir_all(root.join("bin").join("tools")).unwrap();

        assert_eq!(claimed(&root), vec!["obj"]);
    }

    #[test]
    fn configuration_names_match_whatever_their_casing_is() {
        // `debug`/`RELEASE` come out of case-insensitive filesystems and hand-typed
        // `-c` flags alike.
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        restored_obj(&root, "app.csproj");
        build_config(&root, "debug");
        build_config(&root, "RELEASE");

        assert_eq!(claimed(&root), vec!["bin", "obj"]);
    }

    #[test]
    fn an_empty_bin_is_not_claimed() {
        let dir = tempdir().unwrap();
        let root = project(dir.path());
        restored_obj(&root, "App.csproj");
        fs::create_dir_all(root.join("bin")).unwrap();

        assert_eq!(claimed(&root), vec!["obj"]);
    }

    #[test]
    fn a_missing_or_bogus_project_file_is_refused() {
        let dir = tempdir().unwrap();
        let policy = EnforcePolicy::default();
        assert!(DotnetBuild.enforce_lockfile(dir.path(), policy).is_err());
        fs::write(dir.path().join("App.csproj"), "hello there").unwrap();
        assert!(DotnetBuild.enforce_lockfile(dir.path(), policy).is_err());
        project(dir.path());
        assert!(DotnetBuild.enforce_lockfile(dir.path(), policy).is_ok());
    }

    #[test]
    fn dotnet_build_is_opt_in() {
        assert!(DotnetBuild.opt_in());
    }
}
