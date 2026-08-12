# 📚 `dev-prune` Documentation Hub

Welcome to the central documentation index for **`dev-prune`** (`devp`), a universal, lockfile-safe workspace maintenance CLI and background dependency cleaner built in Rust 1.85+ (Edition 2024).

This documentation hub is structured according to the **Diataxis Framework** (Tutorials, How-To Guides, Technical Reference, and Explanations) to provide clear navigation for users, contributors, system administrators, and AI pair programming agents.

---

<p align="center">
  <img src="../assets/banner.png" alt="dev-prune Banner" width="800" />
</p>

---

## 🗺️ Documentation Directory

### 📑 Technical Reference & Specifications
- **[CLI Command Reference](CLI_REFERENCE.md)**
  Complete reference for all 13 subcommands (`init`, `link`, `unlink`, `undo`, `run`, `status`, `config`, `restore`, `update`, `skill`, `setup`, `doctor`, `uninstall`), global flags (`--dry-run`, `--ignore-idle`, `-y`, `-v`, `-V`), status shortcuts, and aliases.
- **[System Architecture Entry Point](ARCHITECTURE.md)**
  High-level system architecture overview linking directly to HLD and LLD specifications.
- **[High-Level Design Specification (HLD)](architecture/HLD.md)**
  High-level system architecture, multi-layer design, data flow lifecycle, execution sequence diagrams, and diagram element breakdowns.
- **[Low-Level Design Specification (LLD)](architecture/LLD.md)**
  Low-level technical specification detailing crate module map (`src/`), data schemas (`registry.json`, `.devprune.json`), the `PackageManager` trait contract, multi-adapter conflict resolution, intra-repository project discovery, the atomic state file swap algorithm, binary aliasing, and CWD determinism.
- **[Safety Invariants & Risk Mitigation](SAFETY_INVARIANTS.md)**
  In-depth guide to the seven core safety invariants: `.git` boundary guards, two-tier lockfile pre-verification, hybrid activity solver (`git log` + source `mtime`), atomic state writes, 0ms fast-path ignore checks, symlink and junction refusal, and the nested repository boundary.

### 🛠️ How-To & Automation Guides
- **[GitHub Releases, DIY Manual Install & Source Build Guide](RELEASES_AND_MANUAL_INSTALL.md)**
  Step-by-step DIY manual installation guide for pre-built release binaries (Windows ZIP, macOS Intel/Silicon, Linux x64), manual build from source instructions, quick 1-liner installer scripts, and setup verification checks.
- **[Background Automation & Subsystems](BACKGROUND_AUTOMATION.md)**
  Guide to the self-installing `devp setup` pass and the two background subsystems it puts in place, including OS-native schedulers (Windows Task Scheduler, macOS LaunchAgent, Linux systemd user timers) and non-blocking Git hook auto-registration (`post-commit`, `post-checkout`, `post-merge`).
- **[Troubleshooting Directory & Synopsis](troubleshooting/README.md)**
  Central troubleshooting hub and sub-guides:
  - 🚀 [Installation, PATH & Permissions](troubleshooting/INSTALLATION_ISSUES.md)
  - 🔒 [Lockfile Sync & Ecosystem Adapter Errors](troubleshooting/LOCKFILE_AND_ADAPTERS.md)
  - 🤖 [Background Daemon & Git Hooks](troubleshooting/DAEMON_AND_HOOKS.md)
  - 🧹 [Uninstall, Reinstall & State Recovery](troubleshooting/UNINSTALL_AND_REINSTALL.md)
  - ⚠️ [Registry Corruption & Edge Cases](troubleshooting/CORRUPTION_AND_EDGE_CASES.md)
- **[Multi-Ecosystem Distribution & Packaging Manual](DISTRIBUTION.md)**
  Every install channel and what each one actually ships: the shell and PowerShell one-liners, the six checksummed GitHub release archives, `npx`/`npm install -g` via platform packages, `uv tool install`/`uvx`/`pipx`/`pip` via platform wheels, and `cargo install`.
- **[Releasing dev-prune](RELEASING.md)**
  The maintainer's guide: one-time registry setup and every credential the automation needs, what a tag push triggers, the changelog contract the release notes are built from, which registries review submissions (npm, PyPI and crates.io do not), why a Rust binary belongs on npm and PyPI, the gated channels (Homebrew, WinGet, Scoop, Chocolatey), and recovery when a release goes wrong.

### 🎓 Tutorials & Contribution Guides
- **[Adding New Ecosystem Adapters](ADDING_ADAPTERS.md)**
  Step-by-step tutorial for implementing the `PackageManager` Rust trait to add support for new package managers (e.g. Maven, Gradle, Composer, Mix, Swift SPM) along with `tempfile` unit testing protocols.
- **[Contributing Guide](../CONTRIBUTING.md)**
  Development environment setup, code style formatting (`cargo fmt`), linting standards (`cargo clippy`), unit testing (`cargo test`), local site development, and pull request submission checklist.

### 📊 Market Analysis & Positioning
- **[Market Analysis & Competitive Matrix](MARKET_ANALYSIS.md)**
  Detailed comparison of `dev-prune` against existing developer tools (`npkill`, `cargo-clean-all`, `pyclean`, `git clean`, `dust`/`ncdu`, `BleachBit`) and breakdown of Unique Selling Propositions (USPs).

---

## 🤖 AI Pair Programming & Agent Integration

`dev-prune` includes a token-efficient AI Skill definition located at [`.agents/skills/dev-prune/SKILL.md`](../.agents/skills/dev-prune/SKILL.md).

AI coding assistants (Gemini Antigravity, Claude Code, Cursor, Windsurf, Copilot, OpenClaw) can execute `devp skill` to inspect ready-to-copy AI onboarding prompts or run `devp` commands natively.
