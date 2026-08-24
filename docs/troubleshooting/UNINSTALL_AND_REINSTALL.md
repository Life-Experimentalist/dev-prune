# 🧹 Uninstall, Reinstall & State Recovery Guide

This document details procedures for standard uninstallation, deep configuration wipes, residual file cleanup, PATH restoration, clean reinstall passes, and state recovery using `devp undo`.

---

<p align="center">
  <img src="../../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## 🔍 Section Index

- [🧹 Uninstall, Reinstall \& State Recovery Guide](#-uninstall-reinstall--state-recovery-guide)
  - [🔍 Section Index](#-section-index)
    - [1. Standard Uninstallation](#1-standard-uninstallation)
    - [2. Deep Uninstallation \& Config Wipe](#2-deep-uninstallation--config-wipe)
    - [3. Manual Residual Directory Cleanup](#3-manual-residual-directory-cleanup)
    - [4. Clean Reinstallation Workflow](#4-clean-reinstallation-workflow)
    - [5. State Recovery with `devp undo`](#5-state-recovery-with-devp-undo)

---

### 1. Standard Uninstallation

To remove background daemon schedulers and non-blocking Git hooks while preserving your global configuration and registered repository state:

```bash
devp uninstall
```

What `devp uninstall` does:
- Uninstalls the OS background task scheduler (`schtasks`, LaunchAgent, systemd timer).
- Clears the global `core.hooksPath` — but only if it still points at dev-prune's hooks
  directory. If you have since pointed it at husky or lefthook, that value is left alone.
- Removes the `devp` alias link sitting beside the real binary.
- Leaves `~/.config/dev-prune/registry.json` and workspace `.devprune.json` files intact for future use.

It does not delete the `dev-prune` binary itself, and it does not edit your PATH or your
shell startup files — see [Manual Residual Directory Cleanup](#3-manual-residual-directory-cleanup).

---

### 2. Deep Uninstallation & Config Wipe

To perform a complete removal of background schedulers, Git hooks, global configuration folders, registry files, and per-repository `.devprune.json` files:

```bash
devp uninstall --deep
```

What `devp uninstall --deep` does:
- Everything the light uninstall does.
- Wipes `~/.config/dev-prune/` (or `%APPDATA%\dev-prune\`) — including the binary, if you
  installed it there, and the entire prune history.
- Removes `.devprune.json` from every registered repository.

Because this deletes files inside your own repositories, it asks for confirmation first.
In a script or CI, where there is no terminal to prompt on, it refuses outright unless
you pass `--yes`:

```bash
devp uninstall --deep --yes
```

---

### 3. Manual Residual Directory Cleanup

If the binary executable has already been deleted and you wish to clean residual state manually:

- **Windows**:
  Delete `%APPDATA%\dev-prune\` and remove `%APPDATA%\dev-prune\bin` from User PATH in System Environment Variables.
- **macOS & Linux**:
  Remove `~/.config/dev-prune/` and remove `export PATH="$HOME/.config/dev-prune/bin:$PATH"` from `~/.zshrc` or `~/.bashrc`.

---

### 4. Clean Reinstallation Workflow

To perform a complete clean reinstall from scratch:

1. Execute deep uninstall:
   ```bash
   devp uninstall --deep
   ```
2. Run the one-liner production installer:
   - **macOS / Linux**: `curl -fsSL https://devprune.vkrishna04.me/install.sh | sh`
   - **Windows**: `iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex`
3. Re-scan project workspaces:
   ```bash
   devp init ~/Code
   ```

---

### 5. State Recovery with `devp undo`

If you accidentally ran `devp init` or `devp link` on an unintended directory path, revert the registration action instantly:

```bash
devp undo
```
`devp undo` un-registers the repositories added by the most recent `init` or `link` pass,
without modifying any workspace files on disk.

> [!NOTE]
> `undo` reverses **registration**, not pruning. Deleted dependency directories are
> restored with `devp restore`, which reinstalls them from the lockfiles that were
> verified before deletion. To reverse a prune pass rather than a registration, use
> `devp restore --last-run`: it reinstalls exactly what the most recent pass deleted,
> across every repository it touched, without needing to remember which those were.
