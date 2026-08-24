# 📦 GitHub Releases, DIY Manual Installation & Build-from-Source Guide

This document provides a step-by-step DIY manual installation guide for **`dev-prune`** (`devp`). Whether downloading pre-built binaries from GitHub Releases, building directly from Rust source code, or using one-liner installer scripts, this guide covers all platforms, permissions, PATH environment setups, and troubleshooting verification checks.

---

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## ⚡ Quick 1-Liner Automated Installers (Recommended)

The installer scripts download the release binary, verify it against the published
SHA-256 checksum, place it and a `devp` copy in the dev-prune config directory, and add
that directory to your PATH.

As their last step they run `dev-prune setup`, which installs the integrations: the
exported `SKILL.md`, the file-manager icon registration, the global Git auto-registration
hooks, and the OS scheduler. That step skips the hooks when `git` is not on `PATH`, or
when `core.hooksPath` already belongs to another tool and `auto_hooks_chain` is off
(`devp hook install --chain` takes the slot without displacing that tool). The whole step
is skipped by `--no-auto-setup`. It does **not** modify your editor settings or register
any repositories (`devp init <dir>`) — both stay your call. `devp uninstall` reverses it.

### macOS & Linux (Shell)
```bash
curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
```

### Windows (PowerShell)
```powershell
iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
```

### Windows (Command Prompt)
```bat
powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"
```

`cmd.exe` has no `Invoke-WebRequest`, so it borrows PowerShell for the download and runs
the same script. One difference matters: the installer adds the bin directory to the User
PATH *and* to its own process, so that PowerShell's `iwr … | iex; devp init` works on the
very next line. A `cmd` session cannot inherit the process PATH of the PowerShell child it
spawned, so `devp` resolves in the next Command Prompt you open, not the current one.

`-ExecutionPolicy Bypass` is belt and braces rather than a requirement — see
[Execution policy](#execution-policy-and-why-the-one-liner-ignores-it) below.

The shell script also works on Windows under Git Bash, MSYS2 or Cygwin, and installs to
the same `%APPDATA%\dev-prune\bin` the PowerShell script uses.

### Execution policy, and why the one-liner ignores it

PowerShell's execution policy governs *script files*. `iwr … | iex` never creates one — it
evaluates a string in the current session — so the one-liner runs unchanged under the
default `RemoteSigned`, and even under `Restricted`. There is nothing to configure.

It only becomes a problem if you download `install.ps1` and run it as a file, which trips
two separate guards at once: the execution policy, and the Mark of the Web your browser
attached to the download. Run it without disturbing either:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

`-ExecutionPolicy` on the command line applies to that one process and expires with it.
Prefer it to `Set-ExecutionPolicy`, which changes the setting for every script you run
afterwards, for the sake of a single install.

### Installer options

| Shell | PowerShell | Environment variable | Effect |
| --- | --- | --- | --- |
| `--version <tag>` | `-Version <tag>` | `DEV_PRUNE_VERSION` | Install a specific release instead of the default |
| `--bin-dir <dir>` | `-BinDir <dir>` | `DEV_PRUNE_BIN_DIR` | Install somewhere other than the config directory's `bin/` |
| `--no-path` | `-NoPath` | `DEV_PRUNE_NO_PATH=1` | Leave every shell rc file and the User PATH alone |
| `--no-auto-setup` | `-NoAutoSetup` | `DEV_PRUNE_NO_AUTO_SETUP=1` | Install the binary only — no SKILL.md, hooks or scheduler |
| `--force` | `-Force` | `DEV_PRUNE_FORCE=1` | Download and write the binary even when this version is already installed here, or when the install is pinned with `version_lock` |
| `--help` | `-Help` | — | Print the options and exit |

An install that has `devp config set version_lock true` set is the one thing the script
will not touch on its own: it reports the pin, changes nothing, and exits `0`. That is
the point of the setting — a machine that has to keep shipping the same version has to
survive somebody re-running the one-liner out of habit. `--force` / `-Force` /
`DEV_PRUNE_FORCE=1` installs over it anyway, and has to be typed; `devp config set
version_lock false` releases it properly.

A flag wins over its environment variable. Both one-liners pipe the script into a shell,
which is why passing a flag takes a slightly different form:

```bash
curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --no-auto-setup
```

```powershell
& ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -NoAutoSetup
```

The environment variables need no such rewriting, which is why they stay supported:
`DEV_PRUNE_NO_AUTO_SETUP=1` in a Dockerfile or CI environment covers install time *and*
every command after it, since dev-prune itself reads the same variable.

---

## 🛠️ DIY Option A: Installing Pre-Built GitHub Release Binaries

Pre-compiled production binaries for all supported operating systems and architectures are available on [GitHub Releases](https://github.com/Life-Experimentalist/dev-prune/releases).

### 1. Windows Installation (`x86_64-pc-windows-msvc`)

1. **Download Archive**: Download `dev-prune-v1.8.0-windows-x64.zip` from GitHub Releases.
   On Windows on ARM take `dev-prune-v1.8.0-windows-arm64.zip` instead, and on a machine
   with no 64-bit mode at all take `dev-prune-v1.8.0-windows-x86.zip`. Everything below
   is the same for all three.
2. **Create Target Directory**:
   Open PowerShell and create the application directory:
   ```powershell
   $binDir = "$env:APPDATA\dev-prune\bin"
   New-Item -ItemType Directory -Path $binDir -Force
   ```
3. **Extract & Copy Binary**: Extract `dev-prune.exe` into `$env:APPDATA\dev-prune\bin\dev-prune.exe`.
4. **Register in User PATH**:
   ```powershell
   $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
   if ($userPath -notlike "*$binDir*") {
       [Environment]::SetEnvironmentVariable('Path', "$userPath;$binDir", 'User')
       $env:PATH = "$binDir;$env:PATH"
   }
   ```
5. **Create the `devp` Short Name**:
   `devp` is a second copy of the executable, not a shell alias — that is what makes it
   work in cmd, PowerShell, Git Bash, an IDE terminal and a scheduled task alike, none of
   which read a PowerShell `$PROFILE`.
   ```powershell
   Copy-Item "$binDir\dev-prune.exe" "$binDir\devp.exe" -Force
   ```
   You can skip this step: `dev-prune setup` below creates it for you, and any later
   `dev-prune` command refreshes it if it goes stale against an upgraded binary.
6. **Run Setup Checks**:
   ```powershell
   dev-prune setup
   dev-prune init .
   devp doctor
   ```

---

### 2. macOS Installation (Intel `x86_64` & Apple Silicon `arm64`)

1. **Download Archive**:
   - Apple Silicon (M1/M2/M3/M4): `dev-prune-v1.8.0-darwin-arm64.tar.gz`
   - Intel Mac: `dev-prune-v1.8.0-darwin-x64.tar.gz`
2. **Extract & Relocate Binary**:
   On macOS the config directory is `~/Library/Application Support/dev-prune`, not `~/.config` — that is where dev-prune reads its registry from, so install the binary alongside it.
   ```bash
   BIN="$HOME/Library/Application Support/dev-prune/bin"
   mkdir -p "$BIN"
   tar -xzf dev-prune-v1.8.0-darwin-*.tar.gz -C "$BIN"
   chmod +x "$BIN/dev-prune"
   ```
3. **Add to Shell PATH**:
   Add the following line to your `~/.zshrc` (or `~/.bash_profile`):
   ```bash
   export PATH="$HOME/Library/Application Support/dev-prune/bin:$PATH"
   ```
   Apply changes: `source ~/.zshrc`

   Do **not** add `alias devp='dev-prune'`. `devp` is a second executable — a hard link
   to the same binary, or a copy where the filesystem will not allow one — created by
   `dev-prune setup` in the next step, so it works in every shell and in the scheduler.
   A shell alias would exist only in the shell whose startup file defined it.
4. **Verify**:
   ```bash
   dev-prune setup       # creates devp, SKILL.md, hooks, scheduler, icons
   devp doctor
   devp init ~/Code      # register the repos you want tracked
   ```

---

### 3. Linux Installation (`x86_64-unknown-linux-musl`)

> **One binary per architecture, every distribution.** The Linux assets are statically
> linked against musl, so there is no glibc version floor and no per-distribution build:
> the same `linux-x64` file runs on Debian, Ubuntu, Fedora, RHEL, Arch, openSUSE, NixOS
> and Alpine. Pick by CPU architecture — `linux-x64` for `x86_64`, `linux-arm64` for
> `aarch64` (`uname -m` tells you which) — and nothing else.


1. **Download Archive**: Download `dev-prune-v1.8.0-linux-x64.tar.gz` from GitHub Releases.
2. **Extract & Relocate Binary**:
   ```bash
   mkdir -p ~/.config/dev-prune/bin
   tar -xzf dev-prune-v1.8.0-linux-x64.tar.gz -C ~/.config/dev-prune/bin/
   chmod +x ~/.config/dev-prune/bin/dev-prune
   ```
3. **Configure Shell PATH**:
   Add to `~/.bashrc` (or `~/.zshrc`):
   ```bash
   export PATH="$HOME/.config/dev-prune/bin:$PATH"
   ```
   For Fish shell (`~/.config/fish/config.fish`):
   ```fish
   fish_add_path ~/.config/dev-prune/bin
   ```
   Apply changes: `source ~/.bashrc`

   No `alias devp='dev-prune'` line is needed or wanted — `devp` is a second executable,
   hard-linked to the same binary by `dev-prune setup` in the next step, so it resolves
   in every shell and in the systemd timer, which reads no startup file at all.
4. **Verify**:
   ```bash
   dev-prune setup       # creates devp, SKILL.md, hooks, scheduler, icons
   devp doctor
   devp init ~/Code      # register the repos you want tracked
   ```

---

## 🛠️ DIY Option B: Building Directly from Source Code

Building from source ensures maximum binary performance optimized for your specific CPU architecture.

### Prerequisites
- **Rust Toolchain**: 1.88 or newer (`rustup update stable`)
- **Git**: Installed and available on PATH

### Step-by-Step Build Instructions

1. **Clone Repository**:
   ```bash
   git clone https://github.com/Life-Experimentalist/dev-prune.git
   cd dev-prune
   ```

2. **Compile Release Binary**:
   ```bash
   cargo build --release
   ```
   The compiled binary will be placed at:
   - Windows: `target\release\dev-prune.exe`
   - macOS / Linux: `target/release/dev-prune`

3. **Install Binary to Global Location**:
   ```bash
   # Using Cargo direct install:
   cargo install --path .
   ```
   Or manually copy binary to system bin directory:
   - Windows: Copy `target\release\dev-prune.exe` to `%APPDATA%\dev-prune\bin\`
   - macOS: Copy `target/release/dev-prune` to `~/Library/Application Support/dev-prune/bin/`
   - Linux: Copy `target/release/dev-prune` to `$XDG_CONFIG_HOME/dev-prune/bin/` (default `~/.config/dev-prune/bin/`)

4. **Verify Environment**:
   ```bash
   devp -V
   ```

---

## 🔒 Current Working Directory (CWD) Determinism

`dev-prune` is engineered to be **100% CWD-independent**:
- Configuration files (`registry.json`) are loaded strictly from the system user configuration directory (`%APPDATA%\dev-prune\` on Windows, `~/Library/Application Support/dev-prune/` on macOS, `$XDG_CONFIG_HOME/dev-prune/` on Linux).
- Binary alias links (`devp`) resolve relative to `std::env::current_exe()`.
- Relative CLI paths (e.g. `devp run .`) are immediately converted to absolute filesystem paths before execution.

You can invoke `devp` or `dev-prune` safely from any working directory in any terminal session.

---

## ⚠️ Troubleshooting Installation & Pitfalls Check

| Symptom | Root Cause | Solution |
| :--- | :--- | :--- |
| `devp: command not found` | Binary directory not in system PATH or shell profile not reloaded. | Run `source ~/.zshrc` (or restart terminal session). Run `devp -V` to audit PATH status. |
| `Permission denied` on macOS/Linux | Binary file lacks executable execution bit. | Run `chmod +x ~/.config/dev-prune/bin/dev-prune`. |
| Windows Anti-Virus Warning | Unsigned binary false positive. | Add `%APPDATA%\dev-prune\bin\` to Windows Defender exclusions. |
| `dev-prune` works but `devp` does not | The second binary was never created — a manual install that skipped `dev-prune setup`. | Run `dev-prune setup`, or copy the executable yourself. `devp` is a real file next to `dev-prune`, not a shell alias, so no `$PROFILE` or `.bashrc` edit is involved. |
| Everything installed, still unsure what is wrong | — | Run `devp doctor`. It checks the binary, PATH, registry, settings, integrations and reachable package managers in one read-only pass. |
