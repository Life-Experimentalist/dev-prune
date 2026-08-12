# 🤖 Background Daemon & Git Hook Subsystem Troubleshooting

This guide provides diagnostics and fix procedures for OS background schedulers and Git auto-registration hooks in **`dev-prune`** (`devp`).

---

## 🔍 Issue Index

- [🤖 Background Daemon \& Git Hook Subsystem Troubleshooting](#-background-daemon--git-hook-subsystem-troubleshooting)
  - [🔍 Issue Index](#-issue-index)
    - [1. Checking Background Subsystem Status](#1-checking-background-subsystem-status)
    - [2. Windows Task Scheduler Issues](#2-windows-task-scheduler-issues)
      - [Symptom](#symptom)
      - [Solution](#solution)
    - [3. macOS LaunchAgent Issues](#3-macos-launchagent-issues)
      - [Symptom](#symptom-1)
      - [Solution](#solution-1)
    - [4. Linux systemd User Timer Issues](#4-linux-systemd-user-timer-issues)
      - [Symptom](#symptom-2)
      - [Solution](#solution-2)
    - [5. Git Hook Auto-Registration Issues](#5-git-hook-auto-registration-issues)
      - [Symptom](#symptom-3)
      - [Solution](#solution-3)
    - [6. `core.hooksPath` Is Already Taken](#6-corehookspath-is-already-taken)
    - [7. systemd Timer Written Where systemd Does Not Look](#7-systemd-timer-written-where-systemd-does-not-look)

---

### 1. Checking Background Subsystem Status

Run the status shortcuts to verify background scheduler and hook states:
```bash
# Check background daemon scheduler status
devp status daemon

# Check Git background hook status for current workspace
devp status hook
```

---

### 2. Windows Task Scheduler Issues

#### Symptom
Background daemon is not running on Windows or `schtasks` returns Access Denied.

#### Solution
1. Verify task presence in PowerShell:
   ```powershell
   schtasks /Query /TN DevPrune
   ```
2. Manually re-install task:
   ```powershell
   devp config daemon enable
   ```
3. Test task execution manually:
   ```powershell
   schtasks /Run /TN DevPrune
   ```

---

### 3. macOS LaunchAgent Issues

#### Symptom
LaunchAgent fails to run or `launchctl` reports error.

#### Solution
1. Inspect LaunchAgent file:
   `~/Library/LaunchAgents/com.devprune.daemon.plist`
2. Unload and reload LaunchAgent:
   ```bash
   launchctl unload ~/Library/LaunchAgents/com.devprune.daemon.plist
   launchctl load ~/Library/LaunchAgents/com.devprune.daemon.plist
   ```
3. Verify status:
   ```bash
   launchctl list | grep devprune
   ```

---

### 4. Linux systemd User Timer Issues

#### Symptom
systemd timer is inactive or disabled on Linux.

#### Solution
1. Check timer status:
   ```bash
   systemctl --user status dev-prune.timer
   ```
2. Enable and start user timer:
   ```bash
   systemctl --user enable --now dev-prune.timer
   ```
3. View service logs:
   ```bash
   journalctl --user -u dev-prune.service -n 50
   ```

---

### 5. Git Hook Auto-Registration Issues

#### Symptom
Newly checked-out repositories are not being automatically registered in `dev-prune`.

#### Solution
1. Ask the tool what it thinks the state is:
   ```bash
   devp hook
   ```
   It reports the configured `core.hooksPath`, dev-prune's own hooks directory, and
   whether all three hook files are on disk — and names the mismatch when there is one:
   - *"`core.hooksPath` points here but the hook files are missing"* — re-run
     `devp hook install`.
   - *"Hook files exist but `core.hooksPath` points elsewhere"* — something else claimed
     the setting. The hooks are dead files until it is pointed back.
2. Check whether hooks are disabled for this repository in `.devprune.json`:
   ensure `"disable_hooks": false`, or run `devp hook . enable`.
3. Enable hooks globally:
   ```bash
   devp hook install
   ```

---

### 6. `core.hooksPath` Is Already Taken

#### Symptom
`devp hook install` refuses, reporting that `core.hooksPath` is already set globally.

#### Cause
Git supports exactly one hooks directory, machine-wide, and there is no way to chain
two. Overwriting the existing value would disable husky, pre-commit or lefthook in
**every** repository on the machine, so dev-prune refuses rather than breaking a setup
it did not create.

#### Solution
Auto-registration is a convenience, not a requirement — `devp link .` in a new
repository does the same thing on demand. If you would rather have it, release the
setting first:

```bash
git config --global --unset core.hooksPath
```

Then re-run `devp hook install`. Note that this is what the other tool was using.

---

### 7. systemd Timer Written Where systemd Does Not Look

#### Symptom
`devp daemon` reports the timer as installed, but `systemctl --user status
dev-prune.timer` cannot find the unit.

#### Cause
systemd reads user units from `$XDG_CONFIG_HOME/systemd/user`. If that variable points
somewhere other than `~/.config` in your interactive shell but not in the session
systemd sees (or vice versa), the units and the search path disagree.

#### Solution
Check where each side is looking, then re-run `devp daemon install` from a shell whose
`XDG_CONFIG_HOME` matches:

```bash
systemctl --user show-environment | grep XDG_CONFIG_HOME
```
