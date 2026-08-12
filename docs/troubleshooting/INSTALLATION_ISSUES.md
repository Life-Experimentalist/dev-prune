# 🚀 Installation, PATH & Permission Troubleshooting

This document addresses all potential issues related to installing **`dev-prune`** (`devp`), shell profile configuration, PATH environment variables, execution permissions, anti-virus false positives, and CWD determinism.

---

## 🔍 Issue Index

- [🚀 Installation, PATH \& Permission Troubleshooting](#-installation-path--permission-troubleshooting)
  - [🔍 Issue Index](#-issue-index)
    - [1. `devp: command not found`](#1-devp-command-not-found)
      - [Symptom](#symptom)
      - [Cause](#cause)
      - [Diagnostic Check](#diagnostic-check)
      - [Solution](#solution)
    - [2. `Permission denied (os error 13)`](#2-permission-denied-os-error-13)
      - [Symptom](#symptom-1)
      - [Cause](#cause-1)
      - [Solution](#solution-1)
    - [3. Windows Defender Anti-Virus Warning](#3-windows-defender-anti-virus-warning)
      - [Symptom](#symptom-2)
      - [Cause](#cause-2)
      - [Solution](#solution-2)
    - [4. `dev-prune` Works but `devp` Does Not](#4-dev-prune-works-but-devp-does-not)
      - [Symptom](#symptom-3)
      - [Cause](#cause-3)
      - [Solution](#solution-3)
    - [5. Current Working Directory (CWD) Determinism Check](#5-current-working-directory-cwd-determinism-check)

---

### 1. `devp: command not found`

#### Symptom
Executing `devp` or `dev-prune` in a new terminal window produces `command not found: devp`.

#### Cause
The binary installation directory (`%APPDATA%\dev-prune\bin` or `~/.config/dev-prune/bin`) is not present in your active environment `PATH` variable, or your shell profile has not been reloaded.

#### Diagnostic Check
Run the doctor under whichever name *does* work. It reports the binary's location, whether
that directory is on `PATH`, and whether `devp` exists beside `dev-prune`:
```bash
dev-prune doctor
```

#### Solution
1. **Reload Shell Configuration**:
   - macOS / Linux (Zsh): `source ~/.zshrc`
   - Linux (Bash): `source ~/.bashrc`
   - Linux (Fish): `source ~/.config/fish/config.fish`
   - Windows: Restart PowerShell or Command Prompt window.

2. **Manually Add to Environment PATH**:
   - **macOS / Linux**:
     Add the following line to `~/.zshrc` or `~/.bashrc`:
     ```bash
     export PATH="$HOME/.config/dev-prune/bin:$PATH"
     ```
   - **Windows (PowerShell)**:
     ```powershell
     $binDir = "$env:APPDATA\dev-prune\bin"
     [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + $binDir, 'User')
     ```

---

### 2. `Permission denied (os error 13)`

#### Symptom
Running `dev-prune` on Linux or macOS returns `bash: /home/user/.config/dev-prune/bin/dev-prune: Permission denied`.

#### Cause
The binary file was extracted without the filesystem executable bit set (`+x`).

#### Solution
Grant executable permission to the binary file:
```bash
chmod +x ~/.config/dev-prune/bin/dev-prune
```

---

### 3. Windows Defender Anti-Virus Warning

#### Symptom
Windows SmartScreen or Windows Defender displays "Windows protected your PC" or blocks binary execution.

#### Cause
Newly compiled or unsigned open-source CLI binaries may trigger false positives in Windows Defender heuristic scans.

#### Solution
1. Click **More info** on the SmartScreen dialog and select **Run anyway**.
2. Alternatively, add `%APPDATA%\dev-prune\bin\` to Windows Defender Exclusion list in PowerShell:
   ```powershell
   Add-MpPreference -ExclusionPath "$env:APPDATA\dev-prune\bin"
   ```

---

### 4. `dev-prune` Works but `devp` Does Not

#### Symptom
`dev-prune` runs, but `devp` fails with `The term 'devp' is not recognized` (PowerShell),
`'devp' is not recognized as an internal or external command` (cmd), or
`devp: command not found` (bash, zsh, fish).

#### Cause
`devp` is **not** a shell alias, a PowerShell `$PROFILE` function, or a `doskey` macro — it
is a real second executable sitting next to `dev-prune` in the same directory, hard-linked
to it (or copied, where the filesystem refuses a hard link — a `%TEMP%` on a different
volume, or a network share). That is deliberate: an alias exists only inside
the shell that defined it, so it would be missing from cmd, from an IDE terminal, and from
the OS scheduler that runs the automatic prune pass. Nothing to add to `$PROFILE` here; the
file simply is not there. A manual install that skipped `dev-prune setup` is the usual
reason.

#### Solution
Let dev-prune create it — this is idempotent and also refreshes a `devp` that has gone
stale against an upgraded binary:
```bash
dev-prune setup
```
Then confirm both names resolve to the same directory:
```bash
dev-prune doctor
```
The **Installation** section names the binary's path and says whether `devp` was found
beside it. To create the file by hand instead:
```powershell
Copy-Item "$env:APPDATA\dev-prune\bin\dev-prune.exe" "$env:APPDATA\dev-prune\bin\devp.exe" -Force
```
```bash
ln -sf ~/.config/dev-prune/bin/dev-prune ~/.config/dev-prune/bin/devp
```

---

### 5. Current Working Directory (CWD) Determinism Check

`dev-prune` state files and binary alias resolutions operate completely independently of the directory from which `devp` is called:
- `~/.config/dev-prune/registry.json` (or `%APPDATA%\dev-prune\registry.json`) is global.
- Relative arguments (e.g. `devp run .`) are immediately converted to absolute filesystem paths before execution.
