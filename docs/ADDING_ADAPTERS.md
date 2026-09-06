# How to Add a New Package Manager Adapter to `dev-prune`

`dev-prune` (`devp`) is designed to be highly modular and contribution-friendly. Adding support for a new package manager or ecosystem (e.g. Composer, Bundler, CocoaPods, Mix, Swift SPM, etc.) takes only a few minutes.

There are three things you can add, and all three are one table entry:

| What | Where | What it gives you |
|---|---|---|
| **An adapter** — a per-repository ecosystem | `src/adapters/` | `devp run` deletes that ecosystem's directories and can prove the lockfile rebuilds them. [Steps 1–7 below.](#step-1-create-a-new-module-file) |
| **A cache probe** — a machine-wide store | `src/commands/caches.rs` | A row in `devp caches` and a working `devp caches clear <name>`. [How to add one.](#-adding-a-cache-probe) |
| **A container engine** | `src/commands/containers.rs` | A report in `devp caches containers` and a working `devp caches clear <engine>`. [How to add one.](#-adding-a-container-engine) |
| **A rebuild check** — for declared directories | beside the adapter, in `src/adapters/` | A declared `rebuild` command using your tool is verified against the manifest it would read, instead of only against `PATH`. [How to add one.](#-adding-a-rebuild-check) |

They are independent. An ecosystem can have an adapter and no probe (its cache is
per-project, or it has none), a probe and no adapter (`pip`, `playwright`,
`huggingface` — nothing on disk to prune per-repository), or both.

---

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

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

    /// Restore dependencies from lockfile (used by `devp restore`). `timeout` is the
    /// user's `command_timeout_secs`, threaded explicitly.
    fn restore(&self, project_path: &Path, timeout: std::time::Duration) -> Result<()>;

    /// `restore`, told the name the pruned directory had and the runtime tag recorded
    /// when it was deleted (`None` when nothing was recorded). Has a default impl that
    /// ignores both and calls `restore`; override it only if your manager can rebuild
    /// under more than one directory name (venv does — `venv`, `env`, ...).
    fn restore_named(
        &self,
        project_path: &Path,
        dir_name: &str,
        runtime: Option<&str>,
        timeout: std::time::Duration,
    ) -> Result<()> { let _ = (dir_name, runtime); self.restore(project_path, timeout) }

    /// The language runtime `dir_name` was built against, recorded at prune time and
    /// handed back through `restore_named`. Defaults to `None`; only the Python
    /// managers answer it, because only their directories embed one interpreter.
    fn runtime_tag(&self, project_path: &Path, dir_name: &str) -> Option<String> {
        let _ = (project_path, dir_name);
        None
    }

    /// Installed-but-unrecorded packages, surfaced by `devp status --drift` before a
    /// prune is attempted. Defaults to empty — "nothing detected", not "proven clean".
    fn drift(&self, project_path: &Path) -> Vec<DriftReport> {
        let _ = project_path;
        Vec::new()
    }

    /// Lockfiles that identify this manager. Only needed if your adapter can
    /// share a bloat directory with another one. Defaults to an empty slice.
    fn lockfiles(&self) -> &'static [&'static str] { &[] }

    /// Whether this adapter must be switched on explicitly before it detects
    /// anything. Defaults to `false`. Return `true` when what you delete is a
    /// build tree that has to be *recompiled* back rather than re-downloaded —
    /// cargo, gradle, maven and swift do — and add a matching `enable_<name>` setting in
    /// `src/commands/config.rs`. While the setting is off, the adapter is
    /// invisible: not detected, not listed, and `--only <name>` prunes nothing.
    /// Opt-in adapters are also idle-gated by `build_idle_days`, applied as
    /// `max(build_idle_days, idle_days)`.
    fn opt_in(&self) -> bool { false }
}
```

Several adapters detecting in the same directory is normal and fully supported — a
directory with `package-lock.json`, `uv.lock` and `Cargo.toml` runs three adapters,
each owning a different bloat directory. Only adapters that would fight over the
**same** directory need resolving; see [Step 6](#step-6-if-your-adapter-shares-a-bloat-directory).

---

## 🛠️ Step-by-Step Implementation Tutorial

> The worked example below is Gradle — which has since shipped as a real (opt-in)
> adapter, built by following exactly these steps. The finished version is
> [`src/adapters/gradle.rs`](../src/adapters/gradle.rs); compare against it as you go.

### Step 1: Create a new module file
Create a new Rust file in `src/adapters/` named after your package manager, e.g. `src/adapters/gradle.rs`.

### Step 2: Implement the `PackageManager` trait

Here is a complete, production-ready example for a Gradle adapter:

```rust
// Gradle adapter for dev-prune.

use std::path::Path;
use anyhow::Result;
use super::{
    PackageManager, BloatDir, EnforcePolicy, dir_size, enforce_two_tier,
    run_command_with_timeout,
};

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

    fn restore(&self, project_path: &Path, timeout: std::time::Duration) -> Result<()> {
        run_command_with_timeout("gradle", &["build", "-x", "test"], project_path, timeout)
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
- **`run_command_with_timeout(program, args, cwd, timeout)`**: Executes an external
  command bounded by `timeout` — pass the `timeout` your `restore` was handed, or
  `policy.timeout` inside `enforce_lockfile`.
- **`capture_command_with_timeout(program, args, cwd, timeout)`**: Same, but returns the
  command's stdout — for commands that answer a question instead of doing work.
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
        Box::new(poetry::Poetry),
        Box::new(venv::Venv),
        Box::new(cargo_adapter::Cargo),
        Box::new(go::Go),
        Box::new(gradle::Gradle),
        Box::new(maven::Maven),
    ]
}
```

---

### Step 5b: Put your adapter in a language group

`ADAPTER_GROUPS` in the same file is what `devp config wizard` draws its checklist
from, grouped by language so a heading can switch a whole ecosystem on, off, or onto
its own idle window in one keypress. Add your adapter to the group it belongs to, or
add a new group:

```rust
pub const ADAPTER_GROUPS: &[(&str, &[&str])] = &[
    ("JVM", &["gradle", "maven"]),
    // ...
];
```

`every_adapter_is_grouped_exactly_once` fails if you forget — an ungrouped adapter is
one a user cannot find in the only screen that lists them.

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
- **Python** gives uv and poetry priority over the plain-venv adapter, because they
  have a real lockfile and can reproduce the environment exactly; between uv and
  poetry, whichever one's lockfile is actually on disk owns the environment.

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

---

## 🔎 Adding a Rebuild Check

`project.devprune.json` lets a repository declare extra directories with the command
that rebuilds them. Before honouring one, `src/declared/` checks that the command's
tool is on `PATH`, and then, for tools it has a check for, that the *target* the
command names actually exists in the manifest the tool would read: `npm run build`
against the `scripts` table of `package.json`, `make generate` against the makefile's
targets, and so on. That second half is a rebuild check.

A check is one implementation of the `RebuildCheck` trait (in `src/declared/mod.rs`),
written beside your adapter so all of one tool's knowledge stays in one file, plus one
line in the `REBUILD_CHECKS` registry:

```rust
pub(crate) struct GradleTasks;

impl RebuildCheck for GradleTasks {
    fn tools(&self) -> &'static [&'static str] {
        &["gradle", "gradlew"]
    }

    fn gap(&self, repo_path: &Path, tool: &str, args: &[&str]) -> Option<Gap> {
        // Read the manifest. Never run anything.
    }
}
```

The contract is asymmetric on purpose, and it is the part to get right:

- Return `Some(Gap)` **only on positive proof**: the manifest is present, readable,
  and definitively does not provide what the command names.
- Everything unanswerable returns `None`, which **allows** the prune. A flag your
  check does not model, a manifest that is absent or unparseable, a shape you cannot
  read: all of those are "cannot tell", never "refuse". A false refusal blocks a safe
  prune on a committed file the user cannot easily debug, which is a worse bug than
  the one you are closing.
- Never execute the rebuild command or any part of it. Every check is a read.

Two registry tests in `src/declared/mod.rs` enforce the mechanical half of that
contract on every registered check automatically (no tool claimed twice; empty and
unmodelled arguments never refuse). Add your own tests beside them for the refusals
your check does make, pinning the exact wording.

Rebuild checks are optional and independent of the adapter itself: a tool with no
check simply stops at the `PATH` test, which is safe. `make` has a check and no
adapter at all; it lives in `src/declared/make.rs` because there is no adapter file to
sit beside.

---

## 💾 Adding a Cache Probe

A cache probe is the machine-wide half: the shared store a package manager downloads
into once and reinstalls from forever. It is what `devp caches` sizes and what
`devp caches clear <name>` empties.

Adding one is **one `PROBES` entry and one `fallbacks()` arm**. Everything else in the
subsystem derives from that table — `known_managers()`, `is_cache_manager()`, the
`<MANAGER>` list in the usage error, the `--over-cap` accounting and the `used_by`
column all read `PROBES` rather than a list of their own. There is no second place to
register a name, and no way to add one to half of them.

### Step 1: The `PROBES` entry

In `src/commands/caches.rs`:

```rust
Probe {
    manager: "poetry",
    kind: "artifact cache",
    // The manager's own answer to "where is it?". It must print a path and exit,
    // and it must not create the directory — a probe that installs something is
    // a probe that lies about the disk it was measuring.
    query: Some(("poetry", &["config", "cache-dir"])),
    // Appended to that answer, for the managers that will only name a directory
    // one level above their cache. Poetry's cache-dir holds `virtualenvs/` beside
    // `artifacts/`; only the second is a cache.
    query_suffix: Some("artifacts"),
    clear_command: POETRY_ARTIFACTS_CLEAR,
    clear: Clear::Directory,
    note: Some(
        "only `artifacts/`, the wheels and sdists Poetry downloaded. `virtualenvs/` \
         sits in the same cache directory and holds the environments themselves — \
         deleting that would not be clearing a cache.",
    ),
}
```

Three fields carry the judgement:

- **`query`** is the manager asked rather than assumed. `None` is correct when the
  ecosystem has no such command, and the conventional locations are all there is.
- **`query_suffix`** is where most of the care goes. `gem env gemdir` prints the gem
  home, which holds installed gems and binstubs as well as the `.gem` archives;
  `poetry config cache-dir` prints a parent of `virtualenvs/`. Sizing or clearing
  either answer whole would take working environments with it. Name the one
  subdirectory that is genuinely a download cache.
- **`clear`** is what dev-prune is willing to run. `Clear::Command(program, args)` when
  the manager has a real, non-interactive clean subcommand; `Clear::Directory` to
  delete the probed path; `Clear::Manual { why }` when neither is honest — Maven is the
  only one, because its local repository is not a cache and nothing should treat it as
  one.

`clear_command` is the string the report prints on its `runs:` line. On the
`Clear::Directory` rows it is a `const` with a `#[cfg]` per platform, because the
command a user would type is PowerShell on Windows and `rm -rf` elsewhere.

### Step 2: The `fallbacks()` arm

Also in `src/commands/caches.rs`, in `fallbacks()`:

```rust
("poetry", _) => vec![
    std::env::var_os("POETRY_CACHE_DIR").map(|p| PathBuf::from(p).join("artifacts")),
    under(&cache, "pypoetry/Cache/artifacts"),
    under(&cache, "pypoetry/artifacts"),
],
```

The environment variable goes first where the ecosystem has one, because a machine
that sets it has moved the cache and the conventional path is then wrong rather than
merely absent. This list is what runs when the manager is not installed — which is the
interesting case, because a cache outliving its manager is exactly the disk nobody can
account for. Use
the `under()` helper rather than joining paths yourself: it splits on `/` so a Windows
path never comes back spelled `C:\Users\dev/pypoetry`.

### Step 3: If the name is also an adapter name

Nothing to do — `dependents()` seeds the `used_by` count from
`adapters::is_adapter_name(m)`, so a probe whose name matches an adapter gets the
"used by 3 of 9 registered repositories" line automatically, and one that does not
stays silent rather than guessing. `pip` does not claim every venv on the disk;
`playwright` does not claim every `node_modules`.

### Step 4: If re-downloading is not a promise

Give it a `note`. Every other row in the report is safe on the strength of one
sentence — delete it, the manager fetches it again — and `huggingface` is the row where
that is not true: a gated or now-private repository will not hand the weights back, and
the re-download is measured in tens of gigabytes. If your cache has a case like that,
say so in the `note` rather than leaving the row looking like the ones that do not.

---

## 📦 Adding a Container Engine

Docker, Podman, nerdctl, finch and Apple's `container` are five entries in one
`ENGINES` table in `src/commands/containers.rs`. A sixth is one more:

```rust
Engine {
    name: "finch",
    binary: "finch",
    prompts: true,
    df_args: &["system", "df", "--format", "{{json .}}"],
    // Printed, never run. Every command worth knowing about, including the one
    // that deletes data.
    prune: &[
        (
            "finch system prune",
            "stopped containers, networks, dangling images",
        ),
        (
            "finch system prune --volumes",
            "adds unused volumes — the one that deletes data",
        ),
    ],
    // Run, on request, after printing. Nothing here touches a volume.
    reclaim: &[ReclaimStep {
        what: "images, stopped containers and the build cache",
        args: &["system", "prune", "-a", "-f"],
    }],
}
```

Every runtime enumeration derives from this table: which binaries are looked for, the
engine names `devp caches clear` accepts, the list in its usage error, and what
`devp caches containers` runs when given no engine. There is no separate list to keep
in step.

Two fields are the ones to get right:

- **`prune` and `reclaim` are deliberately separate tables.** `prune` is every command
  worth *knowing about*, including the volume-deleting variant; it is printed and never
  run. `reclaim` is only what `devp caches clear <engine>` will execute. There is no
  argv in any `reclaim` list that touches a volume — so "dev-prune never deletes your
  volumes" is a property of the table rather than a flag someone could pass or a check
  someone could forget.
- **`prompts`** says whether the engine stops to ask on its own. Docker, Podman,
  nerdctl and finch all do, and all take `-f` to mean the question has been asked
  already. Apple's `container` does not: its prune subcommands neither ask nor define
  a `-f`, so they run the moment they are typed. Setting `prompts: false` is what makes
  `devp caches containers container` say so in as many words before it lists them.
  dev-prune's own confirmation is unconditional either way.

If the engine's `system df` does not answer in the shape the other four use, teach
`parse_rows` about it rather than reshaping the engine entry — Apple's answers with a
single object rather than a row per resource, and that is where it is handled.

---

## ✅ Whichever you added

The gate is the same four commands, and clippy is `-D warnings` including test code:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
npm --prefix site run build
```

A new adapter changes a count that is written out **in words** across the README, the
site, `llms.txt`, the AI skill and two marketplace manifests. Grep for the number you
are replacing before you open the pull request; CI does not check prose.
