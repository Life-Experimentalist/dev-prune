# Market Analysis & Value Proposition: `dev-prune` (`devp`)

This document details the market landscape, competitive matrix, unique selling propositions (USPs), and strategic positioning of **`dev-prune`** (`devp`) relative to existing developer maintenance and disk cleanup tools.

---

## 🌐 Executive Summary

Software developers, polyglot engineering teams, and DevOps practitioners accumulate tens to hundreds of gigabytes of heavy build artifacts (`node_modules`, `.venv`, `venv`, `target`, `vendor`, `build`, `.next`) over time across dozens of inactive Git repositories. Existing tools fall into 4 main categories:

1. **Single-Ecosystem Interactors** (`npkill` for Node.js, `cargo-clean-all` / `cargo-sweep` for Rust, `pyclean` for Python): Excellent within their single-language niche, but require managing separate CLI tools per ecosystem.
2. **Naive Directory Cleaners** (`git clean -fdx`, raw shell scripts): Delete files destructively without verifying lockfile integrity or protecting uncommitted work.
3. **General Filesystem Visualizers** (`dust`, `ncdu`, `gdu`): Display disk usage by directory size, but lack domain knowledge of software package managers, lockfiles, or Git boundaries.
4. **Generic OS System Cleaners** (`BleachBit`, `CleanMyMac`): Target browser caches and OS temp folders, with zero awareness of developer environments or code safety.

`dev-prune` (`devp`) bridges this gap as the **first universal, lockfile-safe, multi-ecosystem workspace maintenance CLI and background automation tool**.

---

## 📊 Comprehensive Competitive Matrix

| Feature / Capability | `dev-prune` (`devp`) | `npkill` | `cargo-clean-all` | `pyclean` / `pyprune` | `git clean` | `dust` / `ncdu` | `BleachBit` |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **Language Runtime** | **Rust (1.85+)** | Node.js | Rust | Python | C / C++ | Rust / C | Python / C |
| **Multi-Ecosystem Coverage** | **✓ (JS/TS, Python, Rust, Go)** | ✗ (Node only) | ✗ (Rust only) | ✗ (Python only) | ✗ (All untracked) | ✗ (Generic FS) | ✗ (OS Caches) |
| **Many Projects per Repository** | **✓ (monorepo-aware discovery)** | ✗ | ✗ | ✗ | n/a | n/a | ✗ |
| **Git Repository Safety Boundary** | **✓ (`.git` enforced)** | ✗ | ✓ | ✗ | ✓ | ✗ | ✗ |
| **Pre-Deletion Lockfile Verification** | **✓ (Two-tier sync & verify)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Git Commit Log & File `mtime` Solver** | **✓ (Protect active repos)** | ✗ | ✓ (`mtime` only) | ✗ | ✗ | ✗ | ✗ |
| **1-Command Dependency Restore** | **✓ (`devp restore .`)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Cross-Platform Background Daemon** | **✓ (Task Scheduler / systemd)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Non-Blocking Git Hook Subsystem** | **✓ (post-commit/checkout/merge)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **0ms Fast-Path Ignore File** | **✓ (`ignore.devprune.json`)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Per-Repo Structured Config** | **✓ (`.devprune.json` + auto `.gitignore`)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Universal AI Agent Skill Integration** | **✓ (`SKILL.md` for AI agents)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Dual Binary & Auto-Alias** | **✓ (`dev-prune` & `devp`)** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |

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

### 5. 📁 Structured Per-Repo Config (`.devprune.json`) & Auto `.gitignore`
Allows per-repository overrides (`ignore: true`, `disable_daemon`, `disable_hooks`, and the three tuning knobs `override_idle_days`, `min_size_mb`, `scan_depth`). Automatically adds both per-repo files to `.gitignore` whenever `.devprune.json` is written. The file holds inert data only — no key in it names a command to run, deliberately, because it arrives with a `git clone`.

### 6. 🖼️ OS File Manager Icon Integration
`devp config icon` registers `*.devprune.json` with the operating system's own file manager — Explorer, Finder, Nautilus and friends — so the files are recognisable where you actually browse them, and writes the JSON Schema next to the icon so editors get validation and completion from the `$schema` link. It does not edit any editor's settings file; for editors it prints a snippet to paste, leaving that file yours.

### 7. 🤖 Native AI Agent Skill Integration
Includes a token-lean `SKILL.md` file enabling AI pair programming agents (Gemini Antigravity, Claude Code, Cursor, Windsurf, Copilot, OpenClaw) to safely inspect and manage workspace bloat natively.
