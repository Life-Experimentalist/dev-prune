# `dev-prune` (`devp`) Multi-Ecosystem Distribution & Packaging Manual

Every way to install **`dev-prune`** (`devp`) v1.1.0, what each channel actually ships, and the security guarantees behind them. For the maintainer's side — credentials, registry policies, what to do when a release fails — see [RELEASING.md](RELEASING.md).

---

## 🔒 Security Audit & Privacy Guarantees

- **No analytics or diagnostics**: `dev-prune` never transmits workspace directory structures, repository names, user file paths, or usage data. Its single network request is a release check against GitHub's public API — see [PRIVACY.md](PRIVACY.md).
- **Subprocess Command Injection Prevention**: All lockfile verification commands (`npm`, `pnpm`, `yarn`, `bun`, `uv`, `cargo`, `go`) execute binary targets directly via `std::process::Command` without shell expansion.
- **Atomic State Storage**: `registry.json` is never written in place. Each update is written in full to a `.tmp` file and then renamed over the target, so an interrupted or failed write leaves the previous registry intact rather than a half-written one.
- **Sandboxed Scope**: File operations are strictly bounded to verified Git workspaces (`.git` presence) and named bloat folders (`node_modules`, `.venv`, `venv`, `target`, `vendor`).

---

## 🚀 Active & Live Distribution Channels

### 1. Universal One-Liner Shell Installer (macOS & Linux) — **LIVE**
```bash
curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
```
- Installs the pre-built binary to the platform config directory: `~/Library/Application Support/dev-prune/bin/` on macOS, `$XDG_CONFIG_HOME/dev-prune/bin/` (default `~/.config/…`) on Linux — the same directory dev-prune reads its registry from.
- Verifies the download against the release's published SHA-256 checksum and refuses to install if it does not match or is absent.
- Adds that directory to `PATH` in whichever of `.zshrc`, `.bashrc` and `config.fish` exist, and prints the `export` line to paste when none do. `devp` is a second copy of the binary in the same directory, not a shell alias, so it works in every shell — including ones whose startup file was never touched.
- Runs `dev-prune setup` as its last step, installing the integrations described in [Background Automation](BACKGROUND_AUTOMATION.md). `--no-auto-setup` installs the binary and nothing else, and `devp uninstall` reverses it either way.
- Registers **no** repositories. Which directories to track stays your decision: run `dev-prune init <dir>` yourself.
- `curl … | sh` runs in a child process, so it cannot change the PATH of the shell you typed it in. Open a new terminal, or run the `export` line the installer prints. (The PowerShell one-liner *can*, and does — see below.)
- Options: `--version <tag>`, `--bin-dir <dir>`, `--no-path`, `--no-auto-setup`, `--help`. Piping into a shell needs `-s --` to reach them:
  ```bash
  curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --no-auto-setup
  ```
  The environment variables `DEV_PRUNE_VERSION`, `DEV_PRUNE_BIN_DIR`, `DEV_PRUNE_NO_PATH=1` and `DEV_PRUNE_NO_AUTO_SETUP=1` do the same and work with the plain one-liner. An option wins over its variable.

### 2. Windows One-Liner PowerShell Installer — **LIVE**
```powershell
iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
```
- Downloads the pre-built Windows executable to `%APPDATA%\dev-prune\bin\dev-prune.exe` and verifies its published SHA-256 checksum before installing.
- Registers that directory in the User `PATH`, **and** in the current session's `$env:PATH`. `iwr … | iex` runs inside your own process, so `devp` works on the very next line you type — `iwr -useb … | iex; devp init ~/Code` behaves exactly as written.
- Installs `devp.exe` as a second copy of the binary rather than a `$PROFILE` function, so it works in cmd, PowerShell, Git Bash, an IDE terminal and a scheduled task alike, and cannot go stale against a profile nobody re-sources.
- Replaces a running executable safely: the old image is renamed aside before the new one is copied in, which is the only thing Windows permits while a prune pass happens to be mid-run.
- Runs `dev-prune setup` as its last step, installing the integrations described in [Background Automation](BACKGROUND_AUTOMATION.md). `-NoAutoSetup` installs the binary and nothing else, and `devp uninstall` reverses it either way.
- Registers **no** repositories. Run `dev-prune init <dir>` yourself.
- Parameters: `-Version <tag>`, `-BinDir <dir>`, `-NoPath`, `-NoAutoSetup`, `-Help`. `iwr … | iex` runs the script as a bare expression, which has nowhere to put arguments, so passing one means running it as a script block:
  ```powershell
  & ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -NoAutoSetup
  ```
  The environment variables `DEV_PRUNE_VERSION`, `DEV_PRUNE_BIN_DIR`, `DEV_PRUNE_NO_PATH=1` and `DEV_PRUNE_NO_AUTO_SETUP=1` do the same and work with the plain one-liner. A parameter wins over its variable.
- From `cmd.exe`, which has no `Invoke-WebRequest`, the same script runs through PowerShell:
  ```bat
  powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"
  ```
  The install is identical. The one thing `cmd` loses is the current-session PATH update above — a parent shell cannot inherit the environment of the child it spawned — so `devp` resolves in the next Command Prompt rather than immediately. `-ExecutionPolicy Bypass` is defensive rather than required: the policy governs script *files*, and `iwr … | iex` never creates one.

### 3. Pre-Compiled GitHub Release Binaries
Six single-binary archives are built automatically for every tagged release and attached to [GitHub Releases](https://github.com/Life-Experimentalist/dev-prune/releases), each with a `.sha256` sidecar in `sha256sum` format:

| Asset | Rust target |
|---|---|
| `dev-prune-v1.1.0-windows-x64.zip` | `x86_64-pc-windows-msvc` |
| `dev-prune-v1.1.0-windows-arm64.zip` | `aarch64-pc-windows-msvc` |
| `dev-prune-v1.1.0-darwin-x64.tar.gz` | `x86_64-apple-darwin` |
| `dev-prune-v1.1.0-darwin-arm64.tar.gz` | `aarch64-apple-darwin` |
| `dev-prune-v1.1.0-linux-x64.tar.gz` | `x86_64-unknown-linux-musl` |
| `dev-prune-v1.1.0-linux-arm64.tar.gz` | `aarch64-unknown-linux-musl` |

The Linux binaries are statically linked against musl. There is no glibc version floor and no per-distribution build: the same `linux-x64` archive runs on Debian, Fedora, Arch, NixOS and Alpine. Pick by CPU architecture and nothing else.

The install scripts construct these filenames by hand and refuse to install without the matching `.sha256`, so the naming is a contract rather than a convention.

### 4. NPM — packaging exists, channel currently off
Nothing is on the npm registry today: the `publish-npm` release job is gated behind the
`NPM_PUBLISH` repository variable, which is set to `false`. The packaging under `npm/`
still builds on every release and is described here so it can be turned back on without
re-deriving it — the bootstrap steps are in [RELEASING.md](RELEASING.md).

- The tarball **contains the binary**. There is no `postinstall` download step, so the package installs correctly under `npm ci --ignore-scripts`, behind a corporate registry mirror, and with no network access to GitHub.
- Seven packages make that work: six platform packages (`dev-prune-linux-x64`, `dev-prune-darwin-arm64`, `dev-prune-win32-x64`, …), each carrying one executable and declaring `os`/`cpu`, plus the `dev-prune` dispatcher that lists all six as `optionalDependencies`. npm resolves exactly the one that matches the machine and skips the rest.
- Both `dev-prune` and `devp` are registered as `bin` entries.
- When publishing is on, every tarball carries [npm provenance](https://docs.npmjs.com/generating-provenance-statements) — a signed attestation tying it to the workflow run, commit and tag that produced it.

### 5. PyPI (`uv tool install` / `uvx` / `pipx` / `pip`)
```bash
uv tool install dev-prune     # persistent
uvx dev-prune status          # run once, nothing left behind
pipx run dev-prune status
pip install dev-prune
```
- Six platform wheels, each a zip holding the prebuilt executable under `dev_prune-<version>.data/scripts/`. No Python runs, no compiler is invoked, and no build backend is involved — installers unpack the binaries straight into the environment's `bin`/`Scripts` directory.
- The Linux wheels carry both `manylinux` and `musllinux` tags from the same static binary, so Debian and Alpine users are both served.
- Uploaded through PyPI [Trusted Publishing](https://docs.pypi.org/trusted-publishers/): no API token exists anywhere, only a short-lived OIDC credential minted per release.

### 6. Cargo / crates.io (`cargo binstall` or `cargo install`)
```bash
cargo binstall dev-prune   # downloads the release archive
cargo install dev-prune    # compiles from source
```
crates.io hosts source, not binaries — there is no executable on the registry for `cargo install` to fetch, so it always builds, and it is the only channel that does. Requires Rust 1.85+ (edition 2024) and the release profile below.

[`cargo binstall`](https://github.com/cargo-bins/cargo-binstall) closes that gap. The `[package.metadata.binstall]` table in `Cargo.toml` names one GitHub release asset per target — the same six archives listed above — so binstall resolves the version on crates.io, downloads the matching archive, and unpacks the executable without a toolchain. The table restates the asset names from `release.yml`; if an asset is ever renamed and the table is not, `cargo binstall` silently falls back to compiling, which is the only symptom.

---

## 📦 Release profile

All published binaries are built with `lto = true`, `codegen-units = 1`, `strip = true` and `opt-level = 3`.

---

## 🛠️ Maintainer release workflow

A release is one `git push` of one tag. Nothing is built, archived, uploaded or published by hand:

```bash
git tag -a v1.1.0 -m "v1.1.0" && git push origin v1.1.0
```

`.github/workflows/release.yml` then builds all six targets, verifies the tag matches `Cargo.toml` and that `CHANGELOG.md` documents it, publishes a GitHub Release whose body is that changelog section, and pushes to PyPI and crates.io (the npm job is gated off — see section 4).

**[docs/RELEASING.md](RELEASING.md)** has the whole process: the credentials each registry needs, which registries review submissions (npm, PyPI and crates.io do not), what to do when a release goes wrong, and the gated channels — Homebrew, WinGet, Scoop — that are not automated because a human sits on the other side.
