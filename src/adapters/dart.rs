// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Dart and Flutter adapter.
//
// Opt-in (`devp config set enable_dart true`), and held to `build_idle_days`, for a
// reason that is worth stating precisely because it is not the obvious one.
//
// `.dart_tool/` is not where the dependencies live. Pub downloads packages once into a
// machine-wide cache — `~/.pub-cache` — and `.dart_tool/package_config.json` is a list of
// pointers into it. Restoring that part is `dart pub get`, offline, in under a second.
// What actually takes up the space is everything else in there: `build/` from
// `build_runner`'s generated code, and `flutter_build/` from Flutter's incremental
// compiler, which on an app of any size is hundreds of megabytes and comes back only by
// recompiling. That is compiler output, so it follows the same rule cargo, gradle, maven
// and swift do — nobody finds it gone without having switched it on.
//
// `build/` at the project root is Flutter's *output* directory (the APK, the web bundle)
// and is not claimed at all. Neither are `ios/Pods` or `android/.gradle`: those belong to
// the CocoaPods and Gradle adapters, which have their own lockfile proofs.

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, refuse_if_manifest_stale,
    run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// Dart and Flutter adapter.
pub struct Dart;

impl Dart {
    /// Whether this is a Flutter project rather than a plain Dart one.
    ///
    /// It decides which binary restores the tree: `flutter pub get` does the Flutter
    /// SDK's own bookkeeping as well as pub's, and running plain `dart pub get` in a
    /// Flutter app leaves the tooling reconfiguring itself on the next build.
    fn is_flutter(path: &Path) -> bool {
        fs::read_to_string(path.join("pubspec.yaml"))
            .map(|manifest| manifest.contains("flutter:") || manifest.contains("sdk: flutter"))
            .unwrap_or(false)
    }
}

impl PackageManager for Dart {
    fn name(&self) -> &'static str {
        "dart"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("pubspec.yaml").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let tool = path.join(".dart_tool");
        if !tool.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: ".dart_tool".to_string(),
            path: tool.clone(),
            size_bytes: dir_size(&tool),
            shared_bytes: 0,
        }]
    }

    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let lock = path.join("pubspec.lock");
        let content = fs::read_to_string(&lock).map_err(|e| {
            anyhow!(
                "`pubspec.lock` could not be read ({e}) — without it `pub get` resolves \
                 afresh instead of restoring the versions being deleted."
            )
        })?;
        // Every pub lockfile is a YAML document with a `packages:` mapping, even when a
        // project depends on nothing. A file without it is a fragment or a merge
        // conflict, not something `pub get` can be held to.
        if !content.contains("packages:") {
            return Err(anyhow!(
                "`pubspec.lock` has no `packages:` section — it is not a complete pub \
                 lockfile, so `.dart_tool` cannot be proven rebuildable from it."
            ));
        }
        // Same offline evidence as Mix and CocoaPods, and for the same reason: `pub get`
        // resolves and writes rather than reporting, so running it to check would be a
        // write in the middle of a delete pass.
        refuse_if_manifest_stale(&path.join("pubspec.yaml"), &lock, "dart pub get")
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        let program = if Self::is_flutter(path) {
            "flutter"
        } else {
            "dart"
        };
        run_command_with_timeout(program, &["pub", "get"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["pubspec.lock"]
    }

    fn opt_in(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_on_the_pub_manifest() {
        let dir = tempdir().unwrap();
        assert!(!Dart.detect(dir.path()));
        fs::write(dir.path().join("pubspec.yaml"), "name: example\n").unwrap();
        assert!(Dart.detect(dir.path()));
    }

    #[test]
    fn claims_the_tool_directory_and_never_the_output_one() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".dart_tool")).unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        let names: Vec<String> = Dart
            .bloat_dirs(dir.path())
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec![".dart_tool"]);
    }

    #[test]
    fn a_missing_or_malformed_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            Dart.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("pubspec.lock"), "<<<<<<< HEAD\n").unwrap();
        assert!(
            Dart.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(
            dir.path().join("pubspec.lock"),
            "packages:\n  http:\n    version: \"1.2.0\"\n",
        )
        .unwrap();
        assert!(
            Dart.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn a_flutter_app_is_told_apart_from_a_plain_dart_package() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("pubspec.yaml"),
            "name: cli\ndependencies:\n",
        )
        .unwrap();
        assert!(!Dart::is_flutter(dir.path()));
        fs::write(
            dir.path().join("pubspec.yaml"),
            "name: app\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        assert!(Dart::is_flutter(dir.path()));
    }

    #[test]
    fn is_opt_in_because_the_bulk_of_the_directory_is_compiler_output() {
        assert!(Dart.opt_in());
    }
}
