# How to Add a New Package Manager Adapter to `dev-prune`

`dev-prune` (`devp`) is designed to be highly modular and contribution-friendly. Adding support for a new package manager or ecosystem (e.g. Maven, Gradle, Composer, Mix, Swift SPM, etc.) takes only a few minutes.

---

## 🏛️ Architecture Overview

All adapters live in [`src/adapters/`](../src/adapters/) and implement the [`PackageManager`](../src/adapters/mod.rs) Rust trait:

```rust
pub trait PackageManager: Send + Sync {
    /// Unique human-readable name of the adapter (e.g. "npm", "uv", "cargo", "go").
    fn name(&self) -> &'static str;

    /// Check if this adapter applies to a given project path.
    fn detect(&self, project_path: &Path) -> bool;

    /// List existing bloat directories managed by this adapter.
    fn bloat_dirs(&self, project_path: &Path) -> Vec<BloatDir>;

    /// Lockfile safety check: MUST prove the lockfile can rebuild the tree BEFORE any
    /// bloat directory is deleted. If this returns an Err, pruning for this adapter is
    /// ABORTED. `policy` carries the user's `allow_manifest_rewrite` and
    /// `command_timeout_secs`; hand it straight to `enforce_two_tier`.
    fn enforce_lockfile(&self, project_path: &Path, policy: EnforcePolicy) -> Result<()>;

    /// Restore dependencies from lockfile (used by `devp restore`).
    fn restore(&self, project_path: &Path) -> Result<()>;

    /// Lockfiles that identify this manager. Only needed if your adapter can
    /// share a bloat directory with another one. Defaults to an empty slice.
    fn lockfiles(&self) -> &'static [&'static str] { &[] }
}
```

Several adapters detecting in the same directory is normal and fully supported — a
directory with `package-lock.json`, `uv.lock` and `Cargo.toml` runs three adapters,
each owning a different bloat directory. Only adapters that would fight over the
**same** directory need resolving; see [Step 6](#step-6-if-your-adapter-shares-a-bloat-directory).

---

## 🛠️ Step-by-Step Implementation Tutorial

### Step 1: Create a new module file
Create a new Rust file in `src/adapters/` named after your package manager, e.g. `src/adapters/gradle.rs`.

### Step 2: Implement the `PackageManager` trait

Here is a complete, production-ready example for a Gradle adapter:

```rust
// Gradle adapter for dev-prune.

use std::path::Path;
use anyhow::Result;
use super::{PackageManager, BloatDir, EnforcePolicy, dir_size, enforce_two_tier, run_command};

/// Adapter for Gradle projects.
pub struct Gradle;

impl PackageManager for Gradle {
    fn name(&self) -> &'static str {
        "gradle"
    }

    fn detect(&self, project_path: &Path) -> bool {
        project_path.join("build.gradle").exists()
            || project_path.join("build.gradle.kts").exists()
    }

    fn bloat_dirs(&self, project_path: &Path) -> Vec<BloatDir> {
        let mut dirs = Vec::new();
        let build_dir = project_path.join("build");
        let gradle_dir = project_path.join(".gradle");

        if build_dir.is_dir() {
            dirs.push(BloatDir {
                name: "build".to_string(),
                path: build_dir.clone(),
                size_bytes: dir_size(&build_dir),
                shared_bytes: 0,
            });
        }
        if gradle_dir.is_dir() {
            dirs.push(BloatDir {
                name: ".gradle".to_string(),
                path: gradle_dir.clone(),
                size_bytes: dir_size(&gradle_dir),
                shared_bytes: 0,
            });
        }
        dirs
    }

    /// Two commands, and the order matters: the read-only one first, the writing one
    /// second. `gradle dependencies` resolves against the existing lock state and fails
    /// if it has drifted; `--write-locks` *rewrites* `gradle.lockfile`, so it is only
    /// reached when there is no lockfile to preserve or the user opted in.
    fn enforce_lockfile(&self, project_path: &Path, policy: EnforcePolicy) -> Result<()> {
        let lockfile = project_path.join("gradle.lockfile");
        enforce_two_tier(
            &lockfile,
            "gradle",
            &["dependencies"],
            &["dependencies", "--write-locks"],
            project_path,
            policy,
        )
    }

    fn restore(&self, project_path: &Path) -> Result<()> {
        run_command("gradle", &["build", "-x", "test"], project_path)
    }
}
```

---

### Step 3: Available Adapter Helper Functions

`src/adapters/mod.rs` provides shared utility functions for adapter authors:

- **`enforce_two_tier(lockfile, program, verify_args, write_args, cwd, policy)`**: the one
  helper every shipped adapter uses. It picks between your two command forms:
  - `policy.allow_rewrite` is set → run `write_args`; the user asked for repairs.
  - Otherwise, binary installed and `lockfile` exists → run `verify_args`. **Read-only.**
  - Binary installed, no lockfile → run `write_args`; there is nothing to preserve.
  - Binary missing but `lockfile` exists → proceed; the lockfile is itself the proof.
  - Binary missing and no lockfile → error, and deletion is aborted.

  Everything runs under `policy.timeout`, which is the user's `command_timeout_secs`.
- **`lock_verify_or_generate(...)` / `lock_sync_or_verify_with_timeout(...)`**: the two
  shapes `enforce_two_tier` dispatches to. Call them directly only if your ecosystem
  genuinely does not fit — bun does, because it has no writing form to opt into.
- **`run_command(program, args, cwd)`**: Executes external command with standard timeout (`command_timeout_secs`).
- **`run_command_with_timeout(program, args, cwd, timeout)`**: Executes command with explicit timeout.
- **`try_run_command(program, args, cwd)`**: Executes command returning boolean `true`/`false`.
- **`binary_available(program)`**: Whether `program` is on `PATH`.
- **`dir_size(path)`**: Calculates directory size in bytes recursively.
- **`dir_size_with_hardlinks(path)`**: Splits that size into `freed_bytes` (what deleting
  the tree gives back) and `shared_bytes` (hardlinked from outside — a store keeps them).
  Use it if your package manager links files out of a global store instead of copying,
  the way pnpm and bun do, and carry both halves into the `BloatDir`. A manager that
  copies sets `shared_bytes: 0` and uses plain `dir_size`.

> **Rule for `enforce_lockfile`: resolve, never install, never write.**
> The command runs as a precondition for *deleting* the tree, so it must not download
> dependencies or execute their lifecycle scripts, and — this is the part contributors
> get wrong — `verify_args` must not rewrite the lockfile either. A prune pass can be
> started by the OS scheduler; a check that "helpfully" resyncs a drifted lockfile leaves
> a modified tracked file behind with nobody watching. Prefer a frozen, locked or dry-run
> mode (`--frozen-lockfile`, `--immutable`, `--locked`, `--dry-run`). If your ecosystem
> has no such mode, treat an existing lockfile as sufficient proof of recoverability
> rather than running a full install — that is what the Yarn Classic path does.
>
> Pair it with a `write_args` that *does* repair the lockfile. That is what runs when
> there is no lockfile at all, what `allow_manifest_rewrite` opts into, and what
> [`json::lockfile_fix_command`](../src/json.rs) should print for your adapter so the
> user can run it themselves.

---

### Step 4: Write comprehensive unit tests

Every adapter must contain unit tests using `tempfile::TempDir` verifying:
1. Detection logic (positive and negative cases)
2. Bloat directory scanning
3. Adapter name uniqueness

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_gradle_name() {
        assert_eq!(Gradle.name(), "gradle");
    }

    #[test]
    fn test_gradle_detect_positive() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("build.gradle"), "// gradle").unwrap();
        assert!(Gradle.detect(tmp.path()));
    }

    #[test]
    fn test_gradle_detect_negative() {
        let tmp = TempDir::new().unwrap();
        assert!(!Gradle.detect(tmp.path()));
    }

    #[test]
    fn test_gradle_bloat_dirs_present() {
        let tmp = TempDir::new().unwrap();
        let build = tmp.path().join("build");
        fs::create_dir(&build).unwrap();
        fs::write(build.join("output.jar"), "sample binary data").unwrap();

        let dirs = Gradle.bloat_dirs(tmp.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "build");
    }
}
```

---

### Step 5: Register your adapter in `src/adapters/mod.rs`

1. Add `pub mod gradle;` to top of `src/adapters/mod.rs`
2. Add `Box::new(gradle::Gradle)` inside `get_all_adapters()`:

```rust
pub fn get_all_adapters() -> Vec<Box<dyn PackageManager>> {
    vec![
        Box::new(npm::Npm),
        Box::new(pnpm::Pnpm),
        Box::new(yarn::Yarn),
        Box::new(bun::Bun),
        Box::new(uv::Uv),
        Box::new(venv::Venv),
        Box::new(cargo_adapter::Cargo),
        Box::new(go::Go),
        Box::new(gradle::Gradle),
    ]
}
```

---

### Step 6: If your adapter shares a bloat directory

If your ecosystem's bloat directory is already owned by another adapter (as npm, pnpm,
yarn and bun all own `node_modules`), the two cannot both run: whichever loses would
rewrite a lockfile the project does not use. Implement `lockfiles()` so the tie can be
broken, and extend `resolve_conflicts()` in `src/adapters/mod.rs` with a rule for your
family. The existing rules are the model:

- **JavaScript** picks a winner from the `packageManager` field in `package.json`, then
  from the bookkeeping files the installer left inside `node_modules`, then from the
  newest lockfile mtime.
- **Python** gives uv priority over the plain-venv adapter, because uv has a real
  lockfile and can reproduce the environment exactly.

The principle in both: prefer whatever evidence describes the tree **actually on disk**
over evidence about what the project once used.

---

### Step 7: Verify with `cargo test`

Run tests to ensure everything builds cleanly and tests pass:

```bash
cargo test --all
cargo clippy -- -D warnings
cargo fmt -- --check
```
