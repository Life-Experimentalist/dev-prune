# Changelog

All notable changes to `dev-prune` (`devp`) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.17.0] - 2026-09-01

### Added

- **`devp caches clear docker`** (also `podman`, `nerdctl`) runs the container prune
  commands the report has been printing since 1.13.0, instead of asking you to go and type
  one in another window. Build cache, unused images, stopped containers — printed first,
  after a prompt, and counted on its own line in `devp stats`, so the 20 GiB you reclaimed
  on dev-prune's advice is space dev-prune can actually account for. `--dry-run` shows the
  commands and the estimate and touches nothing.

  **It will not touch a volume, and there is no flag that makes it.** No argument anywhere
  in its table contains the word, and a unit test fails the build if one appears. An image
  can be pulled again and a build cache rebuilt; what is inside a named volume is the only
  copy. `docker volume prune` stays a command it prints and you type — and the estimate
  has the engine's unused-volume figure taken out of it and named separately, rather than
  promising back space these commands cannot give.

  Nothing on a schedule reaches any of this: no daemon, no Git hook, no `devp run`, and
  not `devp caches clear all` either. Naming the engine is the consent.
  [Reference](docs/CLI_REFERENCE.md#devp-caches-clear-engine---dry-run---yes---json)

- **[How a repository gets registered](docs/BACKGROUND_AUTOMATION.md#how-a-repository-gets-registered)**
  is every way a repository can land on a machine, and which of the three mechanisms
  catches it. A repository you unzipped, copied from another machine, restored from a
  backup or received over Dropbox never runs `git`, so no hook can ever fire for it —
  discovery is what covers those, and the table says so instead of leaving you to work it
  out. It also names the one case that still needs a hand, and what is skipped on purpose.

- **`devp history`** answers the question `devp stats` raises. That report says ten passes
  freed 27.66 GiB; this one says pass #2 ran at 09:14 as `devp run --daemon`, took
  4.02 GiB out of two repositories, and names all three directories it deleted. Bare, it
  is one line per pass. `devp history --pass 1` opens one in full: the exact command line,
  the version that ran it, and every directory removed with its package manager and size,
  grouped by repository.

  **It records what started each pass**, which nothing did before — `manual` if you typed
  `devp run`, `scheduled` if it was the background pass, `dashboard` if it was the `[p]`
  key in `devp status`. There are three ways to start a prune and those are all three, so
  "was that me or the daemon?" now has an answer rather than an inference.

  Long output has three exits rather than a truncation: detail is per-pass and opt-in, the
  directory list is capped at 200 entries **only when it is going to a terminal** (redirect
  or pipe it and nothing is elided), and `devp history --export` writes the whole document
  to your documents folder — or to exactly the path you name.

  Passes from before this release still appear, marked `(totals only)`: their four numbers
  were always in the registry, and a history that started empty next to a `devp stats`
  reading "10 prune passes" would look like a bug rather than a gap. `devp history --json`
  carries `detail: false` on those, so a script can tell "deleted nothing" from "nobody
  wrote it down".
  [Reference](docs/CLI_REFERENCE.md#20-devp-history---pass-n---limit-n---all---json---export-path)

### Changed

- **`devp trust` now lists every copy of dev-prune on the machine**, not just the ones it
  manages. It looks in every directory on `PATH` and in the install directory of every
  channel it knows about — including the ones that stop being on `PATH` when a venv
  deactivates or a profile line goes — and prints each copy with the manager that
  installed it, its SHA-256 and the lookup URL for that digest.

  The old report knew about the managed directory and the file that was running, so a
  machine carrying `dev-prune`, `devp` and `devpw` in `~/.cargo/bin` as well saw one row
  out of four. That is the wrong count to print under "binaries on this machine", and it
  hid the case the section exists for: several copies, different ages, and only one of
  them the one your antivirus just looked at. Several copies is not a fault — a
  `cargo install` and an installer run each leave one and they upgrade separately — and
  `devp uninstall` names the command that removes each. Shims are left out: a `.cmd`
  wrapper is a text file that runs the real executable, so a digest for it compares
  against nothing. Nothing found is ever executed, only hashed. `--json` gains a
  `channel` field per binary.
  [Reference](docs/CLI_REFERENCE.md#18-devp-trust---json---fix-ownership)

- **`devp caches` now prints `devp caches clear <manager>` beside each cache**, above the
  manager's own command rather than instead of it. The two are not synonyms: what
  dev-prune clears is added to the lifetime total in `devp stats`, and `npm cache clean`
  typed into a terminal is invisible to it — so the report a week later was short by
  exactly the space you had reclaimed. The manager's command still gets its own line,
  labelled `runs:`, because that is what dev-prune executes and hiding it would be worse
  than repeating it. Maven is unchanged and still prints only the manual command: its
  local repository is not a cache, and `devp caches clear maven` refuses.
  [Reference](docs/CLI_REFERENCE.md#16-devp-caches-clear-manager--all---unused---over-cap---dry-run---yes---json)

### For contributors

- **The release workflow registers each published digest with VirusTotal.** A binary with
  no reputation is what heuristic engines score as suspicious, which is how the 1.5.0 zip
  came to be quarantined; the answer is for every image to be known before the first user
  downloads it, so that `devp trust` sends people to a report that exists. The job looks
  each digest up and submits only what nothing has seen — including `devpw.exe`, which
  ships inside the Windows zips and so had no record of its own. It never fails the
  release: same-day detection counts are noise that falls as downloads accumulate, and a
  workflow that gated on them would block every release forever. Skipped with a warning
  when the repository has no `VT_API_KEY` secret.

### Fixed

- **The one safety promise on `devp`'s first screen named the wrong command.** Both the
  bare-`devp` banner and the first screen of `devp setup` ended with "Only what a lockfile
  can rebuild — and `devp undo` puts back the last run." `devp undo` reverses an `init` or
  a `link`; it has never put a pruned directory back, in any release. The command that
  does is `devp restore --last-run`, which is what both screens now say.
- **`devp caches` explains the per-repository figure that looked like an arithmetic
  error.** A row reading `go build cache 2.7 GiB · go is used by 3 of 9 registered
  repositories · 1.5 GiB each` was correct and unreadable: the figure covers both of go's
  caches, and the row it is printed beside is the smaller one. It now says what it summed
  — `1.5 GiB each across its 2 caches` — and only where a manager has more than one.

- **`devp status` → `[p]` can now select the repositories it is showing you.** Prune-select
  mode only ever accepted repositories already past the idle threshold, so on a machine
  where you have touched everything recently — the normal case — pressing `p` armed
  nothing, `[Space]` did nothing, and a mode that opened empty looked exactly like a mode
  that had failed to open. `[Space]` now selects any repository with something to reclaim,
  including one you are working in today, and a row it still refuses says why: nothing to
  reclaim, ignored, path missing, or a `.devprune.json` it could not read. The footer
  counts against what is actually selectable rather than against everything on screen.
  Nothing about what can be *deleted* changed — lockfile verification runs on every
  selected repository exactly as before and still has no bypass. `[a]` and `[p]` continue
  to arm only idle repositories, because `[Enter]` prunes without asking a second time.

### Changed

- **`devp run --ignore-idle` now prints the manual route next to the AI one.** The notice
  already pointed at `devp skill` for anyone still stuck; it now also lists the five
  commands that skill would reach for — `devp run --explain`, `devp status`, `devp doctor`,
  `devp config show` and `devp man` — so you can work the problem yourself instead of
  handing your shell to a language model to find out what `--explain` would have told you.
- **`auto_discover` and `auto_hooks` now say which arrivals each of them actually covers.**
  Their descriptions in `devp config` read as two vague promises to find things, and
  neither mentioned the other, so there was no way to tell that the Git hooks only ever see
  repositories Git itself creates. `auto_hooks` now names its three triggers — clone,
  commit, merge — and hands everything else to `auto_discover`, which now says that it runs
  only on the scheduled pass and finds projects by looking beside the ones you already
  have.

## [1.16.0] - 2026-09-01

### Added

- **`devp caches --volume V:`** reports only the caches sitting on one drive, and the
  unfiltered report now ends with a `By drive` line splitting the total the same way.
  On a machine whose projects live on a second disk, "22 GiB of caches" is not the
  figure that decides anything — the two gigabytes on the drive that is *full* is, and
  until now you had to read twelve absolute paths to find it. `--drive` is the same
  flag; both take a drive (`V:`, `V:\`, or the bare letter `V`), a mount point
  (`/mnt/data`, `/Volumes/Work`), or any path on the one you mean, so `--volume .` is
  the drive you are standing on. In `--json`, every cache row gains a `volume` key so a
  consumer can group by drive without deriving a mount table from `path`.

  The narrowing happens last, after every verdict, so it changes what you see and never
  what a figure means: a `cache_max_gb` cap still weighs a manager's whole footprint
  wherever it is, and a cache no registered repository uses does not become unused just
  because the repositories that use it live on another drive. Container engines are left
  out of a filtered report entirely — an engine reports its disk from inside a VM image
  that has no path on this filesystem, so there is no honest drive to file it under, and
  the JSON document omits `containers` rather than emitting an empty list that would
  claim none are installed. `devp caches docker` still has that number.

  `--volume` with `clear`, `docker` or `containers` is a usage error rather than a flag
  that looks accepted and does nothing: the clear commands empty a manager wherever it
  is, and a `clear` that had silently ignored the drive you handed it would be the worst
  possible way to find that out.

- **`a` finishes the configurator in one keystroke.** `devp config wizard` takes every
  remaining safe recommendation and jumps straight to the summary, which still needs its
  own `Enter` before anything is written. It applies exactly what holding `Enter` down
  would have applied — `allow_manifest_rewrite` is left off by both, because it can edit
  a file Git tracks and that stays a deliberate `Space`. `Shift`+`Enter` does the same
  thing on terminals that report the modifier; `a` is the spelling that works
  everywhere, so `a` is the one the footer names. The first screen now says so too:
  hold `Enter` and the whole setup configures itself.

- **`devp trust` now ends with every binary it owns and the SHA-256 of each**, the one
  you are running marked and listed first, each with a VirusTotal lookup URL for that
  exact digest. When a scanner objects to dev-prune, the file it objected to is the one
  on your disk — not the asset on a release page — and until now there was no way to ask
  the tool which bytes those were. The digests are computed locally and the URLs are
  printed rather than fetched: nothing is uploaded anywhere, and a digest the service has
  never seen returns `not found`, which means unscanned rather than clean. On Windows
  `dev-prune.exe` and `devp.exe` share a digest on purpose — one file under two names, so
  a scanner builds one reputation record instead of two — and `devpw.exe`, the
  console-free build the scheduled task runs, is a separate build target that legitimately
  differs. `devp trust --json` carries the same facts under a new `binaries` array with
  `sha256` as a field of its own, so a check can compare it against a published checksum
  without parsing a sentence.

- **Every Windows release now ships a `.zip.contents.sha256`** listing the digest of each
  file *inside* the archive, in `sha256sum` format — `sha256sum -c` in the folder you
  extracted to verifies all three at once. An archive's hash stops meaning anything the
  moment you unpack it, and the unpacked files are the only ones an antivirus ever looks
  at, so there was previously no published answer to "is the `devpw.exe` on my disk the
  one you built?". The same digests come out of `devp trust`, which is the side of the
  comparison you actually control.

### Changed

- **The banner now says what the tool is.** Under the art, two lines: every dependency
  directory on the machine in one command, and the promise that only what a lockfile can
  rebuild is ever deleted — with `devp undo` behind that. The screen every interactive
  command opens with said the name, the version and nothing else, so the first thing a
  new user saw answered neither "what does this do beyond `node_modules`" nor "what
  stops it deleting my work". The version now sits at the right edge under the art
  instead of trailing it, and `PRUNE` carries the weight `DEV` does not.

### Fixed

- **The configurator no longer skips the settings above the cursor.** After an upgrade
  added a setting, `devp config wizard` opened *on* that setting — halfway down the
  list. But the walk only moves downwards and leaves through the finish line at the
  bottom, so holding `Enter` from there never offered a single row above it. Anyone
  configuring the tool the way its own footer suggests was configuring a suffix of it.
  The cursor now starts at the top and the walk reaches everything; the new setting is
  still marked `NEW` when it comes around.

- **The AI skill now stays current on machines that turned auto-setup off.** dev-prune
  exports `SKILL.md` — the file Claude Code and the other agents read to learn its flags
  — and rewrites it after every upgrade. That refresh rode along with the automatic
  setup pass, so turning setup off (`devp config set auto_setup false`, or
  `DEV_PRUNE_NO_AUTO_SETUP`) also froze the skill at whichever version installed it: the
  agent went on describing flags that had been removed two releases ago, confidently,
  because nothing told it otherwise. An upgrade now refreshes the copies that are
  already on disk regardless of that setting, and creates none that were not — a machine
  that never wanted an exported skill still does not get one. `devp doctor` continues to
  report a stale copy, and `devp skill` still rewrites it on demand.

## [1.15.0] - 2026-09-01

### Fixed

- **Anti-virus no longer quarantines the binary.** Sophos deleted `devp.exe` from
  `%APPDATA%\dev-prune\bin` before it could be run once, VirusTotal reported it as a
  trojan, and Microsoft's WinGet validation had already failed it for the same reason.
  These were not false positives about what dev-prune *does* — they were two things it
  genuinely did that no ordinary tool needs to:

  - Setting your `PATH` ran `powershell.exe -EncodedCommand <base64>`, and the script
    inside compiled C# at runtime to call into `user32.dll`. An encoded PowerShell
    command that builds code on the fly to reach a Windows API is, feature for feature,
    what commodity malware loaders do. dev-prune now calls the registry and `user32`
    directly, and **setup and first run no longer start PowerShell at all**.
  - The install one-liner did the same thing from the outside. `install.ps1` — and the
    Windows branch of `install.sh` — also built code at run time, to tell the desktop
    to re-read your `PATH` after writing it. Windows hands a script to the installed
    anti-virus as it runs, so a `iwr … | iex` install could trip a heuristic before it
    had written a single file, and anyone who read the one-liner before pasting it had
    to take the comment's word for what the C# did. Both scripts now reach a documented
    .NET call that broadcasts the same notification.
  - The windowless build the background task runs, `devpw.exe`, was produced *on your
    machine* by reading `dev-prune.exe`, editing a field in its header and writing the
    result out under a new name. A program that writes a modified executable copy of
    itself and then registers it to run on a schedule is the textbook description of a
    dropper. It is now a normal build target that ships in the archive, so nothing is
    generated locally.
  - `devp uninstall` finished the job by leaving a detached shell behind — a hidden
    PowerShell, or a `cmd` line that pings itself to pass the time and then deletes the
    file — to remove the running binary a few seconds after the command exited. That is
    the canonical self-delete; the strings alone score on a static scan, and launching
    either windowless out of an unsigned binary is exactly what a behavioural engine
    watches for. **dev-prune now starts no child process it does not wait for, anywhere.**
    The uninstall renames the locked binary aside — which Windows allows, where deleting
    it does not — hands the remains to Windows for the next restart, and prints whatever
    is left.

  If a scanner already quarantined a copy, restore that specific file — never add a
  folder exclusion — and see
  [Installation Issues §13](docs/troubleshooting/INSTALLATION_ISSUES.md) for what to send
  the vendor as a false-positive report.

- The `%APPDATA%\dev-prune\bin` Defender-exclusion advice in
  `docs/RELEASES_AND_MANUAL_INSTALL.md` contradicted the troubleshooting guide, which
  correctly tells you not to. An exclusion silences that folder for every future
  detection, including real ones; the row now points at `Unblock-File` and at the
  false-positive submission process instead.

### Changed

- **Windows archives now contain three executables**: `dev-prune.exe`, `devp.exe` and
  `devpw.exe`. The one-liner installs all three, and the npm and PyPI packages carry
  them. Nothing about how you use dev-prune changes — `devpw` is only ever run by the
  scheduled task, so it stays out of your way and stops a console window flashing when
  the daemon fires.

- **`devp uninstall` now prints the package-manager command instead of running it for
  you.** If you installed with `cargo install`, `pip` or `npm`, Windows will not let that
  manager delete the binary while it is executing, and the only order that clears the
  manager's own records is to exit first and uninstall second. dev-prune used to schedule
  that command in a background shell. It now tells you the single line to run —
  `cargo uninstall dev-prune`, say — and exits `0`. Everything else about the uninstall
  is unchanged, and on Linux and macOS nothing changes at all: there a package manager
  can remove a binary that is running.

### For contributors

- `devpw` is a third `[[bin]]` target (`src/devpw.rs`), and it gets the GUI subsystem
  from `#![cfg_attr(windows, windows_subsystem = "windows")]` rather than from patching
  a PE header at runtime. `patch_subsystem_to_gui` and `twin_is_current` are gone from
  `src/daemon/windows.rs`; what is left places the shipped file with the same
  hard-link-or-staged-copy every other managed file gets. Cargo cannot scope a `[[bin]]`
  to one platform, so on Linux and macOS `devpw` is a stub that explains itself and exits
  `2` rather than a third seven-megabyte copy of the CLI on `PATH`; `devp uninstall`
  sweeps the name on every platform.

- `src/commands/uninstall.rs` gained `finish_locked_removals`, which renames a locked file
  aside and queues the residue with `MoveFileEx(.., MOVEFILE_DELAY_UNTIL_REBOOT)` — the
  documented mechanism, and one that needs no child process. `spawn_powershell_helper`,
  `spawn_cmd_helper`, `ps_quote`, `schedule_manager_uninstall` and `spawn::system32` are
  gone with it. No code path starts a process that outlives the command that started
  it, and the only shell dev-prune ever launched is no longer launched at all.

- The Windows binaries now carry a filled-in version block — `CompanyName`,
  `LegalCopyright`, `FileDescription`, `Comments` — and an `asInvoker` application
  manifest. Previously there was no manifest at all and most of those fields were blank,
  which is one of the inputs a reputation engine has to judge an unsigned binary on.
- `src/pathenv.rs` talks to `HKCU\Environment` through `advapi32` and broadcasts
  `WM_SETTINGCHANGE` through `SendMessageTimeoutW`, using two features newly enabled on
  the existing `windows-sys` dependency. No new crate, and no interpreter.

## [1.14.0] - 2026-08-31

### Added

- **`devp init --auto`** works out where your repositories are instead of being told, and
  registers every one it finds. Until now a repository only became visible to dev-prune if
  you named its directory or committed in it — so the repositories you cloned months ago
  and never touched again, exactly the ones worth pruning, stayed invisible. `--auto`
  scans three places: the directory each repository you already registered sits in (which
  is how registering one project finds the rest of the workspace around it, wherever that
  workspace lives — a second drive included), the workspace you are standing in, and the
  conventional code folders under your home directory such as `~/Code`, `~/Projects`,
  `~/Documents/GitHub`, `~/source/repos` and `~/go/src`. Try it with `--dry-run` first:

  ```bash
  devp init --auto --dry-run    # what would it register?
  devp init --auto              # register all of it
  ```

- **`auto_discover`** lets the scheduled background pass do that discovery on its own, so
  a repository you clone today is tracked without you running anything. On by default;
  `devp config set auto_discover false` turns it off, and `DEV_PRUNE_NO_AUTO_SETUP=1`
  turns it off along with everything else unattended. It is safe to leave on because
  registering is not pruning — a newly registered repository is still only touched once it
  has been idle past `idle_days`, and only where a lockfile proves every directory can be
  rebuilt, so the worst a wrong guess can do is add a row to `devp status`.

### Changed

- **A bulk scan now honours `ignore.devprune.json` before registering, not just before
  deleting.** Dropping that file into a repository already kept it out of every prune
  pass; it now also keeps the repository out of the registry, so a project you have
  declined stops reappearing in `devp status` after every scan. `devp link <path>` still
  registers such a repository — naming one repository is not a bulk scan — and this is the
  opt-out for the automatic discovery above.

## [1.13.0] - 2026-08-31

### Fixed

- **Saving the `devp config` wizard no longer erases what happened while it was open.**
  The wizard saved the whole registry it loaded when it opened, so a scheduled pass that
  finished while you were choosing settings had its prune history overwritten by the
  wizard's stale copy. The wizard now re-reads the registry at save time and applies only
  the settings you actually changed.

- **A settings key from a newer dev-prune survives an older dev-prune saving.** Every
  save rewrites the whole `settings` object, so one run of an older binary — a pinned CI
  image, a machine `version_lock` holds back — silently erased any configuration it did
  not recognise. Unknown keys are now carried through a save verbatim.

- **A git repository found inside a bloat directory is now a skip, not a failure.** A
  vendored checkout — a pip `-e git+…` install under `.venv/src/`, a `file:` dependency
  cloned into `node_modules` — is a permanent fact of the repository it lives in, but
  the prune pass reported it as a delete error, so every scheduled pass over such a repo
  exited non-zero forever while `--dry-run` over the same repo exited 0. The refusal
  still happens and is still printed; it now reports as "left alone" with the reason,
  the pass exits 0, and `--json` carries it as the new `skipped_nested_repo` status.

- **A confirmed `devp run <repo>` deletes exactly the list you said yes to.** The pass
  after the prompt re-derived its candidates from scratch, so a directory that crossed
  the size floor while the prompt sat open was deleted without ever appearing on the
  list you confirmed. The confirmed list is now passed to the pass verbatim.

- `devp run` in registry mode no longer lists the same refused declaration twice in
  `--json` output — once from the analysis pass and once from the execution pass.

- **`devp run --json` emits its document even when saving the registry fails.** The
  save error used to abort the command before the JSON was written, so a full-disk
  machine got a truncated contract instead of a report ending in the error.

- **`devp update` leaves a Volta, mise, Nix or distribution-packaged copy alone.** The
  direct-download route knew WinGet, Scoop and Homebrew own their package directories,
  but wrote fresh bytes over a copy inside a foreign manager's tree — desyncing a store
  it has no command to resync, or breaking a shim. The managed copy is still upgraded;
  the foreign manager's copy is now left exactly as it wrote it, and the report names
  the manager instead of staying silent. The unattended auto-update skips entirely when
  the foreign copy is the only one on the machine.

- **A failed update download no longer leaves a `.new` staging file beside the
  binary.** The stage is removed on a half-written download, and `devp uninstall`'s
  sweep now knows the `.new` and `.old` names an interrupted update can leave behind.

- **`devp config` (the interactive configurator) no longer reports a change when the
  adapter picker was left exactly as found.** Toggling into the picker and out again
  compared the deny-list against a re-ordered copy of itself and counted that as an
  edit.

- **Ctrl-D in the plain-terminal `devp config` walkthrough no longer counts as a
  review.** Closing the input at the first prompt marked every setting as reviewed;
  it now says "Input ended — nothing was changed" and leaves the review marker alone.

- The Settings screen footer now says `q/Esc cancel` — Esc always cancelled, but the
  footer only admitted to `q`.

- A terminal that fails half-way into raw mode is now restored before `devp config`
  reports the error, instead of leaving the shell in raw mode with no cursor.

- `devp update --install` no longer leaves the other-named twin beside the copy you
  typed on the previous release. It replaces the managed pair and the running file, but
  the list of companions to rewrite only ever named `devp` — so running the update from
  a cargo-installed `devp` upgraded everything except the `dev-prune` sitting right next
  to it, which then reported the old version whenever it was the name invoked. Both
  public names are now rewritten in both directories; a twin that does not exist outside
  the managed directory is still never invented, and a manager-owned directory is still
  left exactly as the manager wrote it.

## [1.12.0] - 2026-08-30

### Added

- **The banner now names the channel this copy came from.** `devp -V`, `devp init`,
  `devp run` and `devp status` print it right after the version — `· cargo`, `· npm`,
  `· bun`, `· pnpm`, `· yarn`, `· uv`, `· pipx`, `· pip`, `· WinGet`, `· Scoop`,
  `· Homebrew` or `· install script`. dev-prune ships through eleven channels and nothing
  stops two of them from leaving a copy on the same machine, so "devp still says 1.9.0
  after I upgraded" was always really the question "which copy am I running, and who owns
  it". That answer is now in the screenshot before anyone has to ask for it. It is read
  from the path of the running executable, so it spawns nothing, reads nothing from disk,
  and does not need the manager it names to still be installed.
  `devp update --channels` still prints the upgrade command for every channel.

- **A binary you downloaded and placed yourself badges as `· standalone`.** Nothing on the
  machine claims that file, so no package manager is going to upgrade it, and saying so is
  more use than guessing at one. `devp update` already handled this case — it offers
  `devp update --install`, which replaces the file in place — and now the banner says it
  before you get that far. A copy inside a tree dev-prune can name but not drive (Nix,
  Volta, mise, asdf, a distribution's own package) badges with that manager's name
  instead, and dev-prune leaves it alone.

- **A copy installed by a node package manager dev-prune has no name for is still
  recognised as npm-family.** Every npm-registry client — npm, pnpm, yarn, bun, and
  whichever one ships next — installs the package into a `node_modules` tree, so any
  copy inside one now badges as npm-family even when the specific manager is unknown,
  and the uninstall sweep prints removal guidance instead of deleting a file some
  manager still tracks. New clients of the npm registry work on the day they appear,
  not the day dev-prune learns their name.

- **`devp doctor` now reports declared directories.** A repository whose only prunable
  space is what `.devprune.json` declares was told "no prunable directories found" — the
  opposite of what `devp run` would do there. Doctor now resolves the declarations with
  the same checks the prune pass runs, so each one prints with its rebuild command and
  whether it currently holds anything, and a declaration the pass would refuse (a
  symlink, a path outside the repository, tracked files inside) says so and why.

- **`devp doctor` points out config keys dev-prune does not read.** A typo'd key in
  `.devprune.json` or `project.devprune.json` — `idle_days` for `override_idle_days`,
  `directores` for `directories` — has always been silently ignored, on purpose: a file
  written by a newer version must not brick an older binary. That tolerance meant nothing
  ever told you the key was doing nothing. Doctor now names each unread key and says to
  check the spelling against the file's `$schema`. A warning only — the file still loads.

### Changed

- **`devp run <path>` now shows what it will delete and asks.** A targeted run used to
  go straight from analysis to deletion; now it lists every directory that would go,
  each with its size and the total, notes that `devp restore` brings it all back, and
  asks `[y/N]` — the same courtesy the registry-wide pass has always paid. `--yes`
  answers for you, `--dry-run` stops at the report, and without a terminal the run
  exits with an error naming `--yes` rather than waiting on a prompt, so scripts that
  relied on the old behaviour need a `-y`. The scheduled pass already runs with
  `--yes` and is unaffected.

- **Finishing the configurator is now just Enter, held down.** In `devp config wizard`,
  Enter takes the recommendation on the row it is on and moves to the next; on the
  Finish line it opens the summary of exactly what will be written, and one more Enter
  writes it. So a fresh install can review every setting, accept all the safe
  recommendations, and finish, pressing nothing but one key — no more choosing between
  reading thirty settings carefully and abandoning the screen. The walk never takes the
  cautious tier: `allow_manifest_rewrite` still requires a deliberate `Space` on its
  own row, and changing anything to a value *other* than the recommendation is still
  `Space`. `q` still leaves without saving, from any screen.

### Fixed

- **`devp` now knows it was installed by the install script.** Both names live side by side
  in the managed directory, but the check that recognised them compared the whole file
  path against the one name the installer records — `dev-prune`. So `dev-prune update`
  answered correctly while `devp update`, the same binary under the name every page of the
  documentation tells you to type, replied "this copy is not in a location any install
  channel owns" to someone who had run `install.ps1` two minutes earlier. Either name in
  that directory is now the install script's copy.

- **`devp uninstall` no longer deletes a pnpm-installed copy behind pnpm's back.** pnpm puts
  the executable on your PATH in `PNPM_HOME` (`~/.local/share/pnpm`, `%LOCALAPPDATA%\pnpm`)
  and the package itself one level deeper, and only the deeper path was recognised — so the
  one copy the stray-copy sweep can actually find looked like a loose file nothing owned,
  and got removed without `pnpm remove -g` ever running. pnpm was left listing a package
  whose file was gone. The sweep now hands it to pnpm.

- **`devp doctor --fix` no longer downgrades `devp`.** Restoring `dev-prune` and `devp` as a
  pair only compared the two files for *difference*, then let `dev-prune` overwrite `devp`
  on the assumption that the canonical name is always the newer of the two. Run an older
  `dev-prune` — out of a backup, or a package-manager cache — and it is not: the repair
  deleted a newer `devp` and put its own older content there, and reported it as a repair.
  It now asks the twin its version and leaves anything that is not actually behind alone.

- **Windows paths in warnings are readable again.** A skipped symlink, a mount point, a
  nested git checkout or a partly-deleted directory printed its path in the verbatim
  `\\?\C:\Users\...` form that `canonicalize` returns, in the middle of a sentence where
  every other path dev-prune prints is clean.

- **A failed self-update no longer leaves a `.new` file behind.** Refreshing the managed
  copy stages the new binary beside the old one and renames it into place, but only cleaned
  the staging file up when the *rename* failed — a copy that died partway through, on a full
  disk or a revoked permission, left a half-written `dev-prune.new` that nothing ever swept.

- **`devp restore --last-run` no longer trips over a declared directory.** A declared
  directory records its rebuild command where a Python project records its interpreter
  version, and the restore batch probed that field as a version — so one declared
  directory in the last pass made restore ask whether "npm run build" was a Python this
  machine had, conclude it was missing, and abort the whole batch. Declared directories
  now skip the interpreter question they were never part of.

- **`devp doctor` no longer says PATH is broken in the shell you ran `setup` from.**
  Adding the install directory to PATH takes effect in terminals opened afterwards, but
  the shell that ran `setup` predates its own change — and doctor, run as the very next
  command, warned that `devp` was not on PATH and suggested the setup that had just
  succeeded. It now checks the persisted PATH too, and says the honest thing: new
  terminals will find it, this one predates the change.

- **A `command_timeout_secs` of `0` no longer fails every verification instantly.**
  `devp config set` refuses the value, but the registry is a JSON file anyone can edit,
  and a zero that got in did not mean "no timeout" — it killed every package-manager
  command the moment it started, which quietly turned every repository into "lockfile
  could not be verified" and pruned nothing. The stored value is now floored at one
  second everywhere a timeout is built from it.

## [1.11.0] - 2026-08-27

### Added

- **`prunable.exclude` keeps one machine's copy of a directory the committed file calls
  rebuildable.** `project.devprune.json` is checked in, so one person's `scratch` is
  everybody's `scratch` — and the teammate whose copy is holding something had no way
  to say so short of editing a file the whole team shares. Now they name it in their
  own `.devprune.json`, which git never sees:

  ```json
  { "prunable": { "exclude": ["scratch"] } }
  ```

  The declaration is vetoed, not deleted, so removing the exclusion later puts the
  directory back in play without anyone re-declaring it. `dist`, `dist/`, `./dist` and
  `dist\` are one path to it, because an exclusion that missed on a trailing slash
  would delete the exact directory it was written to keep. It silences the refusal as
  well as the delete: a directory that is nobody's business here stops being a standing
  complaint on every pass. Honoured from whichever file names it, since a veto can only
  ever delete less.
  [CLI reference](docs/CLI_REFERENCE.md#8-devp-config-action)

- **`devp doctor` names a config file that is not at the repository root.** Every path
  inside `.devprune.json`, `project.devprune.json` and `ignore.devprune.json` is
  relative to the repository root, so all three are read from the root and nowhere else.
  A copy one directory down parses cleanly, looks applied, and is read by nothing — and
  for the two that are git-excluded, the entry that hides the root copy hides a copy at
  any depth, so `git status` will not mention it either. `doctor` now lists any it finds under `Stray
  config`. It names them rather than moving them: moving one up a level would silently
  change what every path inside it means, and that is a decision for whoever wrote the
  paths.

- **`devp doctor` names a path that is declared and excluded in the same file.** Across
  the two files that combination is the entire point of `exclude`. Inside one file it is
  a typo, and its only symptom is a declaration that quietly never runs — the report is
  the one place it can ever surface. A warning, not a problem: `doctor` still exits `0`,
  because nothing here is broken.

### Changed

- **`devp uninstall` now tells each package manager to remove its own copy, instead of
  deleting the file and leaving the manager to find out.** Deleting cargo's binary
  behind its back leaves `.crates.toml` naming something that is gone, and the
  `cargo uninstall dev-prune` the old sweep printed as the follow-up then exits 101 with
  `corrupt metadata, ... does not exist when it should` — without clearing the entry.
  The manager has to be told first or it can never be told at all. Each one is told once
  however many of its files turned up, because `~/.cargo/bin` holds both `dev-prune` and
  `devp` and a second `cargo uninstall` exits 101 too. A manager that is not on `PATH`
  is named and its copy left alone rather than deleted out from under it.

- **On Windows the manager's uninstall of the running binary is scheduled instead of
  attempted.** Windows will not let `cargo uninstall` delete an image that is executing:
  it fails with `Access is denied` and keeps its ledger entry, and renaming the file
  aside first only trades that for `corrupt metadata` — which keeps the entry as well.
  Exiting first is the only order that clears the record, so the command goes to the
  same detached helper that finishes the rest of the uninstall a few seconds after
  `devp` returns. `devp install --channel <name>`, which removes the old copy before
  installing the new one, takes the same route.

### Fixed

- **A copy installed by Deno, Volta, mise, asdf, Nix or your distribution was deleted
  or handed to the wrong manager.** None of those six directories matched a marker,
  and what happened next depended on whether a `python3` happened to sit beside the
  binary. In `~/.deno/bin`, `~/.volta/bin` and `/nix/store` nothing did, so the copy
  looked like a loose file somebody had dropped in and `devp uninstall --yes` removed
  it — no hint printed, and the manager left listing a binary that is gone. In
  `/usr/bin` and in mise's and asdf's shim directories one always does, so the copy
  was read as a pip install instead: `devp update` on the binary your distribution
  packaged would have run `pip install --upgrade dev-prune`. All six are now
  recognised by name, reported, and left exactly where they are, and a tree that says
  whose it is outranks anything that merely sits next to the file. There is
  deliberately no install or upgrade command for any of them: none was on the machine
  this list was written on, and a wrong upgrade command is worse than none.

- **`devp uninstall` now removes the `.old` file an update leaves behind.** Windows
  will not let a running binary be replaced, so `devp update` renames it to
  `devp.exe.old` and lets the channel write a fresh one at the real name. Deleting the
  leftover afterwards is best-effort — it is still the running image — and the sweep
  that finishes the job runs at the *next* update. Someone who updates once and then
  uninstalls never has a next update: on the machine this was found on,
  `~/.cargo/bin/devp.exe.old` was 5.8 MB of dev-prune still sitting there after the
  command that reported dev-prune removed. The sweep looks for that name now too.

### For contributors

- **Every per-channel command lives in `src/channel.rs`.** Installing, upgrading and
  uninstalling dev-prune through one of the twelve channels used to be three separate
  `match channel` blocks in three command modules, and the uninstall arm in one of them
  had already drifted from the sweep in another. They are now `install_argv`,
  `upgrade_argv` and `uninstall_argv` on `Channel`, next to the markers that detect it —
  one file to read to answer what dev-prune does with, say, pnpm, and one file to edit to
  add a channel.

- **[Why `dev-prune` refuses](docs/WHY.md)** is the argument the design came from: what
  an 18% refusal rate on a real 80-repository machine looked like, why a confirmation
  prompt cannot stand in for it on a schedule, and which parts of the tool follow from
  that — the missing `--force`, build outputs staying out of scope, and reading activity
  from `git log` rather than `mtime`.

## [1.10.0] - 2026-08-26

### Added

- **`devp caches` now names the cache that costs a single repository the most.** The
  report is ordered by total size, and the biggest cache is routinely not the one worth
  emptying: a 2 GiB pnpm store that exists for one project is a worse deal than a 10 GiB
  npm cache shared by eighteen. A `Costliest per repository` line under the total ranks
  the top three by that figure, so the arithmetic is done for you rather than left in
  thirteen separate blocks. It is a ranking and not a recommendation — nothing is
  called "too big", because whether it is depends on what you are about to do with the
  machine.

- **The line saying who still needs a cache is now the line you see.** Every block
  prints a path, a clear command, and a sentence like
  `cargo is used by 1 of 46 registered repositories · 207.51 MiB each`. That sentence is
  the only part you make a decision on, and it was set in the same weight as the
  plumbing above it, so finding it meant reading the whole report line by line. It is
  now bold.

- **The configurator tells you why it opened when you did not ask for it.** `devp caches`
  or `devp status` on a fresh install used to hand you a full-screen settings walkthrough
  with no explanation, which reads as the wrong command having run. The first screen now
  says so in as many words: that you did not ask for this, that dev-prune opens it once
  so you see what its defaults do before they start doing it, that whatever you typed
  runs as soon as you leave, and that it will not open by itself again unless an upgrade
  adds a setting. After such an upgrade it says that instead, with the number of new
  settings and a note that nothing else about your configuration changed.

- **The first screen now says what dev-prune is and where a real copy of it comes
  from.** It listed seven things dev-prune will not do without ever saying whose binary
  was promising them — which is the one claim on that screen a reader can go and check
  for themselves. Above the guarantees it now gives the version, the author, the
  repository, the two official download locations, the three package registries and the
  two editor marketplaces dev-prune is published to, and says plainly that anything from
  anywhere else is not a copy the author published. Under them it gives the terms the whole screen is offered on:
  Apache-2.0, sections 7 and 8, no warranty and no liability, and using it accepts that.
  The same block prints in the plain-prompt configurator that a narrow terminal and
  `DEV_PRUNE_NO_TUI` get, so the short path is not also the vague one.

- **`devp config project --team`** writes `project.devprune.json`, the half of a
  repository's settings meant to be committed. `.devprune.json` is added to
  `.git/info/exclude` the moment it is written, which is right for "keep my copy of this
  one repo out of the sweep" and wrong for "nobody prunes this repository" — a decision
  that should reach a fresh clone by itself rather than being something every teammate
  has to be told. The new file is the same shape and the same schema, and deliberately
  not excluded.

  Where both exist, every key the project file names wins, and the personal file answers
  everything it does not — so a colleague's stale local answer cannot quietly overrule
  what the project decided, and a personal override still works on every setting the
  team left open. "Names a key" means the key is literally in the file: a project file
  that never mentions `ignore` does not un-ignore your repository. It is created holding
  nothing but its `$schema` line for exactly that reason. Run `devp config project` in a
  repository that has both and it prints which file each effective value came from.

  Nothing dev-prune writes for you touches it. `devp config --update`, the workspace
  toggles and `[i]` in the dashboard all still write `.devprune.json` only, so no local
  action of yours turns into a change on a branch your colleagues share. `devp doctor`
  reports a `project.devprune.json` that does not parse and leaves it alone; `--fix`
  repairs the personal file by renaming it aside, and doing that to a tracked file would
  be an unexplained working-tree change.

- **A repository can declare its own prunable directories.** Either config file can
  carry a `prunable.directories` list, and each entry is a path plus the `rebuild`
  command that puts it back:

  ```json
  {
    "prunable": {
      "directories": [
        {
          "path": "tools/vendor",
          "rebuild": "make vendor",
          "why": "regenerated from tools/manifest.toml"
        }
      ]
    }
  }
  ```

  Until now dev-prune could only delete what an adapter recognised, which meant a
  generated fixture set or a vendored toolchain sat there taking gigabytes because no
  lockfile happened to describe it. These go through the ordinary pass under the adapter
  name `declared`: `devp status` lists them, `--dry-run`, `--min-size` and
  `--only declared` all apply, and the scheduled run takes them with everything else.

  `rebuild` is required, and required is the point — an optional one would have made
  "delete this, I have no idea how to get it back" the easiest thing to write in a file
  that gets committed and cloned. When a directory genuinely needs nothing to come back,
  say that: `"rebuild": "echo not needed"` is a legal answer and works on Windows too.
  dev-prune shows the command next to the directory and never runs it.

  Because `project.devprune.json` is committed, a declaration is treated as a claim to be
  checked rather than an instruction to be followed — a repository you cloned this
  morning can say anything, and the pass that acts on it may be a scheduled one with
  nobody watching. Before deleting, dev-prune requires that the path is relative with no
  `..`, that it resolves inside the repository even through a symlinked parent, that Git
  is tracking nothing inside it, and that the first word of `rebuild` is a program this
  machine has. A claim that fails any of those is printed with the reason, reported in
  `--json` as the new `skipped_declaration` status, and nothing is deleted. It is not
  counted in `summary.errors`: nothing was attempted, so nothing failed.

  This is the one part of either config file where the two lists **add up** rather than
  one winning. A list is not a decision, so a team declaration never discards one you
  wrote yourself; naming the same path in both leaves one directory, rebuilt by the
  committed command.

- **`devp update --channels`** prints the upgrade command for every channel dev-prune
  ships through, not only the one that owns the copy in front of you. It reads nothing,
  writes nothing and opens no connection, which is the point: the machine with the stale
  copy is usually not the machine you are sitting at, and the answer to "what do I type
  to upgrade a dev-prune installed through pnpm" does not depend on what the latest
  release is. `devp update` still names one command — the right one — and now says this
  flag exists underneath it.

- **bun, pnpm and Yarn are install channels of their own.** `bun add -g dev-prune`,
  `pnpm add -g dev-prune` and `yarn global add dev-prune` install the same npm package
  npm does, and dev-prune now recognises each by the client's own global directory rather
  than lumping all four together. `devp update`, `devp doctor` and `devp uninstall` name
  the client that actually owns the copy, and `devp install --channel bun|pnpm|yarn`
  moves an installation onto one. Deno is deliberately not a channel: `deno install -g
  npm:dev-prune` writes a shim that re-enters Deno, so the running executable is Deno
  itself and there is nothing to recognise.

- **`devp config recommended`** turns on everything the first run recommends, in one
  command, without the first run. The eight adapters and build trees that are off by
  default because they are not universally wanted — `enable_cargo`, `enable_gradle`,
  `enable_maven`, `enable_swift`, `enable_dart`, `enable_mix_build`, `enable_vcpkg`,
  `enable_cmake_build` — used to be reachable only by walking the configurator, which is
  the wrong price for "yes, all of it", and impossible from a script or a fresh machine
  set up by an agent. It reads the same table the configurator reads, so the shortcut
  and the walkthrough cannot drift apart, and it never marks the settings as reviewed:
  the screen that explains what dev-prune is is still owed to you, and still opens.

- **A second recommendation tier, for the one that comes with something to know first.**
  `allow_manifest_rewrite` lets `cargo` and `go` tidy up `Cargo.lock` and `go.mod` while
  restoring — files Git tracks, so the next `git status` can show a change you did not
  make by hand. It is worth having and it is worth understanding first, so it now sits
  in a tier of its own called **Recommended, with one thing to know first**, printed
  with its reason wherever recommendations are printed, and applied only when you type
  `devp config recommended --with-cautious`. A command nobody passed a flag to is not
  the thing that starts editing your working tree.

- **`devp config show` ends with what you have not taken yet.** The recommendations
  existed only on the first-run screen, so a machine that had already been through it
  had no way left to find out that a recommendation existed at all, let alone that one
  of them carried a caveat. Both tiers are now listed, with the command that applies
  them. Nothing outstanding prints nothing.

- **`devp config set language <code>` prints dev-prune's own headings in one of twelve
  languages.** English, Simplified Chinese, and then Hindi, Telugu, Tamil, Kannada,
  Malayalam, Bengali, Marathi, Gujarati, Punjabi and Sanskrit. What moves is the chrome — section headings,
  summary lines, and the group titles in the configurator, the words that repeat on
  every run. What deliberately never moves is everything a script or a bug report reads:
  `--json`, exit codes, flag and subcommand names, config keys, adapter names, and the
  lockfile refusals that get pasted into issues. A translated interface therefore cannot
  change what a pipeline sees, and a refusal stays readable to whoever upstream has to
  fix it.

  `DEV_PRUNE_LANG=te devp run` overrides the setting for one command. The operating
  system's locale is deliberately never consulted: a machine set to French does not
  start printing French at somebody who has spent a year reading the English. An
  unrecognised code falls back to English at runtime rather than refusing to prune, but
  `devp config set language` rejects it outright and lists what exists — that is the one
  place a typo can still be fixed.

  English is the only catalogue a native speaker has reviewed, and `devp config set
  language` says so when the one you picked has not been. A catalogue is a single JSON
  file in `src/i18n/locales/`, compiled into the binary, so adding a thirteenth language is
  that file plus one line of Rust and fixing a wrong sentence in an existing one touches
  no code at all. Both are written up in
  [Translating dev-prune](docs/TRANSLATIONS.md).

- **`/plugin marketplace add Life-Experimentalist/dev-prune` installs the dev-prune skill
  into Claude Code, before dev-prune itself is on the machine.** Then
  `/plugin install dev-prune@dev-prune`. This repository is now a Claude Code plugin
  marketplace as well as a repository, which is a `.claude-plugin/marketplace.json` and
  nothing more: there is no submission, no account and no review queue, so the plugin is
  installable the moment the file is on `main` and `/plugin update` picks up a change the
  moment one lands.

  Every other editor gets its rules written by the binary, which means installing
  dev-prune first. This is the one that can go in the useful order instead: the agent
  reads how dev-prune works, and then installs it. What arrives is one skill and nothing
  else — no hooks, no MCP server, no agents, no commands — costing about 110 tokens a
  session for its name and description, and about 20k only on the turns it actually
  fires. It is the same `SKILL.md` the binary embeds and `devp skill` exports, pointed at
  rather than copied, so the two cannot drift.

  If you have `devp` installed as well you now have that skill twice, which is not a
  collision but is a choice: `devp skill` re-exports the version you have installed,
  while the plugin follows `main`. [IDE & editor integration](docs/IDE_INTEGRATION.md#claude-code-the-plugin-marketplace)
  covers which to keep.

### Changed

- **The configurator now ends on a Finish line, and finishing takes two presses of
  Enter.** Every way out of it used to be a single keypress — `y` on the declaration,
  `y` on the suggestions, `y` anywhere in the settings list — and two of those three
  wrote your configuration without ever showing you what was about to be written. One
  Enter is also exactly what somebody presses to dismiss a screen they have stopped
  reading. The list now ends in a visible **Finish — review the changes** line, two
  presses of Enter there open the summary, and the summary is now the only exit that
  saves anything. The `y` shortcuts are gone rather than rebound. `q` still leaves
  without saving, from any screen.

  The line-by-line form (`--no-tui`, `DEV_PRUNE_NO_TUI=1`) got the same gesture and the
  same summary: `Keep all of these? [Y/n]` is now two empty lines, and a walk that
  changed anything prints every change as `key  old → new` and asks again before
  writing. It reads each value back after setting it, so a value the setter normalised
  is reported as what will actually be stored rather than as what you typed.

- **`devp config wizard` and `devp config show` now ask in seven groups rather than one
  list of thirty.** The groups are the order the decisions actually arrive in: the
  language the rest of the screen is printed in, what is in scope, what has to be proved before a delete, the build trees that stay off until
  they are asked for, the shared caches nothing deletes on its own, then what may run
  when nobody typed anything, and last whether this copy keeps itself current. A setting
  is now filed by what it does instead of by where there happened to be room for it, and
  the same seven groups appear in `README.md`, the
  [CLI reference](docs/CLI_REFERENCE.md#8-devp-config-action) and `llms.txt`, so "the
  third thing on the build-trees screen" means one thing everywhere.

- **Three settings now say what they actually do.**
  `command_timeout_secs` said "how long a lockfile command may run", which read as though
  it might also cap a Cargo or Gradle recompile. It does not: it bounds the lockfile check
  before a delete and the reinstall `devp restore` performs, the opt-in build adapters run
  no command at all during a prune, and the one place a compile happens under it is a
  restore whose install builds a native module.
  `auto_hooks_chain` said what it does but not why it is off — `core.hooksPath` is a
  single slot, global to the machine, and taking it rewires husky, pre-commit or lefthook
  for every repository you have.
  `enable_mix_build` said "Mix build-tree adapter", which tells somebody who has never
  met Elixir nothing; it now names the language, and says how `_build/` differs from the
  `deps/` the always-on `mix` adapter claims.

- **The published packages now name every ecosystem dev-prune supports.** The keywords on
  crates.io, npm, PyPI and the VS Code marketplace, and the topics on the GitHub
  repository, were all written when this handled npm and pip. It ships twenty-three
  adapters across twelve language groups, and a Go, PHP, Ruby, Swift, Elixir, Dart or C++
  developer searching any of those registries for a cleaner found nothing — not because
  the support was missing but because nothing in the index mentioned it. PyPI also gains
  the two classifiers that were true and unstated: this finds its work by walking Git
  repositories, and emptying a shared cache on a build machine is systems administration.

- **The configurator now shows what each setting ships as, and which ones to turn on.**
  A row said what a setting did and what it was currently set to, which leaves "what
  happens if I just close this" unanswerable without leaving the screen. Every row now
  carries its shipped default alongside the current value, so a changed setting is
  visible as changed.

  Nine settings carry a `REC` badge: the eight build-tree adapters that are off until
  asked for — Rust, Gradle, Maven, Swift, Dart, Elixir, vcpkg and CMake — and the one
  that lets `cargo` and `go` tidy a manifest. The eight arrive already accepted, because
  a first-time reader has no way to tell which of thirty switches matter and the
  answer should not depend on guessing. The ninth does not, and `[a]` will not accept it
  either: it is the one you were told to read about first, and a shortcut that accepts
  that on your behalf is a trap rather than a convenience.

  They are suggestions and the screen says so — every one of them can stay off and
  nothing stops working, they are the settings that make the rest of the tool earn its
  keep. The recommendation stays visible in the settings list on every later run, not
  just on the screen that first offered it; "what did the author think this should be"
  is not a question that expires.

- **The editor-extension offer now recognises the VS Code forks people actually use.**
  It asked VS Code, VSCodium, Cursor, Windsurf, Positron and Kiro for their versions;
  Antigravity and Trae are VS Code builds with a different name on the window, and got
  nothing. Both are now asked too. A fork whose own registry does not carry the
  extension still gets it, from the `.vsix` on the extension's own release that every
  registry copy is built from.

- **The VS Code extension is released separately from the CLI, on its own tags.** Both
  used to ship on one `v*` tag, which was the wrong cadence in both directions: the CLI
  releases often and the extension rarely, so most releases republished an identical
  `.vsix` over itself — and when the extension did need a fix, shipping it meant cutting
  a CLI release with nothing in it. It now has its own version, its own changelog and
  its own release page, tagged `vscode-v<version>`. Nothing changes for installing it:
  it is still **VKrishna04.dev-prune** on the
  [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=VKrishna04.dev-prune)
  and on [Open VSX](https://open-vsx.org/extension/VKrishna04/dev-prune), still attested
  the same way, and `devp setup` still falls back to the `.vsix` for a fork whose
  registry does not carry it. What changes is that a CLI release page no longer carries
  one, and that the release marked *latest* — the one `devp update` reads — is always
  the CLI.

### Fixed

- **An unattended first run no longer spends the declaration screen.** `devp` on a
  schedule, in CI or inside a container has nobody to show the walkthrough to, and
  correctly skipped it — but it also wrote the marker recording that the walkthrough had
  been shown. On any machine where something automated ran first, the one screen saying
  what dev-prune will not delete was consumed by a process with no screen, and the person
  who installed it never saw it. The marker is now written only when somebody was
  actually there to read it.

- **A repository you have just created with `git init` now shows up.** The three Git
  hooks dev-prune installs all fire *after* an operation a brand-new repository has not
  performed yet, and Git has no `post-init` hook to add — so between `git init` and the
  first commit the repository was invisible, and `devp status`, run to check exactly
  that, reported nothing. `devp status` and `devp run` now register the repository you
  are standing in and say so, under precisely the rules the hook applies: a throwaway
  checkout or a repository whose `.devprune.json` sets `disable_hooks` is still left
  alone.

- **Re-running the install one-liner now wins your PATH instead of reporting that it
  did.** The installer added its directory to PATH only when that directory was missing
  altogether. On a machine where a second copy had arrived from another package manager
  since, the entry was already present — behind the newcomer — so nothing moved, the
  older binary went on answering `devp --version`, and the script printed "this directory
  comes first on PATH" anyway. It now moves its directory to the front of both your
  session PATH and your persisted PATH, and instead of asserting an order it names the
  file that actually answers.

- **`devp install --channel installer` now removes the other copies rather than saying
  there is nothing to move.** When the installer found a copy owned by another manager it
  offered to migrate it, and ran that command against the *old* binary — which, on
  anything before 1.8.0, has no `install` subcommand at all. The offer therefore failed
  on exactly the machines it existed for. The copy just installed does the work now:
  asked for the channel it already came from, it lists every other copy on the machine
  beside the uninstall command of the manager that owns it, and runs them once you
  confirm. `devp config set version_lock true` outranks it, as it outranks every other
  path that could change which version answers.

- `llms.txt` said dev-prune had twenty-six settings while listing twenty-nine of them.
  The list was right and the number was three releases stale, which is exactly the kind
  of error a model repeats confidently.

- **A dev-prune installed with bun, pnpm or Yarn was upgraded and removed with npm.** The
  npm package is a dispatcher plus one binary package per platform, so a global install
  through any of the four clients ends up with the executable inside a `node_modules`
  tree — which was the only thing the channel detector looked for. `devp update` then
  offered `npm install -g dev-prune@latest`, which does not upgrade the copy you have: it
  installs a *second* one under npm's prefix and leaves the first, still on `PATH` and
  still owned by bun, at the old version. `devp uninstall` had the mirror of the same
  problem, telling npm to remove something npm never installed. Each client is now
  checked by its own global directory before the shared `node_modules` fingerprint is
  considered.

- **`devp install --channel uv` reported success without installing anything.** It ran
  `uv tool install dev-prune`, and uv answers that with "already installed" and exit `0`
  when it has any version of the tool — so moving a channel onto uv from an older copy
  looked like it had worked and left the old version in place. It now installs
  `dev-prune@latest`, which is what the command was always meant to say.

- **Two lists of install channels were typed out by hand, and both were wrong.**
  `devp install` with no `--channel` named eight destinations of the eleven it accepts,
  and `devp update` on an unrecognised copy named five upgrade commands out of twelve
  channels — including, on the machine that prompted this, not the one that had
  actually installed it. Both lists are now generated from the same tables the commands
  parse and dispatch on, so neither can name a channel that does not exist or omit one
  that does.

- **The quickstart the installers print now says how to reclaim the space.** Its four
  lines offered `devp status` to "see what is reclaimable" and `devp run --dry-run` to
  "preview a prune pass", which are the same sentence to somebody who has not used the
  tool yet, and between them never said what to type to actually get the gigabytes back.
  The second line is now `devp run`, which shows the same plan and asks before it deletes
  anything.

## [1.9.0] - 2026-08-25

### Added

- **`devp caches docker`** reports what the container engine on this machine is holding —
  images, containers, local volumes and build cache, each with a count, a size, and how
  much of that size the engine itself believes it could give back — and then prints the
  commands that would give it back, narrowest first. `devp caches podman` and
  `devp caches nerdctl` are the same report for those engines, and
  [`devp caches containers`](docs/CLI_REFERENCE.md#devp-caches-docker--devp-caches-podman--devp-caches-containers-engine)
  does every engine installed at once. **It is read-only, permanently.** dev-prune deletes
  only what a lockfile proves it can rebuild, and nothing here clears that bar: an image's
  registry tag can be retagged or deleted, the Dockerfile that built it may not be on this
  disk, and a named volume is the one thing on the machine that is not reproducible at
  all. So the prune commands are printed for you to run, with or without `--yes`.

  The figures come from the engine's own `system df` rather than from a directory walk. On
  Docker Desktop and Podman the store lives inside a VM disk image the host cannot see and
  `~/.docker` is configuration rather than data, so a size taken off the filesystem would
  be wrong by orders of magnitude, in the reassuring direction. Asking the engine is also
  the only way to learn what is *reclaimable*, which is the figure that decides anything:
  40 GB of images with 38 GB dangling is a different situation from 40 GB with 2 GB
  dangling. An engine that is installed with its daemon stopped is reported as exactly
  that, in the engine's own words, rather than as an absence.

- **A container-engine line at the foot of `devp caches`**, one per engine installed, so
  the figure is in front of you without your having to go looking for it. The mistake it
  exists to prevent is someone clearing 6 GiB of npm cache while a Docker install nobody
  has opened in a year sits on 40 GiB. Container disk is **not** in the cache total above
  it and never will be, for the reason above, and `devp caches clear docker` is a usage
  error (exit `2`) that says so and points at the detailed report rather than claiming
  Docker is not a manager dev-prune knows.

- **Local Kubernetes clusters are listed by name** by `devp caches containers`, and
  deliberately not sized. kind, k3d and minikube run their nodes as containers, or as a VM
  disk belonging to an engine already in the table, so their disk is counted there — a
  figure beside the cluster name would be the same gigabytes twice. Delete one with its
  own tool (`kind delete cluster`, `minikube delete`, `k3d cluster delete`), which is what
  actually releases the space. The list is read out of your kubeconfig with
  `kubectl config get-contexts`, which contacts nothing: a context pointing at a
  production cluster is filtered out by name here rather than by being dialled.

- **`devp caches --json` carries a `containers` array**, and
  [`devp caches containers --json`](docs/CLI_REFERENCE.md#devp-caches-containers---json)
  is a document of its own with `engines[]`, `kubernetes_contexts[]` and a `summary`. An
  engine that is installed but did not answer appears with `available: false` and a
  `reason` rather than with zeros, because a missing `total_bytes` read as zero is the
  difference between “Docker is holding nothing” and “dev-prune could not find
  out”. Container bytes sit outside `summary.total_bytes` in the `caches` document on
  purpose, so a consumer adding up what `devp caches` could free cannot pick them up, and
  neither document carries a prune command anywhere: no field in this contract should be
  one substitution away from an argv for `docker system prune --volumes`.

- **`devp stats` now counts what `devp caches clear` gave back**, on a **Caches emptied**
  line of its own and as `lifetime.cache_bytes_freed` in
  [`devp stats --json`](docs/CLI_REFERENCE.md#devp-stats---json). Emptying a 6 GiB npm
  cache used to print `Freed 6.00 GiB.` and then vanish from the only report that claims
  to say what this tool has done for you. It is deliberately a second figure rather than
  part of the first, because the two do not cost the same to undo: what pruning frees
  costs one reinstall in one repository, and what emptying a shared cache frees costs a
  download in every project on the disk. Counting starts at 1.9.0, so the line reads zero
  until your first clear after upgrading — it is not a number that could be
  reconstructed from anything already on the machine.

- **`container_disk` is now a row in
  [`devp trust`](docs/CLI_REFERENCE.md#18-devp-trust---json---fix-ownership)** —
  reported, never deleted. It sits beside the no-telemetry and never-touch-build-outputs
  rows, which are the other two things people ask about that are promises rather than
  safety invariants.

- **The install scripts now leave a receipt beside the binary they installed.**
  `install.json`, in the same directory, records the version, which of `install.sh` and
  `install.ps1` wrote it, when, whether the `devp` alias was written and whether the PATH
  entry was made. [`devp doctor`](docs/CLI_REFERENCE.md#12-devp-doctor-path---fix) reads
  it back as an **Install receipt** line — `v1.9.0 by install.sh on 2026-08-25` —
  and `devp install` prints the same line at the end of a run. Three separate pieces of
  code used to work those facts out independently, which is how they drift; now the run
  that did the work writes them down once, and `devp update --install` updates the version
  in an existing receipt rather than writing a new one, so a receipt never claims an
  installer ran when none did. It is a record and never a setting: `--channel` still
  classifies a copy by where its file is, a missing receipt is not an error, and the line
  is shown only for the copy the receipt actually describes, because a date belonging to a
  different file is worse than no date. The
  [field-by-field shape](docs/CLI_REFERENCE.md#19-devp-install---channel-name---dry-run)
  is in the reference.

- **The installers now offer to collapse a duplicate copy instead of only naming one.**
  When `install.sh` or `install.ps1` finds a dev-prune that a different package manager
  owns — a `cargo install` copy, a Homebrew one, a WinGet one — it says so and asks
  whether to move it over. Answer `y` and *that* copy runs
  `devp install --channel installer --yes` itself: it is the one that knows which manager
  owns it, so it installs here and uninstalls there through that manager, which is the
  only way the hand-off can be made correctly. Answer anything else and it prints the
  command and leaves both copies exactly where they were. Neither script deletes another
  manager's file either way. The question is skipped wherever there is nobody to answer it
  — `CI` is set, there is no terminal, or `DEV_PRUNE_NO_MIGRATE_PROMPT=1` — and the
  command is printed instead, so piping the one-liner into a shell from a provisioning
  script behaves exactly as it did before.

### Fixed

- **`npx dev-prune` on Windows now explains a broken install instead of printing a Node
  stack trace.** When the loader refuses the binary — a package downloaded for the
  wrong architecture, or a truncated file — Windows throws out of `spawn` itself rather
  than emitting an `'error'` event, so the message written for exactly that case never
  ran. It runs now, from both paths, and names the file it could not launch and what to do
  about it.

## [1.8.0] - 2026-08-24

### Added

- **`devp caches` now says who still needs each cache.** Beside every package manager
  it reports how many of your registered repositories actually use it, and what its
  cache works out to per repository — two repositories sharing a 12 GiB cache is 6 GiB
  each and worth a look, forty sharing the same 12 GiB is 300 MiB each and is the cache
  doing its job. A manager that **no** registered repository uses is the one case where
  a count is enough to act on: everything in it was downloaded for projects that are not
  on this disk any more, so the new
  [`devp caches clear --unused all`](docs/CLI_REFERENCE.md#devp-caches-clear-manager---over-cap---unused---dry-run---yes---json)
  costs no re-download for anything you still have. The count deliberately ignores
  whether an adapter is enabled or opted in — the question is which managers your
  projects *use*, not which ones a prune pass would touch, and a machine full of Rust
  with `enable_cargo` off must not report the cargo cache as needed by nobody. It is
  shown only for the managers that are also adapter names; `pip`, `conda`, `nuget`,
  `conan` and `hex` get no number rather than a guess that `venv` feeds `pip`. With no
  registered repository on disk nothing is counted at all and `--unused` refuses to run,
  because every cache would otherwise look unused. In `--json`, `dependents` appears on a
  cache row only where there was something to count, and `summary.registered_repositories`
  carries the denominator.

- **`devp caches` now finds the pnpm store on the drive your projects are actually on.**
  pnpm hardlinks its store into every `node_modules` it fills, and a hardlink cannot
  cross a filesystem — so a project kept off the system disk gets a store of its own at
  the root of *that* filesystem: `V:\.pnpm-store` on a second Windows drive,
  `/mnt/data/.pnpm-store` on Linux, `/Volumes/Work/.pnpm-store` on an external macOS
  volume. It is not a Windows idea; it is wherever a developer keeps code off the system
  disk. `pnpm store path` only ever answers for the filesystem it is run on, and
  `devp caches` asks from your home directory — so a machine whose code lives on another
  drive was shown the small store beside the home directory and never the multi-gigabyte
  one holding its projects. The report now looks at the root of every filesystem that
  holds a registered repository, plus the one you are standing in, and gives each store a
  row of its own. `pnpm store prune` acts on the filesystem it is run on too, so that row
  names the store in the command it prints — `pnpm store prune --store-dir <path>` — and
  runs exactly what it printed. `devp caches clear pnpm` empties every pnpm store found,
  and a `cache_max_gb` cap for `pnpm` is measured against all of them together.

- **`devp caches` now reports conda's package cache.** A conda installation keeps every
  package it has ever unpacked, plus the archive it came from, under `pkgs/` inside
  itself — and keeps them after the environment that pulled them in is gone, which is how
  a machine that has not touched conda in a year still has several gigabytes of it. The
  report finds it at the conventional roots (`~/miniconda3`, `~/anaconda3`,
  `~/miniforge3`, `~/mambaforge`, and the `~/.conda/pkgs` conda falls back to when the
  installation is not writable), at `CONDA_PKGS_DIRS`, and — for a conda installed
  somewhere else entirely — at the root that `CONDA_EXE` names, so a shell that can run
  conda is enough to find its cache. `devp caches clear conda` runs
  `conda clean --packages --tarballs --yes`, which is conda's own command and keeps
  whatever its environments still reference. One caveat is conda's, and the row repeats
  it: that check follows hardlinks and not symlinks, so an environment built with
  symlinked packages can be broken by it. Unlike Maven's local repository, such an
  environment reinstalls from the channel it came from — which is why this is a note on
  the row and not a refusal.

- **`devp skill --agent aider`** writes the rules into `CONVENTIONS.md`, the file Aider
  reads project conventions from — the sixteenth editor the command supports, and the
  first one where writing the file is not the whole job. Aider does not pick
  `CONVENTIONS.md` up by finding it: it loads only when you pass
  `aider --read CONVENTIONS.md`, run `/read CONVENTIONS.md` in a chat, or put
  `read: CONVENTIONS.md` in `.aider.conf.yml`. So the command prints that line after
  writing the file, rather than leaving a repository that looks configured and is not.
  As with the other shared files, dev-prune owns only what sits between its
  `<!-- dev-prune:rules:start -->` and `<!-- dev-prune:rules:end -->` markers, so your
  own conventions can live in the same file and a re-run leaves every byte of them
  alone. See
  [`docs/IDE_INTEGRATION.md`](docs/IDE_INTEGRATION.md#ai-agent-rules-devp-skill---agent-editor).

- **`devp config set cache_max_gb uv=10,npm=10`** writes down how big a package cache is
  allowed to get, per manager, in gibibytes. A download cache is a bet that
  re-downloading costs less than the disk it occupies, and somewhere that bet stops
  paying — a `uv` cache past ten gigabytes is mostly wheels for versions nothing resolves
  to any more. A manager over its ceiling is now marked in `devp caches`, measured
  against that manager's *whole* footprint, so cargo's registry cache and its unpacked
  sources are weighed together instead of judged a row at a time. **Setting a cap deletes
  nothing.** It marks, and the new
  [`devp caches clear --over-cap all`](docs/CLI_REFERENCE.md#devp-caches-clear-manager---over-cap---unused---dry-run---yes---json)
  empties exactly what is marked, when you type it — the promise that no schedule, hook
  or prune pass ever touches a cache is unchanged. Empty by default: no cache is too big
  until you say what too big is. The keys are the names `devp caches clear` takes
  (`npm`, `pnpm`, `uv`, `pip`, `cargo`, `go`, `nuget`, …), not adapter names, and
  `devp config set cache_max_gb -` clears the map. In `--json`, `cap_gb` and `over_cap`
  appear on a cache row only where a cap is set, so a report from a machine with no caps
  is byte-for-byte what 1.7.0 printed.

- **`devp config set enable_vcpkg true`** adds C and C++ projects that build their
  dependencies with vcpkg. A repository holding a `vcpkg.json` gets a `vcpkg_installed/`
  directory beside it, full of the headers and static libraries vcpkg produced for that
  one project — anything that pulls in Boost or Qt measures in gigabytes. It is opt-in
  for the same reason `enable_cargo` and `enable_gradle` are, and waits for
  `build_idle_days` (45) rather than `idle_days`: vcpkg builds every port from source, so
  `vcpkg install` puts that directory back by compiling it again rather than by
  downloading it. Only `vcpkg_installed/` is ever claimed — the `build/` sitting next to
  it is CMake's, and a directory name alone never says whose it is — and the manifest has
  to declare a non-empty `dependencies` list before dev-prune accepts it as proof, because
  every vcpkg *port* ships a `vcpkg.json` of its own and a port manifest rebuilds nothing.
  Only manifest mode is an adapter's business; vcpkg's classic mode installs into one tree
  beside vcpkg itself that every project on the machine shares, and `devp caches` has
  reported that one all along. See
  [`docs/CLI_REFERENCE.md`](docs/CLI_REFERENCE.md#8-devp-config-action).

- **`devp config set enable_cmake_build true`** cleans the build trees CMake
  configured, and answers the question that has kept C and C++ builds out of scope:
  whose `build/` is that? Never the directory's name — `cmake` writes a
  `CMakeCache.txt` at the top of every tree it configures, nobody writes one by hand,
  and that file records `CMAKE_HOME_DIRECTORY`, the source directory the tree was
  configured from. A directory is claimed only when it carries that cache *and* the
  source directory the cache names still exists, still holds a `CMakeLists.txt`, and
  sits inside the same repository. A `build/` you filled with your own artefacts has no
  cache file and is never touched. Because the proof is the cache rather than the name,
  the adapter finds `build/`, CLion's `cmake-build-debug/` and Visual Studio's
  `out/build/<preset>/` alike — it looks three levels down, stepping only past
  directories that hold a handful of subdirectories and nothing else, which is what an
  out-of-source container looks like and what a dependency tree never does. The search
  stops descending at the first cache it finds, so the sub-builds `FetchContent` and CPM
  leave under `build/_deps/` are deleted with the tree that configured them rather than
  counted twice. Opt-in like `enable_cargo`, and gated by `build_idle_days` (45) rather
  than `idle_days`: a build tree is object files and linked binaries, and
  `cmake -S . -B build && cmake --build build` puts it back by compiling, which for a C++
  project of any size is the most expensive rebuild dev-prune can ask for. See
  [`docs/CLI_REFERENCE.md`](docs/CLI_REFERENCE.md#8-devp-config-action).

- **`cargo install dev-prune` now installs `devp` too.** It only ever built one
  executable; the second name — the one every page of the documentation uses — arrived
  afterwards, as a copy the binary made of itself on its first run. `devp` is now a real
  build target, so cargo installs both names the way npm, PyPI and the release archives
  already ship both.

- **`devp config set version_lock true`** pins this copy to the version it is, and
  outranks every other way dev-prune can replace its own binary. `auto_update` does not
  run however it is set, `devp update --install` refuses, `devp install --channel`
  refuses because moving channels installs the latest release, and re-running the
  `install.sh` / `install.ps1` one-liner leaves the binary exactly where it found it.
  There is no flag that bypasses it and no environment variable that quietly wins:
  releasing the pin is `devp config set version_lock false`, typed by the same person who
  set it. `auto_update false` was never the whole answer — it stops one path, and a
  machine that has to keep shipping the same tool for a year (a CI image, a reproduction
  that stops reproducing the moment the tool changes underneath it, a locked-down build
  box) also has to survive somebody re-running the install one-liner out of habit. The
  pin never goes quiet about itself: every path that stands down says so and says how to
  release it, `devp update` prints the pin where it would have printed the upgrade
  command, and `devp doctor` reports it as a note above the release check rather than
  warning that you are behind — being behind is the state that was asked for.

  ```console
  $ devp update
  -> `version_lock` is on, so this copy stays at v1.8.0. `devp config set version_lock false` releases it.
  ```

- **dev-prune now notices when it has been installed inside a project's own virtual
  environment**, and says so on its first run from there. `pip install dev-prune` with a
  project activated puts the tool in that project's `site-packages`, where it becomes a
  package the project's `requirements.txt` does not account for — so lockfile
  pre-verification declines to prune that environment, and the one venv you installed
  into is the one venv a prune pass will not touch. The message names the situation while
  the cause is still fresh: where this copy is running from, which environment it is in,
  which project that environment belongs to, and the two repairs. Move it out —
  `pip uninstall dev-prune`, then `uv tool install dev-prune` — and if a copy already
  exists elsewhere on the machine it names that path too, so you can see that removing
  this one still leaves a working `devp`. Or keep it deliberately: answer `y` and the pin
  is appended to `requirements.txt`, which makes it an ordinary recorded dependency and
  the environment prunable again. The prompt defaults to no, appears only with a person
  at the terminal, is asked once per installed version, and is skipped entirely when
  `requirements.txt` already lists the tool — a project that depends on dev-prune on
  purpose is not making a mistake. Dismiss it and the prune pass now names the same
  situation and prints the same two commands when it declines, rather than the generic
  "record these with `pip freeze`", which is the wrong repair for a tool that ended up
  somewhere it should not be. **The refusal itself is unchanged**: an unrecorded package
  is still an unrecorded package, nothing is deleted, and there is no flag that relaxes
  it. See
  [`docs/troubleshooting/INSTALLATION_ISSUES.md`](docs/troubleshooting/INSTALLATION_ISSUES.md#9-pip-install-in-a-virtual-environment--what-happens-when-the-venv-goes-away).

### Changed

- **`devp config wizard`'s adapter checklist gained a third column.** It already showed
  whether an adapter is on and how many days it waits; each row now carries that
  adapter's cache cap beside them, so one screen answers what is on, for how long, and
  how big. `d` sets the idle window as before, `c` sets the cap, and either one typed on
  a language heading applies to every adapter under that heading. A cap is offered only
  where dev-prune knows a cache of that adapter's name — typing one anywhere else says
  so, rather than storing a setting nothing would ever read. Caps on the caches no
  adapter is named after (`pip`, `conda`, `nuget`, `conan`, `vcpkg`, `hex`) have no
  row to be
  drawn on and are carried through the screen untouched; set those with
  `devp config set cache_max_gb`.

- **dev-prune no longer writes a `devp` copy beside the binary you ran.** Every
  invocation used to look for `devp` next to `dev-prune` and create it when it was
  missing — so a binary run out of npm's cache, a virtualenv's `Scripts` folder or your
  Downloads directory quietly wrote a second executable into that directory. Nobody asked
  for it, it was orphaned the moment the delivery directory was replaced, and "a freshly
  downloaded unsigned binary copies itself and registers a scheduled task" is a
  behavioural malware signature — it is what earned the WinGet package a
  `Validation-Defender-Error`. The pair you actually run is untouched: the managed `bin`
  directory the installers put on your `PATH` still holds both names and is still kept in
  step on every pass. Beside the running binary, the twin is now created only when you ask
  for it — `dev-prune setup`, or `devp doctor --fix` — and `devp doctor` reports a
  missing one with the command that fixes it.

- **`devp caches clear` will no longer empty Maven's local repository.** It used to
  delete `~/.m2/repository` like any other download cache, and that was wrong: Maven does
  not call it a cache and neither should we. `mvn install` writes there, and so does
  `mvn install:install-file` — the documented way to use a jar that is in no repository
  at all, which is how a driver behind a click-through licence, a partner SDK, or an
  internal artifact from before there was an internal Nexus ends up on a machine. Those
  files, and `-SNAPSHOT` builds of your own modules, exist nowhere else; no remote can
  hand them back. `devp caches` still finds the repository, sizes it, and prints
  `rm -rf ~/.m2/repository` for you to run if you know yours is disposable.
  `devp caches clear maven` now explains that and exits `2`, and `devp caches clear all`
  clears everything else and says what it kept. In `--json`, that row moves to a new
  `kept` array on both the plan and the result document — read it, or a Maven repository
  will look like a machine that simply has none.

- **Re-running the install one-liner now does only what needs doing.** It is the answer
  to almost every install question — "reinstall it", "get me the latest", "I think mine
  is broken" — and until now it answered all of them the same way: download the release
  again and write over whatever was there. It looks first now. An install already at the
  version being installed, with `devp` beside it and on `PATH`, is left exactly as it is
  and exits `0` without downloading anything. An older one is updated in place and says
  so (`-> Updating dev-prune v1.7.0 -> v1.8.0`). A **newer** one is not touched at all,
  so a machine ahead of the release the script resolved to is never quietly walked
  backwards — naming the version with `--version` / `-Version` is what makes installing
  backwards deliberate, and that still works. And a same-version install that is missing
  `devp`, or missing its `PATH` entry, is reinstalled — which repairs both, and is the
  whole reason anyone runs the one-liner a second time. The new `--force` (`-Force` in
  PowerShell, `DEV_PRUNE_FORCE=1` for the plain one-liner, which has nowhere to put a
  flag) downloads and writes both files regardless, for when you suspect the file on disk
  rather than its version. See
  [Installer options](docs/RELEASES_AND_MANUAL_INSTALL.md#installer-options).

- **`devp doctor` now names the command that removes each other copy it finds.** The
  "Other copies" warning listed the paths and then told you to remove each one "through
  the manager that installed it" — true, and useless, because the reason a second copy
  goes unnoticed for months is precisely that nobody remembers installing it. Each line
  now names the channel that owns that file and the command to type, so a leftover cargo
  copy reads `…\.cargo\bin\dev-prune.exe (v1.6.0, from cargo, remove with
  cargo uninstall dev-prune)`. A copy the install script left behind is removed by
  `devp uninstall`, and one that no package manager owns says so rather than naming a
  command that would report the package is not installed.

### Fixed

- **`devp caches --help` was three managers behind the command itself.** Its `Covered:`
  line still ended at Conan, so Composer, CocoaPods and Hex — all reported by
  `devp caches` since 1.7.0 — went unmentioned in the help for it, as did the fact that
  Hex is cleared by removing a directory. Both lists now name every manager the command
  probes.

- **The install one-liner now wins in every terminal, not only the one you ran it in
  (Windows).** `install.ps1` prepended its directory to the PATH of the session it was
  running in but *appended* it to the User PATH it saved, and those two disagree. Run it
  on a machine that already had dev-prune from `cargo install` or Scoop and it looked
  like it worked — because in that window it had — while every terminal opened afterwards
  went on running the older copy. The saved PATH now prepends as well, which is what the
  script always claimed to do. The directory holds `dev-prune.exe` and `devp.exe` and
  nothing else, so it can shadow nothing you did not ask for. macOS and Linux were
  already correct.

- **Both install scripts now tell you about a `dev-prune` they did not install.** The
  one-liner has always been safe to run over a cargo, npm, uv, pipx, Homebrew, Scoop or
  WinGet install — it needs no uninstall first and never fails because of one — but it
  said nothing about the copy it found, and a second copy nothing upgrades is how a
  machine ends up running a version you fixed months ago. It now prints the path, says
  it deliberately left the file alone (deleting a file another package manager still has
  on its books is how an install becomes unrepairable), and gives you
  `devp install --channel installer`, which installs here and uninstalls there through
  the manager that owns it. [Changing channels](docs/DISTRIBUTION.md#-changing-channels)
  covers it end to end.

- **`install.sh` no longer rewrites your Windows User PATH into something frozen to this
  machine.** Running the shell installer from Git Bash or WSL-on-Windows updates the User
  PATH through PowerShell, and it read that value with a call that *expands* every
  `%USERPROFILE%`-style reference on the way out — then wrote the expanded text back as
  a plain string. Any entry that followed your profile came back hard-coded to this
  machine and this user, the registry value stopped being the expandable kind, and
  nothing about the result looked wrong afterwards. It now reads and writes the raw
  registry value, preserves its type, and — like the PowerShell installer above — puts
  its directory *first* rather than last, so the copy it just installed is the one that
  answers. It also tells the running desktop the value changed, instead of leaving every
  already-open program on the old PATH until the next sign-in.

- **`npm install -g dev-prune` now installs a working binary on Windows.** The three
  Windows platform packages are published under new names — `dev-prune-windows-x64`,
  `dev-prune-windows-arm64` and `dev-prune-windows-x86`. npm had refused every
  `dev-prune-win32-*` spelling with `E403 — Package name triggered spam detection`, while
  the four Linux and macOS names published from the same script in the same run went
  through untouched, so the refusal tracked the name and not the payload. The effect was
  that 1.6.0 and 1.7.0 both shipped an npm channel that installed on Windows and then
  reported it had no binary to run. The new names match the release assets you already
  download (`dev-prune-v1.8.0-windows-x64.zip`), and each package still declares npm's
  own `win32` and `ia32` values in `os` and `cpu`, so npm resolves exactly one of them
  exactly as before. Nothing changes on Linux or macOS, and nothing changes for the
  shell installer, WinGet, Scoop, `cargo install` or `pip`. A Windows machine already
  holding `dev-prune@1.7.0` needs `npm install -g dev-prune@latest` rather than a
  repair — a published manifest cannot be edited, so 1.7.0 names the old packages
  permanently. With all eight names finally on the registry, npm is now listed as an
  install channel alongside the others in the README, on the site and in
  [`DISTRIBUTION.md`](docs/DISTRIBUTION.md), and `npx dev-prune status` runs the tool
  once without installing anything.

## [1.7.0] - 2026-08-23

The tool keeps itself up to date, the first run stops assuming you already know
everything, the output stops shouting — and you can now move the install from one package
manager to another without ending up with two of them.

### Added

- **`devp install --channel <name>`** moves this installation from one package manager
  to another. `devp update` has always upgraded the copy that is running, through
  whichever channel installed it; what there was no way to do was *change* which channel
  owns it. Someone who installed with `cargo install` and later wanted WinGet had to know
  to remove the old copy first, and if they did not, two binaries sat on `PATH` and which
  one won was an accident of ordering. This installs through the manager you name, then
  removes the old copy through the manager that put it there — in that order, so a failed
  install leaves the working copy untouched. Removing through the owning manager rather
  than deleting the file is the point: uv, pipx, npm and cargo each keep a record of what
  they installed, and a manager whose record still says dev-prune is there will put the
  old binary back on its next reinstall. Nothing is migrated, because nothing needs to be
  — the settings, the repository registry and the undo history live in the config
  directory, which no package manager owns. `devp install` on its own says which channel
  owns this copy, and `--dry-run` prints the whole plan without running any of it.

- **Mix's `_build/`, opt-in.** `devp config set enable_mix_build true` adds a second
  Elixir adapter, for the compiled tree, alongside the `deps/` one that has always been
  on. It is opt-in and waits for `build_idle_days` (45) rather than `idle_days`, for the
  same reason `enable_cargo` and `enable_gradle` are: `deps/` comes back by downloading,
  `_build/` comes back by recompiling. Both `mix.exs` and `mix.lock` must be present
  before either adapter deletes anything, and the next `mix compile` puts it back.

- **`devp man` now opens on a contents page.** It used to print roff — the raw `.TH` and
  `.SH` source — at whatever terminal ran it, on the assumption that something downstream
  would format it. On Windows there is no `man` to hand it to, so what people actually got
  was a screenful of markup. `devp man` now prints a page you can read: every command
  grouped by what it is *for* — register repositories, prune and put back, look at what is
  going on, settings and integration, the program itself — one line each, then the flags
  that go before the command and the exit codes. `devp man <command>` prints one command's
  page. Roff still appears wherever something can use it: a redirect or a pipe gets it, so
  `devp man | man -l -` and `devp man --dir ./man` are unchanged, and `--roff` forces it
  at a terminal.

- **`devp trust --fix-ownership`** adds every registered repository Git refuses to read
  to your global `safe.directory` list, after showing you the list and asking. Git's
  "dubious ownership" refusal is routine after a Windows reinstall, a restored backup, or
  a drive moved between machines — and because dev-prune dates a repository by its last
  commit, a repository Git will not read has no known age and is never pruned. One
  command clears the whole set; `--yes` skips the confirmation for a script. Values are
  written with forward slashes, which is the only spelling Git actually compares against,
  even on Windows.

- **A suggested-settings screen on the first run.** Before the full list of settings, the
  first run now shows a short one: the handful of things worth turning on, each with its
  official description, the same thing said again without jargon, and why it is being
  suggested at all. `a` accepts the whole first tier at once. There is a second tier for
  settings that are worth turning on *once you know what they do* — `allow_manifest_rewrite`
  is the one that lives there — and the accept-everything key deliberately does not
  reach it. The screen appears once, on the first run only; everything on it stays
  available from `devp config wizard` and `devp config set` forever.

- **Every setting now has a plain-English second line**, shown under its official one in
  the configurator and in the line-by-line fallback. "Days a repository must sit
  untouched before it is eligible for pruning" is precise; "how long a project has to sit
  untouched before dev-prune will clean it — something you worked on yesterday is never
  touched" is the one that answers the question people are actually asking.

### Changed

- **`devp run` groups repositories it could not examine instead of listing them one by
  one.** A machine where Git refuses twenty-one repositories on ownership used to print
  twelve lines each — two hundred and fifty lines of near-identical text, with the two
  real problems buried in it. It now prints one heading per *cause*, the first eight
  paths under it, how many more there are, an explanation of what the cause means, and
  the single command that fixes all of them. Anything that does not fall into a known
  cause is still printed in full, on its own.

- **`auto_update` is now on by default.** A pruner that runs on a schedule is exactly the
  kind of tool nobody thinks to upgrade, and one that never upgrades keeps whatever bug
  it shipped with. What runs automatically is only the download-and-replace half — the
  same GitHub release binary `devp update --install` fetches, refused unless its SHA-256
  matches the published sidecar. The package-manager half deliberately does not run
  unattended: `winget upgrade` can raise an elevation prompt and pull in upgrades nobody
  asked about. On WinGet, Scoop and Homebrew nothing happens at all — those managers
  replace their whole package directory on upgrade, so they own it, and you get the
  one-line notice naming their command instead. `devp config set auto_update false`
  turns the whole thing off, and `DEV_PRUNE_OFFLINE=1` was already enough to stop it.

- **`devp update` now names the channel you installed from**, instead of printing all
  eight and asking you to remember which one you used. It was never a fair question — the
  channel is a decision made once, possibly a year ago, on a machine since reimaged. It
  was also an unnecessary one: the channel is written in the path of the running binary,
  and dev-prune already reads it to decide what `--install` is allowed to do. You now get
  one line, `Installed with cargo — upgrade with: cargo install dev-prune --force`, and
  the offer to let `devp update --install` do it. The full list survives for the one case
  that earns it: a binary in a location no channel owns, where there is genuinely no
  package manager to name.

- **`devp trust` no longer counts `auto_update` among the settings that widen anything.**
  That list means "things you switched on beyond the defaults", and this is now a
  default. It keeps its own row, saying plainly what it does and how to stop it.

- **The landing page has been rebuilt**, and the README with it. The hero now opens on a
  reclaim ledger — four real directories from one `--dry-run` pass, each with the lockfile
  that proved it recoverable and the one that refused — instead of a stat block. The whole
  page is laid out for phones, tablets, portrait monitors and wide desktops rather than
  three breakpoints, motion respects `prefers-reduced-motion`, and every section still
  ships visible in the prerendered HTML for anything that does not run JavaScript.

- **Non-ASCII paths are now documented, having been verified end to end.** A repository at
  `ワークスペース/项目目录名称测试/프론트엔드` is scanned, verified, pruned and restored like
  any other, and the terminal tables pad by display *column* rather than by character, so a
  full-width name does not knock `devp status` and `devp doctor` out of alignment. This was
  already true; nothing in the code changed. It is written down now — in the README, the
  agent skill and the site — because a tool that never says so is assumed not to. Program
  messages remain English; the site's pitch is available in ten languages.

### For contributors

- **CodeQL runs on every push, every pull request and weekly, with the extended query
  suite.** `cargo audit` answers whether a dependency is known-vulnerable and says nothing
  about the code in this repository; this is the other half. It covers Rust, the site's
  JavaScript and TypeScript, the Python packaging scripts, the Ruby formula, and
  `.github/workflows` — which holds more privilege than anything else here, since a
  release job carries a PyPI identity, a crates.io token and `contents: write`, and had
  nothing looking at it. It is GitHub's default setup rather than a workflow file in this
  repository: the two cannot both be enabled, and the one that needs no maintenance covers
  more languages. The scheduled run matters as much as the push one — the code will not
  have changed, but the queries will have.

- **The third-party actions in CI and the release workflow are pinned to commit SHAs.**
  CodeQL's first pass flagged all fifteen references: a tag is a moving pointer, so
  `@v2` is a promise from whoever can still push that tag. The three that hold something
  worth stealing are now pinned by digest with the version in a trailing comment —
  `Swatinem/rust-cache`, which can poison a build cache, and `softprops/action-gh-release`
  and `pypa/gh-action-pypi-publish`, which carry `contents: write` and the PyPI identity.
  Dependabot updates a digest pin the same way it updates a tag. `dtolnay/rust-toolchain`
  is pinned too, with `toolchain: stable` passed explicitly — `@stable` was never a
  version, it was a branch whose copy of the action defaults that input, and a branch is
  the one reference a compromised account can move without leaving a tag behind.

- **npm publishing moved to trusted publishing — there is no `NPM_TOKEN` any more.** The
  release job authenticates with the same OIDC assertion it already used for the
  provenance attestation, so the one long-lived credential that could publish all eight
  packages no longer exists, and npm's January 2027 removal of direct publish access for
  bypass-2FA tokens is a date this project can ignore. The job now upgrades npm before
  publishing — exchanging an OIDC token needs 11.5.1 or newer, and the npm inside a Node
  release is whatever shipped that day. Trusted publishing cannot create a
  name ([npm/cli#8544](https://github.com/npm/cli/issues/8544)), so the job skips any of
  the eight names the registry does not know yet, publishes the rest, and says which ones
  it skipped in the release summary. A partial npm channel is a real state to be in — a
  new name has to be created once from a workstation with `npm login` — and a release
  that refuses to ship the names that do work helps nobody.

- **Two check-then-use file races are gone.** `site/prerender.js` tested for its inputs
  with `existsSync` and then read them; it now reads them and reports the failure it
  actually got. The VS Code extension's `devprune.createConfig` tested for `.devprune.json`
  before writing it with the `wx` flag — the flag was already the whole check, and the
  test in front of it could only ever be out of date.

- **The seven npm platform packages now ship a README saying not to install them.**
  npmjs.com prints `npm i <name>` at the top of every package page with no way to suppress
  it, so a page for a Linux binary read as an install target. The only lever is what
  appears directly below, and with no README npm showed nothing there.

- **The adapter detection tests no longer read the machine they run on.** `detect_adapters`
  resolves the opt-in and disabled adapter lists from the real registry, so a contributor
  who had switched cargo on in the config wizard watched
  `test_detect_adapters_multiple_ecosystems_coexist` fail on their machine and pass in CI,
  where no config exists. Detection now has an inner form taking both lists as arguments,
  and the tests state what is on rather than inheriting it.

### Fixed

- **`devp doctor` can see other copies of dev-prune again.** The "Other copies" check
  asks each `dev-prune` and `devp` it finds on `PATH` what version it is, and reports the
  ones that differ from the running build — which matters because whichever comes first on
  `PATH` is the one you actually get when you type `devp`. It asked by running
  `--version` and looking for an `x.y.z`, and this CLI does not print one: the banner ends
  `v1.7.0` and the line under it reads `dev-prune (devp) v1.7.0`, so every token had a `v`
  in front of it and none of them parsed. A copy that cannot state a version is deliberately
  left alone — it is far more likely to be an unrelated file under the same name — so the
  check quietly classified every real dev-prune on the machine as "not a dev-prune" and
  reported "none on PATH running a different version" no matter how many were sitting
  there. It now reads the version it prints. On a machine with a `cargo install` copy in
  front of the managed one, that line is the difference between "everything is fine" and
  the reason `devp -V` still says the old number after an upgrade.

- **WinGet manifests are generated with no byte-order mark and CRLF line endings**, which
  is what `winget-pkgs` itself writes and what every manifest already in the catalog
  looks like. The previous generator prepended a BOM on the strength of guidance that
  stopped being true years ago. Both are invisible in an editor, which is exactly why the
  fix is in the generator rather than in the committed files.

## [1.6.0] - 2026-08-23

Two more ecosystems, three more machine-wide caches, and the first answer to the
question space alone never answered: *how long is all of this to put back?*

### Added

- **Terraform.** Any repository with a `*.tf` or `*.tf.json` file is now detected, and
  `.terraform/providers` — the downloaded provider plugins, often hundreds of megabytes
  of them per repository — is pruned once `.terraform.lock.hcl` proves they come back.
  `devp undo` and `devp restore` run `terraform init -backend=false`, so putting them
  back never needs your backend's credentials.

  Nothing else under `.terraform/` is touched, deliberately. `.terraform/environment`
  records your selected workspace and losing it silently returns you to `default` — the
  next `terraform apply` would then target the wrong environment. `terraform.tfstate` in
  there is the backend's initialisation record, and `modules/` comes from module sources
  that the lock file says nothing about. None of those three is provably recoverable, so
  none of them is in scope.

- **Dart and Flutter, opt-in.** `devp config set enable_dart true` turns on an adapter
  for `.dart_tool/`, restored with `flutter pub get` or `dart pub get` depending on what
  `pubspec.yaml` declares. It is opt-in and waits for `build_idle_days` (45) rather than
  `idle_days`, because the pub metadata in there restores offline in about a second but
  the `build_runner` and `flutter_build` caches beside it come back by recompiling —
  which is the same bargain `enable_cargo`, `enable_gradle`, `enable_maven` and
  `enable_swift` offer. Dart's `build/` is never touched, exactly as Mix's `_build/`
  is not.

- **The full CLI reference is now a page on the site.**
  [devprune.vkrishna04.me/reference/](https://devprune.vkrishna04.me/reference/) serves
  every command, flag, exit code, config key and `--json` field as one linkable,
  searchable document, so answering "what does `--except` take again?" no longer means
  opening GitHub. It is generated from `docs/CLI_REFERENCE.md` during the site build
  rather than written a second time, which is the only version of this that stays true:
  a hand-copied reference is the copy that goes stale.

- **`devp caches` now finds Composer, CocoaPods and Hex.** A PHP, iOS or Elixir
  toolchain leaves a machine-wide download cache behind exactly like npm and pip do, and
  those three were invisible. `devp caches --clear composer` runs `composer clear-cache`,
  `--clear cocoapods` runs `pod cache clean --all`, and each is found by asking the
  manager where its cache is — `composer config --global cache-dir` — rather than by
  guessing a path that `COMPOSER_HOME`, `COMPOSER_CACHE_DIR` or a global `cache-dir`
  setting can each move. Hex ships no clean task at all, so that row prints the deletion
  it will actually perform, and `mix deps.get` re-fetches the tarballs.

- **`devp status` estimates what a full restore would cost you in time.** Above the
  table, once there is anything to go on, `status` prints how long putting everything
  back would take — split by adapter, because a `node_modules` and a `target` come back
  at nothing like the same speed. Every second of it was measured on your machine by
  `devp restore --last-run`; there is no built-in table of typical speeds, because that
  would be a number about somebody else's laptop. Until a restore has been timed here
  the line is simply absent, and an adapter never timed here is left out of both the
  estimate and its stated coverage, so a partial answer says that it is one. The record
  is three numbers per adapter — sample count, bytes, milliseconds — kept in the registry
  and never uploaded; see [`PRIVACY.md`](docs/PRIVACY.md).

### Fixed

- **Four links inside the CLI reference went nowhere.** The anchors for `devp run`,
  `devp status`, `devp update` and `devp doctor` did not match their headings, so
  clicking a cross-reference to any of them left you where you were. The site build now
  refuses to render the reference if any in-document anchor does not resolve, so this
  cannot come back quietly.

### For contributors

- **`scripts/bump-version.sh <version>` sets the release version everywhere.** Eleven
  files restate the number by hand and `check-version.sh` has always named them; now
  there is a command that writes them, refreshes `Cargo.lock` and re-runs that check to
  prove it. Releasing no longer starts with nine edits made from a list of failures.

- **The release opens the winget-pkgs pull request.** A `submit-winget` job renders the
  manifests from the published sidecars, strips the licence header, adds the BOM
  winget-pkgs validation requires, and runs `wingetcreate submit`. The two encoding rules
  used to live in `RELEASING.md` as prose for a person to remember, which is how the
  first submission went out with the wrong `Commands` list. `scripts/winget-manifests.sh`
  is the same transform for the manual path, so a submission from a laptop and one from
  CI are byte-identical. The job is gated on a `WINGET_PUBLISH` repository variable and
  reports `skipped` until that and `WINGET_TOKEN` both exist.

- **`scripts/check-schema.sh` now asserts the schema URL as well as the copies.** The
  `$id` in `schemas/devprune.schema.json`, `constants::JSON_SCHEMA_URL` and the path
  `site/public/` publishes at have to agree, so the published schema URL cannot move in
  one place and stay put in the other two.

## [1.5.1] - 2026-08-23

Install, update and uninstall, told the same story. dev-prune now recognises the package
manager that installed it — the installers, cargo, npm, uv, pipx, pip, WinGet, Scoop or
Homebrew — and every command that touches the installation reads from that one answer
instead of guessing separately. The three managers that version their own package
directory are handled properly for the first time: nothing is written into one, nothing
is deleted out of one, and both `dev-prune` and `devp` now ship inside the Windows
archive as real files.

### Added

- **`devp doctor` names your install channel and both of its commands.** A new
  `Install channel` line reports which manager owns this copy and prints the exact
  upgrade and removal commands for it — `winget upgrade --id VKrishna04.dev-prune`,
  `brew upgrade dev-prune`, `pipx upgrade dev-prune`, whichever applies. It is never a
  warning; it is there so "how do I update this?" is answered on the same screen as
  everything else, rather than from memory of an install you did months ago.

- **`devp update` upgrades through pip, WinGet, Scoop and Homebrew.** Those four joined
  cargo, npm, uv and pipx, which were already handled. If dev-prune was installed by a
  manager, `devp update` runs that manager's own upgrade rather than replacing the file
  underneath it and leaving the manager's database describing a version that is gone.

### Changed

- **The Windows zip now carries `dev-prune.exe` and `devp.exe` as two real files.** Both
  names are therefore installed by the archive itself, on every channel that unpacks it,
  before anything runs. WinGet in particular resolves every command a package declares
  against what the install actually put on PATH, and a name created on first run appears
  long after that check has looked — so `devp` could not be a WinGet command until it was
  a genuine second file. Four megabytes of duplicate is the cheap half of that trade.

- **WinGet, Scoop and Homebrew installs create nothing at runtime.** Each of those three
  keeps its package in a directory it versions and replaces wholesale on upgrade, so
  anything dev-prune wrote beside itself there — the `devp` twin, the windowless
  `devpw.exe` the Windows scheduler uses — was orphaned by the next upgrade while still
  sitting on PATH, running the release you thought you had replaced. dev-prune now
  recognises those three and writes nothing into their directories. All three packages
  already ship both names, so nothing is lost.

### Fixed

- **A first run with its output redirected no longer installs anything.** The pass that
  adds the PATH entry, registers the scheduler and installs the git hook now requires a
  terminal on both stdin and stdout, the same condition the first-run config review
  already used. A binary executed once by an automated system, with its output captured,
  used to acquire persistence on that machine silently — nobody saw the report, so nobody
  knew to undo it. Nothing is skipped permanently: the "already done" stamp is not
  written, so the first run a person can actually see does the pass and prints what it
  did.

- **`devp uninstall` no longer half-removes a WinGet, Scoop or Homebrew install.** It
  used to delete the binary out of the manager's package directory, which leaves the
  manager certain the package is still installed and its own uninstall with nothing to
  remove — a state you cannot get out of without editing the manager's database. That
  copy is now named rather than deleted, and the command that really removes it is
  printed with the rest.

- **`devp update`, `devp uninstall` and `devp doctor` now agree on where dev-prune came
  from.** There were three separate detectors: one matched path components, one matched
  substrings, one carried a hand-written list of directories, and only one of them had
  ever heard of WinGet. So `devp doctor` could report a copy `devp uninstall` did not
  look for, and `devp update` could name a different manager than `devp uninstall` for
  the same install. They are one answer now, and `devp doctor` searches exactly the
  directories `devp uninstall` sweeps.

### For contributors

- `scripts/check-schema.sh` runs in CI. The config schema exists three times — embedded
  in the binary, shipped in the VS Code extension, published at the `$id` URL every
  `.devprune.json` points at — and nothing derived one from another, so a new config key
  could reach one copy and silently not the other two.

## [1.5.0] - 2026-08-22

The adapter screen, rebuilt. Adapters are now listed by language, a heading switches a
whole ecosystem on or off in one keypress, and any adapter — or any language — can be
given its own idle window without touching the global one. Cargo joins the opt-in build
adapters, and the build-tool wait drops from 60 days to 45.

### Added

- **`devp config wizard` groups adapters by language.** The adapter checklist used to be
  eighteen names in registry order, which is the order they were written in and no help
  at all when you want "all my Python tooling, off". Now every adapter sits under a
  language heading — JavaScript, Python, Rust, Go, JVM, PHP, Ruby, Swift & Objective-C,
  Elixir — and the heading is itself selectable: <kbd>Space</kbd> on it turns that whole
  language on or off, and the heading shows `[x]`, `[ ]` or `[-]` so a partly-off
  language is visible without expanding anything.

- **`adapter_idle_days` gives one adapter its own idle window.** `devp config set
  adapter_idle_days cargo=90,npm=30` makes cargo wait ninety days and npm thirty, while
  everything else keeps the global `idle_days`. It is applied as
  `max(idle_days, build_idle_days, adapter_idle_days[<name>])` — a floor and never a
  bypass, so no number you put here can make dev-prune touch a repository the global
  window still considers active. `devp config set adapter_idle_days -` clears the map.

- **The same windows are editable from the wizard, per language.** Press <kbd>d</kbd> on
  an adapter to type its window; press <kbd>d</kbd> on a language heading and what you
  type applies to every adapter under it at once, which is the difference between one
  keystroke and five for "all of Python waits a month". The column shows `default` or
  `45d` beside each adapter, so the whole policy reads off one screen.

- **`enable_cargo`** turns the Cargo adapter on. Off by default — see below.

### Changed

- **Cargo is now opt-in, like gradle, maven and swift.** `target/` is compiler output:
  the lockfile proves it comes back, but it comes back by *recompiling* rather than
  downloading, and on a real workspace that is minutes where a dependency reinstall is
  seconds. That is the line the other build adapters were already on, and cargo belonged
  on it. **If you were relying on dev-prune reclaiming `target/`, run `devp config set
  enable_cargo true` after upgrading** — until you do, cargo is invisible: not detected,
  not counted by `stats`, and `--only cargo` prunes nothing.

- **`build_idle_days` now defaults to 45 days, down from 60.** Two months was long
  enough that a repository you had genuinely finished with still sat there taking up the
  space. Forty-five days is still three times the dependency window, and the setting has
  not moved — `devp config set build_idle_days 60` restores the old wait exactly.

- **The wizard paints its first screen faster.** `devp trust` and the first-run
  walkthrough read the machine's scheduler state and Git hook state before drawing
  anything, and they read them one after the other; on Windows the `schtasks` query alone
  was most of a 1.4-second wait staring at an empty terminal. The two checks now run at
  the same time.

### Fixed

- **The documented default for `auto_config` was wrong.** Three places — the CLI
  reference, `llms.txt` and the agent skill — said it defaults to `true`, so an agent
  reading them would tell you `devp link` drops a `.devprune.json` into every repository
  it registers. It does not, and never has: the default is `false`, which is what
  `devp config show` reports. Only the documentation changed.

### For contributors

- `PackageManager::opt_in` adapters are filtered inside `detect_adapters`, so an adapter
  that is off is invisible to `status`, `stats`, `run` and `doctor` at once rather than
  in four places that could disagree.
- A new adapter must be added to `ADAPTER_GROUPS` in `src/adapters/mod.rs` as well as to
  the registry; `every_adapter_is_grouped_exactly_once` fails if it is not, because an
  ungrouped adapter would vanish from the only screen that lists them.

## [1.4.1] - 2026-08-22

A tidy-up release. Nothing in the tool changed; what changed is that the manifests
1.4.0 promised are actually published, the release stops shipping a stale VS Code
extension alongside the current one, and the test suite that ships in the source
tarball passes on Linux as well as everywhere else.

### Fixed

- **The Homebrew formula and Scoop manifest 1.4.0 described now exist.** The job that
  renders them ran, wrote all five files, and then reported that nothing had changed —
  it asked `git diff` about paths that were untracked, which is a question `git diff`
  answers with silence. So `brew install <raw URL>` and `scoop install <raw URL>` pointed
  at a file that was never committed. Both work now, against the checksums 1.4.0
  published.

- **The release no longer attaches an old copy of the VS Code extension.** A built
  `.vsix` had been committed to the repository, and it matched the glob that uploads the
  freshly built one — so 1.4.0 shipped both 0.3.0 and 0.4.0, with nothing on the release
  page to say which was current. The stale asset has been removed from the 1.4.0 release
  as well.

### Changed

- **Installing is now three choices instead of nine.** The site listed every channel as a
  flat row of tabs, which had grown into a wall to read. They now group by what you are
  actually deciding between — an install script that needs nothing installed, a package
  manager you already use, or the archive itself — and the individual channels appear as
  a second, quieter row inside the group you pick.

- **Homebrew and Scoop are listed as install channels everywhere else too**, not just in
  `docs/DISTRIBUTION.md`: the site, the README and `llms.txt` all carry the by-URL
  commands, which need no tap and no bucket because the file itself carries the SHA-256
  of the archive it installs.

- **The site answers four questions it used to leave to the docs**: whether build output
  is ever deleted (it is not, and the three opt-in build adapters say so), how to switch
  one ecosystem off for good with `disabled_adapters`, what `devp caches` touches outside
  your repositories, and how to tell which of several installed copies of `devp` is the
  one actually running.

### For contributors

- **The bundled agent skill claimed dev-prune "never downloads a binary".** That stopped
  being true when `devp update --install` shipped, so an agent reading the skill would
  have told a user to re-run their installer instead. It now describes `--install` and
  the `auto_update` opt-in alongside each channel's own upgrade command.

- **Two test fixtures were lying, and `cargo test` failed on Linux for anyone who ran it
  against the 1.4.0 source tarball.** Neither was a defect in the tool. `git_repo()` wrote
  the same README with the same author and message every time, so two fixtures created
  inside the same second shared a root commit — git timestamps have second granularity —
  and `link` correctly identified the second as the first one relocated. Separately, a
  test scanned a repository it had built in the OS temp directory and expected `init` to
  register it, but a scan skips anything under a directory named `tmp`, and on Linux the
  temp directory *is* `/tmp`. Fixtures are now distinct by construction and built outside
  the temp directory.

## [1.4.0] - 2026-08-22

Seven more ecosystems and a switch to turn any of them off, a configurator you can
actually see, a command that tells you exactly what dev-prune is allowed to do on your
machine, one that empties a package manager's cache without you looking up fourteen
different incantations, per-repository AI rules for fifteen editors instead of six, and
a 32-bit Windows build. Plus the fix for the two things that made `devp status` lie: a
registry filling with dead paths, and every repository reading as "worked in today"
forever.

### Added

- **`devp update --install` now upgrades itself from the GitHub release, instead of
  asking whichever package manager delivered the first copy to do it.** There is exactly
  one binary that matters — the managed copy under the config directory, which the git
  hooks, the scheduler and `PATH` all point at — and it does not live inside
  `node_modules`, a uv tool directory or `~/.cargo/bin`. Asking `uv` to upgrade a file it
  has never heard of was never going to work, and asking it to upgrade its *own* copy
  left the one that actually runs on the previous version. Now the release binary is
  downloaded once, checked against the SHA-256 published beside it, and written to every
  copy this installation runs: the managed binary, its `devp` alias, the windowless
  scheduler twin, and the binary you typed. Nothing is installed if the checksum does not
  match. The channel's own record of the version is left alone deliberately — correcting
  it means running the channel's installer, which is the thing this route exists to avoid
  — and the one command that resyncs it is printed. If there is no published binary for
  your platform, or the download fails, it falls back to the channel's upgrade command
  exactly as before.

- **`devp doctor` now finds the *other* copies of dev-prune on your machine and says
  which version each one runs.** dev-prune ships through five channels and each keeps its
  own copy; upgrading replaces the one that matters and deliberately leaves the others
  alone. The cost of that is a stale binary sitting on `PATH`, and if it comes first you
  type `devp` and silently get the old release — with every symptom pointing at
  dev-prune rather than at which copy answered. Doctor searches `PATH` and the fixed
  cargo, uv and pipx directories (including ones not on `PATH`, which are exactly the
  copies nobody ever upgrades) and names any whose version differs. It deletes nothing:
  which copy you want is your call, and the manager that installed one is the only thing
  that should remove it.

- **`brew install` and `scoop install` now work, from a URL, with no tap and no bucket to
  add first.** Every release renders a Homebrew formula, a Scoop manifest and the three
  WinGet manifest files from the checksums it just published, and commits them to the
  repository — so the two commands in
  [`DISTRIBUTION.md`](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/DISTRIBUTION.md)
  always install the newest release, and neither of them can quietly drift a version
  behind the way a hand-maintained manifest does. The Homebrew formula covers macOS and
  Linux on x64 and arm64, installs both names, and generates your shell completions from
  the binary; the Scoop manifest covers 64-bit, arm64 and 32-bit Windows and carries
  `checkver`/`autoupdate`, so a bucket that adopts it bumps itself. The WinGet manifests
  are rendered ready to submit — that one still needs a reviewed pull request, so
  `winget install dev-prune` does not resolve yet.

- **The Python interpreter a virtual environment was built with is now recorded when it
  is pruned, and `devp restore --last-run` rebuilds on that same interpreter.** A `.venv`
  created by Python 3.12 and restored on whatever `python` happens to be first on `PATH`
  today is not the environment that was deleted — different wheels resolve, and the
  failure surfaces weeks later as an import error nobody connects back to a restore. The
  version is read from the environment's own `pyvenv.cfg` before the delete, stored in
  the registry, and passed back to `venv`, `uv` and `poetry` on the way in. If the
  recorded interpreter is still installed it is simply used, and the restore says so. If
  it is not, the restore stops and asks once for the whole pass — never once per project
  — and defaults to *no*, with the `uv python install 3.12` line you need to fix it
  properly. Anything pruned before 1.4.0 recorded nothing and restores exactly as it did
  before.

- **`DEV_PRUNE_SCAN_THREADS` sets the status scan's thread count**, for the two cases the
  automatic figure gets wrong: a network filesystem, where the scan is waiting on
  latency and wants far more threads than you have cores, and a spinning disk, where
  `DEV_PRUNE_SCAN_THREADS=1` stops the head thrashing and is genuinely faster. The
  automatic figure now also ramps further on machines that can take it — up to 32
  threads rather than 16.

- **`devp status` separates what you can reclaim *now* from what you could reclaim in
  total.** The header used to show one number that counted the dependency directories in
  every registered repository, including the one you worked in this morning — a figure
  nothing was ever going to act on. It now reads `12.4 GB ready now | 47.1 GB
  reclaimable in all`, where "ready now" is the sum over the repositories that are
  actually idle enough to prune. Both totals are always over the whole registry, so
  applying a filter or a search does not make your machine look tidier than it is. The
  same split appears in the plain (non-TUI) footer.

- **Sort, filter and search in the `devp status` dashboard.** `s` cycles the sort —
  relevance, biggest reclaim, longest untouched, name. `f` cycles the filter — all,
  candidates only, anything holding reclaimable bytes, or just the rows that need a
  decision rather than a prune (a path that is gone, a `.devprune.json` that does not
  parse). `/` searches paths *and* adapter names, so `/uv` finds every Python project on
  the machine without your remembering where they live; `Enter` keeps the query, `Esc`
  clears it. All three change only what is displayed: a repository checked for pruning
  stays checked when a filter hides it, `a` and `p` arm only the candidates actually on
  screen, and the counts in the header always cover the whole registry — a filtered
  dashboard can never make a machine look tidier than it is.

- **`devp status` shows a progress bar while it scans, and finishes in about half the
  time.** Sizing every dependency tree in eighty repositories used to be twenty-five
  seconds of a blank screen, which reads as a hang; people killed it before it drew
  anything. The scan now runs across several threads — the work is one independent
  file-system walk per repository, so it is the disk that is the limit, not the CPU — and
  reports `41/80` against a bar as it goes. `--json` is unchanged and still emits one
  document and nothing else.

- **`devp config wizard` is a configurator you can see.** Every setting on one screen,
  arrow keys to move, `Space` to change the highlighted one — a toggle flips, a number
  opens a field, `disabled_adapters` opens a checklist of every adapter. `r` puts one
  back, and the last screen lists exactly what is about to be written before it is
  written. Nothing is saved until you say so, and `q` leaves without saving anything.

  The first-run walkthrough is the same screen, with the `devp trust` declaration in
  front of it: what the tool guarantees, and what it is about to do on this machine, read
  live rather than restated — on screen before any of it is configurable. `y` still means
  "yes, all of it, carry on", exactly as it did at the old prompt.

  It also reopens after an upgrade that adds a setting you have never been shown, marks
  that one `NEW` and opens on it. Before, a new default started applying quietly and
  nothing ever told you. Settings you already confirmed are not asked about again.

  ```powershell
  devp config wizard
  devp config wizard --no-tui   # one question per line
  ```

  Terminals that cannot run the full-screen view get the line-by-line form automatically,
  and `--no-tui` or `DEV_PRUNE_NO_TUI=1` asks for it deliberately. That switch exists for
  AI agents in particular: an agent driving `devp` through a pty passes every "is this a
  terminal?" test and will never press a key. Agents should use `devp config set`, which
  needs no terminal at all — the bundled skill now says so.

- **`disabled_adapters` switches off an ecosystem you do not want touched.** Name it and
  dev-prune behaves as though that package manager were not installed: not detected, not
  counted by `stats`, not probed for by `doctor`, never pruned, never restored. Reversible
  at any time, and per adapter rather than all or nothing.

  ```powershell
  devp config set disabled_adapters go,composer   # leave Go and PHP projects alone
  devp config set disabled_adapters -             # every adapter active again
  ```

  The value is the whole list every time, so read it before writing it. It is separate
  from `enable_gradle` / `enable_maven` / `enable_swift`, which are off because undoing
  those costs a recompile rather than a download — this one is a preference, and applies
  to all eighteen adapters equally. The adapter checklist in `devp config wizard` is the
  same setting with a list to tick.

- **`devp trust`** answers "what is this thing actually allowed to do on my machine?" on
  one screen, and separates the two halves of that question that usually get muddled.
  The top section is what the code guarantees everywhere — registered repositories only,
  a lockfile verified before every delete, symlinks refused, no telemetry endpoint, build
  output never touched — and none of those rows has a setting behind it. The bottom
  section is read live off this machine: whether the scheduler is installed, whether the
  Git hooks register repositories on their own, how many are registered, the idle window,
  the managed binary's path, and any setting you have switched on that widens what may
  happen without you being asked.

  There is deliberately no letter grade. `trust level: MEDIUM` tells nobody which switch
  to look at, so the switches are listed by name instead, and `--json` gives them to a
  script:

  ```bash
  devp trust
  devp trust --json | jq -e '.summary.widened_count == 0'   # fails if anything is widened
  ```

- **`devp caches clear <MANAGER>`** empties one package manager's cache — or `all` of
  them — after listing and sizing what is about to go. `devp caches` has printed the
  right command for each manager since 1.2.0; this runs it for you, because doing it for
  fourteen managers by hand is the kind of tedium people give up halfway through.

  It goes through the manager's own subcommand wherever one exists (`npm cache clean
  --force`, `pnpm store prune`, `go clean -modcache`), because the manager knows what is
  still referenced and keeps its own bookkeeping consistent. The freed size is measured
  afterwards rather than assumed, so a `prune` that deliberately kept half the store says
  so.

  This changes nothing about what runs on its own: no prune pass, no scheduled run and no
  Git hook clears a cache, and none ever will. `clear` runs when you type it.

  ```powershell
  devp caches clear npm            # one manager, after confirming
  devp caches clear all --dry-run  # everything that would go, nothing touched
  devp caches clear all --yes      # no prompt, for a script
  ```

- **Seven more ecosystems**, taking the total to eighteen. Ruby **Bundler**
  (`vendor/bundle`), **CocoaPods** (`Pods/`), PHP **Composer** (`vendor/`), Elixir
  **Mix** (`deps/`), Python **PDM** (`.venv`) and **Pipenv** (its named virtualenv), and
  **Swift Package Manager** (`.build/`). Each verifies its lockfile read-only before
  anything is deleted and restores with the manager's own install command, exactly like
  the eleven before them.

  SwiftPM is opt-in — `devp config set enable_swift true` — for the same reason Gradle
  and Maven are: `.build/` holds compiled modules that come back by recompiling, not by
  downloading, so it is governed by `build_idle_days` rather than `idle_days`.

- **`devp skill --agent` now writes rules for fifteen editors**, up from six. New:
  `roo`, `kilocode`, `continue`, `amazon-q`, `kiro`, `trae`, `junie`, `gemini` and `zed`,
  alongside `cursor`, `windsurf`, `antigravity`, `cline`, `copilot` and `agents-md`.

  Ten of them get a file of their own. The other five share a file with every other tool
  that reads it — `AGENTS.md`, `.github/copilot-instructions.md`, `GEMINI.md`,
  `.junie/guidelines.md`, `.rules` — so dev-prune owns a marked block inside it and
  leaves every byte outside the markers exactly as it found them. Running it twice
  updates the block rather than stacking a second copy.

  ```powershell
  devp skill --agent zed
  devp skill --help    # every value, with the exact file it writes
  ```

- **A 32-bit Windows build.** `dev-prune-v1.4.0-windows-x86.zip`, built from
  `i686-pc-windows-msvc`, published alongside the six 64-bit archives and carried through
  every channel: `install.ps1` installs it, `cargo binstall` resolves it, npm ships it as
  `dev-prune-win32-ia32` and PyPI as the `win32` wheel. It exists for machines with no
  64-bit mode at all — locked-down corporate images, industrial control PCs — which
  1.3.1's installer correctly refused rather than handing them a binary they could not
  run. Now there is one to hand them.

  A 32-bit *shell* on 64-bit Windows still gets the x64 build: the installer reads the
  machine's architecture, not the shell's. There is no 32-bit Linux, macOS or ARM build,
  and [`docs/ROADMAP.md`](docs/ROADMAP.md) records why.

  Because there are now two Windows builds that can end up on the same machine, `devp -v`
  and `devp doctor` both say when the one you are running is not the one the machine
  wants. `Architecture: x86` on its own reads as a statement about the laptop; it never
  was — it is compiled into the binary — so on a 64-bit machine the line now finishes the
  sentence, naming the machine's own architecture and the command that fetches the
  matching build. `devp doctor` raises the same thing as a warning, never a failure: the
  32-bit build works, it is just capped at 4 GB of address space for no reason.

- **A repository you moved is recognised as the same repository.** Move or rename a
  workspace and its registry entry used to die with the old path: `devp status` grew a
  `Path missing` row nothing could prune, and the new location registered from scratch
  with a lifetime total of zero. dev-prune now records each repository's root commit —
  the one thing about it that survives a move — and when `devp link` or `devp init` finds
  a repository whose root commit matches an entry whose path is gone, it takes over that
  entry instead of starting a new one. The dead row disappears, and `added_at`, the prune
  history, the lifetime total, `override_idle_days` and a repository you had disabled all
  come across intact.

  ```powershell
  devp link .          # after moving the repo, from inside it
  devp init ~/code     # or in bulk, for everything under a directory
  ```

  It happens on its own too: the Git hook links on first commit, so the first commit
  after a move reconnects the history without you doing anything. Two missing entries
  sharing one root commit are clones rather than a move, so nothing is guessed and
  dev-prune says why. Entries registered before 1.4.0 carry no root commit yet — running
  `devp init` over your code directory once records them, and every move after that is
  recognised.

### Fixed

- **`devp doctor` no longer reports "Git hooks ✓ active" on a hook set that is silently
  shadowing your repositories' own hooks.** The check only asked whether the hook files
  existed and named a binary that is still there, both of which are true of a set
  installed before this release — so the machine most affected by the bug above got a
  clean bill of health. Doctor now inspects the hooks themselves, reports the missing
  passthrough, and `devp doctor --fix` rewrites them.

- **Git hooks installed by dev-prune no longer disable your repositories' own hooks.**
  This is the important one in this release. Registering the auto-link hooks sets the
  global `core.hooksPath`, and Git treats that as a *replacement* for `.git/hooks`, not
  an addition — so on a machine where dev-prune had installed hooks, every
  repository-local `pre-commit`, `commit-msg` and `pre-push` had quietly stopped
  running. Lint gates, secret scanners and conventional-commit checks were being skipped
  with nothing to indicate it, because a hook that never runs has no output to notice.
  dev-prune now writes a shim for every hook name Git looks for, and each one ends by
  `exec`ing your repository's own hook — same arguments, same stdin, same exit code, so a
  `pre-commit` that fails still blocks the commit. Existing installations are repaired
  automatically the first time 1.4.0 runs; nothing to do by hand. (`reference-transaction`
  and `post-index-change` are deliberately left unshimmed — they fire hundreds of times
  per fetch, and the shell spawns would be a cost you would feel. Use `--chain` if you
  rely on either.)

- **The status scan no longer aborts when the OS refuses a thread.** Under a low
  `ulimit -u`, in a constrained container or on a busy machine, spawning a scan thread
  can fail — and `devp status` would panic rather than scan. It now starts as many
  threads as it is given, does the work on the calling thread as well, and falls back to
  a single-threaded scan if it gets none at all. A slow scan instead of no scan.

- **A plugin manager's throwaway clones no longer fill the registry.** `devp init` on a
  home directory used to sweep up every `temp_git_1787245534782`-style checkout an editor
  or plugin manager had left behind — twenty-eight of them on the machine that motivated
  this — and they sat in `devp status` forever as rows that were never going to be pruned
  and were never going to go away. Directories whose name begins `temp_git_`/`tmp_git_`,
  and anything below a `cache`/`.cache`/`tmp` directory *inside* the scan root, are now
  counted and skipped with one summary line instead of listed. `devp init ~/.cache/things`
  still works: a directory you name outright is never second-guessed, and
  `devp link <path>` registers any single repository regardless. Ones already registered
  come out with `devp unlink <path>`.

- **A failed package manager no longer dumps its usage screen into the report.** When
  `npm ci` refused to run, dev-prune relayed all hundred-odd lines of npm's help text into
  the middle of a prune summary, and the one line that said *why* was somewhere in it. The
  output is now reduced to its diagnostic lines — plus the "complete log of this run can be
  found in" pointer, which is the line you actually want next — and a count of what was
  dropped. Every adapter and the `--json` document get this, not just npm.

- **A Python version mismatch warning now tells you the command that fixes it.** Warning
  that a venv was built with 3.12 while `python` on PATH is 3.14 left the reader to work
  out the rest. It now prints the rebuild command underneath, `uv venv --python 3.12`
  first because that one downloads the interpreter if the machine no longer has it, with
  the `py -3.12 -m venv` form after it.

- **`devp stats` no longer explains a version boundary that has stopped mattering.** The
  "per-repository totals are recorded from 1.1.0 onward" line printed on every run, long
  after everybody's numbers started at 1.1.0 anyway. It survives only in the case it was
  written for: when there are no per-repository figures at all yet.

- **Go is detected again.** dev-prune asked every package manager for its version with
  `<manager> --version`, and Go is the one that does not answer to it: `go --version`
  exits `2` with `flag provided but not defined: -version`, because the toolchain reads
  everything after `go` as a subcommand and the answer is `go version`. So on every
  machine with Go installed, dev-prune concluded Go was not.

  That was visible as `devp doctor` reporting `go ! not on PATH` next to a working `go`,
  and as `devp caches clear go` refusing to run. It was also invisible where it mattered
  most: before deleting a Go module cache the adapter runs `go mod download` to prove
  `go.sum` can rebuild it, and a manager it believes is absent falls back to the weaker
  "is `go.mod` newer than `go.sum`?" check instead. Nothing was ever deleted without
  *some* proof, but Go projects were pruned on the lesser one. They now get the real one.

- **`devp doctor` says how to install a package manager it cannot find.** The warning
  named the missing manager and stopped there, which left the one finding in the report
  that came with no repair. Each now carries where to get it —
  `go ! not on PATH … Install it: https://go.dev/dl/`.

- **`devp status` no longer fills up with repositories that no longer exist.** The Git
  hook registered every repository it saw its first commit in — including the throwaway
  ones that tools create constantly: a test fixture in `mktemp -d`, a plugin manager's
  checkout under `~/.claude/plugins/cache/temp_git_<id>`, anything under a `cache`,
  `tmp`, `temp` or `node_modules` ancestor. They are deleted minutes later and their
  registry entries are not, so one real dashboard reached thirty-four `Path missing` rows
  that nothing could prune and nothing could find. The hook now declines to register
  those unasked. `devp link` on one still works — this only stops it happening by
  itself — and `devp unlink --missing` clears out what is already there.

- **A repository no longer reads as "worked in today" because dev-prune wrote to it.**
  Last activity is the newest modification time in the tree, and dev-prune's own writes
  were counted as yours. `devp link` and `auto_config` write `.devprune.json`, so linking
  eighty repositories in one afternoon reset all eighty activity clocks to that
  afternoon; a `devp restore` stamps every file it puts back with the moment it ran. The
  effect was a dashboard where every date was the day you set the tool up and nothing
  ever went idle again — eighty repositories, zero candidates, and no error anywhere.
  Files dev-prune writes and directories package managers refill are now excluded from
  the scan.

- **`devp status` sorts by what you can act on.** Rows were ordered by path, which on
  Windows put every dead `C:\Users\…\Temp` entry above every live `V:\Code` one — the
  rows that mattered started below the fold. The order is now: reclaimable, then present
  but idle, then missing, and by path within each band.

- **`devp doctor` stops reporting an older release as an upgrade.** It compared the
  cached release string to the running version with `!=`, so a machine on 1.2.0 with
  1.1.0 still in the cache was told to upgrade *to 1.1.0*, and a development build one
  commit ahead of the tag was told the same. Versions are compared as versions now, and a
  build newer than the last published release is reported as exactly that.

- **The fix command for a failed lockfile check names the right directory.** It offered
  `cd "<repository root>"; uv lock`, but the project that failed is `backend/`, not the
  repository — so the command as printed either rebuilt a different project or found
  nothing to rebuild, in exactly the monorepos where working out which directory was
  meant is hardest. It now points at the project directory the adapter actually detected.
  The same failure reported from inside `devp status` used to print the error alone, with
  no fix command at all; it now prints what `devp run` prints.

- **Virtual-environment warnings print a path you can paste.** The two `venv` notices —
  an environment not named `.venv`, and one built against a different Python than the one
  on `PATH` — printed the raw Windows spelling, `\?\V:\Code\…`, which no shell accepts.
  Every other path in the tool was already cleaned; these three were not.

- **The scanner stops descending into dependency trees.** `deps/` is full of Elixir
  packages with their own `mix.exs`, and `.build/` of Swift checkouts with their own
  `Package.swift`, so a crawl would register a dependency as a project of its own and
  offer to prune inside something the parent rebuilds wholesale.

### For contributors

- Every dependency is on its current major again: `clap_mangen` 0.2 → 0.3 and `sha2` 0.10
  → 0.11. Both were the whole of libraries.io's `-1` for outdated dependencies, which is
  the only part of that score the repository itself controls. sha2 0.11 returns a
  `hybrid_array::Array` rather than a `GenericArray`, so the digest is hex-encoded by hand
  now; nothing else changed and the MSRV is untouched — the new crates all declare 1.85.

- `scripts/check-version.sh` now also checks `npm/package.json`, which had sat at `1.1.0`
  for three releases. `scripts/npm-prepare.sh` rewrites every version in it from the tag
  before publishing, so the stale number never reached the registry — it reached readers.
  The check pins the package count the prepare script asserts at the same time, so adding
  a platform package and forgetting the count fails loudly at packaging time rather than
  quietly at install time.
- CI's `cross` job is a matrix over both non-native Windows targets
  (`aarch64-pc-windows-msvc`, `i686-pc-windows-msvc`), and the `packaging` job fabricates
  seven assets instead of six, so the 32-bit path through both packaging scripts is
  exercised on every push rather than first at a tag.

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
- [`docs/ROADMAP.md`](docs/ROADMAP.md) is now a triaged roadmap — *in flight*, *next*,
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
