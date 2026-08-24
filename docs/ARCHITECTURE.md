# 🏗️ `dev-prune` System Architecture

Welcome to the technical architecture guide for **`dev-prune`** (`devp`), a cross-platform, multi-ecosystem workspace maintenance CLI and background automation tool written in Rust (edition 2024).

---

## 🖼️ System Visual Overview

![dev-prune Hero Banner](../assets/github-readme-banner.png)

---

## 🏛️ Architecture Specifications

The system architecture and technical design specifications are divided into two dedicated documents:

### 1. 📐 [High-Level Design Specification (HLD)](architecture/HLD.md)
- Multi-layer system architecture (Interface Layer, Core Engine, Adapter Subsystem, Background Automation).
- Component interaction flowcharts and sequence diagrams.
- Execution lifecycle sequences and system boundaries.

### 2. 🔬 [Low-Level Design Specification (LLD)](architecture/LLD.md)
- Complete crate module map (`src/`).
- Data schemas for global `registry.json` and per-repository `.devprune.json`.
- `PackageManager` trait contract, multi-adapter resolution and intra-repository project discovery.
- Atomic state file swap algorithm (`registry.json.tmp` -> `registry.json`).
- Binary alias linkage (`dev-prune` <-> `devp`) and CWD determinism mechanics.

---

## 🔄 Architectural Overview

```mermaid
flowchart TD
    CLI["CLI Router & TUI Interface<br/>(src/main.rs & src/lib.rs)"] --> Registry["Registry & Config Manager<br/>(src/config.rs)"]
    CLI --> Engine["Pruning & Restore Engine<br/>(src/engine.rs)"]

    Registry <--> StateFile["~/.config/dev-prune/registry.json"]

    Engine --> FastIgnore{ignore.devprune.json?}
    FastIgnore -->|Exists| Skip["Skip Repo O(1) 0ms"]
    FastIgnore -->|Missing| ReadPerRepo["Read .devprune.json"]

    ReadPerRepo --> Scanner["Git Activity Scanner<br/>(src/scanner/git.rs)"]
    Engine --> Discover["Project Discovery<br/>(src/workspace.rs)"]
    Discover -->|"every project in the repo"| Adapters["Multi-Ecosystem Adapters<br/>(src/adapters/)"]

    Adapters --> LockfileVerify{"Lockfile Pre-Verification"}
    LockfileVerify -->|Success| Delete["Safely Remove Bloat Directories"]
    LockfileVerify -->|Failure| Abort["Abort Deletion & Log Fix Snippet"]

    Daemon["OS Background Daemon Scheduler"] -->|Every 2 Days| Engine
    GitHooks["Git Auto-Registration Hooks"] -->|On Git Activity| Registry
```
*Figure 1: High-level overview of system components.*
