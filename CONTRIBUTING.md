# Contributing to `dev-prune`

Thank you for your interest in contributing to **`dev-prune`** (`devp`)! We welcome contributions from engineers of all experience levels.

`dev-prune` is written in modern **Rust (Edition 2024)** and designed to be highly modular, testable, and contribution-friendly.

---

## 🛠️ Prerequisites & Local Development Setup

### 1. Requirements
- **Rust Toolchain**: 1.85 or newer (`rustup update stable`)
- **Components**: `rustfmt`, `clippy` (`rustup component add rustfmt clippy`)
- **Node.js** (optional, for web landing page under `site/`): 20+

### 2. Clone & Build
```bash
git clone https://github.com/Life-Experimentalist/dev-prune.git
cd dev-prune

# Build debug binary
cargo build
```

Before opening a pull request, run the same four commands CI runs, on the same flags. The
flags matter: `--all-targets` is what puts test code under `clippy`, and without
`--all-features` a feature-gated path goes unchecked locally and fails in CI instead.

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
npm --prefix site run build
```

### 3. Windows: set the execution policy before running any `.ps1`

`scripts/install.ps1` and the other PowerShell scripts in this repository are *files*, and
PowerShell's execution policy governs files. The default for a user account is
`RemoteSigned`, so running one from your checkout stops with:

```
File install.ps1 cannot be loaded because running scripts is disabled on this system.
```

Scope the relaxation to the one process that needs it, rather than changing the machine:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 -NoAutoSetup
```

The setting dies with that process. Prefer it to `Set-ExecutionPolicy`, which changes the
policy for every script you run from then on — a permanent change to your machine bought
to run one script once. If you would rather set it for your own session only, that is
`Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass`, which lasts until you close
the window.

None of this applies to the published one-liner. `iwr … | iex` evaluates a string and
never creates a file, so there is nothing for the policy to govern — which is also why the
`-ExecutionPolicy Bypass` in the documented `cmd.exe` command is belt-and-braces rather
than load-bearing.

---

## 📦 How to Add a New Package Manager Adapter

`dev-prune` uses a simple, extensible [`PackageManager`](src/adapters/mod.rs) trait defined in [`src/adapters/mod.rs`](src/adapters/mod.rs).

To add support for a new ecosystem (e.g. `maven`, `gradle`, `composer`, `mix`, `swift`):

1. **Create module file**: Create `src/adapters/my_adapter.rs`.
2. **Implement trait**: Implement `PackageManager` trait (`name`, `detect`, `bloat_dirs`, `enforce_lockfile`, `restore`).
3. **Register adapter**: Add `pub mod my_adapter;` and register `Box::new(my_adapter::MyAdapter)` in `get_all_adapters()` inside `src/adapters/mod.rs`.
4. **Add unit tests**: Use `tempfile::TempDir` to test `detect`, `bloat_dirs`, name uniqueness, and lockfile safety.

See the detailed step-by-step tutorial: **[How to Add a New Package Manager Adapter (`docs/ADDING_ADAPTERS.md`)](docs/ADDING_ADAPTERS.md)**.

---

## 🧪 Local Testing Workflow

### Safe Local Installation
Install `dev-prune` locally to test subcommands without affecting global binaries:
```powershell
cargo install --path .
```

Run CLI commands:
```powershell
# Scan current directory
dev-prune init .

# Check status dashboard
dev-prune status

# Simulate prune pass safely
dev-prune run --dry-run
```

---

## 🎨 Local Site / Landing Page Development

If you'd like to test the React site landing page locally:
```bash
cd site
npm install
npm run dev
```

To test the pre-compiled production build:
```bash
npm run build
```

---

## 📜 Pull Request Checklist

Before submitting a pull request, verify:
- [ ] Code is formatted with `cargo fmt`
- [ ] Lints pass clean with `cargo clippy -- -D warnings`
- [ ] All unit and integration tests pass with `cargo test --all`
- [ ] New adapters or features include unit tests using `tempfile::TempDir`
- [ ] Documentation and comments are updated to reflect code changes
- [ ] Every new source file starts with the two-line licence header (see below)

---

## ⚖️ Licensing

`dev-prune` is Apache-2.0. Contributing means agreeing that your contribution is
licensed the same way — the terms are in
[section 5 of `LICENSE.md`](LICENSE.md), and there is no separate CLA to sign.

Every source file starts with these two lines, using whatever comment syntax the
language wants (`//`, `#`, `/* */`), directly below the shebang if there is one:

```rust
// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0
```

That is the short form of the boilerplate in the appendix of `LICENSE.md`. It says the
same thing in one line that automated licence scanners can read, which the thirteen-line
prose version does not.
