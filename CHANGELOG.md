# Changelog

All notable changes to `dev-prune` (`devp`) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-08-14

Three new commands — `devp stats`, `devp completions` and `devp status --top` — plus the
Windows installation and onboarding fixes, and a full audit pass over the pruning engine,
every adapter, the installers and the docs. Verification only got stricter: the seven
safety invariants are untouched, no new directory became eligible for deletion, and
several kinds that were eligible no longer are.

### Added

- **`devp stats`** answers the question `devp status` cannot: what has dev-prune already
  done for you. Lifetime space reclaimed, how many prune passes there have been, the most
  recent pass with the command that undoes it, the last ten passes, and the ten
  repositories that have given back the most. It is read-only, and `--json` gives an agent
  the same figures. `devp status` still answers what you could reclaim *next*; folding the
  history into it would have put a screen of the past above the list people open it for.

  ```bash
  devp stats
  devp stats --json | jq '.lifetime.bytes_freed'
  ```

  Per-repository totals and the pass history start recording in this release, so a machine
  upgraded from 1.0.0 shows a large lifetime total beside an empty history. The report says
  so rather than implying nothing was ever pruned, and the JSON document carries a
  `history_starts_at` field for the same reason.
- **`devp completions <shell>`** prints a tab-completion script for `bash`, `zsh`, `fish`,
  `powershell` or `elvish`. It is generated from the same argument definition the binary
  parses with, so a flag cannot exist in one and be missing from the other. The script is
  written for whichever name you invoked — `devp completions zsh` completes `devp`,
  `dev-prune completions zsh` completes `dev-prune`.

  ```bash
  source <(devp completions bash)          # this shell only
  devp completions zsh > ~/.zfunc/_devp    # permanently
  ```

  ```powershell
  devp completions powershell | Out-File -Append -Encoding utf8 $PROFILE
  ```
- **`devp status --top N`** lists only the N repositories with the most reclaimable space.
  Tracking a hundred repositories pushed the handful actually worth pruning off the screen.
  The survivors keep the dashboard's usual order, so it reads as a shorter version of the
  same list rather than a re-sorted one, and **the totals above the table are unaffected** —
  they are still computed over every registered repository, so `--top 5` cannot make a
  machine look tidier than it is. Works in the TUI, the plain table and `--json` alike.

  ```bash
  devp status --top 10
  ```
- **The installers now tell you how to register repositories**, which was the missing step
  between "installed" and "does anything". Both ways are spelled out: `devp init ~\Code`
  against the one folder that holds your projects, which finds every Git repository inside
  it however deep, or `devp link .` from inside a single project to register just that one.
- **`devp setup` says the same thing when nothing is tracked yet.** The installer scripts
  are not the only way in — `cargo install`, `npm i -g` and `pipx install` never run one —
  and `devp setup` is the step every channel has in common.
- **Packages that no file records are now grounds for refusal.** A virtual environment
  can hold a `pip install` that was never written back to `requirements.txt`; deleting it
  would lose that package with no way to reinstall it. The venv adapter now reads the
  environment's own `site-packages` metadata, walks the installed dependency graph from
  every pinned package, and refuses to prune when anything installed is unreachable from
  the file — naming the packages and suggesting `pip freeze > requirements.txt`.
  Transitive dependencies of pinned packages are fine; only the genuinely unrecorded are
  flagged. npm gets the same guard for a `node_modules` holding packages
  `package-lock.json` does not know about (including `npm link`ed ones), and uv for a
  `.venv` that has drifted from `uv.lock`. A requirements file that cannot be fully
  accounted for without running pip — editable installs, bare URLs — skips the comparison
  rather than guessing in either direction.
- **Python projects owned by poetry, pipenv or pdm are left to their own tools.** Their
  `requirements.txt` is usually an exported — and usually stale — copy of the real
  lockfile, and rebuilding from it would quietly produce a different environment than the
  one deleted. A project with `poetry.lock`, `Pipfile.lock`, `pdm.lock` or a
  `[tool.poetry]` table is no longer claimed by the venv adapter at all.
- **Three more refusals close the remaining gaps.** A bloat directory that turns out to
  contain a nested `.git` repository is refused rather than deleted with the repository
  inside it. When a package manager's binary is absent and only the on-disk lockfile can
  vouch for a rebuild, a manifest *younger* than that lockfile is refused — whatever just
  changed is not in the lockfile. And go's `vendor/` is claimed only when
  `vendor/modules.txt` proves `go mod vendor` built it, and refused when git reports it
  holds uncommitted changes.
- **A pass re-checks idleness at the moment it deletes.** Between the scan and your `y`,
  a repository can receive a commit — from you, from a pull, from an editor. Unless you
  passed `--ignore-idle`, that repository is now skipped as active instead of pruned
  against stale information.
- **A prune that would restore surprisingly says so before deleting**: several virtual
  environments all rebuilt from one `requirements.txt`, an environment whose folder name
  a plain `devp restore` would not recreate, one built with a Python that is no longer
  the `python` on PATH, or a `target/` holding criterion benchmark history that no
  lockfile brings back.
- **`devp run --json` reports three new statuses**: `skipped_symlink` (the directory is
  or contains a symlink; `message` names it), `activity_check_error` (idleness could not
  be proven, so nothing was deleted — counted in `summary.errors`), and `path_missing`
  (the registered directory no longer exists; `devp unlink --missing` clears such
  entries). New statuses do not bump the `schema` number — parse permissively.

### Fixed

- **A prune started from the `devp status` dashboard is now undoable.** Pressing `p`,
  selecting repositories and hitting `Enter` deleted them without recording the pass, so
  `devp restore --last-run` afterwards silently restored an *older* one — or reported that
  there was nothing to restore. The dashboard now records exactly what `devp run` records.
- **"Historical Space Saved: … across N prune passes" counts passes.** It previously
  counted whatever the command that pruned happened to iterate over: `devp run` added one
  per *repository*, the dashboard added one per *directory*. A single pass across four
  repositories could therefore report as four passes or as eleven, and the two numbers were
  not comparable. There is now one place in the code that increments it, and it means what
  the label says. The figure already accumulated on your machine is left alone; it is the
  sum of the old inconsistent counting and cannot be recomputed.
- **`devp` and `dev-prune` now put each other back.** Either name repairs the pair, so a
  `dev-prune.exe` lost to an antivirus quarantine, a half-finished uninstall or a
  `Remove-Item` aimed at one name comes back from `devp setup`. Previously that reported
  the alias as already present and did nothing, because the only direction it knew how to
  repair was `dev-prune` → `devp`. `dev-prune` stays canonical and remains the only one
  allowed to replace a *stale* twin, so a repair can never reinstall an older binary over
  a newer one.
- **`install.ps1` clears the Mark of the Web itself**, on the downloaded archive and on
  both installed executables, so the `Windows protected your PC` dialog has nothing left
  to challenge and there is no `Unblock-File` to remember afterwards.
- **The Windows installer now tells you when Smart App Control is going to block
  dev-prune.** It reads the policy state before running the binary it just installed, and
  a machine in enforcement mode gets an explanation instead of what otherwise looks
  exactly like a corrupt download. A binary Windows refuses to start also no longer ends
  the install in a stack trace: the binary is on disk, and only `devp setup` is left over.
- **`devp restore` works on the directory a prune just deleted.** Restore re-detected the
  project before reinstalling, and for a venv the marker it detects by — `pyvenv.cfg` —
  was inside the directory that was just deleted, so the restore reported nothing to do.
  It now uses the package manager recorded at prune time, and rebuilds a virtual
  environment under the folder name it actually had, so activate scripts and IDE
  interpreter paths keep pointing at something real.
- **The dashboard prunes the repositories you selected.** Selection was tracked by row
  position against a list that can re-sort mid-session, so pressing `Enter` could prune a
  different repository than the one highlighted. Selections now travel as paths, and a
  dashboard-started pass reads the same per-repository settings `devp run` does.
- **An interrupted pass no longer forgets what it already deleted.** The record was
  written once at the end, so a Ctrl+C, a crash or a shutdown mid-pass left directories
  deleted with `devp restore --last-run` unaware of them. The registry is now saved after
  each repository's deletions. When a deletion fails partway — an open file handle, a
  permissions error — the pass now names what remains and suggests the restore, instead
  of failing silently with a half-deleted tree.
- **Two passes can no longer corrupt the registry.** A scheduled pass colliding with a
  manual one wrote through the same temporary file before the atomic swap; each process
  now writes through its own, so the last writer's file lands whole rather than as an
  interleaving of both.
- **Prune history lands on the right repository regardless of path spelling.** A
  repository pruned via a differently-spelled path than it was registered under — `.`
  versus the absolute path, a drive-letter case difference — recorded its statistics
  under a key that matched nothing, so `devp stats` and `devp restore --last-run` missed
  it. The lookup now canonicalises the path the same way registration does.
- **Yarn Berry verification failures are failures again.** A failed
  `yarn install --immutable` was downgraded to "the lockfile exists" for every yarn
  project. That concession exists for Yarn Classic — which rejects the
  `--mode update-lockfile` flag outright — and now applies only to Classic projects; a
  Berry project whose lockfile cannot rebuild `node_modules` is refused.
- **Cargo workspace members verify against the workspace root's `Cargo.lock`.** A member
  crate has no lockfile of its own; that used to read as "no lockfile at all", which
  could end in `cargo generate-lockfile` writing a spurious one inside the member. The
  root lockfile is the record for every member, and it is now the one consulted.
- **A symlinked bloat directory is a skip, not an error.** Refusing to delete through a
  link is deliberate protection, but it was reported as a failure and made the whole pass
  exit `1`. It now reports as `skipped_symlink` with the link named, and does not count
  as an error.
- **`devp unlink` clears the undo list too.** Unlinking a repository that the last
  `init` or `link` had added left it in the undo record, so a later `devp undo` reported
  removing repositories that were already gone.
- **Confirmation prompts go to stderr.** `devp run > log.txt` used to hang on a question
  you could not see, because the prompt was redirected into the file with everything
  else. A pass with no terminal attached and confirmation still required now exits with a
  message naming `--yes` instead of waiting forever, and `devp status --top 0` is a usage
  error rather than an empty dashboard.
- **Scanning is harder to derail.** One unreadable directory no longer aborts repository
  discovery — it is skipped and the walk continues. The activity check's file-time walk
  is depth-capped like discovery, and a file whose modification time is in the future — a
  bad clock, a mangled archive — no longer keeps a repository "active" forever.
- **Windows housekeeping.** `devp doctor` compares PATH entries case-insensitively, so a
  correctly installed binary is no longer reported missing; uninstalling a scheduled task
  that does not exist succeeds instead of failing the uninstall; `devp setup` repairs a
  scheduler or hook whose registered executable has gone missing; `devp doctor` warns
  when the `devp` and `dev-prune` executables have drifted apart; and paths are shown
  without the `\\?\` prefix even for UNC shares. On macOS, reinstalling the LaunchAgent
  unloads the old one first, so an upgrade cannot leave two copies loaded.
- **The installers and the npm wrapper handle the awkward machines.** `install.ps1`
  enables TLS 1.2 on PowerShell 5.1 (github.com refuses the older defaults), detects
  ARM64 correctly under x64 emulation, compares PATH entries with trailing slashes
  normalised, and is wrapped so a download truncated mid-stream parses as an error
  instead of executing half an installer. `install.sh` tolerates CRLF checksum files and
  ties its "PATH already configured" detection to the actual install directory, so a
  reinstall with a different `--bin-dir` updates PATH instead of assuming the old entry
  still covers it. The npm wrapper forwards `SIGTERM`/`SIGINT`/`SIGHUP` to the binary, so
  a process manager killing the wrapper no longer orphans a prune mid-pass.

### Changed

- **dev-prune now says who wrote it.** `devp --version` prints the author, the repository
  and the homepage alongside the environment audit it already showed, the `devp status`
  dashboard carries a one-line credit in its footer, and an interactive command closes with
  the same line. All three are plain constants in
  [`src/constants.rs`](https://github.com/Life-Experimentalist/dev-prune/blob/main/src/constants.rs) —
  greppable, changeable, and load-bearing on nothing. Delete them and the binary still
  builds and still passes the test suite.

  The credit line is printed only when stdout is a terminal. It is never in `--json`, never
  in a pipe or a redirect, never in a CI log, and never in a completion script, because
  those outputs are read by programs rather than by people.
- **A `NOTICE` file ships with the source and the crate**, as Apache-2.0 §4(d) expects of a
  work that wants attribution carried into derivatives. It also lists how to enumerate the
  dependency licences.
- **Every install one-liner now says which shell it is for.** Pasting
  `curl -fsSL … | sh` into a Command Prompt answers `'sh' is not recognized`, which reads
  like a broken installer rather than the wrong command for the window you are in. The
  README, the site and the release notes label each form, and
  [troubleshooting §4](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/INSTALLATION_ISSUES.md#4-sh-is-not-recognized--the-install-one-liner-is-for-the-wrong-shell)
  maps your prompt to the right one.
- **`pip install dev-prune` is listed on its own**, next to `uv tool install` and `pipx`,
  with the one thing that actually differs between them: pip follows whichever environment
  is active, so inside a virtualenv `devp` lives in that venv's `Scripts`/`bin` and
  disappears with it. `pip install --user` is the machine-wide form.

### Documentation

- **SmartScreen and Smart App Control are now told apart**, because the fixes are not
  interchangeable and the previous guidance conflated them. SmartScreen challenges
  unsigned files that carry a Mark of the Web, and `Unblock-File` settles it. Smart App
  Control refuses unsigned executables outright, never looks at the mark, and ships
  enabled only on clean installs of Windows 11 22H2 and later — which is the whole reason
  one laptop installs cleanly and the next one blocks. It cannot be worked around by
  installing from npm, from PyPI, or by building from source, and turning it off is a
  one-way switch Windows cannot reverse without a reinstall.
  [Troubleshooting §3](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/INSTALLATION_ISSUES.md#3-windows-will-not-run-dev-pruneexe)
  now says all of that, including how to read the block out of the CodeIntegrity event log,
  and how to get past the SmartScreen one you *can* get past — **More info → Run anyway**,
  or the **Unblock** tick box in the file's Properties.
- **`uv tool install --system` is documented as the wrong flag**, because it looks like the
  right one. `--system` belongs to `uv pip install`; `uv tool install` rejects it outright.
  Where `devp` lands is decided by `UV_TOOL_BIN_DIR`, and no Python is involved at run time
  regardless — dev-prune is a Rust binary riding inside a wheel.

### For contributors

- **`CONTRIBUTING.md` documents the PowerShell execution policy.** Running
  `scripts/install.ps1` from a checkout on Windows stops with "running scripts is disabled
  on this system"; the fix is a process-scoped `-ExecutionPolicy Bypass` rather than a
  permanent `Set-ExecutionPolicy`. It also explains why the published `iwr … | iex`
  one-liner is not subject to the policy at all.
- The pre-PR commands in `CONTRIBUTING.md` now match the four CI runs. `--all-targets`,
  `--all-features` and the site build were missing, so lint failures that CI catches were
  invisible locally.

## [1.0.0] - 2026-08-12

First public release. `dev-prune` reclaims disk space from idle Git repositories by
deleting dependency and build directories that a lockfile can rebuild — and refuses to
delete anything it cannot prove is recoverable.

### Pruning engine

- **Lockfile-verified deletion.** No directory is removed until its package manager has
  confirmed a usable lockfile. Verification cannot be bypassed by any flag or setting.
- **Read-only verification, everywhere.** Every adapter proves the lockfile can rebuild
  the tree without writing to it — `npm ci --dry-run`, `pnpm install --lockfile-only
  --frozen-lockfile`, `yarn install --immutable`, `uv lock --locked`, `cargo metadata
  --locked`, `go mod download`. A lockfile that has drifted from its manifest is a
  refusal, not something to quietly fix: a pass can be started by the OS scheduler, and
  it must never leave a modified tracked file behind. The writing form runs in exactly
  two cases — no lockfile exists at all, or `allow_manifest_rewrite` is set, which is
  the informed opt-in and now means the same thing in every ecosystem.
- **Two enforcement tiers.** With the manager installed, it resolves the manifest against
  the lockfile. With the manager missing but a lockfile on disk, the lockfile is itself
  the proof and `devp restore` can rebuild later. With neither, nothing is deleted.
- **`command_timeout_secs` bounds every verification.** Each package-manager command runs
  under the configured ceiling (600s by default) and a hang fails the check rather than
  the pass.
- **Idle detection.** A repository is a candidate only after `idle_days` with no commit
  and no source modification. `--ignore-idle` lifts that threshold and nothing else.
- **Inverse selection.** `run --except <repos>` prunes everything but the named
  repositories, so "clean up but keep the API project" does not mean pruning it and
  downloading it back.
- **Adapter and size filters.** `--only`, `--skip` and `--min-size`, with `min_size_mb`
  as the persistent form of the last.
- **Symlink refusal.** A symlinked or junctioned bloat directory points at storage the
  repository does not own and is never deleted.
- **Per-directory selection.** The interactive selector prunes exactly the directories
  left ticked, and starts with every candidate ticked so keeping one is a single
  keystroke.
- **Dry run.** `--dry-run` reports every candidate and its size without running a
  package manager or touching disk.
- **Machine-readable output.** `--json` on `run` and `status` emits one document on
  stdout and nothing else; every diagnostic goes to stderr, so the output is parseable
  even when something went wrong.

### Multi-ecosystem repositories

- **Eight adapters**: npm, pnpm, yarn, bun, uv, pip/venv, cargo, go.
- **Any number of managers per repository.** A repository may hold uv, npm and cargo in
  its root, spread them across `frontend/`, `services/api/` and `tools/cli/`, or mix
  both. Every project is discovered, verified and pruned on its own terms, and each
  directory is reported by its repository-relative path.
- **Bounded discovery.** The walk descends `scan_depth` levels — six by default,
  configurable globally and per repository — and never enters `node_modules`,
  `target`, `vendor`, virtual environments, hidden directories, or nested repositories —
  a submodule is pruned as itself, never as part of its parent.
- **Single owner per directory.** When npm, pnpm, yarn or bun all claim the same
  `node_modules`, one is chosen: the `packageManager` field of `package.json`, else the
  manager whose bookkeeping files are inside the installed tree, else the most recently
  written lockfile. uv takes precedence over plain venv for the Python environment.
- **Virtual environments by marker, not by name.** Any directory containing
  `pyvenv.cfg` is recognised, whatever it is called.

### Safety

- Deletion is refused when the lockfile is missing, unparseable, or — for
  `requirements.txt` — lists no packages, because the tree could not be rebuilt.
- `ignore.devprune.json` in a repository root opts it out with a single file-existence
  check, before any config is parsed.
- `.devprune.json` holds only inert data: ignore flags, a display name, daemon/hook
  opt-outs, and per-repository overrides for the same numeric and boolean settings the
  global config takes. There is no key that names a command, a path to execute, or a
  binary to run — nothing in a repository-tracked file can cause command execution, which
  matters because these files arrive with a `git clone`.
- A `.devprune.json` that cannot be parsed skips the repository and reports the syntax
  error, rather than falling back to defaults — the unreadable file may have been the
  one saying `"ignore": true`.
- Nothing dev-prune installs edits an editor's settings, a shell startup file, or the
  system PATH outside the installer scripts. `devp config icon` registers the file type
  with the OS file manager and *prints* an editor snippet for you to paste.
- A run that fails any repository exits non-zero. Exit codes are `0` success, `1`
  failure, `2` unusable arguments.

### Commands

- `init`, `link`, `unlink`, `undo` — register repositories, individually or by scanning
  a tree. `unlink --missing` clears every entry whose directory no longer exists in one
  pass, which is what a registry accumulates from deleted clones and moved workspaces.
- `run [PATH]` — prune every registered repository, or one target.
- `status` — an interactive dashboard of every registered repository, its state,
  reclaimable space and last activity, with `i` to ignore and `p` to prune.
  `status daemon` and `status hook` report the background integrations.
- **`caches [--json]`** — the answer to "where did my disk actually go?". Finds every
  package manager cache and store on the machine — npm, pnpm, yarn, bun, uv, pip, cargo's
  registry, Go's module and build caches — sizes each one, orders them largest first, and
  prints the command that clears it. Each manager is asked where its cache lives rather
  than assumed, so a `CARGO_HOME` or a corporate `.npmrc` is followed; a manager you have
  since uninstalled still has its leftover cache reported. **It deletes nothing, and no
  flag makes it.** A cache is shared by every project on the machine, so no single
  lockfile can prove it recoverable — and it is what makes `restore` fast. Run
  `devp caches` when you want the number, and the clear command yourself when you want
  the space more than the speed.
- `restore [PATH] [--last-run]` — reinstall dependencies for every project in a tree.
  `--last-run` restores exactly what the most recent prune pass deleted, wherever those
  projects were, so an over-eager pass is one command to undo.
- `doctor [PATH]` — a read-only diagnosis. Without a path it checks the installation:
  the binary and its PATH entry, the registry and every setting in it, the integrations —
  including the binary the scheduler and the hooks will actually run, so one left pointing
  at a deleted directory is reported rather than silently doing nothing forever — which
  package managers are actually reachable, and the release-check state. With a path
  it checks one repository and names the reason a prune pass would skip it. It runs no
  package manager and repairs nothing, so it can be run twice to see whether a fix
  worked. Warnings exit `0`; only genuine breakage exits `1`.
- `config` — global settings (`get`, `set`, `show`, and a `wizard` that walks through
  every one of them), per-repository `.devprune.json`, the OS scheduler, Git hooks, and
  the file-manager icon for `*.devprune.json`.
- `update [--offline]` — reports the installed version, asks GitHub's public API for the
  latest release, and prints the upgrade command for how it was installed.
- `skill` — exports `SKILL.md` for AI coding assistants.
- `setup [--status]` — installs any missing integration; `--status` reports without
  changing anything.
- `uninstall [--deep]` — removes the scheduler and hooks; `--deep` additionally clears
  configuration after confirming the number of repositories affected.
- `-V` — version plus an environment audit: OS, architecture, config path, binary
  directory, and PATH activation.
- **Shorthands.** `devp hook`, `devp daemon` and `devp icon` reach the `config`
  subcommands of the same name, and `install` / `uninstall` / `on` / `off` are accepted
  wherever `enable` / `disable` are. A misspelled action is rejected instead of quietly
  printing status.
- **Paths.** `.` means the current directory and is the default wherever a path is
  optional. A leading `~` is expanded by dev-prune itself, not by the shell, so
  `devp init ~/Code` behaves the same in bash, PowerShell and cmd, quoted or not.

### Background automation

A pruner that has to be remembered is a pruner that never runs, so the integrations
install themselves — at install time, and again on the first command after an upgrade if
anything is missing. `devp setup` is that pass, run by hand; it installs only what is
absent and reports what it declined to touch.

- **OS scheduler** — `schtasks` on Windows, a LaunchAgent on macOS, a systemd user timer
  on Linux, each running at the configured `check_interval_days` interval. Scheduled
  passes are non-interactive and skip repositories that set `disable_daemon`.
- **Durable paths.** The scheduler entry and the hook scripts both outlive the process
  that wrote them, so both record the binary in `<config>/bin` rather than wherever the
  command happened to be run from. Installing through `npx dev-prune` or `uvx dev-prune`
  would otherwise register a path inside a cache the package manager deletes, and neither
  a scheduled task nor a Git hook has anywhere to report that it has stopped working.
- **Git hooks** — `post-commit`, `post-checkout` and `post-merge` auto-register the
  repository you are working in. Git allows one global `core.hooksPath` and no chaining,
  so when husky, pre-commit or lefthook already hold it, `devp hook install --chain`
  takes the slot and writes shims that `exec` the displaced tool's hooks — same
  arguments, same stdin, same exit codes — and uninstall puts the original path back.
  The pass skips entirely when `git` is not on `PATH`. Repositories that set
  `disable_hooks` are skipped.
- **The `devp` second binary and `SKILL.md`** — `devp` is a real executable beside
  `dev-prune`, not a shell alias, so it works in cmd, in an IDE terminal and in the OS
  scheduler rather than only in the shell whose profile was edited. Both are kept in step
  with the installed binary, so an upgrade cannot leave a stale copy or an outdated skill
  file behind.
- **File-manager icons** — `*.devprune.json` is registered with the OS file manager as
  part of the same pass, as far as each platform allows.
- **First-run walkthrough** — on a fresh install the config wizard runs once, so the
  defaults are agreed to rather than inherited. It is skipped, never guessed at, when
  there is no terminal to ask on.
- **Off switches.** `auto_daemon`, `auto_hooks` and `auto_setup` each turn off part or all
  of it; `auto_hooks_chain` governs the chained install specifically.
  `DEV_PRUNE_NO_AUTO_SETUP=1` turns off all of it without a config file, and CI and
  container environments are detected and treated as unattended without being told.

### Privacy and distribution

- **No telemetry.** No diagnostics, no usage data, no identifiers, no analytics of any
  kind, on any code path.
- **One network request, and only one.** The release check makes a single unauthenticated
  `GET` to GitHub's public releases endpoint — no body, nothing identifying the machine —
  at most once every `update_check_interval_days` (7). It is opt-out
  (`devp config set update_check false`, or `--offline` for one run), because a pruner
  nobody thinks about is a pruner nobody updates. dev-prune never downloads or replaces
  its own binary; it prints the upgrade command and stops.
- Installer scripts verify the published SHA-256 checksum of the release archive and
  refuse to install without one.
- **Six prebuilt binaries, no per-distribution builds.** Windows, macOS and Linux, x64
  and arm64. The Linux assets are statically linked against musl, so one file per
  architecture runs on Debian, Ubuntu, Fedora, RHEL, Arch, NixOS and Alpine alike, with
  no glibc version floor.
- **Install it however you already install things.** The shell and PowerShell one-liners,
  `npx dev-prune` / `npm install -g dev-prune`, `uv tool install dev-prune` / `uvx` /
  `pipx` / `pip`, `cargo binstall dev-prune` / `cargo install dev-prune`, or a direct
  download from GitHub Releases.
- **`cargo binstall dev-prune` needs no Rust toolchain.** crates.io distributes source, so
  `cargo install` has no binary to fetch and always compiles — surprising if you expected
  a registry install to be instant. `Cargo.toml` now declares where each release archive
  lives, so `cargo binstall` downloads and unpacks the same executable the installer
  scripts use, in seconds, on all six platforms.
- **The npm and PyPI packages contain the binary.** No `postinstall` step downloads
  anything, so they install correctly under `npm ci --ignore-scripts`, behind a corporate
  registry mirror, and with no network access at all — and a dependency install never
  turns into an outbound call to GitHub. npm gets six platform packages selected by
  `os`/`cpu`; PyPI gets six platform wheels. Every npm tarball is published with
  provenance, and every wheel through PyPI Trusted Publishing.
- **Apache-2.0, and provable.** Copyright 2026 VKrishna04. Every source file carries an
  `SPDX-License-Identifier`, so a licence scanner in your CI answers the same as the
  `LICENSE.md` in the repository, and every distributed artefact — the crate, all seven
  npm packages, all six wheels — ships the full licence text rather than only a field
  naming it.
- Configuration lives in the platform config directory: `%APPDATA%\dev-prune` on
  Windows, `~/Library/Application Support/dev-prune` on macOS,
  `$XDG_CONFIG_HOME/dev-prune` on Linux.

### Built with

Rust 1.85 (edition 2024), clap 4, ratatui, and no runtime dependencies beyond the
package managers already installed on the machine.
