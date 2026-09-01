# High-Level Technical Design Specification (HLD): `dev-prune`

This document defines the High-Level Design (HLD) specification for **`dev-prune`** (`devp`), a universal, lockfile-safe workspace maintenance CLI and background automation tool written in Rust (edition 2024).

---

<p align="center">
  <img src="../../assets/github-readme-banner.png" alt="dev-prune Hero Banner" width="800" />
</p>

---

## 🏛️ 1. Executive System Architecture

`dev-prune` is architected as a modular, multi-layered CLI application. It acts as an automated workspace maintenance layer between developer filesystem repositories, multi-ecosystem package managers, and operating system background schedulers.

```mermaid
graph TD
    subgraph UI_Layer ["Interface & Entry Layer"]
        CLI["CLI Router (src/main.rs & src/lib.rs)"]
        TUI["Ratatui Terminal Dashboard (src/tui/)"]
    end

    subgraph Core_Layer ["Core Engine & Storage Layer"]
        RegistryManager["Registry Manager (src/config.rs)"]
        Engine["Pruning & Restore Engine (src/engine.rs)"]
        GitScanner["Git Activity Scanner (src/scanner/)"]
        Workspace["Project Discovery (src/workspace.rs)"]
        GlobalConfig["~/.config/dev-prune/registry.json"]
    end

    subgraph Adapter_Layer ["Multi-Ecosystem Adapter Subsystem"]
        AdapterRegistry["Adapter Manager (src/adapters/mod.rs)"]
        NPM["npm Adapter"]
        PNPM["pnpm Adapter"]
        Yarn["Yarn Adapter"]
        Bun["Bun Adapter"]
        UV["uv Adapter"]
        Venv["venv Adapter"]
        Cargo["Cargo Adapter"]
        Go["Go Adapter"]
    end

    subgraph Automation_Layer ["Background Automation Subsystem"]
        OSDaemon["OS Scheduler (Task Scheduler / LaunchAgent / systemd)"]
        GitHooks["Git Auto-Registration Hooks (post-commit/checkout/merge)"]
    end

    CLI --> RegistryManager
    CLI --> Engine
    TUI --> Engine
    RegistryManager <--> GlobalConfig
    Engine --> GitScanner
    Engine --> Workspace
    Workspace --> AdapterRegistry
    AdapterRegistry --> NPM & PNPM & Yarn & Bun & UV & Venv & Cargo & Go
    OSDaemon -->|Triggers `devp run --yes --daemon`| Engine
    GitHooks -->|Registers repos| RegistryManager
```
*Figure 1: High-Level System Architecture and Layer Interactions.*

#### Diagram Description & Element Breakdown
- **CLI**: Argument parsing, flag evaluation, binary alias handling (`dev-prune` and `devp`).
- **TUI**: Interactive Ratatui interface displaying space metrics, repository status, and inline actions.
- **RegistryManager**: Serde-backed registry reader/writer with atomic file swap protection.
- **Engine**: Orchestrates repository scanning, inactivity calculations, pre-deletion checks, and dependency restoration.
- **GitScanner**: Evaluates `.git` commit timestamps (`git log`) and fallback file modification timestamps (`mtime`).
- **Workspace**: Bounded walk of a registered repository that finds every package-manager project inside it, at any depth, so a monorepo's `frontend/`, `services/api/` and `cli/` are each handled on their own terms.
- **GlobalConfig**: Persistent JSON storage at `~/.config/dev-prune/registry.json` (or `%APPDATA%\dev-prune\registry.json`).
- **AdapterRegistry**: Interface managing concrete ecosystem package manager implementations.
- **The adapters**: Twenty-three of them, one per package manager, each enforcing lockfile safety and owning that ecosystem's directories. JavaScript (npm, pnpm, Yarn, Bun), Python (uv, Poetry, PDM, Pipenv, venv), Rust (Cargo), Go, PHP (Composer), Ruby (Bundler), Elixir (Mix, and Mix builds), Apple (CocoaPods, SwiftPM), Dart, Infrastructure (Terraform), JVM (Gradle, Maven) and C/C++ (vcpkg, CMake builds). Eight are opt-in; the list is in [`docs/CLI_REFERENCE.md`](../CLI_REFERENCE.md).
- **OSDaemon**: Native background task triggering periodic 2-day automated maintenance passes.
- **GitHooks**: Non-blocking background shell hooks registering newly checked-out Git repositories.

---

## 🔄 2. Data Flow & Execution Sequence

```mermaid
sequenceDiagram
    autonumber
    actor User as User / OS Scheduler
    participant CLI as CLI / TUI Layer
    participant Reg as Registry Manager
    participant Scan as Git Activity Scanner
    participant Eng as Prune Engine
    participant PM as Ecosystem Adapter
    participant FS as File System / Disk

    User->>CLI: Invokes `devp run`
    CLI->>Reg: Load registered repositories
    Reg-->>CLI: Return repository entries
    loop For each repository
        CLI->>FS: Check for `ignore.devprune.json`
        alt `ignore.devprune.json` exists
            FS-->>CLI: Fast-path skip (0ms latency)
        else Scan repository
            CLI->>Scan: Compute last activity timestamp
            Scan->>FS: Query `.git` commit log & source file `mtimes`
            Scan-->>CLI: Return activity timestamp
            alt Repository is Idle (>= idle_days) or --ignore-idle
                CLI->>Eng: Discover every project inside the repository
                Eng->>FS: Bounded walk (depth scan_depth, skipping bloat dirs & nested repos)
                FS-->>Eng: Project directories
                loop For each project in the repository
                    Eng->>PM: Detect matching package manager(s)
                    PM-->>Eng: Return detected adapters (conflicts already resolved)
                    Eng->>PM: Enforce lockfile verification pass
                    alt Lockfile Verification Succeeded
                        PM->>FS: Delete the adapter's directories (node_modules, .venv, vendor, ...)
                        FS-->>PM: Confirmed deletion & freed bytes
                        PM-->>Eng: Report successful prune
                    else Lockfile Verification Failed
                        PM-->>Eng: Abort deletion for this project
                        Eng-->>CLI: Log review error & shell fix snippet
                    end
                end
                Eng->>Reg: Update last_pruned_at timestamp
            else Repository is Active
                CLI-->>User: Skip (Active repository)
            end
        end
    end
    CLI->>Reg: Atomically persist updated registry state
```
*Figure 2: Complete Execution Lifecycle Sequence.*

#### Diagram Description & Element Breakdown
1. **Invokes `devp run`**: User or OS background scheduler triggers execution pass.
2. **Load registered repositories**: Registry Manager loads `~/.config/dev-prune/registry.json`.
3. **Fast-path skip**: Immediate 0ms skip if `ignore.devprune.json` is present in workspace root.
4. **Compute last activity timestamp**: Scanner checks `git log -1 %ct` and source file `mtime` modification timestamps. Activity is measured for the repository as a whole, so an actively-developed monorepo protects all of its projects.
5. **Discover every project**: Bounded walk of the repository returning each directory an adapter applies to; every one is then verified and pruned independently.
6. **Enforce lockfile verification**: Executes lockfile verification CLI command with timeout guard.
7. **Delete bloat directories**: Safe removal of target bloat directories upon successful lockfile verification.
8. **Abort deletion**: Immediately aborts deletion if lockfile verification fails, preserving workspace safety. One project aborting never stops the others.
9. **Atomically persist**: Atomically writes updated state to `registry.json`.

---

## 🎯 3. System Boundaries & Guarantees

> [!IMPORTANT]
> **Git Repository Boundary Guarantee**: Operations are strictly restricted to folders containing a valid `.git` root. Non-Git folders are ignored.

> [!NOTE]
> **Two-Tier Lockfile Pre-Verification**: Before deleting any directory an adapter claims (`node_modules`, `.venv`, `vendor`, `Pods` and the rest), `dev-prune` verifies that ecosystem's lockfile (`package-lock.json`, `pnpm-lock.yaml`, `uv.lock`, `Cargo.lock`, `go.sum`, `composer.lock`, `Gemfile.lock`, `mix.lock` — one per adapter).

> [!NOTE]
> **Any Number of Ecosystems per Repository**: A registered repository may hold any number of projects, in any combination — three managers side by side in the root, one per subtree, or both at once. Each is detected, verified, pruned and restored independently. The discovery walk never leaves the repository, never enters a dependency tree, and never crosses into a nested `.git`.

> [!TIP]
> **Privacy**: `dev-prune` collects no analytics, diagnostics or usage data. The only network request it makes is a release check against GitHub's public API — no body, no identifier, at most weekly, and switched off by `devp config set update_check false`. See [PRIVACY.md](../PRIVACY.md).
