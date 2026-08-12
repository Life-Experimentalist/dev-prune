// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Go package manager adapter.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, enforce_two_tier, run_command};
use anyhow::Result;
use std::path::Path;

/// Adapter for Go modules.
pub struct Go;

impl PackageManager for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn detect(&self, path: &Path) -> bool {
        path.join("go.mod").exists()
    }

    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let vendor_path = path.join("vendor");
        if vendor_path.exists() {
            dirs.push(BloatDir {
                name: "vendor".to_string(),
                path: vendor_path.clone(),
                size_bytes: dir_size(&vendor_path),
            });
        }
        dirs
    }

    /// `go mod tidy` reconciles `go.mod` and `go.sum` against the real imports, and can
    /// *remove* a requirement nothing imports any more — which is exactly why it is not
    /// the default. With `go.sum` present the module cache is verified instead, which
    /// never touches tracked files. `tidy` is reached only when there is no `go.sum` to
    /// bootstrap from, or when the user opted in.
    fn enforce_lockfile(&self, path: &Path, policy: EnforcePolicy) -> Result<()> {
        enforce_two_tier(
            &path.join("go.sum"),
            "go",
            &["mod", "download"],
            &["mod", "tidy"],
            path,
            policy,
        )
    }

    fn restore(&self, path: &Path) -> Result<()> {
        if path.join("vendor").exists() {
            run_command("go", &["mod", "vendor"], path)
        } else {
            run_command("go", &["mod", "download"], path)
        }
    }

    fn lockfiles(&self) -> &'static [&'static str] {
        &["go.sum"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_name() {
        let adapter = Go;
        assert_eq!(adapter.name(), "go");
    }

    #[test]
    fn test_detect_positive() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("go.mod")).unwrap();

        let adapter = Go;
        assert!(adapter.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative() {
        let dir = tempdir().unwrap();

        let adapter = Go;
        assert!(!adapter.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("vendor")).unwrap();

        let adapter = Go;
        let dirs = adapter.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "vendor");
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();

        let adapter = Go;
        let dirs = adapter.bloat_dirs(dir.path());
        assert!(dirs.is_empty());
    }
}
