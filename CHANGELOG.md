# Changelog

All notable changes to `dev-prune` (`devp`) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
  `pipx` / `pip`, `cargo install dev-prune`, or a direct download from GitHub Releases.
- **The npm and PyPI packages contain the binary.** No `postinstall` step downloads
  anything, so they install correctly under `npm ci --ignore-scripts`, behind a corporate
  registry mirror, and with no network access at all — and a dependency install never
  turns into an outbound call to GitHub. npm gets six platform packages selected by
  `os`/`cpu`; PyPI gets six platform wheels. Every npm tarball is published with
  provenance, and every wheel through PyPI Trusted Publishing.
- Configuration lives in the platform config directory: `%APPDATA%\dev-prune` on
  Windows, `~/Library/Application Support/dev-prune` on macOS,
  `$XDG_CONFIG_HOME/dev-prune` on Linux.

### Built with

Rust 1.85 (edition 2024), clap 4, ratatui, and no runtime dependencies beyond the
package managers already installed on the machine.
