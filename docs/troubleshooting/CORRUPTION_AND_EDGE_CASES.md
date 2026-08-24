# ⚠️ Registry Corruption, Symlinks & Edge Cases Troubleshooting

This guide addresses edge-case scenarios, state file recovery, symlink handling, network mounts, invalid JSON syntax, and disk space exhaustion.

---

<p align="center">
  <img src="../../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## 🔍 Issue Index

- [⚠️ Registry Corruption, Symlinks \& Edge Cases Troubleshooting](#️-registry-corruption-symlinks--edge-cases-troubleshooting)
  - [🔍 Issue Index](#-issue-index)
    - [1. Corrupted `registry.json` File](#1-corrupted-registryjson-file)
      - [Symptom](#symptom)
      - [Cause](#cause)
      - [Solution](#solution)
    - [2. Invalid `.devprune.json` Syntax](#2-invalid-devprunejson-syntax)
      - [Symptom](#symptom-1)
      - [Cause](#cause-1)
      - [Solution](#solution-1)
    - [3. Workspace Directory Renames](#3-workspace-directory-renames)
      - [Symptom](#symptom-2)
      - [Cause](#cause-2)
      - [Solution](#solution-2)
    - [4. Symlinked Directories](#4-symlinked-directories)
      - [Behavior](#behavior)
    - [5. Network Mounts \& NAS Drives](#5-network-mounts--nas-drives)
      - [Behavior](#behavior-1)
      - [Solution](#solution-3)
    - [6. Disk Space Exhaustion](#6-disk-space-exhaustion)
      - [Behavior](#behavior-2)
      - [Solution](#solution-4)

---

### 1. Corrupted `registry.json` File

#### Symptom
Executing any `devp` command returns `Failed to parse registry.json: syntax error`.

#### Cause
Manual editing. `dev-prune` never writes `registry.json` in place: it writes
`registry.json.tmp` in full and then renames it over the real file, so an interrupted
run leaves the previous registry intact and untouched.

⚠️ A leftover `registry.json.tmp` is **not** a backup — it is a write that never
finished, and it may be truncated. Do not copy it over `registry.json`.

#### Solution
The registry holds only the list of tracked repositories and your settings. Nothing in
it is unrecoverable, so the fix is to rebuild it:

1. Move the damaged file aside (in `%APPDATA%\dev-prune\` or `~/.config/dev-prune/`) so
   you can still read your old repository paths out of it:
   ```bash
   mv ~/.config/dev-prune/registry.json ~/registry.json.broken
   ```
2. Re-register your workspaces:
   ```bash
   devp init ~/Code
   ```
3. Re-apply any non-default settings, then confirm:
   ```bash
   devp config show
   ```

---

### 2. Invalid `.devprune.json` Syntax

#### Symptom
Warning printed during scan: `Failed to parse .devprune.json in /path/to/repo`.

#### Cause
Syntax error or invalid JSON formatting inside a workspace's `.devprune.json` file.

A repository whose `.devprune.json` cannot be parsed is treated as having no per-repo
config — its overrides (`ignore`, `override_idle_days`, `disable_daemon`,
`disable_hooks`) are **not** applied, so a repository you meant to exclude becomes a
prune candidate again. Fix it promptly.

#### Solution
Print the parse error, with the offending line and column:
```bash
devp config project /path/to/repo
```

Fix the syntax by hand if the overrides matter. If they do not, reset the file to a
valid default — this discards whatever was in it:
```bash
devp config project /path/to/repo --update
```

To sweep every registered repository at once:
```bash
devp config show --update
```

---

### 3. Workspace Directory Renames

#### Symptom
`devp status` displays `[Path Missing]` for a previously registered repository.

#### Cause
The workspace directory was moved or renamed on disk.

#### Solution
1. Unlink the old missing path:
   ```bash
   devp unlink /old/path/to/repo
   ```
2. Link the new path:
   ```bash
   devp link /new/path/to/repo
   ```

If several entries have gone missing rather than one — deleted clones, a reformatted
drive, a workspace tree that moved wholesale — clear them all in one pass instead of
naming each:

```bash
devp unlink --missing
```

It touches nothing on disk (those directories are already gone) and leaves every
registered path that still exists in place. `devp doctor` counts these entries and points
here; they are a warning, not breakage, because a prune pass reports them and carries on.

---

### 4. Symlinked Directories

#### Behavior
A bloat directory that is itself a symlink or a Windows junction is **never deleted**.
In a monorepo, `packages/app/node_modules` is routinely a link to the workspace root's
real tree, and deleting through it would take storage the repository does not own. The
run reports:

> `…/node_modules` is a symlink — refusing to delete linked storage. Remove the link yourself if you really want it gone.

Everything else in the repository is still pruned; only the linked directory is skipped.

Project discovery does not follow links either — the walk never descends into a
symlinked directory, so a link pointing outside the repository cannot pull the scan out
of it. Size totals are computed the same way, which also makes a link loop harmless.

---

### 5. Network Mounts & NAS Drives

#### Behavior
Lockfile sync commands on network mounts (SMB / NFS) may experience higher latency.

#### Solution
Increase command timeout when operating on network shares:
```bash
devp config set command_timeout_secs 1800
```

---

### 6. Disk Space Exhaustion

#### Behavior
Lockfile verification runs the package manager, and on a completely full disk the
package manager itself may fail to write its own temporary files. dev-prune then aborts
the deletion — a project it cannot prove rebuildable is never pruned. This is the
chicken-and-egg case: no space to verify, so no space reclaimed.

`--ignore-idle` does **not** help here. It lifts the idle-day threshold so
recently-touched repositories become eligible; it has no effect on verification, and
nothing does.

#### Solution
1. See what is available without deleting anything:
   ```bash
   devp run --dry-run
   ```
2. Free a few hundred megabytes by hand first — one `node_modules` in a project whose
   lockfile you can see is committed is enough:
   ```bash
   rm -rf ~/Code/some-old-project/node_modules
   ```
3. With headroom restored, run the normal pass and let verification do its job:
   ```bash
   devp run -y
   ```
