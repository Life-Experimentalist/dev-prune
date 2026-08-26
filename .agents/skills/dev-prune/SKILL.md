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

**Skill version: 1.10.0.** This file describes that release of `devp` and no other, and
it is rewritten from the binary rather than maintained by hand. Before you rely on
anything below, run `devp --version`. If it prints a different number, you are reading
another release's instructions — its flags, JSON statuses and exit codes may not be
the ones installed, and a command you quote from here may not exist. The fix is one
command: `devp skill` rewrites every copy of this file from the installed binary,
including the one in your own skills directory. Re-read it afterwards, and use the file,
not what you remember of it. `devp doctor` reports a stale copy and `devp doctor --fix`
replaces it.

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
5. **dev-prune never upgrades itself unasked.** Its one background network request is a
   release check against GitHub's public API (no body, no identifier, no usage data, at
   most weekly, disabled by `devp config set update_check false`; `DEV_PRUNE_OFFLINE=1`
   keeps the whole process off the network regardless of any setting). Nothing is
   downloaded until someone asks: `devp update --install` fetches the release binary for
   the platform, refuses it unless its SHA-256 matches the sidecar published beside it,
   and writes it to every copy this installation runs. `auto_update` — on by default,
   `devp config set auto_update false` to stop it — does that same verified download at
   the end of a prune pass, and never runs a package manager unattended. `version_lock`,
   off by default, outranks all of it: with `devp config set version_lock true` nothing
   dev-prune does replaces the binary, including a re-run of the install one-liner, and
   there is no flag that bypasses it. The alternative is the install
   channel's own command — re-run the `install.sh`/`install.ps1` one-liner,
   `npm install -g dev-prune@latest`, `bun add -g dev-prune@latest`, `pnpm add -g
   dev-prune@latest`, `yarn global upgrade dev-prune`,
   `uv tool upgrade dev-prune`, `pipx upgrade dev-prune`, `pip install --upgrade
   dev-prune`, `winget upgrade --id VKrishna04.dev-prune`, `scoop update dev-prune`,
   re-running the `brew install <formula URL>` line (a formula installed by URL has no
   tap for `brew upgrade` to consult), `cargo binstall dev-prune --force` (prebuilt), or
   `cargo install dev-prune --force` (compiles). `devp update --channels` prints that
   whole table offline, for when the stale copy is on a machine you are not sitting at.

## What it will never do

Useful when the user asks "is this safe?" — these are enforced in code, not conventions:

- Operates only inside a directory containing a `.git` root.
- Never deletes a bloat directory whose lockfile is missing, unparseable, or (for
  `requirements.txt`) lists no packages.
- Never follows a symlinked or junctioned bloat directory — that storage belongs to
  something else.
- Never crosses into a nested repository; a submodule is pruned as itself or not at all.
- Never executes anything named in a repository-tracked file. `.devprune.json` and
  `project.devprune.json` hold inert data only, and deserialize into the same seven keys —
  so a cloned repository cannot grant itself `allow_manifest_rewrite` or name a
  post-prune command however it is configured.
- Never touches a package-manager cache on its own. No prune pass, scheduler or Git hook
  clears one, ever. `devp caches` reports their sizes and prints the clear command, and
  `devp caches clear <manager>` runs it — but only when the user asks for it, and it asks
  before it acts. Do not reach for `clear` to free space: a cache is shared by every
  project on the machine, so no one lockfile can prove it is recoverable, and it is what
  makes `devp restore` fast. Suggest it only when the user has said they want the disk
  space more than the speed. The one exception worth offering unprompted is
  `--unused`: a manager no registered repository uses has nothing behind it, so
  emptying it costs no re-download for anything still on the disk.
- Never deletes container disk. `devp caches docker`, `caches podman` and `caches
  containers` report what an engine is holding and print the prune commands; there is no
  flag, and no `clear` verb, that makes dev-prune run one. An image has no lockfile to
  prove it can be rebuilt and a named volume cannot be rebuilt at all. `devp caches clear
  docker` is a usage error that says so. **Do not run a `docker system prune` yourself on
  the user's behalf either** — `--volumes` deletes databases, and the whole design of this
  report is that the person at the keyboard decides. Show them the command.
- Never empties `~/.m2/repository`. Maven's local repository is an install target as well
  as a download cache — `mvn install:install-file` puts artifacts there that exist in no
  remote at all — so `devp caches` sizes it and prints `rm -rf ~/.m2/repository` and
  stops. `devp caches clear maven` is a usage error that says why; `clear all` skips it.
  If the user wants that space, hand them the command; do not run it for them.
- Non-ASCII paths are not a special case. A repository at
  `ワークスペース/项目目录名称测试/프론트엔드` scans, verifies, prunes and restores like any
  other, on Windows, macOS and Linux. Terminal tables pad by display column, not by
  character, so full-width names stay aligned. Program messages are English only.

---

## 🗺️ Command map

| The user wants | Run |
| :--- | :--- |
| "how much space can I get back?" | `devp run --dry-run` |
| "show me my repos" | `devp status` (interactive; prints a plain table when not a TTY). In the dashboard: `s` sorts, `f` filters, `/` searches — display only, the totals always cover every registered repository |
| "just the worst offenders" / "top 10 biggest" | `devp status --top 10` — trims the list, never the totals |
| "how much has this saved me?" / "what did it clean last week?" | `devp stats` — lifetime total from pruning, a separate lifetime total from `devp caches clear`, prune passes, the last pass, and the repositories that gave back the most |
| "add tab completion" | `devp completions <bash\|zsh\|fish\|powershell\|elvish>` — prints the script to stdout; the user redirects it |
| "clean up" / "free space" | `devp run --dry-run`, then `devp run -y` |
| "clean this project" | `devp run . -y` |
| "clean it even though I'm working on it" | `devp run . --ignore-idle -y` — **ask first** |
| "clean everything but the API project" | `devp run --except api -y` — never verified, never deleted, never reinstalled |
| "put the dependencies back" | `devp restore .` |
| "undo that prune" / "I need it all back" | `devp restore --last-run` — reinstalls exactly what the last pass deleted, in every repository it touched, rebuilding each virtual environment on the Python version it was originally built with |
| "where is my disk actually going?" / "how big is my npm cache?" | `devp caches` — sizes every package-manager cache and store (npm, pnpm, yarn, bun, uv, pip, conda, cargo, go, maven, gradle, nuget, vcpkg, conan, composer, cocoapods, hex) and prints the command that clears each. Read-only |
| "my uv cache is over 10 GB" / "tell me when a cache gets too big" | `devp config set cache_max_gb uv=10,npm=10` — a ceiling in GiB, per cache manager (the names `devp caches clear` takes, not adapter names). A manager past its cap is marked in `devp caches`; **the cap itself never deletes anything**. Off by default; 10 is the figure the wizard offers. `-` clears the map |
| "clear the caches that got too big" | `devp caches clear --over-cap all` — empties only the managers past their `cache_max_gb`, leaves the rest alone. Still asks first, still costs the same re-download. With no caps set it clears nothing and says so |
| "which caches do I still need" / "clear the ones nothing uses" | `devp caches` says how many registered repositories use each manager and what its cache works out to per repository. A manager no registered repository uses is the one case a count is enough to act on: `devp caches clear --unused all` empties exactly those, and costs nothing to re-download for anything still on the disk. Shown only for the twelve managers that are also adapter names — `pip`, `conda`, `nuget`, `conan` and `hex` have no adapter of the same name, so dev-prune says nothing rather than guessing. Refuses when no registered repository is on disk, because every cache would look unused |
| "my pnpm store looks tiny" / "my projects live on another drive" | `devp caches` looks for a pnpm store at the root of every filesystem that holds a registered repository, not just the one beside the home directory. pnpm hardlinks into `node_modules` and a hardlink cannot cross a filesystem, so projects off the system disk get a store of their own — `V:\.pnpm-store`, `/mnt/data/.pnpm-store`, `/Volumes/Work/.pnpm-store`. Not a Windows thing. Each is its own row and its clear command names it: `pnpm store prune --store-dir <path>` |
| "how much is Docker using?" / "what is taking my disk, I have containers" | `devp caches docker` (or `caches podman`, or `caches containers` for every engine plus local Kubernetes clusters) — images, containers, volumes and build cache, each sized, with how much the engine says is reclaimable. **Read-only, permanently.** It prints the prune commands; you never run them. Figures come from the engine's own `system df`, not a directory walk — on Docker Desktop and Podman the store is inside a VM disk the host cannot see. An engine that is installed with its daemon stopped is reported as that, with no figures: a blank, not a zero. `devp caches` shows a one-line summary per engine, outside its own total |
| "empty my npm cache" / "clear all the caches" | `devp caches clear npm` (or `all`) — **ask first**, and say what it costs: every project on the machine re-downloads on its next install, and `devp restore` stops being fast. `--dry-run` shows what would go. Never run this unprompted. `maven` is refused: print `rm -rf ~/.m2/repository` and let the user decide |
| "what is dev-prune allowed to do on my machine?" / "is this safe?" | `devp trust` — the guarantees the code enforces, then this machine read live: scheduler, hooks, and the three settings that widen anything (`require_confirmation=false`, `allow_manifest_rewrite`, opt-in adapters — `enable_cargo`, `enable_gradle`, `enable_maven`, `enable_swift`, `enable_dart`, `enable_mix_build`, `enable_vcpkg`, `enable_cmake_build`). Read-only |
| "git says dubious ownership" / "devp run says it cannot examine my repos" | `devp trust --fix-ownership` — adds every registered repository Git refuses to read to the global `safe.directory` list, after showing the list and asking; `--yes` for a script. Repositories Git will not read have no known age and are never pruned, which is why `devp run` reports them |
| "did I install anything my lockfiles don't know about?" | `devp status --drift` — every environment holding packages its lockfile never recorded, with the one command that records them. A pure read; this is what a prune would refuse on |
| "why isn't it cleaning this?" | `devp doctor .` — ends by naming the one reason a pass would or would not touch it. Across every repository at once: `devp run --explain` — read-only, every verdict listed including the quiet ones (still active with the actual age, opted out, under the size floor). Conflicts with `--json`; the `--json --dry-run` document already carries every status |
| "is anything wrong with my install?" | `devp doctor` |
| "fix whatever's broken" | `devp doctor --fix` — repairs installed-but-broken integrations (stale twin, dead-target hooks/scheduler, drifted chain, missing SKILL.md, dead registry entries); never a first-time install |
| "track my projects folder" | `devp init ~/Code` |
| "track this repo" | `devp link .` |
| "stop tracking this" | `devp unlink .` |
| "the registry is full of folders I deleted" | `devp unlink --missing` — clears every entry whose directory is gone |
| "I moved a repo and it lost its history" | `devp link <new path>` — matching root commits let it adopt the old entry |
| "undo that" | `devp undo` (reverts the last `init` or `link`) |
| "never touch this repo" | create `ignore.devprune.json` in its root, or press `i` in `devp status` |
| "what's my config?" | `devp config show` |
| "change a setting" | `devp config set idle_days 30` |
| "is the background stuff working?" | `devp setup --status` |
| "set it all up" | `devp setup` |
| "turn the automation off" | `devp config set auto_setup false` |
| "remove it" | `devp uninstall` — removes the program itself, PATH entry and agent skill included, then sweeps PATH and the well-known install dirs (`~/.cargo/bin`, `~/.local/bin`, npm global, venv Scripts) for every other copy and removes them after one confirmation. Non-interactively the sweep needs `-y` or it skips those copies with a note. Each manager-owned copy gets its manager's uninstall line printed (add `--deep` to wipe config — confirm first) |
| "is there a newer version?" | `devp update` — prints the installed and latest versions plus the right upgrade command; never installs anything by itself |
| "upgrade it" | `devp update --install` — downloads the release binary from GitHub, verifies its SHA-256, and replaces every copy this install runs (managed binary, `devp` alias, `devpw` scheduler twin, the running binary); the package manager that delivered the first copy is not run, and the one command that resyncs its version record is printed. Falls back to that manager's own upgrade command if there is no binary for this platform. `auto_update` does the download half by itself after a pass and is on by default; `devp config set auto_update false` stops it, and `devp config set version_lock true` stops every update path at once |
| "I installed it with cargo, I want it from winget instead" / "move it to uv" | `devp install --channel <name>` — installs through the manager named, then removes the old copy through the manager that owns it, in that order. `devp install` alone reports which channel owns this copy; `--dry-run` prints the plan and runs none of it. Names: `installer`, `cargo`, `npm`, `bun`, `pnpm`, `yarn`, `uv`, `pipx`, `winget`, `scoop`, `homebrew`. bun, pnpm and yarn install the same npm package but are each their own channel: a copy bun put there is upgraded and removed with bun, and running npm against it adds a second copy under npm's prefix while bun's stays stale on `PATH` |
| "I ran the one-liner but cargo/brew installed it first — which one runs?" | The one the one-liner installed. It works over any previous channel, needs no uninstall first, and never fails because of one; it puts its own directory *first* on PATH (prepended to the rc file on macOS/Linux, prepended to the User PATH on Windows) and names the copy it found rather than deleting another manager's file — then asks whether to collapse the two, running `devp install --channel installer --yes` from that older binary if you answer `y`. Anything else prints the command instead, and the question is skipped entirely with `DEV_PRUNE_NO_MIGRATE_PROMPT=1`, in CI, or with no terminal attached. The script deletes nothing either way. `devp doctor` lists every copy at any time, and reports the install receipt (`install.json`, written beside the binary by whichever script installed it: version, which script, when) |
| "clean my Rust `target/` too" / "clean my Java/Gradle/Maven builds too" / "clean my Flutter build caches too" / "clean my Elixir `_build/` too" / "clean my C++ `vcpkg_installed/` too" / "clean my CMake `build/` too" | `devp config set enable_cargo true` / `enable_gradle true` / `enable_maven true` / `enable_swift true` / `enable_dart true` / `enable_mix_build true` / `enable_vcpkg true` / `enable_cmake_build true` — opt-in adapters, idle-gated separately by `build_idle_days` (45). cmake_build claims only a tree holding a `CMakeCache.txt` that names a source directory inside the same repository, so a `build/` made by hand is never touched |
| "make npm wait a month" / "give one adapter its own idle window" | `devp config set adapter_idle_days npm=30,cargo=90` — a floor per adapter, applied as `max(idle_days, build_idle_days, this)`; `-` clears it. `devp config wizard` edits it beside the adapter checklist, where a language heading sets every adapter under it at once |
| "give my editor's agent the rules" | `devp skill --agent <editor>` — writes the per-repository rules file that editor reads. Own file: `cursor`, `windsurf`, `antigravity`, `cline`, `roo`, `kilocode`, `continue`, `amazon-q`, `kiro`, `trae`. Marked block inside a shared file: `agents-md` (`AGENTS.md` — Codex, Jules, Amp, OpenCode), `copilot`, `gemini`, `junie`, `zed`, `aider` (`CONVENTIONS.md`, which Aider reads only once `read: CONVENTIONS.md` is in `.aider.conf.yml` or `--read CONVENTIONS.md` is passed — the command says so after writing it). `devp skill --help` lists the exact paths |
| "man pages" / "show me the manual" / "what commands are there?" | `devp man` — the contents page, every command grouped by what it is for. `devp man <command>` for one page (the same text as `devp <command> --help`). Roff only where something formats it: a redirect or a pipe gets roff, `devp man \| man -l -` reads it formatted, `devp man --dir ./man` writes the full set |
| "what version?" | `devp -V` (also prints OS, arch, config path, PATH audit) |
| you need to *read* the answer rather than show it | add `--json` to `run`, `status`, `stats`, `trust` or `caches` — see below |

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

`devp run --json`, `devp status --json`, `devp stats --json`, `devp caches --json` and
`devp trust --json` each
emit **one** JSON document on stdout and nothing else; warnings go to stderr. `--json`
implies non-interactive, so it never blocks on a prompt. `status --json` and
`stats --json` change nothing at all, not even the registry file. (When a human runs
`--json` in a real terminal the document is also copied to their clipboard, with a
dimmed stderr note — piped output, the way you consume it, never triggers this.)

```bash
devp status --json          # what exists, what is reclaimable, are the integrations up
devp status --top 10 --json # the same, trimmed to the ten biggest — totals still cover all
devp stats --json           # what has already been reclaimed, and by which repositories
                            # .lifetime.cache_bytes_freed is `caches clear`, counted apart
devp run --dry-run --json   # what a pass would do, with exact byte counts
devp run -y --json          # do it
devp caches --json          # every package-manager cache, sized, largest first
devp caches containers --json  # what Docker/Podman/nerdctl hold; carries no prune command
devp trust --json           # what the tool guarantees, and what this machine has switched on
devp status --drift --json  # unrecorded packages per environment, with the record command
```

`stats` reports history, `status` reports opportunity — reach for `stats` when the user
asks what dev-prune *has* done and `status` when they ask what it *could* do. On a machine
upgraded from 1.0.0 the per-repository figures and the pass list in `stats` start empty
while the lifetime total does not; the document's `history_starts_at` field says so, so
report the gap rather than reading it as "nothing was ever pruned".

`devp status` sizes every dependency tree in every registered repository, so on a large
registry it takes several seconds even though it is a pure read. Prefer `--top N` when
the user only wants the worst offenders.

Every document carries `schema`, an integer that increases **only** when a consumer would
have to change: a field removed, renamed, or given a new meaning. Adding a field does not
bump it — parse permissively and ignore what you do not recognise. It is `1` today.

The fields worth reading first:

| Path | Use it for |
| :--- | :--- |
| `summary.errors` (run) | "did anything go wrong" — the whole answer, in one integer (counts `lockfile_error`, `activity_check_error`, `delete_error` and `config_error`) |
| `results[].status` | `pruned`, `skipped_dry_run`, `skipped_active`, `skipped_symlink`, `skipped_declaration`, `ignored`, `no_bloat`, `disabled`, `path_missing`, `lockfile_error`, `activity_check_error`, `delete_error`, `config_error` |
| `results[].message` | the failure detail — present on the four error statuses, on `skipped_symlink`, where it names the link, and on `skipped_declaration`, where it says why the declaration was refused |
| `results[].fix_command` | present only on `lockfile_error`, and only when the fix is one mechanical command you may run unattended |
| `repositories[].state` (status) | `candidate`, `active`, `ignored`, `no_bloat`, `path_missing`, `config_error` |
| `repositories[].error` | the parse failure — present only on `config_error` |
| `totals.reclaimable_bytes` | the number to quote back to the user |
| `totals.restore_estimate` / `repositories[].restore_estimate_secs` | how long putting it back would take, from restores timed on *this* machine — `null` until there is one. Never quote it as a whole answer unless `covered_bytes` equals `totals.reclaimable_bytes` |
| `results[].shared_bytes` / `directories[].shared_bytes` | bytes hardlinked into a pnpm/bun store and therefore excluded from `bytes` — if the user asks why the figure is smaller than the folder size, this is the answer |
| `summary.total_bytes` (caches) | every package-manager cache added up; `caches[].clear_command` is what the *user* runs, never you |
| `containers[]` (caches) / `engines[]` (caches containers) | what each container engine holds. **Never added to `summary.total_bytes`** — that figure answers "what could `devp caches clear` free", and none of this is that. An engine with `available: false` carries `reason` and *no* size keys: absent is not zero, so do not report 0 GB when dev-prune could not find out |

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
| poetry | `poetry.lock`, `[tool.poetry]` | `.venv` | `poetry check --lock`, plus no installed package missing from the lockfile | `poetry install` |
| pdm | `pdm.lock`, `[tool.pdm]` / `pdm.backend` | `.venv`, `__pypackages__` | `pdm lock --check` | `pdm install` |
| pipenv | `Pipfile` | `.venv` (in-project installs only) | `pipenv verify` | `pipenv install --deploy` |
| cargo *(opt-in)* | `Cargo.toml` | `target` | `cargo metadata --locked` | next `cargo build` |
| go | `go.mod` | `vendor` | `go mod download` | `go mod vendor` |
| composer | `composer.json` | `vendor` (skipped when it holds a `vendor/bundle`) | `composer validate --no-check-publish --no-check-all` | `composer install` |
| bundler | `Gemfile` | `vendor/bundle` (vendored installs only) | `bundle lock --check` | `bundle install` |
| cocoapods | `Podfile` | `Pods` | `Podfile.lock` has its `SPEC CHECKSUMS` section and is no older than the `Podfile` | `pod install` |
| mix | `mix.exs` | `deps` (never `_build`) | `mix.lock` is a complete Elixir map and no older than `mix.exs` | `mix deps.get` |
| terraform | any `*.tf` / `*.tf.json` | `.terraform/providers` (never `environment`, `terraform.tfstate` or `modules`) | `.terraform.lock.hcl` records at least one provider | `terraform init -backend=false` |
| gradle *(opt-in)* | `build.gradle[.kts]` / `settings.gradle[.kts]` | `build`, `.gradle` | manifest present and readable (rebuild-from-source proof) | next `./gradlew build` |
| maven *(opt-in)* | `pom.xml` | `target` | `pom.xml` present and looks like a Maven manifest | next `mvn package` |
| swift *(opt-in)* | `Package.swift` | `.build` (never `.swiftpm`) | `Package.swift` declares a `Package(` (rebuild-from-source proof) | next `swift build` |
| dart *(opt-in)* | `pubspec.yaml` | `.dart_tool` (never `build`) | `pubspec.lock` has a `packages:` section and is no older than `pubspec.yaml` | `dart pub get` / `flutter pub get` |
| mix_build *(opt-in)* | `mix.exs` | `_build` (never `deps`) | `mix.exs` and `mix.lock` both present | next `mix compile` |
| vcpkg *(opt-in)* | `vcpkg.json` | `vcpkg_installed` (never `build`) | `vcpkg.json` parses and declares a non-empty `dependencies` list | next `vcpkg install` |
| cmake_build *(opt-in)* | `CMakeLists.txt` | any tree holding a `CMakeCache.txt` (`build`, `cmake-build-debug`, `out/build/<preset>`) | the tree's own `CMakeCache.txt` records a `CMAKE_HOME_DIRECTORY` that still holds a `CMakeLists.txt` and sits inside this repository | next `cmake -S . -B <dir> && cmake --build <dir>` |

cargo, gradle, maven, swift, dart, mix_build, vcpkg and cmake_build ship **disabled**
— a build directory takes far longer to regenerate than a dependency directory (a full
recompile, not a download). Enable them with `devp config set enable_cargo true` /
`enable_gradle true` / `enable_maven true` / `enable_swift true` / `enable_dart true` /
`enable_mix_build true` / `enable_vcpkg true` / `enable_cmake_build true`; while disabled
they are invisible (not detected, not listed, `--only cargo` prunes nothing). Their candidates also wait for
`build_idle_days` (45 by default), applied as the **maximum** of it and the
repository's effective `idle_days` — the build-tool gate can only ever make pruning
later, never earlier.

Any single adapter can wait longer still: `adapter_idle_days` (`cargo=90,npm=30`) is a
per-adapter floor, applied as `max(idle_days, build_idle_days, adapter_idle_days[name])`
— it can raise one adapter's window and never lower it.

bundler and pipenv claim only the in-repository install — `vendor/bundle` after
`bundle config set path vendor/bundle`, and `.venv` under `PIPENV_VENV_IN_PROJECT`.
Their defaults are shared stores under the user's home, which dev-prune never touches —
not as a prune and not through `devp caches` either: those directories are where other
projects' dependencies are installed, not a cache. If the user asks why a Ruby or
Pipenv project reclaims nothing, that is the answer.

Any adapter, opt-in or not, can be switched off with `disabled_adapters` — that one is
a preference, not a safety gate.

Every verification command is **read-only**. A lockfile that has drifted from its
manifest is a refusal, not something to quietly fix — a prune pass can be started by the
scheduler, and it must never leave a modified tracked file behind. Each adapter's writing
form (`npm install --package-lock-only`, `uv lock`, `cargo generate-lockfile`,
`go mod tidy`, …) runs in exactly two cases: no lockfile exists at all, or the user set
`allow_manifest_rewrite true`.

## 📄 Configuration

**Global** (`devp config get|set|show`) — in `%APPDATA%\dev-prune` on Windows,
`~/Library/Application Support/dev-prune` on macOS, `$XDG_CONFIG_HOME/dev-prune` on Linux:

**The language this is all in**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `language` | `en` | Catalogue for dev-prune's own headings and summary lines: `en`, `zh`, `hi`, `te`, `ta`, `kn`, `ml`, `bn`, `mr`, `gu`, `pa`, `sa`. **Never translate a value you read out of `--json`, a key you pass to `config set`, a flag name or an adapter name** — those are English in every catalogue by design. `DEV_PRUNE_LANG` overrides it for one command; an untranslated key falls back to English |

**What gets pruned**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `idle_days` | `15` | Days untouched before a repository is a candidate |
| `min_size_mb` | `0` | Smallest bloat directory worth deleting, in MiB; `0` means no floor |
| `scan_depth` | `6` | How many directory levels below a repository root discovery descends; `config set` accepts `1`–`32` |
| `disabled_adapters` | *(none)* | Adapters to leave alone entirely, comma-separated by name. A disabled adapter is not detected, not counted, not pruned — as if that ecosystem were not installed. `-` clears the list |
| `adapter_idle_days` | *(none)* | Per-adapter idle windows, as `cargo=90,npm=30`. A floor, never a bypass: `max(idle_days, build_idle_days, this)`. `-` clears it |

**Before anything is deleted**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `require_confirmation` | `true` | Whether a prune pass asks before deleting |
| `allow_manifest_rewrite` | `false` | Whether a pass may run the *writing* sync form that repairs a drifted or missing lockfile |
| `command_timeout_secs` | `600` | Ceiling on any one package-manager command dev-prune runs: the lockfile check before a delete, and the reinstall `devp restore` performs. Nothing is compiled under it — the opt-in build adapters run no command at all during a prune — except a restore whose install builds a native module |

**Build trees — every one off by default**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `enable_cargo` | `false` | Turn the opt-in Cargo adapter on (`target/` is compiler output — it comes back by recompiling, not downloading) |
| `enable_gradle` | `false` | Turn the opt-in Gradle adapter on |
| `enable_maven` | `false` | Turn the opt-in Maven adapter on |
| `enable_swift` | `false` | Turn the opt-in Swift Package Manager adapter on |
| `enable_dart` | `false` | Turn the opt-in Dart/Flutter adapter on |
| `enable_mix_build` | `false` | Turn the opt-in **Elixir** Mix build-tree adapter on. Mix is Elixir's build tool; `_build/` is where it puts the compiled project and every compiled dependency, so it comes back by recompiling. Distinct from the always-on `mix` adapter, which claims only the downloaded `deps/` beside it |
| `enable_vcpkg` | `false` | Turn the opt-in vcpkg adapter on (`vcpkg_installed/` holds ports vcpkg built from source — it comes back by recompiling) |
| `enable_cmake_build` | `false` | Turn the opt-in CMake adapter on (claims only a tree holding a `CMakeCache.txt` that names a source directory inside the same repository — a `build/` you made by hand is never touched) |
| `build_idle_days` | `45` | Extra idle gate for the build-tool adapters (cargo, gradle, maven, swift, dart, mix_build, vcpkg, cmake_build); applied as `max(build_idle_days, idle_days)` |

**Shared download caches**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `cache_max_gb` | *(none)* | Per-manager cache size caps in GiB, as `uv=10,npm=10`. Keyed by cache-manager name (`npm`, `pnpm`, `uv`, `pip`, `cargo`, `go`, `nuget`, …), not adapter name. Marks an over-cap manager in `devp caches`; only `devp caches clear --over-cap` acts on it. `-` clears it |

**Running without being asked**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `auto_setup` | `true` | Whether the integration pass may run unattended |
| `auto_config` | `false` | Whether `link`/`init` write a default `.devprune.json` into repositories they register |
| `auto_daemon` | `true` | Whether that pass may register the OS scheduler |
| `check_interval_days` | `2` | How often the OS scheduler runs |
| `auto_hooks` | `true` | Whether that pass may install global Git hooks |
| `auto_hooks_chain` | `false` | Whether unattended setup may take a `core.hooksPath` another tool holds, forwarding every hook on to it. Off by default: that setting is one slot, global to the machine, and taking it rewires husky, pre-commit or lefthook for every repository the user has |

**Keeping dev-prune current**

| Key | Default | Meaning |
| :--- | :---: | :--- |
| `update_check` | `true` | Whether to ask GitHub for the latest release. Sends nothing but the request itself |
| `update_check_interval_days` | `7` | Days between automatic release checks; `devp update` always asks |
| `update_check_timeout_secs` | `5` | How long the release check waits for GitHub before giving up |
| `auto_update` | `true` | Download and install a newer release by itself at the end of a prune pass, once the release check has found one. Never runs a package manager unattended, and does nothing on WinGet/Scoop/Homebrew, where the manager owns the upgrade; a failed upgrade warns and never fails the pass |
| `version_lock` | `false` | Pin this copy to the version it is. Outranks everything above: `auto_update` does not run however it is set, `devp update --install` and `devp install --channel` refuse, and the install scripts leave the binary alone. No flag bypasses it; `devp config set version_lock false` is the only way back |


**Per repository:**
- `ignore.devprune.json` in the root — instant skip, checked before anything is parsed.
- `.devprune.json` — `"project_name"`, `"ignore": true`, `"override_idle_days": 30`,
  `"min_size_mb": 100`, `"scan_depth": 10`, `"disable_daemon": true` (excluded from
  scheduled passes only), `"disable_hooks": true` (not auto-registered by the global
  hook). Inert data only. Written into `.git/info/exclude`, so it never reaches a commit.
- `project.devprune.json` — the same keys, the same schema, and committed. Created by
  `devp config project . --team`, and deliberately not excluded.

Where both exist, **every key the project file names wins; the personal file answers every
key it does not.** That is the inverse of the usual local-overrides-shared convention and
is deliberate: these are decisions a project makes, and "this repository is not worth
pruning" should survive a colleague's stale local file. "Names a key" means the key is
literally in the file — a project file that never mentions `ignore` does not un-ignore
anything, which is why `--team` creates it holding only its `$schema` line.

**Declared directories.** Either file may also carry a `prunable` section:

```json
{ "prunable": { "directories": [
  { "path": "tools/vendor", "rebuild": "make vendor", "why": "regenerated from tools/manifest.toml" }
] } }
```

That is how a repository names a tree no adapter can recognise. `rebuild` is required; if
nothing has to rebuild the directory, write `"rebuild": "echo not needed"`, which works on
every platform. dev-prune prints the command and never runs it. These are pruned by the
ordinary pass under the adapter name `declared`, so they obey `--dry-run`, `--min-size`,
`--only` and the schedule. This is the one section where the two files' lists **add up**
rather than one winning.

When you fill this in for a user, name only directories you have confirmed are
regenerated — and give a `rebuild` you have actually seen work in that repository, not
a plausible-looking one. dev-prune will refuse a declaration whose path leaves the
repository, holds Git-tracked files, or whose `rebuild` names a tool the machine does not
have, and report it as `skipped_declaration`; that refusal is a backstop, not a substitute
for checking.

`devp config project <PATH>` prints an **Effective values** table naming the file each
value in force came from, whenever both files exist. Prefer reading that over inferring
precedence from the two files yourself.

Only ever write `.devprune.json`. `devp config --update`, the workspace toggles and the
dashboard's `i` key all do, and none of them touches the shared file — a tool editing a
tracked file on somebody's branch is a change they did not ask for. Edit
`project.devprune.json` only when the user has asked for a project-wide decision, and tell
them it is a commit.

Either file failing to parse skips the repository and reports the syntax error rather
than falling back to defaults — the unreadable file may have been the one saying
`"ignore": true`. `devp doctor --fix` repairs the personal file by renaming it aside; it
reports a broken `project.devprune.json` and leaves it for `git checkout`.

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

> Git supports exactly one global `core.hooksPath`, and while it is set Git stops
> looking in any repository's own `.git/hooks`. dev-prune restores that itself: it writes
> a shim for every hook name Git looks for, each ending by `exec`ing the repository's own
> hook of the same name with the same arguments, stdin and exit code — so a repo-local
> `pre-commit` gate still runs and can still block the commit. `reference-transaction`
> and `post-index-change` are left unshimmed on purpose; both fire many times per
> ordinary fetch and the shell spawns would be a cost users feel. dev-prune also does not
> simply give up when another tool holds the slot. `devp hook install --chain` takes the slot and writes a shim per hook
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

### Changing settings on the user's behalf

Use `devp config set <key> <value>` and `devp config show`. Both are one-shot, both
print what they did, and neither waits for a keypress.

**Never run `devp config wizard`.** It opens a full-screen configurator for a person to
drive, and an agent holding a pty passes every terminal check it makes while never
sending the keypress it waits for. If something must run it anyway, `--no-tui` or
`DEV_PRUNE_NO_TUI=1` gets the line-by-line form instead — but `config set` is the
right answer, because it does not need a terminal at all.

There is a second reason, and it is the more important one. The configurator's first
screen is a **declaration**: what dev-prune is, who published it, the channels a real
copy comes from, the guarantees the code enforces, and the Apache-2.0 terms the whole
thing is offered under. It is shown **once**, and running the configurator marks it as
seen. An agent that opens it therefore spends the one screen that exists so a human can
decide whether to trust this tool — and the human never gets it. Do not consume it on
their behalf.

`devp trust` and `devp trust --json` report the same facts, read-only, as often as you
like. Use those. If the user has never run dev-prune interactively, say so and let them
run `devp config wizard` themselves rather than doing it for them.

**`devp config recommended` is safe for you to run** when the user has asked for the
recommended setup. It is one-shot, prints every change, needs no terminal, and does not
mark the declaration as seen — the walkthrough still opens for them later. It turns on
`enable_cargo`, `enable_gradle`, `enable_maven`, `enable_swift`, `enable_dart`,
`enable_mix_build`, `enable_vcpkg` and `enable_cmake_build`, all of which make build
trees deletable.

`--with-cautious` additionally sets `allow_manifest_rewrite`, which lets `cargo` and
`go` update `Cargo.lock` and `go.mod` during a restore — files Git tracks. **Do not
pass it unless the user asked for that specifically.** An unexpected working-tree change
is exactly the kind of thing an agent must not cause on somebody’s behalf.

To switch one ecosystem off without touching anything else:

```bash
devp config get disabled_adapters              # (none), or the current list
devp config set disabled_adapters go,composer  # the whole list, not an addition
devp config set disabled_adapters -            # every adapter active again
```

The value is the complete list every time — there is no add or remove verb, so read
the current one before writing a new one or the user loses the rest of their choices.

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
| **Path Missing** | The directory was moved or deleted | If moved: `devp link <new path>` alone — the matching root commit adopts the old entry, history and all, and the dead row goes. If deleted: `devp unlink <old path>`, or `devp unlink --missing` for a registry full of them |
| Lockfile verification fails | The lockfile has drifted from the manifest, and verification is read-only so it refuses rather than repairing | Run the exact command dev-prune printed (it is the writing form for that ecosystem), in that project. Or `devp config set allow_manifest_rewrite true` to let dev-prune run it during the pass. Never delete the lockfile |
| "holds package(s) that the lockfile does not record" | Something was installed without recording it — `npm install --no-save`, a bare `pip install` into a pinned venv, an ad-hoc `uv pip install` | `devp status --drift` lists every such environment with the exact record command (`npm install <pkg>`, `uv add <package>`, `pip freeze > requirements.txt`). Run it, or uninstall the extras. Never delete the directory manually |
| "holds dev-prune, which requirements.txt does not account for" | The tool was `pip install`ed inside that project's own virtualenv, so it is an unrecorded package in the very environment it would prune | Move it out — `pip uninstall dev-prune`, then `uv tool install dev-prune` — or record it deliberately with `pip freeze > requirements.txt`. Either one makes the environment prunable. The refusal is the ordinary unrecorded-package one; only the suggested repair differs |
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
devp doctor          # installation: binary, PATH, install channel, config, integrations, registry
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
