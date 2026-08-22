// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// CocoaPods adapter for Apple-platform projects.
//
// `Pods/` is a download restore: `pod install` reads `Podfile.lock` and checks out the
// exact pod versions recorded in it, so this adapter is not opt-in.
//
// The proof is offline. CocoaPods has no read-only "is the lockfile in sync" command —
// `pod install` and `pod update` both *fix* drift by rewriting `Podfile.lock` and
// re-downloading, which is a write and a network round trip in the middle of a delete
// pass. So the check is the lockfile's own structure plus the manifest timestamps; see
// [`super::refuse_if_manifest_stale`].

use super::{
    BloatDir, EnforcePolicy, PackageManager, dir_size, refuse_if_manifest_stale,
    run_command_with_timeout,
};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// CocoaPods adapter.
pub struct CocoaPods;

/// The section every `Podfile.lock` CocoaPods wrote ends with. Its absence means the
/// file is a fragment — a half-written lock or a merge conflict left in the tree — and
/// `pod install` would resolve afresh rather than restore what was deleted.
const LOCK_SENTINEL: &str = "SPEC CHECKSUMS";

impl PackageManager for CocoaPods {
    fn name(&self) -> &'static str {
        "cocoapods"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("Podfile").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let pods = path.join("Pods");
        if !pods.is_dir() {
            return Vec::new();
        }
        vec![BloatDir {
            name: "Pods".to_string(),
            path: pods.clone(),
            size_bytes: dir_size(&pods),
            shared_bytes: 0,
        }]
    }

    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let lock = path.join("Podfile.lock");
        let content = fs::read_to_string(&lock).map_err(|e| {
            anyhow!(
                "`Podfile.lock` could not be read ({e}) — without it `pod install` \
                 resolves afresh instead of restoring the versions being deleted."
            )
        })?;
        if !content.contains(LOCK_SENTINEL) {
            return Err(anyhow!(
                "`Podfile.lock` has no `{LOCK_SENTINEL}` section — it is not a complete \
                 CocoaPods lockfile, so `Pods/` cannot be proven rebuildable from it."
            ));
        }
        refuse_if_manifest_stale(&path.join("Podfile"), &lock, "pod install")
    }

    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("pod", &["install"], path, timeout)
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["Podfile.lock"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn complete_lock() -> &'static str {
        "PODS:\n  - Alamofire (5.9.1)\n\nSPEC CHECKSUMS:\n  Alamofire: abc\n\nCOCOAPODS: 1.15.2\n"
    }

    #[test]
    fn detects_on_the_podfile() {
        let dir = tempdir().unwrap();
        assert!(!CocoaPods.detect(dir.path()));
        fs::write(dir.path().join("Podfile"), "platform :ios").unwrap();
        assert!(CocoaPods.detect(dir.path()));
    }

    #[test]
    fn claims_the_pods_directory() {
        let dir = tempdir().unwrap();
        assert!(CocoaPods.bloat_dirs(dir.path()).is_empty());
        fs::create_dir(dir.path().join("Pods")).unwrap();
        let dirs = CocoaPods.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "Pods");
    }

    #[test]
    fn a_missing_or_truncated_lockfile_is_refused() {
        let dir = tempdir().unwrap();
        assert!(
            CocoaPods
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("Podfile.lock"), "PODS:\n").unwrap();
        assert!(
            CocoaPods
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_err()
        );
        fs::write(dir.path().join("Podfile.lock"), complete_lock()).unwrap();
        assert!(
            CocoaPods
                .enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }
}
