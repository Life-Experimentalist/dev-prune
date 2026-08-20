# 🛡️ Safety Invariants & Risk Mitigation in `dev-prune`

`dev-prune` (`devp`) is built on a strict **"Safety First"** philosophy. Deleting files is high-risk, so the engine enforces seven independent validation layers before any directory is removed. Every one of them is a refusal: when a check cannot be satisfied, nothing is deleted and the reason is printed.

---

## 🛡️ Safety Invariant Flowchart

The order below is the order the checks actually run in
[`prune_repo_selected`](../src/engine.rs). Every diamond that exits sideways returns a
`PruneResult` describing the refusal — the pass never stops silently.

```mermaid
flowchart TD
    Start(["prune_repo_selected(repo)"]) --> Inv1{"Inv 1<br/>.git directory present?"}
    Inv1 -->|No| Abort1["return empty<br/>not a repository"]
    Inv5a -->|Yes| Abort5["SkippedIgnored<br/>0 ms, no JSON parsed"]
    Inv1 -->|Yes| Inv5a{"Inv 5a<br/>ignore.devprune.json exists?"}
    Inv5a -->|No| Parse{".devprune.json parses?"}
    Parse -->|No| AbortCfg["ConfigError<br/>refuse to guess at defaults"]
    Parse -->|Yes| Inv5b{"Inv 5b<br/>ignore: true?"}
    Inv5b -->|Yes| Abort5
    Inv5b -->|No| Force{"--ignore-idle given?"}
    Force -->|Yes| Walk
    Force -->|No| Inv3{"Inv 3<br/>idle for override_idle_days<br/>or idle_days?"}
    Inv3 -->|No| Abort3["SkippedActive"]
    Inv3 -->|Check failed| AbortAct["ActivityCheckError<br/>idleness could not be proven"]
    Inv3 -->|Yes| Walk

    Walk["Inv 7<br/>workspace::discover — every project,<br/>bounded walk, nested repos excluded"] --> PerAdapter["for each project × adapter:<br/>collect bloat dirs, dedupe claimed paths"]
    PerAdapter --> Dry{"--dry-run?"}
    Dry -->|Yes| Report["SkippedDryRun<br/>sizes reported, nothing verified,<br/>nothing deleted"]
    Dry -->|No| Inv2{"Inv 2<br/>adapter.enforce_lockfile"}
    Inv2 -->|Err| Abort2["LockfileError<br/>every dir of this adapter kept"]
    Inv2 -->|Ok| Inv6{"Inv 6<br/>symlink or junction?"}
    Inv6 -->|Yes| Abort6["SkippedSymlink<br/>refuse to delete linked storage"]
    Inv6 -->|No| Delete["remove_dir_all → Pruned"]
    Delete --> Inv4["Inv 4<br/>registry saved via tmp file + rename"]
```

*Figure 1: the refusal ladder, in execution order.*

Two things this diagram makes explicit that prose tends to hide:

- **`--dry-run` verifies nothing.** It reports sizes and stops before
  `enforce_lockfile`, which is why a dry run is instant and never touches a lockfile.
  A directory listed by `--dry-run` is not yet proven deletable.
- **`--ignore-idle` skips the idle check only.** It re-enters the same pipeline at the
  discovery step; lockfile verification and the symlink refusal still stand. It was
  called `--force` before 1.0.0 and was renamed for exactly this reason: the old name
  claimed a power the flag never had. That spelling is still accepted and prints a note.

---

## 🛡️ Detailed Safety Invariants

### 1. Mandatory `.git` Directory Boundary Guard
`dev-prune` **NEVER** processes or deletes files in any directory that does not contain a valid `.git` folder root:
```rust
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").is_dir()
}
```
If a folder lacks `.git`, it is completely ignored, protecting arbitrary user home directories, system folders, or downloads from accidental cleanup.

The check requires a `.git` **directory**. A linked worktree (`git worktree add`) and a
submodule checkout carry a `.git` *file* pointing at the real gitdir; dev-prune does not
treat those as repositories, so they are never registered and never pruned. That is the
conservative side of the boundary: refusing a worktree costs a manual clean-up, while
guessing at its idle state — its history lives in another checkout — could cost work.

---

### 2. Two-Tier Lockfile Pre-Verification
Before deleting any bloat directory, the project's package manager must confirm that the
directory is rebuildable. Verification runs under a configurable timeout
(`command_timeout_secs`) and cannot be bypassed — not by `--ignore-idle`, not by any
setting. `allow_manifest_rewrite` is not an exception: it changes *which* command an
adapter is allowed to run, never whether the check happens.

- **Tier 1 (binary installed)**: the manager resolves the manifest against the lockfile.
- **Tier 2 (binary missing, lockfile present)**: an on-disk lockfile is itself the proof
  of recoverability, so deletion proceeds — `devp restore` can rebuild the tree once the
  manager is installed again.
- **Abort**: verification failed *and* there is no lockfile. Nothing is deleted, and the
  command to fix it is printed.

Every adapter reaches this gate through one shared helper, `enforce_two_tier` in
[`src/adapters/mod.rs`](../src/adapters/mod.rs). It takes two command forms per
ecosystem — a read-only one and a writing one — and picks between them by the rule
below.

```mermaid
flowchart TD
    P{"allow_manifest_rewrite?"}
    P -->|"Yes — the user asked"| W["run write_args<br/>under command_timeout_secs"]
    P -->|"No — the default"| A1{"manager binary on PATH?"}
    A1 -->|No| A2{"lockfile on disk?"}
    A2 -->|Yes| A3["Ok — the lockfile is itself the proof.<br/>devp restore rebuilds it later."]
    A2 -->|No| A4["Err — nothing proves this is rebuildable"]
    A1 -->|Yes| B5{"lockfile on disk?"}
    B5 -->|Yes| B6["run verify_args — read-only.<br/>Fails rather than rewriting<br/>a lockfile that has drifted."]
    B5 -->|No| B7["run write_args — only reached when<br/>there is no lockfile to preserve"]
    B6 -->|exit 0| A6["Ok"]
    B6 -->|non-zero or timeout| A7["Err — the prune is refused"]
    B7 --> A6
    W --> A6
```

*Figure 2: `enforce_two_tier`, the one shape every adapter follows. Tier 2 — the "binary
missing, lockfile present" path — is the same in all of them.*

Two adapters sit slightly outside it:

- **yarn** runs the shape, then downgrades a failure to `Ok` when `yarn.lock` exists. It
  only errors when verification failed *and* there is no lockfile, because
  `--mode update-lockfile` is a Berry flag that Yarn Classic rejects outright.
- **venv** runs no command at all. Its check is pure inspection: `requirements.txt`
  must exist and list at least one non-comment package, and every distribution
  installed in the environment (read from its `site-packages` `*.dist-info` metadata)
  must be reachable from something the file pins — directly, or as a transitive
  dependency of a pinned package. A `pip install foo` that was never written back is
  recoverable from nowhere, so the prune refuses, names the unrecorded packages, and
  suggests `pip freeze > requirements.txt`. A file that cannot be fully parsed
  (editable installs, bare URLs, unreadable includes) skips the comparison rather than
  guessing in either direction. Projects carrying `poetry.lock`, `Pipfile.lock`,
  `pdm.lock` or a `[tool.poetry]` table are not claimed at all: their
  `requirements.txt` is usually a stale export of the real lockfile, and rebuilding
  from it would quietly produce a different environment than the one deleted.

The verification command per ecosystem, and the writing form it refuses to run for you:

| Ecosystem | Verification (default)                                     | Writes? | Writing form, on `allow_manifest_rewrite` or a missing lockfile |
| :-------- | :--------------------------------------------------------- | :------ | :--- |
| npm       | `npm ci --dry-run --ignore-scripts`                        | no      | `npm install --package-lock-only --ignore-scripts` |
| pnpm      | `pnpm install --lockfile-only --frozen-lockfile`           | no      | `pnpm install --lockfile-only` |
| yarn      | `yarn install --immutable --mode update-lockfile`          | no      | `yarn install --mode update-lockfile` |
| bun       | `bun install --frozen-lockfile --dry-run --ignore-scripts` | no      | *(none — bun's natural check is already read-only)* |
| uv        | `uv lock --locked`                                         | no      | `uv lock` |
| venv      | `requirements.txt` lists ≥1 package; no installed package is unrecorded | no      | *(none — nothing is executed)* |
| cargo     | `cargo metadata --locked --format-version 1`               | no      | `cargo generate-lockfile` |
| go        | `go mod download`                                          | no      | `go mod tidy` |

The rule behind that table: **verification never writes, and a lockfile that has drifted
from its manifest is a refusal, not something to quietly fix.** A stale lockfile cannot
rebuild the tree we are about to delete, which is the one thing this page promises.

Some consequences worth spelling out:

- **The default is refusal even when the fix is trivial.** `npm install
  --package-lock-only` would repair a drifted `package-lock.json` in a second. It is
  still not run, because a prune pass can be started by the OS scheduler while nobody is
  watching, and a background process that leaves a modified tracked file behind is a
  surprise regardless of how small the edit was. `devp config set allow_manifest_rewrite
  true` is the informed opt-in, and it now means the same thing in every ecosystem —
  before 1.0.0 it was consulted by cargo and go only, while the other four rewrote their
  lockfiles unconditionally.
- **bun verifies with `--dry-run`.** A plain `bun install --frozen-lockfile` is a real
  install: it downloads every dependency and runs their lifecycle scripts. Executing
  third-party code in order to delete the tree it just built is not verification.
- **Yarn Classic is verified by lockfile presence.** `--mode update-lockfile` is a Yarn
  Berry flag. Classic has no resolve-only mode, and its nearest equivalent performs a
  full install with lifecycle scripts, so on Classic an existing `yarn.lock` is what is
  required.
- **Every one of these runs under `command_timeout_secs`** (default 600). A manager that
  hangs on a slow network fails the verification and the directory survives.

---

### 3. Hybrid Inactivity Solver (`git log` + Source File `mtime`)
To prevent deleting build artifacts in active projects where a developer has uncommitted local work:
1. `dev-prune` queries `git log -1 --format=%ct` for last commit timestamp.
2. It walks the source directory tree scanning source file `mtime` modification timestamps (excluding bloat dirs and `.git`).
3. It computes `max(last_commit_time, latest_source_mtime)`.
If the activity timestamp is less than `idle_days` old (default 15 days), the project is considered **Active** and protected.

---

### 4. Atomic Registry State Persistence
All mutations to `~/.config/dev-prune/registry.json` write to an atomic temporary file (`registry.json.tmp`) before calling OS filesystem rename:
```rust
let tmp_path = path.with_extension("json.tmp");
std::fs::write(&tmp_path, content)?;
std::fs::rename(&tmp_path, &path)?;
```
The registry is therefore never edited in place. A write that is interrupted — crash,
reboot, full disk — leaves the previous `registry.json` untouched, and at worst an
orphaned `.tmp` file that the next save overwrites. That file is a partial write, not a
backup; nothing reads it back.

---

### 5. Fast 0ms `ignore.devprune.json` & Per-Repo Settings
- **`ignore.devprune.json`**: File presence check running in **0ms O(1) latency** bypassing directory iteration without reading or parsing JSON file contents.
- **`.devprune.json`**: Per-repository configuration supporting `project_name`, `ignore`, `disable_daemon` (excluded from the scheduled pass only), `disable_hooks` (the global Git hook will not auto-register this repo), and the three tuning overrides `override_idle_days`, `min_size_mb` and `scan_depth`. Automatically recorded in the repository's `.git/info/exclude` when created, so it never shows up in `git status` and the shared `.gitignore` is never touched.

A `.devprune.json` that cannot be parsed is treated as a refusal to guess, not as a
missing file: the repository is skipped and the syntax error is printed. Falling back to
defaults would silently discard whatever the file said — including the `"ignore": true`
that was the whole reason it existed — and prune a repository that had opted out.

`.devprune.json` is a repository-tracked file, so it deliberately holds **inert data
only** — flags, a number, a display name. Nothing in it can name a command, a path to
delete, or a directory to add. Cloning an untrusted repository and running `devp` on it
must not be a way to execute anything.

---

### 6. Symlink and Junction Refusal
A bloat directory that is a symlink or a Windows junction points at storage the
repository does not own — in a monorepo it is typically the workspace root's real
`node_modules`. Deleting it recursively would reach outside the repository, so
`dev-prune` refuses and says so, rather than following the link:

```rust
if fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
    // reported as SkippedSymlink, never removed
}
```

The directory walk itself also runs with `follow_links(false)`, so no linked tree is
ever traversed, sized, or counted.

---

### 7. Nested Repository Boundary
A repository may contain several projects at several depths, and each is discovered,
verified and pruned on its own terms. That walk is bounded so it can never leave the
repository it was asked about or wander into dependency trees:

- **Depth cap of `scan_depth` levels** below the repository root — six by default,
  configurable globally and per repository. `config set` accepts `1`–`32` and rejects
  anything outside that range; the clamp to the same range survives only as the
  backstop for a hand-edited config file.
- **Never descends into** `node_modules`, `target`, `vendor`, `bower_components`, any
  directory containing `pyvenv.cfg`, or any hidden directory.
- **Never descends into a nested repository.** A directory containing its own `.git` is
  a submodule or a vendored checkout with its own history and its own idle state. It is
  pruned only when registered and pruned in its own right, never as part of its parent.

The `node_modules` exclusion is a correctness invariant as well as a performance one: a
dependency tree contains thousands of `package.json` files, and treating any of them as
a project would mean verifying and deleting inside somebody else's package.
