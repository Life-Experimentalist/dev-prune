# Changelog

All notable changes to `dev-prune` (`devp`) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.1] - 2026-08-22

Every release archive can now be proved to have come from this repository, the Windows
installer stops guessing on machines it has no build for, and the terminal output uses
four colours instead of seven.

### Added

- **Build provenance on every release archive.** GitHub now signs a statement that each
  `.tar.gz`, `.zip` and `.vsix` was built by this repository's release workflow, from a
  named commit. Anyone can check it before unpacking, and a tampered or re-uploaded
  archive fails:

  ```bash
  gh attestation verify dev-prune-v1.3.1-linux-x64.tar.gz --repo Life-Experimentalist/dev-prune
  ```

  The `.sha256` sidecars are still published and still worth checking, but a checksum
  only proves the file survived the download — whoever produced the archive also
  produced the sidecar. Provenance is the part that says who that was.

### Changed

- **Terminal colour, dialled back.** `devp status`, the interactive views and
  `devp --version` used seven colours, several of which marked nothing: adapter names
  were magenta, repositories with nothing to reclaim were blue, the author line was a
  hard-coded turquoise that ignored your terminal theme. Now green means "you can have
  these bytes back", red and yellow mean something is wrong, cyan marks paths, links and
  keys, and everything else is your terminal's own colour — so the columns that *are*
  coloured are the ones worth looking at. `NO_COLOR` and piped output still disable it
  entirely.

### Fixed

- **The Windows installer refuses 32-bit machines instead of installing a binary they
  cannot run.** dev-prune publishes x64 and ARM64 Windows builds and no 32-bit build;
  `install.ps1` used to fall through to the x64 archive for any architecture it did not
  recognise, and the result was `is not a valid Win32 application` — an error that names
  neither the cause nor the fix. It now says which architecture it saw, that only 64-bit
  builds are published, and that `cargo install dev-prune` builds one from source. This
  matches what `install.sh` already did on Linux and macOS.
- Three documentation pages quoted release archive filenames from **1.2.0** in their
  manual-download and troubleshooting steps, so following them downloaded a two-release-old
  binary. They now name the current release, and a check keeps them there.
- **`devp status` colours its table again.** In a non-interactive terminal — a CI log, a
  pager, anything that is not a full-screen TUI — every row printed in plain white, so
  the reclaimable-bytes column looked no different from the dates beside it. The table
  had asked for colour since 1.0.0 and silently got none. Run `devp status` in a plain
  terminal and the space you can reclaim is green, a missing repository is red, and a
  broken `.devprune.json` is yellow.

### For contributors

- **`sh scripts/check-version.sh`** fails when any file that spells the release version
  out by hand disagrees with `Cargo.toml`: both install scripts' offline fallbacks, the
  site's version banner, `llms.txt`, and the three docs that quote whole asset filenames.
  CI runs it on every push and the release workflow runs it one step after checking the
  tag, so "agrees with `Cargo.toml`" and "agrees with the tag" become the same statement.
- The README and documentation banners are regenerated: one 1280×640 image
  (`assets/readme-banner.png`, also the correct size for GitHub's social preview) and one
  1200×630 Open Graph card, both centre-crops of `assets/banner-master.png`. The old hero
  was 5.8 MB of generated lettering that spelled `developements` and `READM.md`.
- [`docs/FUTURE.md`](docs/FUTURE.md) is now a triaged roadmap — *in flight*, *next*,
  *later*, *not planned* — so "we have not done that yet" and "we decided against that"
  no longer read the same. 32-bit builds, deleting build outputs, and any bypass for the
  seven safety invariants are recorded under *not planned*, with the reason attached.

## [1.3.0] - 2026-08-21

Three new ecosystems (Poetry, and opt-in Gradle and Maven), a real upgrade command,
man pages, per-editor AI rules, colour in the terminal — plus the full hardening pass
from a top-to-bottom audit of the codebase. No change to the seven safety invariants.

### Added

- **The Poetry adapter.** A project with `poetry.lock` (or `[tool.poetry]` in
  `pyproject.toml`) now gets its `.venv` pruned and restored like any other:
  verified read-only with `poetry check --lock` — plus a refusal if the environment
  holds packages the lockfile never recorded — and restored with `poetry install`.
  When uv and Poetry both describe the same project (usually a half-finished
  migration), whichever one's lockfile is actually on disk owns the environment.
- **Gradle and Maven adapters, opt-in.** `devp config set enable_gradle true` /
  `enable_maven true` lets a pass reclaim Gradle `build/`+`.gradle/` and Maven
  `target/` from idle repositories. They ship disabled because a build tree is
  regenerated by *recompiling*, not downloading — so they also wait for their own
  idle gate, **`build_idle_days`** (60 by default), applied as the maximum of it and
  `idle_days`: the build-tool gate can only ever make pruning later, never earlier.
  While disabled they are invisible — not detected, not listed, and `--only gradle`
  prunes nothing.
- **`devp update --install`** actually performs the upgrade, through the package
  manager that owns the running binary — installer script, cargo (`cargo binstall`
  when available), npm, uv or pipx, auto-detected from where the binary lives. One
  channel owns one binary: a copy installed through uv upgrades through uv, never
  through npm. And **`devp config set auto_update true`** runs it by itself at the
  end of a prune pass when the release check already knows a newer version exists —
  an upgrade never interrupts the scheduler, because the scheduled task runs the
  managed copy, which refreshes itself on its next healthy run.
- **`devp man`** renders the manual as man pages, generated from the same clap
  definitions `--help` prints — so the manual cannot describe a flag the program
  does not have. `devp man | man -l -` reads it now; `devp man --dir ./man` writes
  the full set, one page per subcommand.
- **`devp skill --agent <editor>`** writes per-repository AI rules in the file your
  editor's agent actually reads: `cursor`, `windsurf`, `antigravity`, `cline`,
  `copilot` (a marked block in `.github/copilot-instructions.md`) or `agents-md` (a
  marked block in `AGENTS.md` — the convention Codex, Jules, Amp, OpenCode and
  others follow). Shared files are only ever touched inside dev-prune's markers.
- **Colour in the terminal.** Candidates are green, active repositories cyan,
  errors red, adapters magenta, sizes bold — in `devp run`, `devp status` and the
  summaries — instead of a wall of bold white. Colour vanishes automatically when
  output is piped or `NO_COLOR` is set, so scripts parse exactly what they always
  did.
- **`auto_config`** (on by default): repositories registered by `devp link` and
  `devp init` get a starter `.devprune.json` with the `$schema` line, so editor
  validation and per-repo overrides are one keystroke away instead of a
  documentation lookup. `devp config set auto_config false` turns it off.
- **`devp run --explain`** answers "why wasn't my repo pruned?" without you having to
  guess. It lists every registered repository (or one, with a path) and each directory's
  verdict — including the states a normal pass keeps quiet about: still active (with how
  many days ago the last activity actually was, against your idle threshold), opted out,
  under the size floor, excluded by `--except`. It is read-only — nothing is verified,
  nothing is deleted — and it composes with `--only`/`--skip`/`--min-size`/
  `--ignore-idle`, so you can test a hypothesis one flag at a time. It cannot be
  combined with `--json`; the `--json --dry-run` document already carries every status.
- **`DEV_PRUNE_OFFLINE=1`** keeps the process off the network entirely — the release
  check and the editor-extension `.vsix` download fallback alike — regardless of any
  stored setting. For air-gapped machines and CI images. The durable per-user switch is
  still `devp config set update_check false`.

### Fixed

- **Windows: the background task no longer flashes a console window.** The scheduled
  task used to run with the interactive logon, so every firing — typically moments after
  opening the laptop — popped a black terminal window that vanished before it could be
  read, which looks like malware to anyone watching their own screen. The task now runs a
  windowless build of the binary, `devpw.exe`, generated locally beside the managed copy
  the same way `pythonw.exe` relates to `python.exe`: it has no console to show, so
  nothing flashes, and because it still runs in your own logged-on session, mapped
  network drives and Dev Drives keep working. If that build cannot be created — a policy
  or filesystem that forbids it — setup falls back to a hidden password-less task (an S4U
  logon), and then to the old visible task, so the daemon itself is never lost. Existing
  installations are upgraded automatically on the next setup pass. macOS and Linux never
  had this problem: their schedulers (launchd and systemd user timers) never attach a
  terminal to a background job.
- **Windows: PATH edits no longer flatten other tools' registry entries.** Setup and the
  PowerShell installer previously read the user `Path` through the environment API, which
  expands entries like `%USERPROFILE%\bin` before handing them over — and writing the
  result back froze those entries to their expanded text. Both now read the value raw,
  preserve its registry type, and broadcast the change so new shells pick it up.
- **`--json` stdout can no longer be polluted by first-run setup.** The automatic
  integrations pass now waits when any `--json` flag is present, the same way it already
  waited for `--quiet` and `--daemon`, so a script's very first `devp status --json` on a
  fresh machine parses.
- **The `devp status` dashboard scrolls.** The table previously discarded its scroll
  state every frame, so on a list taller than the window the selection walked off-screen
  and never came back. It also no longer rebuilds every repository's display name once
  per row per frame, and pressing `i` on a "Path Missing" row no longer tears the whole
  view down trying to write a config file into a directory that does not exist.
- **TUIs no longer open with stdin redirected.** `devp status < /dev/null` used to draw
  an interactive screen that could never receive a key; both interactive views now
  require a terminal on stdin *and* stdout, and fall back to the plain output otherwise.
- **A power cut during a save can no longer leave an empty registry.** The temporary
  file is flushed to disk before the atomic rename; previously the rename could survive
  a crash that the data did not. Stale temp files older than an hour are also swept.
- **`devp unlink <path>` works after the directory is deleted.** Registry keys are
  canonicalized paths and a deleted directory cannot be canonicalized, so unregistering
  it by name used to report "not registered". It now falls back to a lexical match.
- **`devp doctor` no longer calls an off-PATH binary breakage.** The binary demonstrably
  runs — doctor *is* it running — so this is now a warning naming `dev-prune setup` as
  the fix, and doctor exits `0`.
- **A mounted `node_modules` is no longer deleted.** Symlinks and junctions were already
  refused, but a bind mount, an NFS export or a container's
  `-v shared_modules:/app/node_modules` leaves an ordinary-looking directory whose
  contents are shared with whoever mounted it — and no lockfile rebuilds the *other*
  consumers' copy, because there is only one copy. A bloat directory sitting on a
  different filesystem from the repository around it is now reported and left alone, in
  `devp run` and in what `devp status` counts as reclaimable.
- **`devp uninstall` is safer about what it deletes.** The stray-copy sweep and the
  `--deep` confirmation prompt on stderr (so they survive redirected output), a bare
  Enter now declines instead of confirming, Windows files that are merely *in use* are
  correctly queued for the detached deletion helper instead of being reported as
  permission errors, and the helper itself now runs through PowerShell, whose
  single-quoted literals do not expand `%` — so an installation under
  `C:\Users\100%Sure\bin` is removed like any other instead of being refused. (`cmd.exe`
  stays as the fallback for a machine without PowerShell.) Anything neither helper can
  take is now listed individually — name, directory, type and size — with a
  ready-to-paste `Remove-Item` command, instead of one line saying some files were left
  behind.
- **Tables line up when a repository name is not ASCII.** Column padding counted
  `char`s, but a terminal draws in columns and a CJK character or an emoji occupies two
  of them — so a path like `~/代码/项目目录` pushed every column after it out by the
  number of wide characters in the name, and the further down the list you read the more
  crooked `devp status` looked. Widths are now measured in terminal columns, and a name
  too long for its column is truncated with an ellipsis rather than shoving the rest of
  the row sideways.
- **`devp skill` reports export failures.** A failed `SKILL.md` write used to be
  swallowed and the command claimed success over a file that was not there.
- **Windows: scheduler intervals above 365 days no longer fail.** `schtasks` rejects
  `/MO` values outside 1–365; the interval is now clamped.
- **The Go adapter fails closed on an unanswerable `vendor/` check.** When `git` cannot
  say whether `vendor/` holds uncommitted changes, the directory is now skipped rather
  than assumed clean.

### Changed

- **The VS Code extension is 0.3.0**, and its status bar now walks a workspace through
  dev-prune's whole lifecycle instead of showing one machine-wide total: devp not
  installed → not a Git repository → not registered → active (space occupied and which
  managers are in use) → idle candidate (the reclaimable size, with a "why so low?"
  note when pnpm or bun hardlink most of the bytes into their store) → cleaned (space
  saved here). Clicking it opens a state-aware menu, and new palette commands create a
  `.devprune.json`, ignore the repository, register it, or `git init` it. Its own
  changelog: `editors/vscode/CHANGELOG.md`.
- **`devp status --json` repositories now carry `bytes_freed`** — the lifetime space
  reclaimed from that repository — next to the existing `last_pruned_at`, so a tool
  (the VS Code status bar is one) can say "devp saved 1.2 GiB here" without also
  reading `devp stats`. Purely additive; nothing existing moved or changed shape.
- **The editor-extension offer names its listings and defaults to Yes.** The one-time
  "install the dev-prune extension?" question now prints the Marketplace, Open VSX and
  source-repository URLs before it asks, and takes a bare Enter as yes (`[Y/n]`). The
  three links are what make the default defensible: everything you would need in order
  to decline is on screen at the moment you answer. The downloaded `.vsix` fallback is
  also stored under the config directory (not a shared temp dir) and removed after
  installation. The uninstall sweep, which deletes rather than installs, still defaults
  to No.
- **`DEV_PRUNE_NO_AUTO_SETUP=1` now applies to `devp uninstall` too.** The variable has
  always meant "I manage the integrations by hand" — but the uninstall still deleted
  the scheduler entry, the agent skills and anything it could guess from the home
  folder, which is the wrong move against integrations you installed yourself. With the
  variable set, uninstall now leaves the scheduler and skills alone (saying so), and
  its stray-copy sweep searches only the directories on `PATH`. Unset, nothing changes.

### For contributors

- **Dependabot now watches the whole lockfile, not just the manifests.** The cargo
  entry allows `dependency-type: all`, so transitive crates are updated too. Releases
  build with `--locked`, which means `Cargo.lock` is what ships — and under the previous
  `direct`-only default it had drifted twenty-odd crates behind while every Dependabot
  run reported success. The site's npm checks moved from monthly to weekly for the same
  reason; grouping already keeps a routine month down to one pull request.
- **`devp uninstall` has integration tests** (`tests/uninstall_test.rs`): light and
  `--deep` modes, the confirmation refusals, and the stray-copy sweep — including that
  it deletes the planted strays and nothing beside them. The hands-off variable plus
  `DEV_PRUNE_CONFIG_DIR` is what makes the command safe to run on a contributor's
  machine at all; the tests pin `PATH` to their own directories on top of that.

## [1.2.0] - 2026-08-20

An uninstall that actually uninstalls, an install that survives the environment it was
installed from, automatic AI-agent skill setup, and color in the terminal output. No
change to pruning, verification or any of the seven safety invariants.

### Added

- **The AI agent skill installs itself.** Setup now detects an on-disk agent skills
  directory (`~/.claude/skills/`) and places the bundled skill at
  `~/.claude/skills/dev-prune/SKILL.md`, so agents like Claude Code discover `devp`
  automatically — no copy-paste prompt needed. The skill costs the agent almost nothing
  until it is actually used: only its one-line description is loaded per session. `devp
  skill` does the same install on demand and still prints the onboarding prompts for
  agents without a skills directory, and `devp setup --status` shows an "AI agent skills"
  line telling you where it landed. On a machine with no agent installed the step is
  skipped silently — nothing warns about software you don't have.
- **`devp` stays on your PATH no matter how you installed it.** Setup now puts the
  managed copy's directory (`<config>/bin`) on your user PATH on Windows, and symlinks
  both names into `~/.local/bin` on Linux and macOS. This is what makes `pip install
  dev-prune` inside a virtual environment work permanently: the venv's copy disappears
  when the venv does, but the managed copy it registered on first run remains reachable
  from every new terminal. `devp setup --status` shows a "Command on PATH" line.
- **Color in the output.** Backticked commands are highlighted so instructions stand out
  from prose, headers are cyan, sizes and paths carry their own colors, and `devp -V`
  colorizes the version report. Everything still degrades to plain text when piped —
  `--json` and redirected output are byte-identical to before.
- **`--json` output lands on your clipboard.** When you run `devp run`, `status`,
  `stats` or `caches` with `--json` in an actual terminal, the document is also copied
  to the clipboard, so pasting it into an issue, a chat or an editor is one keystroke.
  A dimmed `(also copied to your clipboard)` note goes to stderr. Piped or redirected
  output — the way scripts and agents consume `--json` — is untouched: stdout still
  carries the document and nothing else, and no clipboard is involved.
- **Setup offers the editor extension — in VS Code and its forks.** When a VS
  Code-family editor is on your PATH (VS Code, VSCodium, Cursor, Windsurf, Positron,
  Kiro, or an Insiders build) and the dev-prune extension is not installed, `devp
  setup` (and the first-run walkthrough) asks once whether to install it into each
  editor found — the extension validates `.devprune.json` as you type and shows the
  reclaimable size in the status bar. Each editor installs from its own registry
  (Marketplace or OpenVSX); if a fork's registry does not carry the extension, the
  `.vsix` from the latest GitHub release is installed instead, so the offer works
  everywhere the CLI does. One question, once ever, only at an interactive terminal:
  decline and it never comes up again, and CI, containers and
  `DEV_PRUNE_NO_AUTO_SETUP=1` never see the question at all. Install it by hand any
  time with `code --install-extension VKrishna04.dev-prune`.
- **`--help` is now the manual.** Every command and every `config` subcommand carries
  full long-form help: what it does, the behaviour that is not obvious from the flag
  list, and worked examples — `devp run --help`, `devp config hook --help`, and so on,
  at every level. `-h` still prints the short version. The same text answers "which
  keys can I set?" (`devp config get --help` lists all fourteen with defaults) and
  "how do I install completions?" (`devp completions --help` shows the line per shell).

### Changed

- **The per-repo config no longer touches your `.gitignore`.** When the CLI writes a
  `.devprune.json`, it now records it (and `ignore.devprune.json`) in the repository's
  `.git/info/exclude` instead of appending to `.gitignore`. The result is the same —
  the config never shows up in `git status` — but `.gitignore` is a tracked file shared
  with everyone who clones the repository, and a disk-cleanup preference that applies to
  one machine has no business appearing in your diff. Entries already added to a
  `.gitignore` by earlier versions are left alone; remove them by hand if you like.

### Fixed

- **The dashboard is readable on light-theme terminals.** Repository paths and the
  header row of `devp status` (and the `devp run` selection list) were drawn in fixed
  white, which vanishes on a white background. Text now uses the terminal's own default
  foreground, switching to white only on rows the dashboard paints dark itself — so
  both light and dark themes get legible contrast without any configuration.
- **`devp uninstall` now removes the program.** Previously it stopped the scheduler and
  hooks but left both binaries in place and on PATH, so `devp` kept working as if
  nothing had happened. Both modes now delete the managed pair and the copy you ran,
  remove the PATH entry (or the `~/.local/bin` symlinks), and delete the installed
  agent skill; `--deep` additionally purges the config directory and per-repository
  `.devprune.json` files. On Windows, where a running executable cannot delete itself,
  a detached helper removes the last files a few seconds after the command exits — no
  reboot, no closing the terminal. It then sweeps for every *other* copy of the binary —
  installing from pip, npm, cargo and uv over time leaves `devp` in `~/.cargo/bin`,
  `~/.local/bin`, npm's global directory and one `Scripts` folder per virtualenv, and
  any one of them keeps the command resolving after an "uninstall". The sweep scans
  your PATH and the well-known install directories, lists what it found (annotated
  with the package manager that owns each copy), and removes them all after one
  confirmation — `--yes` covers it, and declining leaves them in place without failing
  the uninstall. For each manager-owned copy the exact `pip uninstall` /
  `npm uninstall -g` / `cargo uninstall` / `uv tool uninstall` / `pipx uninstall` line
  is still printed at the end, so the manager's own records get cleared too.

## [1.1.0] - 2026-08-14

New commands and flags — `devp stats`, `devp completions`, `devp status --top` and
`--drift`, `devp doctor --fix` — plus cache coverage for the JVM, .NET and C/C++
ecosystems, the Windows installation and onboarding fixes, and a full audit pass over the
pruning engine, every adapter, the installers and the docs. Verification only got stricter: the seven
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
- **`devp status --drift`** lists every environment holding packages its lockfile never
  recorded — an `npm install --no-save`, a bare `pip install` into a pinned venv, an
  ad-hoc `uv pip install` — and shows the one command that records them. It is the same
  comparison a prune refuses on, surfaced as a pure read, so you can fix the drift on
  your own schedule instead of discovering it the moment a prune declines. `--json`
  hands the same report to an agent.

  ```bash
  devp status --drift
  ```
- **`devp doctor --fix`** repairs what the checks found. Plain `devp doctor` stays
  diagnosis-only and now says when a finding is repairable; `--fix` is the treatment,
  and it mends *installed-but-broken* only — a stale `devp` twin, hooks or a scheduler
  entry pointing at a binary that no longer exists, a drifted hook chain, a missing
  `SKILL.md` export, registry entries whose repository is gone. Each repair is the
  corresponding setup pass re-run, so it can never do more than `devp setup` would, and
  it never performs a first-time install.

  ```bash
  devp doctor --fix
  ```
- **`devp caches` now covers the JVM, .NET and C/C++ ecosystems**: the Maven local
  repository, the Gradle caches and wrapper distributions, the NuGet global-packages
  folder, the vcpkg binary cache and the Conan package cache — found where their
  relocation variables (`GRADLE_USER_HOME`, `NUGET_PACKAGES`,
  `VCPKG_DEFAULT_BINARY_CACHE`, `CONAN_HOME`) say they are, sized, and listed with the
  command that clears each. This is deliberately where these ecosystems live: their
  in-repository `target/`, `build/` and `bin/`+`obj/` directories are compiler outputs
  no lockfile can prove rebuildable, so dev-prune never deletes those — the gigabytes
  worth reclaiming sit in these machine-wide stores.

### Fixed

- **pnpm and bun projects no longer promise space a prune cannot free.** Both managers
  hardlink packages out of a global store rather than copying them (on Windows too —
  NTFS hardlinks, whenever the store and the project share a volume), so most of the
  bytes in their `node_modules` survive its deletion: the store keeps them. Every
  reclaimable and freed figure — `devp status`, `devp run`, `--dry-run`, `devp stats`
  and `--json` — previously counted the apparent size and could report gigabytes for a
  delete that returned megabytes. Sizes are now measured per file via the link count:
  a file also linked outside the tree is excluded and reported separately, the run
  report and status table say how much was excluded and why, and `--json` carries it
  as an additive `shared_bytes` field (no `schema` bump). Installs that genuinely
  copied — a store on another volume, a filesystem without hardlinks — have no
  external links and still count in full, and managers that always copy are untouched.
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
  `uv tool install dev-prune` / `uvx` / `pipx` / `pip`, `cargo binstall dev-prune` /
  `cargo install dev-prune`, or a direct download from GitHub Releases. (npm packaging
  is built for every release but publishing to the npm registry is gated off, so
  `npx dev-prune` / `npm install -g dev-prune` do not resolve yet.)
- **`cargo binstall dev-prune` needs no Rust toolchain.** crates.io distributes source, so
  `cargo install` has no binary to fetch and always compiles — surprising if you expected
  a registry install to be instant. `Cargo.toml` now declares where each release archive
  lives, so `cargo binstall` downloads and unpacks the same executable the installer
  scripts use, in seconds, on all six platforms.
- **The npm and PyPI packages contain the binary.** No `postinstall` step downloads
  anything, so they install correctly under `npm ci --ignore-scripts`, behind a corporate
  registry mirror, and with no network access at all — and a dependency install never
  turns into an outbound call to GitHub. npm gets six platform packages selected by
  `os`/`cpu` (built, though not yet published — the npm channel is gated off); PyPI gets
  six platform wheels, every one uploaded through PyPI Trusted Publishing.
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
