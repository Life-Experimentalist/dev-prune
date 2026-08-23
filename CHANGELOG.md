# Changelog

All notable changes to `dev-prune` (`devp`) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.7.0] - 2026-08-23

The output stops shouting, the first run stops assuming you already know everything, and
the tool keeps itself up to date.

### Added

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

### For contributors

- **CodeQL runs on every push, every pull request and weekly.** `cargo audit` answers
  whether a dependency is known-vulnerable and says nothing about the code in this
  repository; this is the other half. It also analyses `.github/workflows`, which holds
  more privilege than anything else here — a release job carries a PyPI identity, a
  crates.io token and `contents: write` — and had nothing looking at it. The scheduled run
  matters as much as the push one: the code will not have changed, but the queries will
  have.

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
  and [`docs/FUTURE.md`](docs/FUTURE.md) records why.

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
