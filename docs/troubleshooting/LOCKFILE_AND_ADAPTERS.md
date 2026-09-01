# 🔒 Lockfile Sync & Ecosystem Adapter Troubleshooting

This guide details diagnostics, configuration overrides, and fix command workflows for package manager lockfile sync failures, missing ecosystem binaries, timeouts, and network errors.

---

<p align="center">
  <img src="../../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## 🔍 Issue Index

- [🔒 Lockfile Sync \& Ecosystem Adapter Troubleshooting](#-lockfile-sync--ecosystem-adapter-troubleshooting)
  - [🔍 Issue Index](#-issue-index)
    - [1. `npm lockfile sync failed`](#1-npm-lockfile-sync-failed)
      - [Symptom](#symptom)
      - [Cause](#cause)
      - [Solution](#solution)
    - [2. `Command timed out after 600s`](#2-command-timed-out-after-600s)
      - [Symptom](#symptom-1)
      - [Cause](#cause-1)
      - [Solution](#solution-1)
    - [3. Package Manager Binary Missing](#3-package-manager-binary-missing)
      - [Symptom](#symptom-2)
      - [Cause](#cause-2)
      - [Solution](#solution-2)
    - [4. Offline Network Failures](#4-offline-network-failures)
      - [Symptom](#symptom-3)
      - [Cause](#cause-3)
      - [Solution](#solution-3)
    - [5. `--ignore-idle` does not bypass lockfile verification](#5---ignore-idle-does-not-bypass-lockfile-verification)
    - [6. The Wrong JavaScript Manager Was Chosen](#6-the-wrong-javascript-manager-was-chosen)
    - [7. A Project Inside the Repository Was Not Found](#7-a-project-inside-the-repository-was-not-found)

---

### 1. `npm lockfile sync failed`

#### Symptom
During `devp run`, pruning a Node.js repository aborts with `npm lockfile sync failed: npm ci --dry-run --ignore-scripts failed (exit code Some(1))`.

#### Cause
`dev-prune` enforces lockfile integrity before deleting any dependency directory — `node_modules`, `.venv`, `vendor`, `Pods`, whichever of the twenty-three adapters matched — and that check is **read-only**: it proves the lockfile can rebuild the tree, and refuses rather than repairing a lockfile that cannot. This error occurs if:
1. `npm` (or `pnpm`, `yarn`, `bun`, `uv`, `cargo`, `go`) returns a non-zero exit code because the lockfile has drifted from the manifest, or because of a corrupted lockfile or peer dependency conflicts.
2. Network resolution failed during the package verification pass.
3. The command exceeded the default 10-minute timeout limit.

#### Solution
Copy and execute the exact shell-specific fix snippet printed in the error log. It is the *writing* form of the same check — the one dev-prune will not run for you, because a prune pass can be started by the scheduler and must never leave a modified tracked file behind:

- **PowerShell (Windows)**:
  ```powershell
  cd "V:\Path\To\YourRepo"; npm install --package-lock-only --ignore-scripts
  ```
- **Bash / Zsh (Linux / macOS)**:
  ```bash
  cd "/path/to/your-repo" && npm install --package-lock-only --ignore-scripts
  ```

If you would rather dev-prune resynced the lockfile itself during the pass, that is an
informed opt-in, and it applies to every ecosystem:

```bash
devp config set allow_manifest_rewrite true
```

---

### 2. `Command timed out after 600s`

#### Symptom
Pruning aborts with `Command timed out after 600s: npm ci --dry-run --ignore-scripts`.

#### Cause
Large monorepos or slow network connections may require more than the default 600 seconds (10 minutes) for package resolution.

#### Solution
Increase the global command timeout setting using `devp config`:
```powershell
# Increase timeout to 20 minutes (1200 seconds)
devp config set command_timeout_secs 1200
```

---

### 3. Package Manager Binary Missing

#### Symptom
`devp run` reports: `` `uv` is not available and no lockfile was found at `uv.lock`. Cannot safely delete dependencies ``.

#### Cause
The required ecosystem package manager binary is missing from your system `PATH` and no lockfile exists on disk.

#### Solution
1. Install the missing package manager CLI (`uv`, `pnpm`, `cargo`, `go`, etc.).
2. If a lockfile already exists on disk, `dev-prune`'s Two-Tier Safety mechanism will automatically allow safe pruning even if the CLI binary is absent!

---

### 4. Offline Network Failures

#### Symptom
Running `devp run` while disconnected from the internet causes lockfile verification calls to hang or fail.

#### Cause
Some package managers attempt to reach online registries during verification passes.

#### Solution
Every verification command runs under `command_timeout_secs`, so an offline run fails
rather than hanging indefinitely. If a package manager cannot reach its registry,
dev-prune falls back to accepting an existing, committed lockfile on disk — no network
required. Raise the timeout if your connection is merely slow:

```bash
devp config set command_timeout_secs 1200
```

If a repository has no lockfile at all, dev-prune refuses to prune it. That is not an
error to work around: without a lockfile the deleted dependency tree cannot be
reinstalled. Commit one first.

---

### 5. `--ignore-idle` does not bypass lockfile verification

`--ignore-idle` overrides **only** the idle-day threshold, letting you prune a repository
you are actively working in:

```bash
devp run --ignore-idle
```

Lockfile verification is not overridable by any flag. If a repo fails verification, the
run prints the exact sync command for that ecosystem — run it, then re-run `devp run`.

*`--ignore-idle` also still enforces `.git` repository boundaries, the size floor, the
symlink refusal, and `ignore.devprune.json` fast-path rules.*

This flag was called `--force` before 1.0.0. The old spelling still works, and prints
both the rename note and the full list of the seven reasons a directory gets skipped —
only one of which `--ignore-idle` is the answer to.

---

### 6. The Wrong JavaScript Manager Was Chosen

#### Symptom
A directory contains more than one JS lockfile (say `package-lock.json` and
`pnpm-lock.yaml`), and `devp` verifies with the one you no longer use.

#### Cause
npm, pnpm, yarn and bun all own the same `node_modules`, so exactly one of them runs.
dev-prune picks it from, in order: the `packageManager` field in `package.json`; the
bookkeeping files the installer left inside `node_modules`; the most recently written
lockfile.

#### Solution
Say so explicitly — this also fixes the same ambiguity for Corepack, CI and your
teammates:

```bash
npm pkg set packageManager="pnpm@9.1.0"
```

Then delete the stale lockfile and commit both changes. A leftover lockfile from a
finished migration is worth removing regardless.

---

### 7. A Project Inside the Repository Was Not Found

#### Symptom
A monorepo prunes some of its projects but not one you expected.

#### Cause
Discovery is deliberately bounded. It stops at `scan_depth` directory levels below the
repository root — six by default — and never descends into `node_modules`, `target`, `vendor`,
`bower_components`, any directory containing `pyvenv.cfg`, any hidden directory, or any
directory with its own `.git`.

#### Solution
- **Nested `.git`** (a submodule or vendored checkout): register it in its own right
  with `devp link <path>`. It has its own history and its own idle state, so it is
  never pruned as part of its parent.
- **Deeper than `scan_depth` levels**: raise the cap, for this repository only or for
  every one. The cap exists to keep the walk out of large trees, so raise it as far as
  the tree actually needs and no further — `config set` accepts `1`–`32` and rejects
  anything outside that range.

  ```bash
  devp config set scan_depth 10          # everywhere
  devp config project ~/Code/deep-mono   # then set "scan_depth": 10 in its .devprune.json
  ```

  Registering the subdirectory in its own right with `devp link <path>` also works, and
  is the better answer when it is genuinely a separate project rather than a deep one.
- **Bloat directory is a symlink or junction**: it points at storage the repository
  does not own — typically a workspace root's real `node_modules` — and dev-prune
  refuses to delete through a link. Prune the workspace root instead.
