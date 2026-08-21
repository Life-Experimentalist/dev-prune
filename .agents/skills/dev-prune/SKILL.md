---
name: dev-prune
description: Use when asked to reclaim disk space, clean or prune node_modules / .venv / target / vendor, restore project dependencies, or diagnose the devp CLI. Covers the full dev-prune (devp) command surface, its safety rules, exit codes, and troubleshooting.
---

# dev-prune AI Skill (`devp`)

`dev-prune` deletes dependency and build directories that a lockfile can rebuild, from
Git repositories that have been idle. It refuses to delete anything it cannot prove is
recoverable. The binary is `dev-prune`; `devp` is the same executable under a second
name, and either works everywhere.

**The user should not have to learn this tool.** Read their intent, pick the command,
run it, and explain the result. Everything you need is below; the docs map at the end is
for anything that isn't.

---

## 🔒 Non-negotiable rules

1. **Dry-run before deleting.** Run `devp run --dry-run` and show what would be
   reclaimed before running anything that deletes. The only exception is when the user
   has already seen the plan or explicitly says "just do it".
2. **Never work around a verification failure.** If a project fails lockfile
   verification, `devp` prints the exact command to fix it. Surface that command. Do not
   delete the directory manually, do not delete the lockfile, do not pass a flag to skip
   it — no flag can skip it.
3. **`--ignore-idle` only lifts the idle-day wait.** It lets a repository the user is
   actively working in be pruned. Ask first. It does not bypass lockfile verification.
   The flag was `--force` before 1.0.0; that spelling still works and prints a note.
   When a user reaches for it, they are usually stuck on something else entirely — the
   seven reasons a directory is skipped are in the troubleshooting table at the bottom
   of this file, and `devp run --dry-run` names the actual one per repository.
4. **`--deep` on uninstall wipes configuration.** Confirm with the user before using it.
5. **dev-prune never updates itself.** Its one network request is a release check against
   GitHub's public API (no body, no identifier, no usage data, at most weekly, disabled by
   `devp config set update_check false`; `DEV_PRUNE_OFFLINE=1` keeps the whole process off
   the network regardless of any setting). It never downloads a binary — if a newer version
   exists, tell the user the upgrade command for their install channel: re-run the
   `install.sh`/`install.ps1` one-liner, `uv tool upgrade dev-prune`, `pipx upgrade
   dev-prune`, `cargo binstall dev-prune --force` (prebuilt), or
   `cargo install dev-prune --force` (compiles).

## What it will never do

Useful when the user asks "is this safe?" — these are enforced in code, not conventions:

- Operates only inside a directory containing a `.git` root.
- Never deletes a bloat directory whose lockfile is missing, unparseable, or (for
  `requirements.txt`) lists no packages.
- Never follows a symlinked or junctioned bloat directory — that storage belongs to
  something else.
- Never crosses into a nested repository; a submodule is pruned as itself or not at all.
- Never executes anything named in a repository-tracked file. `.devprune.json` holds
  inert data only.
- Never touches a package-manager cache. `devp caches` reports their sizes and prints the
  clear command; running it is the user's decision, not yours. A cache is shared by every
  project on the machine, so no one lockfile can prove it is recoverable — and it is what
  makes `devp restore` fast.

---

## 🗺️ Command map

| The user wants | Run |
| :--- | :--- |
| "how much space can I get back?" | `devp run --dry-run` |
| "show me my repos" | `devp status` (interactive; prints a plain table when not a TTY) |
| "just the worst offenders" / "top 10 biggest" | `devp status --top 10` — trims the list, never the totals |
| "how much has this saved me?" / "what did it clean last week?" | `devp stats` — lifetime total, prune passes, the last pass, and the repositories that gave back the most |
| "add tab completion" | `devp completions <bash\|zsh\|fish\|powershell\|elvish>` — prints the script to stdout; the user redirects it |
| "clean up" / "free space" | `devp run --dry-run`, then `devp run -y` |
| "clean this project" | `devp run . -y` |
| "clean it even though I'm working on it" | `devp run . --ignore-idle -y` — **ask first** |
| "clean everything but the API project" | `devp run --except api -y` — never verified, never deleted, never reinstalled |
| "put the dependencies back" | `devp restore .` |
| "undo that prune" / "I need it all back" | `devp restore --last-run` — reinstalls exactly what the last pass deleted, in every repository it touched |
| "where is my disk actually going?" / "how big is my npm cache?" | `devp caches` — sizes every package-manager cache and store (npm, pnpm, yarn, bun, uv, pip, cargo, go, maven, gradle, nuget, vcpkg, conan) and prints the command that clears each. It deletes nothing; the user runs the clear command |
| "did I install anything my lockfiles don't know about?" | `devp status --drift` — every environment holding packages its lockfile never recorded, with the one command that records them. A pure read; this is what a prune would refuse on |
| "why isn't it cleaning this?" | `devp doctor .` — ends by naming the one reason a pass would or would not touch it |
| "is anything wrong with my install?" | `devp doctor` |
| "fix whatever's broken" | `devp doctor --fix` — repairs installed-but-broken integrations (stale twin, dead-target hooks/scheduler, drifted chain, missing SKILL.md, dead registry entries); never a first-time install |
| "track my projects folder" | `devp init ~/Code` |
| "track this repo" | `devp link .` |
| "stop tracking this" | `devp unlink .` |
| "the registry is full of folders I deleted" | `devp unlink --missing` — clears every entry whose directory is gone |
| "undo that" | `devp undo` (reverts the last `init` or `link`) |
| "never touch this repo" | create `ignore.devprune.json` in its root, or press `i` in `devp status` |
| "what's my config?" | `devp config show` |
| "change a setting" | `devp config set idle_days 30` |
| "is the background stuff working?" | `devp setup --status` |
| "set it all up" | `devp setup` |
| "turn the automation off" | `devp config set auto_setup false` |
| "remove it" | `devp uninstall` — removes the program itself, PATH entry and agent skill included, then sweeps PATH and the well-known install dirs (`~/.cargo/bin`, `~/.local/bin`, npm global, venv Scripts) for every other copy and removes them after one confirmation. Non-interactively the sweep needs `-y` or it skips those copies with a note. Each manager-owned copy gets its manager's uninstall line printed (add `--deep` to wipe config — confirm first) |
| "what version?" | `devp -V` (also prints OS, arch, config path, PATH audit) |
| you need to *read* the answer rather than show it | add `--json` to `run`, `status`, `stats` or `caches` — see below |

Global flags, valid on any subcommand: `--dry-run`, `--ignore-idle` (`--force` is the
deprecated alias), `--yes` / `-y`.

A `.` wherever `[PATH]` appears means the current working directory, and it is the
default for `init`, `link`, `unlink`, `restore` and `config project` — so `devp link` and
`devp link .` are the same command. `devp run` is the exception: with no path it runs
across *every* registered repository, and `devp run .` restricts it to this one. A
leading `~` is expanded by dev-prune itself, so `devp init ~/Code` works in PowerShell
and cmd as well as in bash.

Shorthands: `devp hook`, `devp daemon` and `devp icon` are the `config` subcommands of
the same name, and `install` / `uninstall` / `on` / `off` work wherever `enable` /
`disable` does. A misspelled action is rejected, not silently treated as `status`.

## 🔢 Exit codes

| Code | Meaning | What you should do |
| :---: | :--- | :--- |
| `0` | Success — including "nothing was idle enough to prune" and a cancelled TUI selection | Report what happened; do not retry |
| `1` | The command failed; the reason is on stderr | Read stderr, act on it, do not retry blindly |
| `2` | The arguments were not usable | Fix the command, consult the command map above |

A run in which any repository failed exits `1` even if others succeeded.

---

## 🔌 Machine-readable output — prefer this over parsing the human report

`devp run --json`, `devp status --json`, `devp stats --json` and `devp caches --json` each
emit **one** JSON document on stdout and nothing else; warnings go to stderr. `--json`
implies non-interactive, so it never blocks on a prompt. `status --json` and
`stats --json` change nothing at all, not even the registry file. (When a human runs
`--json` in a real terminal the document is also copied to their clipboard, with a
dimmed stderr note — piped output, the way you consume it, never triggers this.)

```bash
devp status --json          # what exists, what is reclaimable, are the integrations up
devp status --top 10 --json # the same, trimmed to the ten biggest — totals still cover all
devp stats --json           # what has already been reclaimed, and by which repositories
devp run --dry-run --json   # what a pass would do, with exact byte counts
devp run -y --json          # do it
devp caches --json          # every package-manager cache, sized, largest first
devp status --drift --json  # unrecorded packages per environment, with the record command
```

`stats` reports history, `status` reports opportunity — reach for `stats` when the user
asks what dev-prune *has* done and `status` when they ask what it *could* do. On a machine
upgraded from 1.0.0 the per-repository figures and the pass list in `stats` start empty
while the lifetime total does not; the document's `history_starts_at` field says so, so
report the gap rather than reading it as "nothing was ever pruned".

Every document carries `schema`, an integer that increases **only** when a consumer would
have to change: a field removed, renamed, or given a new meaning. Adding a field does not
bump it — parse permissively and ignore what you do not recognise. It is `1` today.

The fields worth reading first:

| Path | Use it for |
| :--- | :--- |
| `summary.errors` (run) | "did anything go wrong" — the whole answer, in one integer (counts `lockfile_error`, `activity_check_error`, `delete_error` and `config_error`) |
| `results[].status` | `pruned`, `skipped_dry_run`, `skipped_active`, `skipped_symlink`, `ignored`, `no_bloat`, `disabled`, `path_missing`, `lockfile_error`, `activity_check_error`, `delete_error`, `config_error` |
| `results[].message` | the failure detail — present on the four error statuses and on `skipped_symlink`, where it names the link |
| `results[].fix_command` | present only on `lockfile_error`, and only when the fix is one mechanical command you may run unattended |
| `repositories[].state` (status) | `candidate`, `active`, `ignored`, `no_bloat`, `path_missing`, `config_error` |
| `repositories[].error` | the parse failure — present only on `config_error` |
| `totals.reclaimable_bytes` | the number to quote back to the user |
| `results[].shared_bytes` / `directories[].shared_bytes` | bytes hardlinked into a pnpm/bun store and therefore excluded from `bytes` — if the user asks why the figure is smaller than the folder size, this is the answer |
| `summary.total_bytes` (caches) | every package-manager cache added up; `caches[].clear_command` is what the *user* runs, never you |

`status` tags and `state` tags are separate vocabularies: the first is what a pass *did*,
the second is why a repository *is or is not* a candidate. Both are stable and lowercase
snake_case; the human wording above them is not, so never match on it.

## 🎛️ Narrowing a pass

| Flag | Effect |
| :--- | :--- |
| `--only npm,pnpm` | act on these package managers alone; an unknown name is an error, not a silent no-op |
| `--skip cargo` | leave these alone — the usual way to keep a slow `cargo build` out of a pass |
| `--min-size 100` | ignore anything under 100 MiB for this run, overriding `min_size_mb` |

---

## 🧩 One repository, many projects

A repository is not assumed to be one project. dev-prune walks the root and up to six
levels below it, and every directory a package manager recognises is verified and pruned
on its own terms — uv, npm and cargo in one root, or spread across `frontend/`,
`services/api/` and `tools/cli/`. Output is by repository-relative path:

```
  • MyMonorepo → frontend/node_modules (412.7 MB) [pnpm]
  • MyMonorepo → services/api/.venv (188.2 MB) [uv]
  • MyMonorepo → tools/cli/target (1.4 GB) [cargo]
```

Discovery never enters a dependency tree, a virtual environment, a hidden directory, or
a nested `.git`.

When npm, pnpm, yarn and bun all claim the same `node_modules`, one owner is chosen: the
`packageManager` field in `package.json`, else whichever manager's bookkeeping files are
inside the installed tree, else the most recently written lockfile. For Python, uv wins
over plain venv when `uv.lock` or `[tool.uv]` is present.

## 📦 Adapters

| Ecosystem | Detected by | Deletes | Verified with | Restored with |
| :--- | :--- | :--- | :--- | :--- |
| npm | `package-lock.json` | `node_modules` | `npm ci --dry-run --ignore-scripts` | `npm ci` |
| pnpm | `pnpm-lock.yaml` | `node_modules` | `pnpm install --lockfile-only --frozen-lockfile` | `pnpm install --frozen-lockfile` |
| yarn | `yarn.lock` | `node_modules` | `yarn install --immutable --mode update-lockfile` (Berry) | `yarn install --immutable` |
| bun | `bun.lockb`, `bun.lock` | `node_modules` | `bun install --frozen-lockfile --dry-run --ignore-scripts` | `bun install --frozen-lockfile` |
| uv | `uv.lock`, `[tool.uv]` | `.venv` | `uv lock --locked` | `uv sync` |
| venv | `requirements.txt` + `pyvenv.cfg` | every dir holding `pyvenv.cfg` | `requirements.txt` lists ≥1 package and accounts for every installed package | `python -m venv .venv && pip install -r requirements.txt` |
| cargo | `Cargo.toml` | `target` | `cargo metadata --locked` | next `cargo build` |
| go | `go.mod` | `vendor` | `go mod download` | `go mod vendor` |

Every verification command is **read-only**. A lockfile that has drifted from its
manifest is a refusal, not something to quietly fix — a prune pass can be started by the
scheduler, and it must never leave a modified tracked file behind. Each adapter's writing
form (`npm install --package-lock-only`, `uv lock`, `cargo generate-lockfile`,
`go mod tidy`, …) runs in exactly two cases: no lockfile exists at all, or the user set
`allow_manifest_rewrite true`.

## 📄 Configuration

**Global** (`devp config get|set|show`) — in `%APPDATA%\dev-prune` on Windows,
`~/Library/Application Support/dev-prune` on macOS, `$XDG_CONFIG_HOME/dev-prune` on Linux:

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `idle_days` | `15` | Days untouched before a repository is a candidate |
| `check_interval_days` | `2` | How often the OS scheduler runs |
| `auto_setup` | `true` | Whether the integration pass may run unattended |
| `auto_hooks` | `true` | Whether that pass may install global Git hooks |
| `auto_daemon` | `true` | Whether that pass may register the OS scheduler |
| `command_timeout_secs` | `600` | Ceiling on any package-manager command |
| `require_confirmation` | `true` | Whether a prune pass asks before deleting |
| `min_size_mb` | `0` | Smallest bloat directory worth deleting, in MiB; `0` means no floor |
| `scan_depth` | `6` | How many directory levels below a repository root discovery descends; `config set` accepts `1`–`32` |
| `allow_manifest_rewrite` | `false` | Whether a pass may run the *writing* sync form that repairs a drifted or missing lockfile |
| `auto_hooks_chain` | `false` | Whether unattended setup may take `core.hooksPath` from another tool and forward to it |
| `update_check` | `true` | Whether to ask GitHub for the latest release. Sends nothing but the request itself |
| `update_check_interval_days` | `7` | Days between automatic release checks; `devp update` always asks |
| `update_check_timeout_secs` | `5` | How long the release check waits for GitHub before giving up |

**Per repository:**
- `ignore.devprune.json` in the root — instant skip, checked before anything is parsed.
- `.devprune.json` — `"project_name"`, `"ignore": true`, `"override_idle_days": 30`,
  `"min_size_mb": 100`, `"scan_depth": 10`, `"disable_daemon": true` (excluded from
  scheduled passes only), `"disable_hooks": true` (not auto-registered by the global
  hook). Inert data only.

A `.devprune.json` that will not parse skips the repository and reports the syntax error
rather than falling back to defaults — the unreadable file may have been the one saying
`"ignore": true`.

## 🤖 Background automation

The integrations install themselves: at install time, and again on the first command
after an upgrade if anything went missing. `devp setup` is that same pass run by hand,
and `devp setup --status` reports without changing anything.

It installs five things — the `devp` second binary, the exported `SKILL.md`, global Git
auto-registration hooks (`post-commit`, `post-checkout`, `post-merge`, each running
`dev-prune link . --quiet` in the background), the OS scheduler (`schtasks` on Windows, a
LaunchAgent on macOS, a systemd user timer on Linux) running `dev-prune run --yes
--daemon`, and the file-manager icon registration for `*.devprune.json`.

It declines rather than forces. It skips the hooks when `git` is not on `PATH`. It does
not touch editor settings — `devp config icon` registers the icon with the **OS file
manager** (Explorer, Finder, Nautilus and friends) and, for editors, only *prints* a
`settings.json` snippet for you to paste. And it does not register repositories: which
directories to track stays the user's decision.

> ⚠️ Git supports exactly one global `core.hooksPath`, and while it is set, per-repo
> `.git/hooks` are ignored machine-wide. dev-prune does not simply give up when another
> tool holds it. `devp hook install --chain` takes the slot and writes a shim per hook
> that `exec`s the displaced directory's hook of the same name — same arguments, same
> stdin, same exit code — so husky, pre-commit or lefthook keep running exactly as before.
> `devp hook uninstall` restores the previous `core.hooksPath`. Chaining is **opt-in**
> (`auto_hooks_chain`, default `false`): unattended setup will not rewire another tool's
> Git configuration, so with it off the pass skips the hooks and names the owner. The
> chain is a snapshot — a hook the other tool adds later has no shim; `devp hook status`
> names the drifted ones and `devp hook install --chain` rebuilds. To decline the whole
> thing instead: `devp config set auto_hooks false` and `devp config hook disable`.

Off switches, narrowest first: `auto_hooks_chain`, `auto_daemon`, `auto_hooks`,
`auto_setup`. For containers
and CI, `DEV_PRUNE_NO_AUTO_SETUP=1` overrides all four with no config file — the same
variable the install scripts read, so setting it once covers install time and every run
after it.

---

## 🩺 Troubleshooting decision tree

Start with `devp doctor` for the installation, or `devp doctor <path>` for one repository
— it runs every check in the table below at once, changes nothing, and ends by naming the
single reason a prune pass would or would not touch that repository. Exit `1` means
something is genuinely broken; warnings alone exit `0`. When the verdict says findings
can be repaired automatically, `devp doctor --fix` mends them — it re-runs the same setup
passes automatic installation uses, so it can never do more than `devp setup` would, and
it never performs a first-time install. Reach for the table when you need the fix for a
specific symptom the report named.

| Symptom | Cause | Fix |
| :--- | :--- | :--- |
| `devp: command not found` | The bin directory is not on `PATH` in this shell | `devp -V` prints the bin directory and a PATH audit. Open a new terminal; if it persists, re-run the installer |
| Command works as `dev-prune` but not `devp` | The second executable is missing — `devp` is a real binary next to `dev-prune`, not a shell alias | `dev-prune setup` |
| "not a git repository" | dev-prune only operates inside a `.git` root | `git init`, or point at the real repository root |
| Repository shows as **Active**, nothing pruned | Committed or modified inside `idle_days` | Expected. `devp config set idle_days N`, or `--ignore-idle` for one run (ask first) |
| Repository shows **Ignored** | `ignore.devprune.json` exists, or `"ignore": true` | Delete the file, or press `i` in `devp status` |
| A project in the repository is never listed | It sits deeper than `scan_depth` (6 levels) | `devp config set scan_depth N`, or set `"scan_depth"` in that repository's `.devprune.json` |
| A small bloat directory is never offered | It is under `min_size_mb` | `devp run --min-size 0` for one pass, or `devp config set min_size_mb 0` |
| A repository is not in `devp status` at all | It was never registered | `devp link .` in it, or `devp init <parent dir>` |
| **Path Missing** | The directory was moved or deleted | `devp unlink <old path>`, then `devp link <new path>`. For a registry full of them, `devp unlink --missing` clears the lot |
| Lockfile verification fails | The lockfile has drifted from the manifest, and verification is read-only so it refuses rather than repairing | Run the exact command dev-prune printed (it is the writing form for that ecosystem), in that project. Or `devp config set allow_manifest_rewrite true` to let dev-prune run it during the pass. Never delete the lockfile |
| "holds package(s) that the lockfile does not record" | Something was installed without recording it — `npm install --no-save`, a bare `pip install` into a pinned venv, an ad-hoc `uv pip install` | `devp status --drift` lists every such environment with the exact record command (`npm install <pkg>`, `uv add <package>`, `pip freeze > requirements.txt`). Run it, or uninstall the extras. Never delete the directory manually |
| "`npm` is not available" | that manager is not installed, or not on `PATH` | Install it. dev-prune will not delete a tree it cannot prove it can rebuild |
| Verification times out | A slow registry or a very large project | `devp config set command_timeout_secs 1800` |
| Hooks not installed | `git` absent from `PATH`, or another tool owns `core.hooksPath` | `devp setup --status` says which. Install git, or `devp hook install --chain` to sit in front of the other tool and forward to it |
| Scheduler not running | `auto_daemon false`, or the pass was declined | `devp setup --status`, then `devp daemon install` |
| Automation came back after uninstall | An upgrade re-ran the pass | `devp config set auto_setup false` |
| A bloat directory was skipped silently | It is a symlink or junction | Expected and deliberate — it points at storage the repository does not own |

## 🧭 Recipes

**Audit a machine's reclaimable space, safely:**
```bash
devp init ~/Code        # register everything under a workspace root
devp run --dry-run      # report only — deletes nothing, runs no manager
```

**Reclaim space, then get one project working again:**
```bash
devp run -y
devp restore ~/Code/the-one-I-need-now
```

**Set up on a fresh machine without background automation:**
```bash
DEV_PRUNE_NO_AUTO_SETUP=1 devp init ~/Code
devp config set auto_setup false
```

**Diagnose before asking the user anything:**
```bash
devp doctor          # installation: binary, PATH, config, integrations, registry
devp doctor .        # this repository: why it would or would not be pruned
```

---

## 📚 Documentation

Read these when the answer is not above. Local paths work in a checkout; the URLs work
anywhere.

| Topic | Path | URL |
| :--- | :--- | :--- |
| Index | `docs/README.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/README.md |
| Every command, flag and exit code | `docs/CLI_REFERENCE.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/CLI_REFERENCE.md |
| Safety invariants, in depth | `docs/SAFETY_INVARIANTS.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/SAFETY_INVARIANTS.md |
| Setup pass, scheduler, hooks | `docs/BACKGROUND_AUTOMATION.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/BACKGROUND_AUTOMATION.md |
| Install, PATH, permissions | `docs/troubleshooting/INSTALLATION_ISSUES.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/INSTALLATION_ISSUES.md |
| Lockfile & adapter errors | `docs/troubleshooting/LOCKFILE_AND_ADAPTERS.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/LOCKFILE_AND_ADAPTERS.md |
| Daemon & hook problems | `docs/troubleshooting/DAEMON_AND_HOOKS.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/DAEMON_AND_HOOKS.md |
| Uninstall & state recovery | `docs/troubleshooting/UNINSTALL_AND_REINSTALL.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/UNINSTALL_AND_REINSTALL.md |
| Corruption & edge cases | `docs/troubleshooting/CORRUPTION_AND_EDGE_CASES.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/CORRUPTION_AND_EDGE_CASES.md |
| Architecture (HLD / LLD) | `docs/architecture/` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/architecture/HLD.md |
| Writing a new adapter | `docs/ADDING_ADAPTERS.md` | https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/ADDING_ADAPTERS.md |

A machine-readable summary for agents lives at <https://devprune.vkrishna04.me/llms.txt>.
