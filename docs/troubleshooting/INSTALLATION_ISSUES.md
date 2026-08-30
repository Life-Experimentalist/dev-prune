# 🚀 Installation, PATH & Permission Troubleshooting

This document addresses all potential issues related to installing **`dev-prune`** (`devp`), shell profile configuration, PATH environment variables, execution permissions, anti-virus false positives, and CWD determinism.

---

<p align="center">
  <img src="../../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## 🔍 Issue Index

1. [`devp: command not found`](#1-devp-command-not-found)
2. [`Permission denied (os error 13)`](#2-permission-denied-os-error-13)
3. [Windows will not run `dev-prune.exe`](#3-windows-will-not-run-dev-pruneexe)
4. [`'sh' is not recognized` — the install one-liner is for the wrong shell](#4-sh-is-not-recognized--the-install-one-liner-is-for-the-wrong-shell)
5. [`install.ps1` will not run: "running scripts is disabled on this system"](#5-installps1-will-not-run-running-scripts-is-disabled-on-this-system)
6. [`uv tool install` put the executables somewhere unexpected](#6-uv-tool-install-put-the-executables-somewhere-unexpected)
7. [`dev-prune` works but `devp` does not](#7-dev-prune-works-but-devp-does-not)
8. [Current working directory (CWD) determinism check](#8-current-working-directory-cwd-determinism-check)
9. [`pip install` in a virtual environment — what happens when the venv goes away](#9-pip-install-in-a-virtual-environment--what-happens-when-the-venv-goes-away)
10. [A terminal window flashes briefly after logging in (Windows)](#10-a-terminal-window-flashes-briefly-after-logging-in-windows)
11. [I installed with one manager, then ran the one-liner — which copy am I running?](#11-i-installed-with-one-manager-then-ran-the-one-liner--which-copy-am-i-running)
12. [I re-ran the one-liner and it said there was nothing to do](#12-i-re-ran-the-one-liner-and-it-said-there-was-nothing-to-do)

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

### 3. Windows Will Not Run `dev-prune.exe`

#### Symptom
The install succeeds, and then the binary refuses to start — either behind a blue
**Windows protected your PC** dialog, or with a flat refusal and no dialog at all. The
same one-liner, on the same day, works on a colleague's machine.

#### Cause
Two different Windows features do this, they have different triggers, and the fixes are
not interchangeable. Identify which one you have before doing anything.

| | SmartScreen App Reputation | Smart App Control |
|---|---|---|
| What you see | Blue **Windows protected your PC** dialog, with **More info → Run anyway** | Blocked outright, no "run anyway" — often `System Integrity policy has been violated` or nothing at all |
| Trigger | Unsigned **and** carrying a Mark of the Web | Unsigned. That is the whole condition |
| Where it comes from | Browsers stamp downloads with a `Zone.Identifier`; File Explorer copies the mark onto everything it extracts | On by default on **clean installs** of Windows 11 22H2+; **always off** on machines upgraded from an earlier build |
| Fix | `Unblock-File` | Only a valid Authenticode signature |

That second row is the whole answer to "why did it work on my other laptop?" — Smart App
Control's default depends on how Windows got onto the machine, so two otherwise identical
Windows 11 installs disagree about whether an unsigned binary may run.

The releases are not code-signed. A certificate that Windows trusts is issued by a
commercial CA against a verified legal identity, which is not something a one-person
project has lying around — so on a Smart App Control machine, no installation route works,
including npm, PyPI and a hand-downloaded zip. `Unblock-File` does **not** help here; Smart
App Control never looked at the mark.

#### Diagnostic Check
Ask which one you are facing. Smart App Control first, because it is the one with no
workaround:
```powershell
(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -Name VerifiedAndReputablePolicyState -ErrorAction SilentlyContinue).VerifiedAndReputablePolicyState
```
`1` is enforcement — this is your problem. `2` is evaluation mode, `0` is off, and no
output at all means the feature does not exist on this build of Windows.

Then ask whether the file is marked:
```powershell
Get-Item "$env:APPDATA\dev-prune\bin\dev-prune.exe" -Stream Zone.Identifier -ErrorAction SilentlyContinue
```
Output means it is marked and SmartScreen will challenge it. No output means it will not.

#### Solution — SmartScreen
Strip the mark. This edits one metadata stream: it does not disable a security feature,
touch Defender, or affect any other file.
```powershell
Unblock-File "$env:APPDATA\dev-prune\bin\dev-prune.exe","$env:APPDATA\dev-prune\bin\devp.exe"
```

`install.ps1` already does this for you, on the archive and on both installed copies. It is
listed here for the hand-downloaded path — where unblocking the **archive before
extracting** means nothing inside it is ever marked:
```powershell
Unblock-File .\dev-prune-v1.12.0-windows-x64.zip
Expand-Archive .\dev-prune-v1.12.0-windows-x64.zip -DestinationPath .
```

If you are standing in front of the dialog right now, you do not need any of the above:
click **More info**, then **Run anyway**. That both starts the binary and teaches
SmartScreen this specific file is acceptable, so it stops asking. There is nothing unsafe
about answering the prompt — it is the prompt's purpose.

There is one more place the same block appears. If the *installer* was what got challenged,
right-click `install.ps1` → **Properties**, tick **Unblock** at the bottom of the General
tab, → **OK**. Same mark, same removal, from the GUI.

> Do **not** add a Windows Defender exclusion for the install directory. It suppresses real
> detections in that folder from then on, for a problem an `Unblock-File` solves outright.

#### Solution — Smart App Control
There is no fix from this side, and it is worth being blunt about how few options that
leaves. Signing is the only thing Smart App Control accepts, so:

- **Installing from a different channel does not help.** npm, PyPI and a hand-downloaded
  zip all deliver the same unsigned executable.
- **Building from source does not help either.** Smart App Control blocks unsigned
  binaries it has never seen, and a binary you compiled thirty seconds ago is the most
  unknown file on the machine. It blocks Visual Studio's own `MSBuild.exe` for exactly
  this reason. `cargo install dev-prune` will build and then refuse to run.
- **`Unblock-File` does nothing.** Smart App Control never looked at the mark.
- **Running it under WSL is not the escape hatch it looks like.** Smart App Control has
  been reported blocking [`wsl.exe` itself](https://github.com/microsoft/WSL/issues/10300),
  so the Linux build is only reachable on a machine where WSL already starts — and it then
  prunes the Linux filesystem, not `C:\`, unless you register paths under `/mnt/c` and
  accept the performance of crossing that boundary.
- **There is no "run anyway".** Unlike SmartScreen, Smart App Control gives you no prompt
  to answer, because there is no decision it is asking you to make.

That leaves turning it off, under **Windows Security → App & browser control → Smart App
Control** — and the switch is **one-way**. Windows cannot turn Smart App Control back on
afterwards without a reset or a reinstall. Disabling a system-wide protection permanently
is a large price for one disk-cleanup tool. If you are not already running unsigned
software on that machine, the honest recommendation is to leave it on and use dev-prune
somewhere else.

To confirm Smart App Control is what blocked a specific run, rather than inferring it,
read the block out of the event log:
```powershell
Get-WinEvent -LogName 'Microsoft-Windows-CodeIntegrity/Operational' -MaxEvents 20 |
    Where-Object Id -in 3076,3077 | Format-List TimeCreated, Id, Message
```
`3077` is an enforcement block — the file named in the message was refused. `3076` is the
same decision recorded in evaluation mode, where the file was allowed to run anyway.

---

### 4. `'sh' is not recognized` — the install one-liner is for the wrong shell

#### Symptom
```
'sh' is not recognized as an internal or external command,
operable program or batch file.
```
or, in PowerShell, `sh : The term 'sh' is not recognized as the name of a cmdlet`.

#### Cause
You pasted the Linux/macOS one-liner into a Windows shell. `curl` succeeds — Windows 10
and 11 ship a real `curl.exe` — and then the pipe hands the script to `sh`, which is a
Unix shell that Windows does not have. Nothing was installed, and nothing was damaged: the
downloaded script was piped into a command that does not exist.

The reverse mistake fails the same way. `iwr` is a PowerShell alias, so the PowerShell
one-liner in a Command Prompt gives `'iwr' is not recognized`.

#### Solution
Use the one-liner for the shell you are in.

| Prompt looks like | Shell | Command |
|---|---|---|
| `PS C:\>` | PowerShell | `iwr -useb https://devprune.vkrishna04.me/install.ps1 \| iex` |
| `C:\>` | Command Prompt | `powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 \| iex"` |
| `$` or `user@host:~$` | bash, zsh, fish, Git Bash, WSL | `curl -fsSL https://devprune.vkrishna04.me/install.sh \| sh` |

If you are unsure which you have, this answers it in all three, because each shell expands
the variable syntax it understands and leaves the other one alone:
```
echo %COMSPEC% $SHELL
```

| Output | You are in |
|---|---|
| `C:\WINDOWS\system32\cmd.exe $SHELL` | Command Prompt |
| `%COMSPEC%` and nothing after it | PowerShell |
| `%COMSPEC% /bin/bash` (or `/bin/zsh`, …) | a Unix shell |

Git Bash, MSYS2, Cygwin and WSL are Unix shells running on Windows, and the `install.sh`
line is the correct one there — but note that WSL installs into the Linux filesystem, as a
Linux binary, and will not be on the PATH of your Windows terminals.

---

### 5. `install.ps1` will not run: "running scripts is disabled on this system"

#### Symptom
```
File install.ps1 cannot be loaded because running scripts is disabled on this system.
```

#### Cause
PowerShell's execution policy governs **script files**. The default for a user account is
`RemoteSigned`, which refuses to run a downloaded `.ps1` unless it is signed — and a
browser-downloaded `install.ps1` is exactly that.

The `iwr … | iex` one-liner is *not* subject to this. It never creates a file; it evaluates
a string in the current session, which the execution policy does not govern. If the
one-liner is what you ran, this is not your error.

#### Diagnostic Check
```powershell
Get-ExecutionPolicy -List
```

#### Solution
Scope the relaxation to the single process that needs it:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

The setting expires when that process exits. `Set-ExecutionPolicy` would change it for
every script you run from then on, which is a large permanent change to buy one install.

From `cmd.exe`, where there is no `iwr` to pipe, the same flag carries the one-liner:
```bat
powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex"
```

---

### 6. `uv tool install` put the executables somewhere unexpected

#### Symptom
`uv tool install dev-prune` reports `Installed 2 executables: dev-prune, devp`, but they
are in `~/.local/bin` (`C:\Users\<you>\.local\bin` on Windows) rather than the
`%APPDATA%\dev-prune\bin` the native installers use.

#### Cause
That is uv's own convention, not something dev-prune chooses. uv installs every tool's
executables into one shared directory so a single entry on `PATH` covers all of them, and
it resolves that directory in this order:

| Precedence | Source | Notes |
| --- | --- | --- |
| 1 | `UV_TOOL_BIN_DIR` | Explicit override, wins over everything |
| 2 | `XDG_BIN_HOME` | Honoured on Windows too, despite the name |
| 3 | `XDG_DATA_HOME/../bin` | Only when `XDG_DATA_HOME` is set |
| 4 | `~/.local/bin` | The default you are seeing |

The same applies to `pipx`. Plain `pip install` is different again: it follows the *active
environment*, landing in `<venv>/Scripts` (Windows) or `<venv>/bin`, and in the user base
directory for `pip install --user`.

#### Diagnostic Check
Ask uv directly rather than guessing:
```powershell
uv tool dir --bin
```

#### Solution
Point uv wherever you want it, permanently, for every tool it manages:
```powershell
[Environment]::SetEnvironmentVariable('UV_TOOL_BIN_DIR', "$env:APPDATA\dev-prune\bin", 'User')
uv tool install --force dev-prune
```

Open a new terminal afterwards so the change is visible. `--force` is what makes uv
relocate an already-installed tool instead of reporting it up to date.

If the goal was simply for `devp` to resolve, the directory only needs to be on `PATH` —
`uv tool update-shell` adds it for you, and `devp doctor` confirms the result.

> **`--system` is not the flag you want here.** It belongs to `uv pip install`, where it
> means "install into the system Python instead of the active virtualenv", and
> `uv tool install` rejects it. It would be the wrong lever anyway: `uv tool` always builds
> each tool its own isolated environment, and `UV_TOOL_BIN_DIR` — not the Python it was
> installed against — is what decides where `devp` ends up. dev-prune is a Rust binary
> riding inside a wheel, so no Python is involved at run time at all.

---

### 7. `dev-prune` Works but `devp` Does Not

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

### 8. Current Working Directory (CWD) Determinism Check

`dev-prune` state files and binary alias resolutions operate completely independently of the directory from which `devp` is called:
- `~/.config/dev-prune/registry.json` (or `%APPDATA%\dev-prune\registry.json`) is global.
- Relative arguments (e.g. `devp run .`) are immediately converted to absolute filesystem paths before execution.

---

### 9. `pip install` in a virtual environment — what happens when the venv goes away

#### Symptom
`pip install dev-prune` inside an activated virtualenv works, but the worry is what
happens later: the executables land in `<venv>\Scripts` (Windows) or `<venv>/bin`, and a
venv is exactly the kind of directory that gets deleted — sometimes by dev-prune itself.

#### What actually happens
Nothing is lost. On its first run, dev-prune copies itself to a *managed* location —
`%APPDATA%\dev-prune\bin` on Windows, `~/.config/dev-prune/bin` elsewhere — creates the
`devp` twin beside it, and puts that directory on your user `PATH` (on Linux and macOS it
symlinks both names into `~/.local/bin` instead). Every integration — scheduler, hooks,
agent skill — is registered against that managed copy, never against the venv's.

So the venv copy is just the delivery vehicle. Deactivate the venv, delete it, or let a
prune pass reclaim it: `devp` keeps working from every new terminal, because the shell
finds the managed copy.

#### The catch: this project's environment stops being prunable

Surviving is not the same as being harmless. For as long as that copy is installed,
dev-prune is a package in the project's `site-packages` — and if `requirements.txt` does
not mention it, it is a package nothing can rebuild. Lockfile pre-verification then does
exactly what it is for and declines the environment, so the one venv you installed into
becomes the one venv a prune pass will not touch.

dev-prune says so on its first run from that location, while the cause is still fresh:

```text
dev-prune is installed inside this project's virtual environment

  running from  V:\Code\api\.venv\Scripts\dev-prune.exe
  environment   V:\Code\api\.venv
  project       V:\Code\api

A tool install belongs outside a project: it outlives the environment, every repository
shares it, and it never has to appear in an application's requirements file to stay out
of the way.

Until that is fixed, a prune pass will decline this project's environment - a package
`requirements.txt` does not account for is a package nothing can rebuild.

Record dev-prune in requirements.txt instead, as a deliberate dev dependency? [y/N]:
```

Two repairs, and either one is enough.

**Move it out**, which is what almost everybody wants — the tool stops being this
project's problem, and every other repository on the machine gets it too:

```powershell
pip uninstall dev-prune
uv tool install dev-prune     # or: pipx install dev-prune
```

If a copy already exists outside the project, the message names its path, so you can see
that removing this one still leaves you a working `devp`.

**Or keep it, on purpose.** Some projects really do want the tool pinned in their own
environment. Answer `y` and dev-prune appends its own pin to `requirements.txt`;
`pip freeze > requirements.txt` does the same thing by hand. Once it is recorded it is an
ordinary dependency, the environment is prunable again, and the subject never comes up
again — the check is silent when the file already lists the tool, because a project that
depends on dev-prune deliberately is not making a mistake.

The prompt defaults to no, appears only when a person is at the terminal, and is asked
once per installed version rather than on every run. Dismissing it costs nothing: a
prune pass that meets the same environment names the same situation and prints the same
two commands before declining it. That refusal is the failsafe, not the plan — it is
what catches a copy installed before this check existed.

#### Removing it cleanly
Two halves, because pip only knows about its own files:

```powershell
devp uninstall            # removes the managed copy, PATH entry, scheduler, hooks, skill
pip uninstall dev-prune   # removes the venv's copy so pip's records stay consistent
```

`devp uninstall` detects a pip-owned copy, leaves it alone, and prints that second
command for you. Running only `pip uninstall` leaves the managed copy behind — which is
by design (it is what keeps `devp` alive after the venv dies), but means `devp
uninstall` is the half you must not skip.

---

### 10. A terminal window flashes briefly after logging in (Windows)

#### Symptom
A black console window appears for under a second — typically moments after opening the
laptop — and closes before anything in it can be read.

#### Cause
The background task registered by dev-prune 1.2.0 and earlier ran with the *interactive*
logon type, so Windows attached a visible console to it every time it fired. The window
is the scheduled `devp run` doing its normal, silent work — nothing is wrong, but it
looks alarming, and it should not be visible at all.

Since 1.2.1 the task runs a **windowless build of the binary, `devpw.exe`** — the same
relationship `pythonw.exe` has to `python.exe`. It is generated locally, beside the
managed copy, by taking `dev-prune.exe` and setting one field in its header so Windows
never gives it a console; nothing is downloaded and no second binary ships in any
package. Because it runs in your own logged-on session, it also keeps full access to
mapped network drives and Dev Drives. Upgrading re-registers the existing task
automatically on the next setup pass, and refreshes `devpw.exe` after every dev-prune
upgrade so the daemon never runs a stale build.

If that windowless build cannot be created, setup falls back to a hidden password-less
task (an S4U logon), and then to the old visible task, so the daemon is never lost.

> **macOS and Linux never show this.** Their schedulers — launchd LaunchAgents and
> systemd user timers — run background jobs without ever attaching a terminal, so there
> is nothing to hide and nothing to configure.

#### Diagnostic Check
```powershell
schtasks /Query /TN DevPrune /XML | Select-String -Pattern 'Command|LogonType'
```
A `<Command>` ending in `devpw.exe` is the windowless task — nothing will flash. A
`Command` ending in `dev-prune.exe` with `LogonType` `S4U` is the hidden fallback (also
no window). `InteractiveToken` on `dev-prune.exe` is the old visible task.

#### Solution
Upgrade to 1.2.1 or later and run any `devp` command once — the setup pass re-registers
the task windowless. If the diagnostic still shows the interactive `dev-prune.exe` task
afterwards, re-register once from an elevated PowerShell, which retries the preferred
routes from the top:
```powershell
devp daemon on
```

One caveat applies **only to the S4U fallback**, not the default `devpw.exe` task: an
S4U logon has no access to mapped network drives, so if a registered repository lives on
one, that background pass skips it (everything fails closed) — prune those from a normal
terminal instead. The `devpw.exe` task does not have this limitation.

---

### 11. I installed with one manager, then ran the one-liner — which copy am I running?

The one this install put there. The one-liner works over any previous channel — cargo,
npm, bun, pnpm, yarn, uv, pipx, Homebrew, Scoop, WinGet, or an earlier run of itself — and
it needs no uninstall step first.

#### What actually happened

The script wrote `dev-prune` and `devp` into the managed `<config>/bin` directory and put
that directory **first** on your PATH: prepended to the rc file on macOS and Linux,
prepended to the User PATH on Windows. The other copy is untouched — the script never
runs another manager's uninstaller, because deleting a file a package manager still has
on its books is how an install becomes unrepairable. It names the path it found, and asks
whether to collapse the two:

```text
[!] Another dev-prune is on your PATH as well:
        /home/you/.cargo/bin/dev-prune
    A different package manager owns that copy, so this script left it alone.
    This directory comes first on PATH, so 'devp' is the copy in /home/you/.config/dev-prune/bin.
    Moving it over means installing here and uninstalling there, through the
    manager that put it there. 'devp install --channel installer' does both.
    Do that now? [y/N]:
```

Answer `y` and the old copy runs `devp install --channel installer --yes` itself —
installed here first, uninstalled there second, through the manager that owns it. Anything
else, a bare newline included, prints the command and moves on. Nothing is deleted by the
script either way.

Where there is nobody to ask, the question is skipped and the command printed: with
`DEV_PRUNE_NO_MIGRATE_PROMPT=1` set, with `CI` set, or with no terminal attached at all,
which covers most container builds.

#### Diagnostic

```bash
devp doctor
```

Three lines answer this. **Install channel** names the manager that owns the copy that
just ran. **Install receipt** appears when that copy is one an install script wrote, and
names its version, which of `install.sh` and `install.ps1` wrote it, and when — read
from `install.json` beside the binary rather than worked out again. **Other copies** lists
every other `dev-prune` on the machine that reports a different version — searching the
per-channel install directories as well as your PATH, because a copy that is not on PATH
is also a copy nothing upgrades.

To be certain which file answered, ask the shell rather than the tool:

```bash
command -v dev-prune && devp --version
```

```powershell
(Get-Command dev-prune).Source; devp --version
```

#### Solution

If you want one copy, move the old one over instead of deleting it:

```bash
devp install --channel installer
```

That installs through the target manager first and then uninstalls through the manager
that owns the old copy — in that order, so a failure leaves you with a working `devp`
either way. `--dry-run` prints the numbered plan and runs none of it. Substitute any
channel name for `installer` to go the other direction: `cargo`, `npm`, `bun`, `pnpm`,
`yarn`, `uv`, `pipx`, `winget`, `scoop`, `homebrew`.

Nothing is migrated because nothing needs to be. Your settings, the repository registry
and the undo history live in the config directory, which no package manager owns and none
of them touch — so switching channels never costs you the list of repositories you
registered.

Leaving both copies in place is a legitimate choice too. It costs a few megabytes, and
the only consequence is that `devp doctor` keeps mentioning the older one until its
version stops differing.

---

### 12. I re-ran the one-liner and it said there was nothing to do

That is the installer working, not refusing. Re-running it is the ordinary answer to
"get the latest" and "I think mine is broken", so it looks before it downloads.

#### What it decides

| What it finds at the managed path | What it does |
| --- | --- |
| Nothing | Installs. |
| An **older** version | Updates it in place, without asking. Prints `-> Updating dev-prune v1.8.0 -> v1.9.0`. |
| **This** version, with `devp` beside it and on PATH | Nothing at all. Prints `[OK] ... Nothing to do.` and exits `0`. |
| **This** version, but `devp` or the PATH entry is missing | Reinstalls, which repairs both. |
| A **newer** version | Leaves it alone, and says so. |
| Any version with `version_lock` set | Nothing at all, whatever the versions are. Prints the pin and exits `0`. |

The last row is the only one that can surprise: a machine ahead of the release the script
resolved to is not silently walked backwards. Naming the version is what makes going
backwards deliberate, so `--version` / `-Version` overrides it.

The `version_lock` row outranks every other row, including the silent in-place update.
It is set by `devp config set version_lock true` and by nothing else, and it is there
precisely so that a re-run of the one-liner — the habit this page is about — cannot
change which version a machine is shipping. `devp config set version_lock false`
releases it.

#### Making it install anyway

```bash
curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --force
```

```powershell
& ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -Force
```

`DEV_PRUNE_FORCE=1` does the same for the plain one-liner, which has nowhere to put a
flag. All three download the release and write both files again, whatever is already
there — the right move when you suspect the file on disk, rather than its version, is
the problem. It is also the only thing that installs over a `version_lock`, which is
why it has to be typed rather than picked up from a setting.

#### What it will never do on a re-run

Touch a copy it did not install. If another manager put a `dev-prune` on your PATH, the
script names it and leaves it alone; see
[which copy am I running](#11-i-installed-with-one-manager-then-ran-the-one-liner--which-copy-am-i-running).
