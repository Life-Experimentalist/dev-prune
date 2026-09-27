// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Report-only footprint of AI-assistant tools that leave large, expensive-to-rebuild
// output behind in a repository, so they belong nowhere near the adapter system.
//
// A `PackageManager` adapter exists because a lockfile proves the directory it deletes
// can be rebuilt for the cost of a network download. graphify's `graphify-out/` and
// understand-anything's `.understand-anything/` have no such proof: rebuilding either
// means re-running the tool's own extraction passes, LLM calls included, so there is
// nothing here for `devp run` to verify or delete. `devp status` reports the size so it
// is not a mystery, and stops there.

use std::path::{Path, PathBuf};

use crate::adapters::dir_size;
use crate::constants;

/// One tool's output directory found at a repository's root, and its size on disk.
#[derive(Debug, Clone)]
pub struct ToolFootprint {
    pub name: &'static str,
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Look for graphify's and understand-anything's output directories at the root of
/// `repo_root`, in exactly those two names, nothing walked or guessed.
pub fn scan(repo_root: &Path) -> Vec<ToolFootprint> {
    let candidates = [
        ("graphify", constants::GRAPHIFY_OUTPUT_DIR),
        ("understand-anything", constants::UNDERSTAND_ANYTHING_DIR),
    ];

    candidates
        .into_iter()
        .filter_map(|(name, dir_name)| {
            let path = repo_root.join(dir_name);
            path.is_dir().then(|| ToolFootprint {
                name,
                size_bytes: dir_size(&path),
                path,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_directories_that_exist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan(dir.path()).is_empty());

        std::fs::create_dir(dir.path().join(constants::GRAPHIFY_OUTPUT_DIR)).unwrap();
        std::fs::write(
            dir.path()
                .join(constants::GRAPHIFY_OUTPUT_DIR)
                .join("graph.json"),
            b"{}",
        )
        .unwrap();

        let found = scan(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "graphify");
        assert_eq!(found[0].size_bytes, 2);
    }

    #[test]
    fn finds_both_when_both_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(constants::GRAPHIFY_OUTPUT_DIR)).unwrap();
        std::fs::create_dir(dir.path().join(constants::UNDERSTAND_ANYTHING_DIR)).unwrap();

        let found = scan(dir.path());
        assert_eq!(found.len(), 2);
    }
}
