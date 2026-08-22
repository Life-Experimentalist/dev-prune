# `dev-prune` & `devp` Complete CLI Command Reference

`dev-prune` (executable via its shorthand alias **`devp`**) provides a full set of subcommands, global flags, configuration options, and status shortcuts for universal workspace maintenance.

---

## ⚡ Two Names for One Binary

`devp` is **not** a shell alias, a PowerShell profile function or a `doskey` macro — it is
a second executable sitting next to `dev-prune`. Both names behave identically and can be
used interchangeably in any terminal:
```bash
dev-prune status
devp status
```

On installation, `dev-prune` creates a hard link (or, where that is not possible, a copy) named `devp` alongside `dev-prune`, so both names work in every shell — cmd, PowerShell, bash, fish, an IDE terminal, a scheduled task — without a profile alias that has to be re-sourced. It is refreshed whenever it no longer matches the binary, so an upgrade cannot leave `devp` running the previous version.

---

## 🔤 How arguments are read

Before clap parses anything, [`normalize_args()`](../src/lib.rs) rewrites the argument
vector so that the phrasings people actually type resolve to the real subcommand. This is
the only rewriting that happens; nothing else about your arguments is second-guessed.

```mermaid
flowchart TD
    Argv(["std::env::args()"]) --> V{"exactly one arg,<br/>-v / -V / --version?"}
    V -->|Yes| Ver["print the environment audit<br/>exit 0"]
    V -->|No| H{"-h, --help or help anywhere,<br/>or no args at all?"}
    H -->|Yes| Ban["print the banner, then continue"]
    H -->|No| Lower
    Ban --> Lower{"no args at all?"}
    Lower -->|Yes| AsIs["hand argv to clap unchanged"]
    Lower -->|No| Case["lowercase argv[1]<br/>only if it is not a flag"]
    Case --> R1{"argv[1] is daemon / hook / icon?"}
    R1 -->|Yes| Ins["insert 'config' before it<br/>devp hook install → devp config hook install"]
    R1 -->|No| R2
    Ins --> R2{"argv[1] is status and the<br/>last word is daemon or hook?"}
    R2 -->|Yes| Sw["devp status PATH hook<br/>→ devp config hook PATH status"]
    R2 -->|No| R3{"argv is config PATH daemon|hook ...?"}
    R3 -->|Yes| Sw2["devp config PATH hook ACTION<br/>→ devp config hook PATH ACTION"]
    R3 -->|No| Out(["clap parses the result"])
    Sw --> Out
    Sw2 --> Out
    AsIs --> Out
```

Practical consequences:

- `devp STATUS`, `devp Status` and `devp status` are the same command. Only the
  subcommand word is lowercased — paths and values keep their case, which matters on
  case-sensitive filesystems.
- `devp hook install` and `devp config hook install` are the same command, and so are
  `devp status ~/proj hook` and `devp config hook ~/proj status`.
- An unrecognised subcommand is **not** guessed at. It reaches clap and exits `2`.

---

## 🚩 Global Flags

| Flag | Short | Description |
| :--- | :---: | :--- |
| `--dry-run` | | Simulate pruning without deleting files or running lockfile updates |
| `--ignore-idle` | | Prune repositories you are still working in. Lifts the idle-day threshold and **nothing else** — lockfile verification, `ignore.devprune.json`, `"ignore": true`, the size floor, symlink refusal and nested-repository refusal all still apply |
| `--force` | | Deprecated spelling of `--ignore-idle`. Still accepted. Prints the rename note and, because nobody types it for fun, the full list of the seven reasons a directory gets skipped and how to fix each one |
| `--yes` | `-y` | Bypass interactive confirmation prompts |
| `--help` | `-h` | Print help information |
| `--version` | `-V` | Display rich version, system architecture, and PATH audit details |

`--force` was renamed because the word promised something it never delivered: it reads
like "override the safety checks", and it only ever skipped the idle-day wait. No flag
bypasses lockfile verification, and none ever will.

Wherever a command takes `[PATH]`, a `.` means the current working directory — and it is
the default for `init`, `link`, `unlink`, `restore` and `config project`, so `devp link`
and `devp link .` are the same command. `devp run` is the deliberate exception: with no
path it runs across *every* registered repository, so pruning one project has to be asked
for by name. A leading `~` is expanded by dev-prune itself rather than by the shell, which
means `devp init ~/Code` works identically in bash, PowerShell and cmd, and keeps working
when the argument is quoted.

---

## 🔢 Exit Codes

| Code | Meaning |
| :---: | :--- |
| `0` | Success. A prune that deleted nothing because nothing was idle is a success, and so is a cancelled TUI selection. Also used when output is piped into a reader that closes early (`devp status \| head`). |
| `1` | The command failed. The reason is printed to stderr. |
| `2` | The arguments were not usable — unknown subcommand, missing value, invalid action word. |

---

## 💻 Subcommands

This reference is also built into the binary: `devp <command> --help` prints the
long-form help for any command — the full description plus worked examples — and it
goes all the way down (`devp config hook --help`, `devp config get --help`, …). `-h`
prints the short version, `devp help <command>` is equivalent to `--help`.

### 1. `devp init [PATHS...]`
- **Aliases**: `scan`, `onboard`
- **Description**: Crawls the provided directory trees (defaults to current directory `.`, max depth 8) for valid Git repositories and registers them in `~/.config/dev-prune/registry.json` (`%APPDATA%\dev-prune\` on Windows, `~/Library/Application Support/dev-prune/` on macOS). It then runs the same integration pass as [`devp setup`](#11-devp-setup---status), installing anything missing and reporting anything it skipped, and checks for a newer release the same way [`devp update`](#10-devp-update---offline----install) does.
- **Examples**:
  ```bash
  devp init ~/Code
  devp init /path/to/project1 /path/to/project2
  ```

---

### 2. `devp link [PATH]`
- **Description**: Registers a single Git repository for pruning (defaults to current directory `.`).
- **Flags**:
  - `--quiet` — print nothing, and skip repositories whose `.devprune.json` sets `disable_hooks`. This is the form the global Git hook invokes; it exists so a hook firing inside someone's commit never writes to their terminal or registers a workspace that opted out.
- **Moved repositories are reconnected.** Registering a repository records its root commit. If an entry whose path no longer exists carries the same root commit, that entry *is* this repository at its old location: `link` and `init` take it over — its `added_at`, prune history, lifetime total, idle override and disabled state all move across — and remove the dead row instead of leaving it beside a second entry starting from zero. Two missing entries sharing one root commit are clones rather than a move, so nothing is adopted and dev-prune says so. Entries registered before 1.4.0 have no root commit recorded; running `devp init` over your code directory once records them all.
- **Examples**:
  ```bash
  devp link
  devp link /path/to/my-repo
  ```

---

### 3. `devp unlink [PATH]`
- **Description**: Removes a repository from the `dev-prune` registry. Does **not** delete any workspace files on disk.
- **Flags**:
  - `--missing` — remove every registered path whose directory no longer exists, instead of one named repository. Clones that were deleted, drives that were reformatted and workspaces that were moved all leave entries behind; [`devp doctor`](#12-devp-doctor-path---fix) counts them in a single warning and sends you here rather than printing one `devp unlink` line per dead path. Conflicts with `PATH`.
- **Examples**:
  ```bash
  devp unlink
  devp unlink /path/to/my-repo
  devp unlink --missing
  ```

---

### 4. `devp undo`
- **Description**: Reverts the most recent `init` or `link` registration action.
- **Examples**:
  ```bash
  devp undo
  ```

---

### 5. `devp run [TARGET_PATH]`
- **Description**: Executes a prune pass across all registered repositories or a specific target repository path.
  1. Checks for 0ms fast-path `ignore.devprune.json` file.
  2. Evaluates repository inactivity (`git log` commit history + source `mtime` modification timestamps).
  3. Discovers every package-manager project in the repository — the root and up to `scan_depth` levels below it (six by default).
  4. Pre-verifies required package manager binaries (`npm`, `pnpm`, `uv`, `cargo`, `go`, etc.).
  5. Calculates reclaimable disk space and launches interactive selection TUI (unless `-y` is passed).
  6. Enforces two-tier lockfile safety with configurable command timeout (`command_timeout_secs`).
  7. Safely removes bloat directories (`node_modules`, `.venv`, `target`, `vendor`).
- **Flags**:

  | Flag | Meaning |
  | :--- | :--- |
  | `--only <ADAPTERS>` | Restrict the pass to a comma-separated list of package managers |
  | `--skip <ADAPTERS>` | Exclude a comma-separated list of package managers |
  | `--min-size <MIB>` | Ignore bloat directories smaller than this, overriding `min_size_mb` for the run |
  | `--except <REPOS>` | Prune every registered repository *except* these (comma-separated) |
  | `--daemon` | Mark this as the scheduled background pass; repositories that set `disable_daemon` are skipped. Set by the installed scheduler |
  | `--json` | Emit one machine-readable document instead of the human report |
  | `--explain` | Explain every decision instead of pruning: each repository and directory with the reason it would or would not be touched. Read-only |

  `--explain` reports the states a normal pass keeps quiet about — a repository still
  active (with how many days ago the last activity was, against the threshold), one
  opted out, a directory under the size floor — alongside what would be pruned. It
  verifies nothing and deletes nothing, always exits `0` when the analysis itself ran,
  and cannot be combined with `--json` (whose `--dry-run` document already carries every
  status). It composes with a target path and with `--only`/`--skip`/`--min-size`/
  `--except`/`--ignore-idle`, so "why did that repo not prune?" is answerable one flag
  at a time.

  Adapter names are `npm`, `pnpm`, `yarn`, `bun`, `uv`, `venv`, `poetry`, `pdm`,
  `pipenv`, `cargo`, `go`, `composer`, `bundler`, `cocoapods`, `mix`, `terraform` —
  plus `gradle`, `maven`, `swift` and `dart`, which are opt-in (see below). An unrecognised name is an error
  listing the valid ones rather than a silently empty pass, and `--only` and `--skip`
  cannot be combined. A name listed in `disabled_adapters` is gone from that set
  entirely, and `--only <that name>` prunes nothing.

  `bundler` and `pipenv` claim only the install inside the repository —
  `vendor/bundle` after `bundle config set path vendor/bundle`, and `.venv` under
  `PIPENV_VENV_IN_PROJECT`. Both tools default to a shared store under your home
  directory, which dev-prune never touches: that is where other projects' dependencies
  are installed, not a cache, so neither a prune nor `devp caches` goes near it.
  `composer` declines `vendor/` outright when a `vendor/bundle` is inside it: deleting
  it would take gems with it under a proof that says nothing about them.

  `terraform` claims `.terraform/providers` and nothing else under `.terraform/`.
  The sibling directories are not bloat: `environment` records the selected workspace,
  and losing it silently returns you to `default` — the next `apply` then targets the
  wrong environment. `terraform.tfstate` is the backend's initialisation record, and
  `modules/` is fetched from module sources that `.terraform.lock.hcl` says nothing
  about. Providers are the bulk anyway, and the lock file proves those exactly.

  **Cargo, Gradle, Maven, SwiftPM and Dart are opt-in adapters.** `devp config set
  enable_cargo true` / `enable_gradle true` / `enable_maven true` / `enable_swift true`
  / `enable_dart true` turns each on; until then the adapter is invisible everywhere — `status`, `run`,
  `--only cargo` all behave as if it did not exist. They are off by default because of
  what it costs to get their directories back, not because of any doubt that they come
  back: `target/`, `build/`, `.gradle/` and `.build/` are compiler output, so they
  return by *recompiling from the sources in the repository* rather than by
  re-downloading, and a large workspace can spend minutes doing that where a dependency
  reinstall spends seconds. For the same reason they answer to their own idle threshold:
  `build_idle_days` (45 by default) is applied as `max(build_idle_days, idle_days)`,
  so a repository must be idle much longer before its build directories go than before
  its dependency directories do. Recoverability is proved by the build manifest
  (`Cargo.lock`, `pom.xml`, `build.gradle`/`settings.gradle`, `Package.swift`,
  `pubspec.lock`) being present and readable — for cargo, `cargo metadata --locked`; for the rest, no network
  command runs at all. User-home caches (`~/.cargo`, `~/.m2`, `~/.gradle`) are never
  touched; those belong to `devp caches`.

  **Any one adapter can wait longer still.** `devp config set adapter_idle_days
  cargo=90,npm=30` gives a named adapter its own window, applied as
  `max(idle_days, build_idle_days, adapter_idle_days[name])` — a floor, never a bypass,
  so no per-adapter number can make dev-prune touch a repository the global window
  still considers active. `devp config set adapter_idle_days -` clears the map, and
  `devp config wizard` edits it as a column beside the adapter checklist, where a
  language heading sets the same window for every adapter under it at once.

  `--except` is the safe spelling of "clean up but keep the API project". Each entry
  matches a registered repository by full path, by directory name, or case-insensitively
  with any trailing slash ignored, and a leading `~` is expanded — a comma-separated list
  arrives as one argument, so no shell would expand a `~` sitting inside it. An excluded
  repository is never verified, never deleted and never restored, which is the difference
  between skipping it and pruning it and downloading it all back. The same shape is
  available interactively: `devp status` → `p` pre-selects every candidate, and `Space`
  deselects the one you are keeping.
- **Examples**:
  ```bash
  devp run --dry-run
  devp run
  devp run ~/Code/MyProject
  devp run --ignore-idle -y
  devp run --except api-service,~/Code/playground
  devp run --only cargo,uv --dry-run
  devp run --skip venv --min-size 50
  devp run --json --dry-run
  devp run --explain
  ```

Directories are reported by their path relative to the repository root, so a monorepo
reads unambiguously:

```
  • MyMonorepo → frontend/node_modules (412.7 MB) [pnpm]
  • MyMonorepo → services/api/.venv (188.2 MB) [uv]
  • MyMonorepo → tools/cli/target (1.4 GB) [cargo]
```

---

### 6. `devp status [--top N] [--drift] [--json]`
- **Description**: Displays an interactive Ratatui terminal dashboard summarizing registered repositories, status (Candidate, Active, Ignored, No Bloat, Path Missing, Unreadable `.devprune.json`), reclaimable space, and last activity date. A repository whose `.devprune.json` does not parse is reported as such rather than as a candidate — `devp run` refuses to touch it, and the dashboard says the same thing.
- **Environment**:
  - `DEV_PRUNE_SCAN_THREADS` — overrides the status scan's thread count (clamped to 32; `1` forces a sequential scan). The automatic figure suits local disks; raise it on a network filesystem, where the scan waits on latency rather than on the CPU, and set it to `1` on a spinning disk, where fewer threads means less head thrashing and a genuinely faster scan.
- **Interactive TUI Keybindings**:
  | Key | Action |
  | :--- | :--- |
  | `↑` `↓` / `k` `j` | Move the selection |
  | `PgUp` `PgDn` | Jump ten rows |
  | `Home` `End` / `g` `G` | Jump to the first or last row |
  | `s` | Cycle the sort: relevance, size, longest idle, name |
  | `f` | Cycle the filter: all, candidates, has bloat, problems |
  | `/` | Search paths and adapter names; `Enter` keeps the query, `Esc` clears it |
  | `p` | Enter Prune-Select mode, with every candidate **on screen** pre-selected |
  | `Space` | Toggle the current row (Prune-Select mode) |
  | `a` | Toggle every candidate on screen (Prune-Select mode) |
  | `Enter` | Prune the selected repositories (Prune-Select mode) |
  | `i` | Toggle `ignore` in the repository's `.devprune.json`; the table refreshes immediately |
  | `Esc` | Leave Prune-Select mode, or exit from the browse view |
  | `q` / `Ctrl-C` | Exit the dashboard |

  The sort, the filter and the search change only what is *displayed*. A repository
  checked for pruning stays checked when a filter hides it, and the counts in the header
  are always over the whole registry — so a filtered dashboard can never make a machine
  look tidier than it is.

  The terminal is restored on every exit path, including an error or a panic inside the view.

  The header shows two totals, not one. **Ready now** is what a prune would reclaim
  today — the sum over the repositories that are actually idle enough to be candidates.
  **Reclaimable in all** counts every dependency directory in the registry, including
  the project you worked in this morning. Both are always over the whole registry,
  never over the filtered view.

  The scan behind the dashboard sizes every dependency tree in every registered
  repository, so on a large registry it takes a few seconds. It runs across several
  threads and reports a progress bar while it does; `--json` produces the document and
  nothing else.

  With no TTY — piped, redirected, or run from a scheduler — `devp status` prints a plain
  table instead of entering the TUI. `--json` replaces it with a machine-readable document
  and makes no changes of any kind.

  Sizes are what deleting the directory actually gives back. For most managers that is
  the folder's apparent size, but pnpm and bun hardlink packages out of a global store
  (on Windows too — NTFS hardlinks, whenever store and project share a volume), so most
  of a pnpm `node_modules` is not freed by deleting it: the store keeps the bytes. Those
  shared bytes are measured per file via the link count and excluded from every
  reclaimable and freed figure; the plain table, the run report and `--json`
  (`shared_bytes`) say how much was excluded and why. A pnpm install that *copied* —
  store on another volume, a filesystem without hardlinks — has no external links, so it
  still counts in full. This is also why `pnpm install` after a prune is fast.
- **Flags**:
  - `--top <N>` — list only the `N` repositories with the most reclaimable space. On a
    machine tracking a hundred repositories the handful worth pruning are otherwise pushed
    off the screen. The survivors stay in the dashboard's usual order, so the list reads as
    a shorter version of the full one rather than a re-sorted one. **The totals above the
    table are unaffected** — they are computed over every registered repository, so
    `--top 5` cannot make a machine look tidier than it is. Applies to the TUI, the plain
    table and `--json` alike; in JSON the trim is reported as a top-level `"top"` field.
  - `--drift` — replace the dashboard with the lockfile-drift report: every registered
    repository, checked for environments holding packages their lockfile never recorded
    (an `npm install --no-save`, a bare `pip install` into a pinned venv, an ad-hoc
    `uv pip install`). This is the same comparison a prune *refuses* on, run early and as
    a pure read — no package manager is executed and nothing is written. Each finding
    names the directory, the unrecorded packages, and the one command that records them
    (`npm install <pkg>`, `uv add <package>`, `pip freeze > requirements.txt`). Only the
    adapters that can compare an environment against its lockfile from files alone take
    part — npm, uv and venv; the others stay silent rather than guess. Cannot be combined
    with `--top`: the report is not a repository list, so trimming it would mean nothing.
  - `--json` — emit one machine-readable document instead of the dashboard.
- **Examples**:
  ```bash
  devp status --top 10
  devp status --top 10 --json | jq '.repositories[].path'
  devp status --drift           # what would a prune refuse on, and how do I fix it
  ```
- **Status Shortcuts & Aliases**:
  - `devp status daemon` (alias for `devp config daemon status`): Inspect OS background daemon scheduler status.
  - `devp status hook` (alias for `devp config hook status`): Inspect Git auto-registration hook status.

---

### 7. `devp caches [--json]` / `devp caches clear <MANAGER>`
- **Description**: Finds every package-manager cache and store on the machine, sizes each one, and prints the command that clears it. Largest first, with a total. On its own **it deletes nothing**, and nothing that runs on a schedule ever will; the `clear` subcommand below empties one deliberately, when you type it.
- **Why the report itself never deletes**: a cache lives outside every repository and is shared by all of them, so no single lockfile can prove its contents are recoverable — which is the bar every deletion in dev-prune has to clear. It is also what makes [`devp restore`](#9-devp-restore-path---last-run) fast: clearing a cache turns the next reinstall into a download. So a cache is only ever emptied by a command you type, when you want the space more than the speed.
- **What it looks at**:

  | Manager | Cache | Cleared by |
  | :--- | :--- | :--- |
  | `npm` | cache | `npm cache clean --force` |
  | `pnpm` | store | `pnpm store prune` |
  | `yarn` | cache | `yarn cache clean` |
  | `bun` | cache | `bun pm cache rm` |
  | `uv` | cache | `uv cache prune` |
  | `pip` | cache | `pip cache purge` |
  | `cargo` | registry cache, registry sources | deleting `$CARGO_HOME/registry/{cache,src}` — cargo ships no cache subcommand |
  | `go` | module cache, build cache | `go clean -modcache`, `go clean -cache` |
  | `maven` | local repository | deleting `~/.m2/repository` — Maven ships no cache subcommand either |
  | `gradle` | caches, wrapper distributions | deleting `~/.gradle/caches` and `~/.gradle/wrapper/dists` (`GRADLE_USER_HOME` respected) |
  | `nuget` | global packages | `dotnet nuget locals global-packages --clear` |
  | `vcpkg` | binary cache | deleting the `vcpkg/archives` directory (`VCPKG_DEFAULT_BINARY_CACHE` respected) |
  | `conan` | package cache | `conan remove "*" --confirm` |

  Each manager is *asked* where its cache is (`npm config get cache`, `pnpm store path`, `go env GOMODCACHE`, …) rather than assumed, because `CARGO_HOME`, a `--cache-dir` and a corporate `.npmrc` all move it. Every one of those queries is read-only and is run from your home directory, so a project-local `.npmrc` cannot skew a machine-wide answer. A manager that is not installed falls back to the conventional location — a cache left behind by a manager you uninstalled is exactly the multi-gigabyte directory nobody remembers. The JVM, .NET and C++ stores are found by convention plus their relocation variables (`GRADLE_USER_HOME`, `NUGET_PACKAGES`, `VCPKG_DEFAULT_BINARY_CACHE`, `CONAN_HOME`) rather than by asking — `mvn help:evaluate` boots a JVM and resolves plugins over the network, which is the wrong price for a read-only size report. Two probes that resolve to the same directory are counted once.

  This is also where the .NET and C/C++ ecosystems live in dev-prune, and why they are *not* adapters: a .NET `bin/`+`obj/` and a CMake `build/` are compiler **outputs** — no lockfile can prove a deleted one comes back byte-for-byte, so deleting them is out of scope by design. Their *dependencies*, meanwhile, never live in the repository at all; they live in these machine-wide stores, which is exactly what this command reports. Cargo `target/`, Maven `target/` and Gradle `build/`+`.gradle/` are the deliberate exception: they have [opt-in adapters](#5-devp-run-target_path) whose recoverability claim is rebuild-from-source (`Cargo.lock`/`pom.xml`/`build.gradle` plus the machine-wide stores this command reports), which is why they ship disabled and idle-gate separately through `build_idle_days`.
- **Flags**:
  - `--json` — emit one machine-readable document instead of the table. Global within `caches`, so `devp caches clear npm --json` parses the same as `devp caches --json clear npm`.
- **Examples**:
  ```bash
  devp caches
  devp caches --json | jq '.summary.total_bytes'
  ```

#### `devp caches clear <MANAGER> [--dry-run] [--yes] [--json]`

- **Description**: Empties one manager's cache, or every one of them. What is about to go is listed and sized first, and unless `--yes` answers for you, it asks.
- **Why this is not a contradiction of the above**: the report still deletes nothing, and nothing that runs on its own ever will — no scheduler, no Git hook and no `devp run` clears a cache. `clear` runs only when you type it. It exists because typing the command the report already prints is the whole of what it does, and doing it by hand for every manager on the machine is tedious.
- **`<MANAGER>`**: a manager name as the report prints it — `npm`, `pnpm`, `yarn`, `bun`, `uv`, `pip`, `cargo`, `go`, `maven`, `gradle`, `nuget`, `vcpkg`, `conan` — or `all` for every one found. A manager that has more than one cache (cargo, go, gradle) clears both. An unrecognised name is a **usage error** (exit `2`) that lists the ones it knows.
- **How each one is emptied**: through the manager's own subcommand wherever one exists (`npm cache clean --force`, `pnpm store prune`, `go clean -modcache`), because the manager knows what is still referenced and keeps its own bookkeeping consistent — `pnpm store prune` and `uv cache prune` deliberately keep part of the store, which no directory delete can work out. Only `cargo`, `maven`, `gradle` and `vcpkg`, which ship nothing equivalent, are cleared by removing a directory, and the path removed is the one this command already resolved and sized — never a string handed to a shell.
- **If the manager is not installed**: the row is reported as failed rather than worked around. dev-prune will not delete a store directly when the manager that owns it is the only thing that knows what is safe to keep.
- **What it costs**: nothing is lost — every manager re-downloads what it needs. What goes is time, in every project on the machine, on the next install and the next [`devp restore`](#9-devp-restore-path---last-run).
- **Freed size is measured, not assumed**: each cache is sized again after the clear and the report shows `before - after`, so a `prune` that kept half the store says so, and a half-failed clear reports what actually went.
- **Flags**:
  - `--dry-run` — list and size what would go; touch nothing. (Global flag.)
  - `--yes` / `-y` — skip the confirmation. (Global flag.) Without a terminal on stdin the prompt is not shown at all: dev-prune says to pass `--yes` and clears nothing.
  - `--json` — emit one machine-readable document. **Requires `--yes` or `--dry-run`**, because a prompt on a pipe is a hang and the fallback notice would land inside the document.
- **Exit codes**: `0` if every named cache was cleared, `1` if any could not be (the rows print either way, and the reason goes to stderr), `2` for an unknown manager name or `--json` without `--yes`.
- **Examples**:
  ```bash
  devp caches clear npm            # one manager, after confirming
  devp caches clear cargo          # both cargo rows: registry cache and sources
  devp caches clear all --dry-run  # everything that would go, nothing touched
  devp caches clear all --yes      # no prompt, for a script
  devp caches clear go --json --yes
  ```

---

### 8. `devp config [ACTION]`
- **Description**: Manage global settings, per-repository configuration (`.devprune.json`), background daemons, Git hooks, or the OS file manager's icon for `.devprune.json`.
- **Sub-Actions**:
  - `config get <key>`: View a global setting.

    | Key | Default | Meaning |
    | :--- | :---: | :--- |
    | `idle_days` | `15` | How long a repository must be untouched before it is a prune candidate |
    | `min_size_mb` | `0` | Smallest bloat directory worth deleting, in MiB; `0` disables the floor |
    | `scan_depth` | `6` | How many directory levels below a repository root discovery descends. Accepts `1`–`32`; every extra level costs walk time |
    | `require_confirmation` | `true` | Whether a prune pass asks before deleting |
    | `allow_manifest_rewrite` | `false` | Whether verification may *repair* a lockfile that has drifted from its manifest, instead of refusing. Off, every adapter verifies read-only; on, each runs its writing form (`npm install --package-lock-only`, `uv lock`, `cargo generate-lockfile`, `go mod tidy`, …) |
    | `command_timeout_secs` | `600` | Ceiling on any one package-manager command run during lockfile verification |
    | `auto_setup` | `true` | Whether the integration pass may run unattended at all |
    | `auto_daemon` | `true` | Whether that pass may register the OS scheduler |
    | `check_interval_days` | `2` | How often the OS scheduler runs a pass |
    | `auto_hooks` | `true` | Whether that pass may install the global Git hooks |
    | `auto_hooks_chain` | `false` | Whether it may take a `core.hooksPath` another tool holds, forwarding every hook on to it |
    | `update_check` | `true` | Whether the periodic release check runs (see [`devp update`](#10-devp-update---offline----install)) |
    | `update_check_interval_days` | `7` | Minimum gap between two release checks |
    | `update_check_timeout_secs` | `5` | How long that one request may hang before it is abandoned |
    | `auto_config` | `false` | Whether `devp init` / `devp link` write a default `.devprune.json` into newly registered repositories |
    | `enable_cargo` | `false` | Turn on the opt-in Cargo adapter (`target/` — compiler output, so it comes back by recompiling rather than downloading) |
    | `enable_gradle` | `false` | Turn on the opt-in Gradle adapter (`build/`, `.gradle/` — they come back by recompiling) |
    | `enable_maven` | `false` | Turn on the opt-in Maven adapter (`target/`) |
    | `enable_swift` | `false` | Turn on the opt-in Swift Package Manager adapter (`.build/` — compiled modules, so they come back by recompiling) |
    | `enable_dart` | `false` | Turn on the opt-in Dart/Flutter adapter (`.dart_tool/` — pub metadata restores in a second, but the `build_runner` and `flutter_build` caches beside it come back by recompiling) |
    | `disabled_adapters` | *(none)* | Adapters to leave alone entirely, by name, comma-separated. A disabled adapter is not detected, not counted by `stats`, not probed for by `doctor` and never pruned — as if the ecosystem were not installed. `devp config set disabled_adapters -` clears the list |
    | `build_idle_days` | `45` | Extra idle threshold for the opt-in build adapters (cargo, gradle, maven, swift, dart), applied as `max(build_idle_days, idle_days)` |
    | `adapter_idle_days` | *(none)* | Per-adapter idle windows, as `cargo=90,npm=30`. Each raises only its own adapter's wait — `max(idle_days, build_idle_days, this)` — and can never lower one. `devp config set adapter_idle_days -` clears the map |
    | `auto_update` | `false` | Run `devp update --install` by itself at the end of a prune pass when a newer release is known |

    Three of these have a per-repository form in that repository's `.devprune.json`,
    where they win for that tree only: `idle_days` (spelled `override_idle_days` there),
    `min_size_mb` and `scan_depth` — the three whose right value genuinely depends on the
    project rather than on you. The rest are deliberately global. Nothing stops a project
    from committing its `.devprune.json`, so a per-repository `allow_manifest_rewrite`
    would let a repository you have never read grant itself permission to have its own
    tracked manifests rewritten during an unattended pass; `auto_*` and `update_check*`
    describe your machine, not a project, and would mean nothing per repository.

    A value outside the accepted range is rejected with the range in the message, not
    silently clamped. `scan_depth` included: `config set` refuses `0` and anything above
    `32` outright. The clamp to that range still exists, but only as the backstop for a
    hand-edited config file — a value typed at the CLI gets the truth, not a silent
    substitution.

  - `config set <key> <value>`: Modify global setting value.
  - `config show [--update]`: View all configuration values or force global update.
  - `config wizard [--no-tui]`: Open the configurator — a full-screen view of
    every setting, with the [`devp trust`](#18-devp-trust---json) declaration in front of
    it so what the tool is allowed to do is on screen before any of it is configurable.
    Arrow keys move, `Space` changes the highlighted setting (a toggle flips, a number
    opens a field, `disabled_adapters` opens the adapter checklist), `r` puts one back,
    `y` accepts everything as shown, and the last screen lists exactly what will be
    written before it is written. `q` leaves without saving anything.

    It runs itself twice: once on a fresh install, so the defaults are something you
    agreed to rather than inherited, and again after an upgrade that added a setting you
    have never been shown — that one is marked `NEW`, and it opens on it. Settings
    you already confirmed are never re-asked.

    It never runs unattended: no TTY means it is skipped, not guessed at. `--no-tui`, and
    the `DEV_PRUNE_NO_TUI` environment variable, ask one question per line instead —
    for terminals the full-screen view cannot drive, and for agents, which hold a real
    terminal and will never press a key. An agent configuring dev-prune should use
    `devp config set <key> <value>`, which needs no interaction at all.
  - `config project [PATH] [--update]`: Inspect or create per-repository `.devprune.json` config.
    Whenever the CLI writes this file it also records `.devprune.json` and
    `ignore.devprune.json` in the repository's `.git/info/exclude`, so the config —
    one machine's preference, not part of the project — never shows up in `git status`
    and the shared, tracked `.gitignore` is never modified.
  - `config daemon [PATH] [enable|disable|status]`: Configure OS background scheduler globally or for workspace.
  - `config hook [PATH] [enable|disable|status] [--chain]`: Configure Git auto-registration hooks globally or for workspace.

    Git has exactly one global `core.hooksPath` and no way to chain two, so a tool that
    holds it shuts every other one out machine-wide. `--chain` is the way out: dev-prune
    takes the slot and writes, for each hook, a small shim that does its own work and then
    `exec`s the same-named hook in the directory it displaced. husky, pre-commit and
    lefthook keep firing, in order, with their own arguments and their own exit codes —
    a non-zero exit from the displaced hook is still a non-zero exit, so a pre-commit
    check that fails still blocks the commit. `devp hook uninstall` puts the original
    `core.hooksPath` back. It is opt-in (`auto_hooks_chain`, `false`) because rewiring
    another tool's Git configuration unasked is not something an install should do.

    The chain is a snapshot of the other tool's directory at install time. If that tool
    later adds a hook dev-prune has no shim for, that hook stops firing — `devp hook
    status` reports exactly which names have drifted, and `devp hook install --chain`
    rebuilds the chain.

    Both rewrite the whole `.devprune.json`, so both refuse a file that does not parse
    rather than starting from the defaults and taking every other override in it with
    them. Fix the file, or run `devp config <PATH> --update` to reset it deliberately.

  - `config icon`: Register `*.devprune.json` with the OS file manager and write the icon files and JSON Schema into the config directory. On Linux this is a complete registration — a `shared-mime-info` package plus hicolor icons, honoured by Nautilus, Dolphin, Thunar, Nemo and PCManFM. On Windows, Explorer resolves icons from the last extension only, so `*.devprune.json` cannot be distinguished from any other `.json` without hijacking every JSON file on the machine; the config folder gets its own icon instead. On macOS a UTI must come from an application bundle, which a single binary is not. The command also prints an editor snippet you can paste yourself — it never edits your editor settings, your PATH, or your shell startup files.
- **Examples**:
  ```bash
  devp config get idle_days
  devp config set command_timeout_secs 1200
  devp config daemon status
  devp config . daemon disable
  devp config icon
  ```

#### Shorthands

`daemon`, `hook` and `icon` may be typed without the leading `config`, and the
enable/disable actions accept the words people reach for:

| You type | Runs |
| :--- | :--- |
| `devp hook install` | `devp config hook enable` |
| `devp hook uninstall` | `devp config hook disable` |
| `devp hook` | `devp config hook status` |
| `devp daemon on` / `devp daemon off` | `devp config daemon enable` / `disable` |
| `devp icon` | `devp config icon` |

Accepted action words: `enable` / `install` / `on`, `disable` / `uninstall` / `remove` /
`off`, and `status` / `show`. Anything else is rejected with an error — a mistyped
action never silently degrades into a status report.

---

## 🤖 Machine-readable output (`--json`)

`devp run --json`, `devp status --json`, `devp stats --json` and `devp caches --json` each
emit exactly one JSON document on stdout and nothing else. `status --json` and
`stats --json` make no changes of any kind — not even to the registry file — and
`caches --json` never touches anything but the sizes it reads.

Every document carries `schema`, an integer that increases only when a consumer would
have to change to keep working: a field removed, renamed, or given a new meaning. **Adding
a field does not bump it**, so parse permissively and ignore what you do not recognise.
The current version is `1`.

When `--json` is run interactively — stdout is an actual terminal, not a pipe or a
redirect — the document is also copied to the clipboard, and a dimmed
`(also copied to your clipboard)` note goes to stderr. Piped output, the way scripts and
agents consume `--json`, is untouched: stdout carries the document and nothing else, no
clipboard is involved, and nothing extra is printed. The copy uses the platform tool
(`clip` on Windows, `pbcopy` on macOS, `wl-copy`/`xclip`/`xsel` on Linux) and is skipped
silently when none is available.

### `devp run --json`

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "run",
  "dry_run": true,
  "results": [
    {
      "repository": "~/Code/api",
      "adapter": "pnpm",
      "directory": "node_modules",
      "status": "skipped_dry_run",
      "bytes": 481296384,
      "shared_bytes": 0   // bytes hardlinked into a pnpm/bun store — excluded from `bytes`
      // "message"     — present on the statuses that carry detail: `lockfile_error`,
      //                 `activity_check_error`, `delete_error`, `config_error`,
      //                 `skipped_symlink`
      // "fix_command" — present only on `lockfile_error`, and only when the fix is a
      //                 single mechanical command an agent can run unattended
    }
  ],
  "summary": {
    "bytes_freed": 0,          // only actual deletions; a dry run always reports 0
    "bytes_reclaimable": 481296384,
    "directories_pruned": 0,
    "errors": 0                // lockfile_error + activity_check_error + delete_error + config_error
  }
}
```

| `status` | Meaning |
| :--- | :--- |
| `pruned` | The directory was deleted. |
| `skipped_dry_run` | A candidate, left in place because this was a dry run. |
| `skipped_active` | The repository has been touched inside its idle window. |
| `skipped_symlink` | The directory is (or contains) a symlink, so deleting it could reach outside the project. Left in place; `message` names the link. |
| `ignored` | The repository sets `"ignore": true`, or has an `ignore.devprune.json`. |
| `no_bloat` | Nothing to delete. |
| `disabled` | The registry entry is disabled. |
| `path_missing` | The registered repository path no longer exists on disk. If the repository was moved rather than deleted, `devp link` at its new location adopts this entry and the row goes away; `devp unlink --missing` clears the ones that are genuinely gone. |
| `lockfile_error` | The lockfile could not be verified, so nothing was deleted. |
| `activity_check_error` | The idle check itself failed, so idleness could not be proven and nothing was deleted. Counts as an error. |
| `delete_error` | Deletion was attempted and failed. |
| `config_error` | `.devprune.json` does not parse; the repository was not touched. |

### `devp status --json`

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "status",
  "config_path": "~/.config/dev-prune/registry.json",
  "integrations": { "daemon": "...", "git_hooks": "..." },
  "settings": { "idle_days": 15, "min_size_mb": 0, "update_check": true /* ... */ },
  "top": 10,  // present only when --top was passed; `repositories` is trimmed, `totals` is not
  "totals": {
    "repositories": 12,
    "candidates": 3,
    "reclaimable_bytes": 4812963840,
    "historical_bytes_freed": 0,
    "prune_passes": 0  // passes that deleted something — not repositories, not directories
  },
  "repositories": [
    {
      "path": "~/Code/api",
      "state": "candidate",
      "enabled": true,
      "idle_days": 15,
      "last_activity": "2026-05-02T09:14:00+00:00",  // null if none could be determined
      "last_pruned_at": null,
      "added_at": "2026-04-11T18:22:31+00:00",
      "adapters": ["pnpm"],
      "reclaimable_bytes": 481296384,
      "directories": [{ "name": "node_modules", "path": "~/Code/api/node_modules", "bytes": 481296384, "shared_bytes": 0 }]
      // "error" — present only when `state` is `config_error`, carrying the parse failure
    }
  ]
}
```

`state` is one of `candidate`, `active`, `ignored`, `no_bloat`, `path_missing`, or
`config_error`. `last_activity` is the later of the last commit and the newest source file
mtime — the same value the idle decision uses, so the two can never disagree.

### `devp status --drift --json`

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "status --drift",
  "drift": [
    {
      "repository": "~/Code/api",
      "project": ".",              // project path relative to the repository root
      "adapter": "venv",
      "directory": ".venv",
      "unrecorded": ["sneaky-pkg"],
      "record_command": "pip freeze > requirements.txt"
    }
  ],
  "summary": {
    "projects_with_drift": 1,
    "unrecorded_packages": 1
  }
}
```

`drift` is sorted by repository, project, adapter and directory, so the same machine
produces the same document twice. An empty `drift` array with both summary counts at `0`
is the healthy state. Exit code is `0` either way — drift is a report, not a failure.

### `devp stats --json`

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "stats",
  "history_starts_at": "1.1.0",  // the version that began recording the two sections below
  "lifetime": {
    "bytes_freed": 6772391936,
    "prune_passes": 9,           // same number and same meaning as totals.prune_passes above
    "repositories": 12
  },
  "last_prune": {                // null if nothing has ever been pruned
    "at": "2026-08-11T14:02:55+00:00",
    "bytes_freed": 812963840,
    "directories": 4
  },
  "recent_passes": [             // newest first, capped at 50 passes; empty before 1.1.0
    { "at": "2026-08-11T14:02:55+00:00", "bytes_freed": 812963840, "directories": 4, "repositories": 2 }
  ],
  "repositories": [              // biggest first; bytes_freed is only recorded from 1.1.0
    { "path": "~/Code/api", "bytes_freed": 481296384, "last_pruned_at": "2026-08-11T14:02:55+00:00" }
  ]
}
```

`lifetime.bytes_freed` and `lifetime.prune_passes` have been accumulating since 1.0.0.
`recent_passes` and the per-repository `bytes_freed` were not recorded before 1.1.0, so on
an upgraded machine they start from zero while the lifetime figures do not — which is what
`history_starts_at` is there to tell a consumer.

### `devp caches --json`

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "caches",
  "caches": [
    {
      "manager": "uv",
      "kind": "cache",
      "path": "~/.cache/uv",
      "bytes": 17448304640,
      "clear_command": "uv cache prune"
      // "note" — present only when clearing this cache costs more than time
    }
  ],
  "summary": {
    "total_bytes": 29268434944,
    "count": 9
  }
}
```

`caches` is ordered largest first, the same as the table. Only caches that exist on the
machine appear, so `count` is a count of what was found rather than of what was looked
for. `clear_command` is a suggestion printed for a human — dev-prune never runs it.

### `devp caches clear --json`

Requires `--yes` (or `--dry-run`): a prompt on a pipe is a hang, and the notice that
replaces it would land inside the document.

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "caches clear",
  "dry_run": false,
  "caches": [
    {
      "manager": "npm",
      "kind": "cache",
      "path": "~/.npm/_cacache",
      "bytes_before": 2147483648,
      "bytes_after": 0,
      "freed_bytes": 2147483648,
      "cleared": true
      // "error" — present only on a row that could not be cleared, with the reason
    }
  ],
  "summary": {
    "freed_bytes": 2147483648,
    "count": 1,
    "failed": 0
  }
}
```

`freed_bytes` is measured — the cache is sized again afterwards and the document reports
`bytes_before - bytes_after` — because `pnpm store prune` and `uv cache prune`
deliberately keep what is still referenced. With `--dry-run` the shape is the plan
instead: `"dry_run": true`, and each row carries `bytes` and `clear_command` rather than
a before/after pair. Exit code is `1` if `summary.failed` is non-zero.

### `devp trust --json`

```jsonc
{
  "schema": 1,
  "version": "1.5.1",
  "command": "trust",
  "guarantees": [
    {
      "key": "lockfile_verification",
      "subject": "Lockfile verification",
      "state": "Required before every delete",
      "verdict": "guaranteed"
    }
  ],
  "machine": [
    {
      "key": "auto_update",
      "subject": "Auto-update",
      "state": "Off — updates only when you run `devp update`",
      "verdict": "safe"
    }
  ],
  "summary": {
    "widened": [],
    "widened_count": 0
  }
}
```

`guarantees` and `machine` stay separate arrays because they are different kinds of
claim — one is structural, one is a reading off this machine — and flattening them would
let a consumer treat a setting as a promise. `key` is the stable identifier and `verdict`
is one of `guaranteed`, `safe`, `widened` or `neutral`; `state` and `subject` are prose
and may be reworded. `summary.widened` lists the subjects whose verdict is `widened`, so
`jq -e '.summary.widened_count == 0'` is a usable CI assertion.

Every document is emitted to stdout; diagnostics go to stderr, so `devp status --json |
jq` is always safe. Exit codes are unchanged by `--json`.

---

### 9. `devp restore [PATH] [--last-run]`
- **Description**: Detects applicable package manager lockfiles (`package-lock.json`, `pnpm-lock.yaml`, `uv.lock`, `Cargo.lock`, `go.sum`) and re-installs missing dependencies (`npm ci`, `pnpm install`, `uv sync`, etc.). Mirrors pruning: every project in the tree is restored, each by its own manager.
- **Flags**:
  - `--last-run` — restore exactly what the most recent prune pass deleted, across every repository it touched, and nothing else. Each prune records what it removed; a pass that deleted nothing leaves the previous record intact, and a `--dry-run` records nothing at all. Fails if no pass has been recorded yet. Cannot be combined with a `PATH` — silently ignoring the path would restore the wrong thing.
- **Python interpreters**: pruning a virtual environment records which Python built it, read from the environment's own `pyvenv.cfg`. `--last-run` rebuilds on that interpreter — `py -3.12` on Windows, `python3.12` elsewhere, `uv sync --python 3.12` under uv, `poetry env use` under poetry — because a 3.12 environment rebuilt on 3.14 resolves different wheels and fails later as an import error nobody traces back to a restore. If that interpreter is installed it is used and the restore says so. If it is not, the restore asks **once for the whole pass**, defaults to no, and prints the `uv python install 3.12` line that fixes it properly. Directories pruned before 1.4.0 recorded no interpreter and restore exactly as they always did.

- **Examples**:
  ```bash
  devp restore
  devp restore ~/Code/my-app
  devp restore --last-run       # undo the last prune pass
  ```

---

### 10. `devp update [--offline | --install]`
- **Description**: Prints the installed version, asks GitHub's public API for the latest release, and shows the upgrade command for how you installed it. By default it never downloads or replaces its own binary — upgrade with `cargo binstall dev-prune --force`, `cargo install dev-prune --force`, or by re-running the installer script.
- **Flags**:
  - `--offline` — skip the release check for this run without changing the setting.
  - `--install` — actually perform the upgrade. It downloads the release binary for this platform straight from GitHub Releases, verifies it against the SHA-256 sidecar published beside it, and writes it to every copy this installation runs: the managed binary under the config `bin` directory, its `devp` alias, the windowless `devpw` scheduler twin, and the running binary if that is a different file — unless that file lives in a directory WinGet, Scoop or Homebrew owns, which each replace wholesale on upgrade, so writing into one produces a copy the next upgrade throws away. **Nothing is installed if the checksum does not match.** The managed copy is the one whose failure aborts the upgrade — it is what the git hooks and the scheduler invoke — and a copy in a directory this process cannot write is reported rather than fatal.

    The package manager that delivered the first copy is deliberately *not* run. Its record of the installed version therefore goes stale, and the one command that resyncs it (`cargo install dev-prune --force`, `npm install -g dev-prune@latest`, `uv tool upgrade dev-prune`, `pipx upgrade dev-prune`, `pip install --upgrade dev-prune`, `winget upgrade --id VKrishna04.dev-prune`, `scoop update dev-prune`, `brew upgrade dev-prune`) is printed after a successful install. Run it or don't — the binaries are already current either way.

    Falls back to that channel's own upgrade command when the release publishes no binary for this platform or the download fails, so a release-page outage costs the fast path and not the upgrade. The channel is detected from where the binary lives, by the one classifier `devp doctor` and `devp uninstall` also read: the managed `bin` directory means the installer script, `.cargo` means cargo (`cargo binstall` when available, `cargo install` otherwise), a `node_modules` tree means npm, uv's tool directory means `uv tool upgrade`, a `pipx` venv means `pipx upgrade`, a `pip` script beside the binary means `pip install --upgrade`, and a WinGet, Scoop or Homebrew package directory means that manager's own upgrade; an unrecognised location prints every channel's command instead. It refuses under `DEV_PRUNE_OFFLINE`, cannot be combined with `--offline`, and does nothing when the installed build is already the latest.
- **`auto_update`** (`false` by default): `devp config set auto_update true` runs `--install` by itself at the end of a prune pass when the release check already knows a newer version exists. A failed upgrade warns and never fails the pass. **An upgrade never interrupts the scheduled pass**: the scheduler runs the managed copy under the config `bin` directory, package managers replace binaries by atomic rename (a running pass keeps its loaded image), and the managed copy and hidden `devpw` twin refresh themselves from the new binary on their next healthy run.
- **The check is opt-out, not opt-in.** It also runs quietly from `devp run` and `devp status`, at most once every 7 days, and prints one line only when a newer version exists. Disable it permanently with `devp config set update_check false`. It sends no body, no identifier and no usage data — see [PRIVACY.md](PRIVACY.md).
- **`DEV_PRUNE_OFFLINE=1`** keeps the process off the network entirely — the release check and the extension-download fallback alike — regardless of any setting. For air-gapped machines and test suites; the durable per-user switch remains `devp config set update_check false`.
- **Examples**:
  ```bash
  devp update
  devp update --install
  devp update --offline
  ```

---

### 11. `devp setup [--status]`
- **Description**: Installs whatever dev-prune integration is missing and leaves the rest alone: the `devp` alias, the managed binary directory on your `PATH` (a user `PATH` entry on Windows, `~/.local/bin` symlinks elsewhere — this is what keeps `devp` working after the venv or npx cache it was installed from disappears), the exported `SKILL.md`, the skill installed into any detected AI agent skills directory (`~/.claude/skills/dev-prune/`), the file-manager icon registration for `*.devprune.json`, the global Git auto-registration hooks, and the OS scheduler. Safe to run repeatedly — it is the same pass the install scripts run, the same one `devp init` runs, and the same one that runs by itself on the first command after an upgrade.
- **`--status`**: Report what is installed, what is not, and which automation settings are in force. Changes nothing.
- **Skips, rather than forces**:
  - Git hooks, when `git` is not on `PATH` (with instructions to install it), or when `core.hooksPath` already belongs to husky, pre-commit or lefthook and `auto_hooks_chain` is off. Take the slot without displacing that tool: `devp hook install --chain`.
  - The alias, when the running process *is* `devp` and the file cannot be replaced under it (Windows).
  - Anything switched off by `auto_setup`, `auto_hooks`, `auto_daemon`, or `DEV_PRUNE_NO_AUTO_SETUP=1`.
- **Editor extension**: when a VS Code-family CLI is on `PATH` — `code`, `code-insiders`, `codium`, `codium-insiders`, `cursor`, `windsurf`, `positron` or `kiro` — and the [dev-prune extension](IDE_INTEGRATION.md) is not installed, one run asks — once ever, only at an interactive terminal, never in CI or containers — whether to install it into each editor found. Each editor installs from its own registry (Marketplace for VS Code, OpenVSX for most forks); if a fork's registry does not carry the extension, the `.vsix` attached to the latest GitHub release is installed instead. Decline and it never asks again; `code --install-extension VKrishna04.dev-prune` installs it by hand later.
- **Examples**:
  ```bash
  devp setup
  devp setup --status
  DEV_PRUNE_NO_AUTO_SETUP=1 devp init ~/code   # register repositories, install nothing
  ```

See [Background Automation](BACKGROUND_AUTOMATION.md) for the full decision flow.

---

### 12. `devp doctor [PATH] [--fix]`
- **Description**: One read-only pass that answers "why is this not doing what I expect". Without a path it checks the installation; with one it checks that repository and ends by naming the single reason a prune pass would or would not touch it. Plain `doctor` changes nothing — no config is created, no integration installed, no package manager run — and when some of what it found is repairable, its verdict says how many findings `--fix` would mend.
- **Without a path** it reports: version, executable location, whether `devp` sits beside it and whether that directory is on `PATH`; **which package manager installed this copy, and the exact commands that upgrade and remove it through that manager** — the installers, cargo, npm, uv, pipx, pip, WinGet, Scoop or Homebrew, never a warning, because an unrecognised location is a valid way to run a binary; **any other copy of dev-prune on the machine that runs a different version** — `PATH` plus every fixed directory those managers install into, searched even when they are not on `PATH`, because a copy nobody can see is a copy nobody upgrades and the one that runs the day `PATH` changes (nothing is deleted: the manager that installed a copy is the only thing that should remove it); the config directory and whether `registry.json` parses; every stored setting revalidated against the range its own `config set` enforces; `SKILL.md`, file icons, Git hook state, the scheduler and the three `auto_*` settings; the package-manager binaries the registered repositories actually need; the registry's own health (missing paths, unreadable per-repo configs, reclaimable totals); and the release-check state.
- **With a path** (`devp doctor .`) it reports: whether it is a Git repository, whether it is registered, any opt-out in force, whether `.devprune.json` parses and what it overrides, the effective idle threshold against real activity, the effective size floor and scan depth, then every discovered project with its manager, whether the file that gates it is present, and each bloat directory's size and status.
- **`--fix`** — diagnosis first, then treatment: run the same checks, then repair what they found. It mends *installed-but-broken* only — a stale or missing `devp` twin, a missing `SKILL.md` export, Git hooks or a scheduler entry whose recorded binary no longer exists, a chained hook set that has drifted from the tool it forwards to, and registry entries whose repository is gone (the same cleanup as `devp unlink --missing`). Each repair is the corresponding `devp setup` pass re-run, so a repair can never do more than setup itself would; each re-checks state first, so a finding that healed in the meantime reports "already in place". It never performs a first-time install (that is `devp setup`'s job, gated by your `auto_*` settings), never touches an unreadable `registry.json` (a parse failure is for you to look at, not for a tool to guess at), and with `DEV_PRUNE_NO_AUTO_SETUP=1` set it skips every repair that writes outside the config directory, naming the command to run yourself. Problems `--fix` cannot mend are re-listed as such. Cannot be combined with a `PATH` — repository findings (not a Git repo, opted out, idle) are facts about your project, not breakage to mend.
- **Exit codes**: `0` when everything works, warnings included — a missing scheduler should not fail a script. `1` only for something actually broken: an unreadable `registry.json`, an out-of-range setting, a registered path that no longer exists, a directory that is not a Git repository. `--fix` exits `0` when everything it found was repaired, `1` when any repair failed, was skipped, or was out of its reach.
- **Examples**:
  ```bash
  devp doctor              # the installation
  devp doctor .            # this repository
  devp doctor ~/Code/api
  devp doctor --fix        # repair what the checks found
  ```

---

### 13. `devp skill [--agent <EDITOR>]`
- **Description**: Exports [`SKILL.md`](../.agents/skills/dev-prune/SKILL.md) into the config directory, installs it into any detected on-disk agent skills directory (`~/.claude/skills/dev-prune/` — the same install `devp setup` performs automatically), and displays ready-to-copy AI Agent onboarding prompts for assistants without a skills directory (Gemini Antigravity, Cursor, Windsurf, Copilot, OpenClaw).
- **`--agent <EDITOR>`**: instead writes per-repository rules into the current repository (it must be a Git repository root), in the file that editor's agent actually reads:

  | Editor | File written |
  |---|---|
  | `cursor` | `.cursor/rules/dev-prune.mdc` (with Cursor's rule frontmatter) |
  | `windsurf` | `.windsurf/rules/dev-prune.md` |
  | `antigravity` | `.agent/rules/dev-prune.md` (Gemini Antigravity) |
  | `cline` | `.clinerules/dev-prune.md` |
  | `roo` | `.roo/rules/dev-prune.md` (Roo Code) |
  | `kilocode` | `.kilocode/rules/dev-prune.md` (Kilo Code) |
  | `continue` | `.continue/rules/dev-prune.md` (Continue) |
  | `amazon-q` | `.amazonq/rules/dev-prune.md` (Amazon Q Developer) |
  | `kiro` | `.kiro/steering/dev-prune.md` (Kiro) |
  | `trae` | `.trae/rules/dev-prune.md` (Trae) |
  | `junie` | `.junie/guidelines.md` — as a marked block (JetBrains Junie) |
  | `gemini` | `GEMINI.md` — as a marked block (Gemini CLI) |
  | `zed` | `.rules` — as a marked block (Zed reads it ahead of every other convention) |
  | `copilot` | `.github/copilot-instructions.md` — as a marked block |
  | `agents-md` | `AGENTS.md` — as a marked block; the cross-tool convention Codex, Jules, Amp, OpenCode and others read |

  The five shared files (`agents-md`, `copilot`, `gemini`, `junie`, `zed`) are edited inside dev-prune's `<!-- dev-prune:rules:start -->`…`<!-- dev-prune:rules:end -->` markers only: a re-run replaces the block, and every byte outside the markers is left exactly as found. The rules file is inert data and safe to commit if the whole team's agents should have it. Claude Code is deliberately not in the list — its skill installs globally, so there is nothing to write per repository.
- **Examples**:
  ```bash
  devp skill
  devp skill --agent cursor
  devp skill --agent agents-md
  ```

---

### 14. `devp uninstall [--deep]`
- **Description**: Removes dev-prune from the machine: the OS daemon scheduler, the global `core.hooksPath` (only if it still points at dev-prune), the installed agent skill, the `PATH` entry (or `~/.local/bin` symlinks), and the binaries themselves — both the managed pair and the copy you invoked. On Windows, where a running executable cannot delete itself, a detached helper removes the last files a few seconds after the command exits; nothing needs a reboot or a closed terminal. Without `--deep` the configuration survives, so a reinstall picks up where you left off. With `--deep`, also wipes the global configuration folder (`~/.config/dev-prune/`) and every registered repository's `.devprune.json`; `--deep` asks for confirmation, and refuses outright with no terminal to ask on unless `-y` is passed. Exits `1` if anything could not be removed, naming each leftover. With `DEV_PRUNE_NO_AUTO_SETUP=1` set, the uninstall is hands-off about the integrations that variable told setup never to install: the scheduler and agent skills are left alone (with a note), and the sweep searches only `PATH` instead of also guessing install directories from the home folder.
- **The stray-copy sweep**: installing from pip, npm, cargo and uv over time leaves copies of `devp` in `~/.cargo/bin`, `~/.local/bin`, npm's global directory and one `Scripts` folder per virtualenv — some on PATH, some not, and any one of them keeps the command resolving after an "uninstall". Both modes therefore end by scanning every directory on your PATH plus the well-known install locations for other copies of `dev-prune`/`devp` (matched by name only; nothing else in those directories is ever touched, and dev builds under a `target/` folder are skipped). What it finds is listed — each copy annotated with the package manager that owns it — and removed after **one confirmation**: `[y/N]` interactively — a bare Enter declines, `--yes` auto-confirms, and with no terminal and no `--yes` the copies are left in place with a note, without failing the uninstall. For each manager-owned copy the manager's own line (`pip uninstall dev-prune`, `npm uninstall -g dev-prune`, `cargo uninstall dev-prune`, `uv tool uninstall dev-prune`, `pipx uninstall dev-prune`) is printed at the end so its records get cleared too.
- **Examples**:
  ```bash
  devp uninstall
  devp uninstall --deep
  devp uninstall --deep -y     # non-interactive; also confirms the stray-copy sweep
  ```

---

### 15. `devp stats [--json]`
- **Description**: What dev-prune has actually done for you, as opposed to what it could do next. Lifetime space reclaimed, how many prune passes there have been, the most recent pass and how to undo it, the last ten passes, and the ten repositories that have given back the most. Read-only — it touches the registry and nothing else.
- **Why it is separate from `status`**: [`devp status`](#6-devp-status---top-n---drift---json) answers "what can I reclaim right now". Folding a history report into it would put a screen of the past above the list people open it for.
- **A note on upgraded machines**: the lifetime total has been accumulating since 1.0.0, but the per-repository figures and the pass history are only recorded from **1.1.0** onward. A machine that pruned for months before upgrading will show a large lifetime total next to an empty "Biggest reclaims" section, and the report says so rather than implying nothing ever happened.
- **Flags**:
  - `--json` — emit one machine-readable document instead of the report.
- **Examples**:
  ```bash
  devp stats
  devp stats --json | jq '.lifetime.bytes_freed'
  ```

---

### 16. `devp completions <SHELL>`
- **Description**: Prints a shell completion script to stdout. `<SHELL>` is one of `bash`, `zsh`, `fish`, `powershell` or `elvish`. The script is generated from the same argument definition the binary parses with, so a flag cannot exist in one and be missing from the other.
- **Output**: the script and nothing else — no banner, no header, no credit line. The output is meant to be redirected to a file or `eval`'d, and anything extra in it is a shell error on every new terminal.
- **Which name gets completed**: whichever one you invoked. `devp completions bash` completes `devp`; `dev-prune completions bash` completes `dev-prune`. They are the same executable, but a completion script is registered against a command *name*, so generate one for each name you actually type.
- **Examples**:

  ```bash
  # Bash — current shell, then permanently
  source <(devp completions bash)
  devp completions bash > ~/.local/share/bash-completion/completions/devp

  # Zsh (a directory already on $fpath)
  devp completions zsh > ~/.zfunc/_devp

  # Fish
  devp completions fish > ~/.config/fish/completions/devp.fish
  ```

  ```powershell
  # PowerShell — append to your profile so it loads in every session
  devp completions powershell | Out-File -Append -Encoding utf8 $PROFILE
  ```

---

### 17. `devp man [--dir <DIR>]`
- **Description**: Renders the manual as man pages, generated from the same clap definitions `--help` prints — so the manual cannot describe a flag the program does not have. With no arguments the main `devp(1)` page goes to stdout, ready to pipe into `man -l -`. With `--dir` the full set is written into a directory: `devp.1`, `dev-prune.1`, and one `devp-<command>.1` per subcommand, ready to copy onto `manpath`.
- **Examples**:
  ```bash
  devp man | man -l -             # read the main page now (Linux/macOS)
  devp man --dir ./man            # write the full set into ./man
  sudo cp man/*.1 /usr/local/share/man/man1/
  ```

---

### 18. `devp trust [--json]`
- **Description**: What dev-prune is allowed to do on this machine, on one screen. Read-only — it reads the registry and the OS and changes nothing.
- **Two sections, and the split is the point**:
  - **Guaranteed by the code** — the seven [safety invariants](SAFETY_INVARIANTS.md) plus the two questions asked as often as any of them: there is no telemetry endpoint, and build output is never deleted. These rows read the same on every machine. None of them has a setting or a flag behind it, and a build where one did not hold would be a bug, not a configuration.
  - **On this machine** — read live: whether the scheduler is installed, whether the Git hooks register repositories on their own, how many repositories are registered, the idle window, the managed binary's path, and the settings that widen what may happen without you asking.
- **The four settings that widen anything** are named individually rather than summed into a grade: `auto_update`, `require_confirmation` set to `false`, `allow_manifest_rewrite`, and any [opt-in adapter](#5-devp-run-target_path) (`enable_cargo`, `enable_gradle`, `enable_maven`, `enable_swift`, `enable_dart`) — the only ones that make a *build tree* deletable. Each was switched on deliberately; [`devp config show`](#8-devp-config-action) has all of them and `devp config set <key> <value>` puts one back.
- **No letter grade, on purpose**: `trust level: MEDIUM` tells nobody which switch to look at. The report lists the switches.
- **Row marks**: `+` guaranteed or safe, `!` widened, blank for a neutral fact (a path, a count).
- **Flags**:
  - `--json` — emit one machine-readable document instead of the table.
- **Exit code**: always `0`. A widened setting is a choice, not a failure — use [`devp doctor`](#12-devp-doctor-path---fix) for something that exits non-zero on breakage.
- **Examples**:
  ```bash
  devp trust
  devp trust --json | jq '.summary.widened'
  devp trust --json | jq -e '.summary.widened_count == 0'   # non-zero if anything is widened
  ```

---

## 🔍 Rich Environment Audit (`devp -V` / `devp --version`)

Executing `devp -V` prints detailed diagnostic information:
```
 ___    _____ __     __    ____  ____  _   _ _   _ _____ 
|  _ \ | ____|\ \   / /   |  _ \|  _ \| | | | \ | | ____|
| | | ||  _|   \ \ / /    | |_) | |_) | | | |  \| |  _|  
| |_| || |___   \ V /     |  __/|  _ <| |_| | |\  | |___ 
|____/ |_____|   \_/      |_|   |_| \_\\___/|_| \_|_____| v1.3.0

dev-prune (devp) v1.3.0
  Binary Aliases:  dev-prune | devp
  Author:          VKrishna04
  Repository:      https://github.com/Life-Experimentalist/dev-prune
  Homepage:        https://devprune.vkrishna04.me
  Target OS:       windows
  Architecture:    x86_64
  Compiler:        Rust 1.88+ (edition 2024)
  License:         Apache-2.0

  Config Path:     C:\Users\username\AppData\Roaming\dev-prune\registry.json
  Binary Dir:      C:\Users\username\AppData\Roaming\dev-prune\bin
  PATH Audit:      ✓ Executable directory is active in system PATH.
```

The author and repository are printed so that a stray copy of the binary — downloaded
once, moved to a server, forgotten — can still say where it came from. Both are plain
constants in [`src/constants.rs`](../src/constants.rs); nothing in the code checks them.
