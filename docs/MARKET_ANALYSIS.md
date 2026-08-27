# Market Analysis & Value Proposition: `dev-prune` (`devp`)

This document details the market landscape, competitive matrix, unique selling propositions (USPs), and strategic positioning of **`dev-prune`** (`devp`) relative to existing developer maintenance and disk cleanup tools.

---

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## 🌐 Executive Summary

Software developers, polyglot engineering teams, and DevOps practitioners accumulate tens to hundreds of gigabytes of heavy dependency and cache directories (`node_modules`, `.venv`, `venv`, `target`, `vendor`) over time across dozens of inactive Git repositories — every one rebuildable from a lockfile. (Build *outputs* like `dist/` or `.next/` are deliberately outside dev-prune's scope: it only deletes what a lockfile can prove restorable.) Existing tools fall into 4 main categories:

1. **Multi-Ecosystem Interactive Scanners** (`kondo`): The closest neighbour, and the only other tool in this list that spans ecosystems. It recognises about twenty project types, reports what each costs, and deletes what you confirm — its README describes it as "essentially `rm -rf` with a prompt". The prompt *is* the safety mechanism, which is a sound design for a supervised sweep and the one thing that cannot be carried over to an unattended one.
2. **Single-Ecosystem Interactors** (`npkill` for Node.js, `cargo-clean-all` / `cargo-sweep` for Rust, `pyclean` for Python): Excellent within their single-language niche, but require managing separate CLI tools per ecosystem.
3. **Naive Directory Cleaners** (`git clean -fdx`, raw shell scripts): Delete files destructively without verifying lockfile integrity or protecting uncommitted work.
4. **General Filesystem Visualizers** (`dust`, `ncdu`, `gdu`): Display disk usage by directory size, but lack domain knowledge of software package managers, lockfiles, or Git boundaries.
5. **Generic OS System Cleaners** (`BleachBit`, `CleanMyMac`): Target browser caches and OS temp folders, with zero awareness of developer environments or code safety.

`dev-prune` (`devp`) occupies the gap category 1 leaves open: a multi-ecosystem cleaner whose safety does not depend on somebody being at the keyboard. Every deletion is gated on the package manager's own dry-run rather than on a confirmation prompt, which is what makes an unattended schedule defensible at all.

---

## 📊 Comprehensive Competitive Matrix

| Feature / Capability | `dev-prune` (`devp`) | `kondo` | `npkill` | `cargo-clean-all` | `pyclean` / `pyprune` | `git clean` | `dust` / `ncdu` | `BleachBit` |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **Language Runtime** | **Rust** | Rust | Node.js | Rust | Python | C / C++ | Rust / C | Python / C |
| **Multi-Ecosystem Coverage** | **✓ (23 managers)** | ✓ (~20 project types) | ✗ (Node only) | ✗ (Rust only) | ✗ (Python only) | ✗ (All untracked) | ✗ (Generic FS) | ✗ (OS Caches) |
| **Many Projects per Repository** | **✓ (monorepo-aware discovery)** | ✓ (walks the tree) | ✗ | ✗ | ✗ | n/a | n/a | ✗ |
| **Git Repository Safety Boundary** | **✓ (`.git` enforced)** | ✗ | ✗ | ✓ | ✗ | ✓ | ✗ | ✗ |
| **Pre-Deletion Lockfile Verification** | **✓ (Two-tier sync & verify)** | ✗ (confirmation prompt) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Git Commit Log & File `mtime` Solver** | **✓ (Protect active repos)** | ✗ (`--older`, `mtime` only) | ✗ | ✓ (`mtime` only) | ✗ | ✗ | ✗ | ✗ |
| **1-Command Dependency Restore** | **✓ (`devp restore .`)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Cross-Platform Background Daemon** | **✓ (Task Scheduler / systemd)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Non-Blocking Git Hook Subsystem** | **✓ (post-commit/checkout/merge)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Machine-Readable Output** | **✓ (`--json`, versioned contract)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Package-Manager Cache Reporting** | **✓ (`devp caches`, per-manager cap)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **0ms Fast-Path Ignore File** | **✓ (`ignore.devprune.json`)** | ✓ (`--ignored-dirs`) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Per-Repo Structured Config** | **✓ (`.devprune.json`, kept out of `git status` automatically)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Universal AI Agent Skill Integration** | **✓ (`SKILL.md` for AI agents)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Graphical Interface** | ✗ (TUI only) | **✓ (`kondo-ui`)** | ✓ (TUI) | ✗ | ✗ | ✓ (TUI) | ✓ | ✓ |
| **Dual Binary & Auto-Alias** | **✓ (`dev-prune` & `devp`)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |

---

## 💎 Unique Selling Propositions (USPs)

### 1. 🔒 Two-Tier Lockfile Safety Guarantee
Unlike `npkill` or `git clean` which execute unrecoverable deletions, `dev-prune` verifies lockfile integrity (`package-lock.json`, `pnpm-lock.yaml`, `uv.lock`, `Cargo.lock`, `go.sum`) before deleting any bloat directory. If a lockfile exists on disk, pruning proceeds safely because dependencies can be restored anytime with `devp restore`.

### 2. ⚡ Instant 0ms Fast-Path Ignored Repository Scanner
`dev-prune` checks for the presence of `ignore.devprune.json` in repository roots with **0ms O(1) latency**, bypassing scanning without reading or parsing JSON files.

### 3. 🕒 Hybrid Inactivity Solver (Git Commit Log + Source `mtime`)
`dev-prune` inspects `git log` commit history and source file modification timestamps (`mtime`) to compute repository inactivity. Active projects worked on in the past 15 days (configurable) are automatically protected.

### 4. 🤖 Self-Installing Background Automation (Daemon + Git Hooks)
`dev-prune` installs native operating system background schedulers (Windows Task Scheduler, macOS LaunchAgent, Linux systemd user timer) and non-blocking Git hooks (`post-commit`, `post-checkout`, `post-merge`) for automated registration — at install time, and again after an upgrade if anything is missing. A pruner you have to remember to enable is a pruner that never runs. The pass declines rather than forces: it skips the hooks when `git` is absent from `PATH`, or when `core.hooksPath` already belongs to husky, pre-commit or lefthook and chaining has not been asked for — `devp hook install --chain` takes the single global slot and forwards every hook back to that tool instead. `auto_setup` / `auto_hooks` / `auto_hooks_chain` / `auto_daemon` / `DEV_PRUNE_NO_AUTO_SETUP=1` turn parts or all of it off.

### 4b. 🧩 Any Number of Ecosystems per Repository
Competing tools assume one repository is one project. `dev-prune` discovers every package-manager project inside a repository — uv, npm and cargo side by side in the root, or spread across `frontend/`, `services/api/` and `tools/cli/` — and verifies, prunes and restores each on its own terms. Where several managers claim the same directory (npm/pnpm/yarn/bun over `node_modules`), it picks the one that actually built the tree on disk rather than guessing.

### 5. 📁 Structured Per-Repo Config (`.devprune.json`) That Never Dirties `git status`
Allows per-repository overrides (`ignore: true`, `disable_daemon`, `disable_hooks`, and the three tuning knobs `override_idle_days`, `min_size_mb`, `scan_depth`). Whenever `.devprune.json` is written, both per-repo files are recorded in the repository's `.git/info/exclude` — the config is one machine's preference, so it stays invisible to `git status` without dev-prune ever appending to the shared, tracked `.gitignore`. The file holds inert data only — no key in it names a command to run, deliberately, because it arrives with a `git clone`.

### 6. 🖼️ OS File Manager Icon Integration
`devp config icon` registers `*.devprune.json` with the operating system's own file manager — Explorer, Finder, Nautilus and friends — so the files are recognisable where you actually browse them, and writes the JSON Schema next to the icon so editors get validation and completion from the `$schema` link. It does not edit any editor's settings file; for editors it prints a snippet to paste, leaving that file yours.

### 7. 🤖 Native AI Agent Skill Integration
Includes a token-lean `SKILL.md` file enabling AI pair programming agents (Gemini Antigravity, Claude Code, Cursor, Windsurf, Copilot, OpenClaw) to safely inspect and manage workspace bloat natively.

---

## 🥊 The Closest Neighbour: `kondo`

[`kondo`](https://github.com/tbillington/kondo) (2,385 stars as of 2026-08-27, Rust,
MIT) is the only other tool in this analysis that solves the same shape of problem:
one binary, many ecosystems, heavy directories reclaimed. It predates `dev-prune`,
it is packaged more widely — `winget install kondo`, `brew install kondo`,
`pacman -S kondo`, `port install kondo` — and it ships a desktop GUI (`kondo-ui`)
that `dev-prune` has no equivalent of. Anyone evaluating `dev-prune` should evaluate
`kondo` first, and for one of the two use cases below they should stop there.

### What it does, precisely

`kondo` walks the directories you name, recognises project types by their marker files
(Cargo, CMake, Composer, Elixir, Godot, Gradle, Jupyter, Pixi, Maven, Node, Pub,
Python, SBT, Stack, Cabal, Swift, Unity, Unreal, Zig, .NET, Turborepo, Terraform,
React Native), reports the reclaimable size of each, and deletes what you confirm.
Its full option surface is small and well chosen: `--all`, `--older <n>`, `--quiet`,
`--follow-symlinks`, `--same-filesystem`, `--dry-run`, `--single-key`, shell
completions.

Its README states the trade-off itself, in a warning it repeats twice:

> Kondo is *essentially* `rm -rf` with a prompt. Use at your own discretion. Always
> have a backup of your projects.

That is not a criticism to level at it — it is the correct description of a
supervised tool, and stating it up front is better engineering than most projects
manage. The consequence is architectural: **the human at the prompt is the safety
mechanism**, and everything that follows from that shapes both tools.

### Where they diverge

| | `kondo` | `dev-prune` |
| :--- | :--- | :--- |
| **Deletion gate** | Your confirmation | The package manager's own dry-run exiting `0` |
| **"Not in use" test** | `--older`, file `mtime` | `git log` activity, with `mtime` as a second signal |
| **After deletion** | Reinstall by hand | `devp restore` replays the verified lockfiles |
| **Unattended operation** | Not designed for it | The primary mode; OS scheduler + git hooks |
| **Scripting** | Human-readable output | `--json`, a contract stable since 1.0.0 |
| **Build output** | Deleted (`target/`, `build/`) | Never by detection; only where declared with a rebuild command |
| **Caches** | Not covered | `devp caches`, with a per-manager size ceiling |
| **Interface** | CLI + native GUI | CLI + TUI |

Three of those rows are the same decision seen from different sides. `dev-prune` runs
`npm ci --dry-run`, `pnpm install --lockfile-only`, `uv lock --locked`,
`cargo metadata --locked`, `bundle lock --check` — the manager's own answer to "can
this be rebuilt?" — because on a schedule there is nobody to ask. `kondo` does not
need that check, because there is always somebody to ask. Neither is a workaround for
the other.

The build-output row is the one real disagreement rather than a difference of mode.
`kondo` deletes `target/` and `build/` because they are large and rebuildable, which is
true. `dev-prune` refuses to delete anything it cannot prove is restorable from a
lockfile, and a compile is not a download: it costs minutes, it can fail on a machine
that no longer has the toolchain, and no lockfile describes its output. Where a user
*wants* that directory gone, the route is an explicit declaration in
`project.devprune.json` carrying the command that puts it back — see
[`SAFETY_INVARIANTS.md`](SAFETY_INVARIANTS.md). That is deliberately more work than
`kondo` asks for, and it is the whole reason an unsupervised pass is defensible.

### Honest recommendation

- **Cleaning up by hand, deciding case by case, wanting a GUI, or wanting build
  artefacts gone with the least ceremony:** use `kondo`. It is mature, broadly
  packaged, and better at that job.
- **Wanting the machine to stay clean on a timer, with a refusal rather than a
  deletion when a lockfile no longer resolves, and a way back if it was wrong:**
  that is what `dev-prune` was built for, and `kondo` does not attempt it.

Neither answer is "it depends on your ecosystem" — both cover most of them. It
depends entirely on whether you intend to be watching.

### The rest of the field

`kondo`'s own README points at The Tin Summer, Detox, Sweep, npkill, Cargo Cleanall,
Cargo Sweep, Cargo Wipe and cargo-clean-recursive. All of them sit in categories 2–3
above: single-ecosystem, or multi-ecosystem without a verification step. As of this
writing no tool in the field runs the package manager's own dry-run before deleting,
which is `dev-prune`'s only genuinely unclaimed ground — and it is unclaimed because
it is only worth the cost if you intend to run unattended.
