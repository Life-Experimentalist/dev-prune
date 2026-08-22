# Future possibilities

Everything else in `docs/` describes what the code does **now** — that rule is in
[`CLAUDE.md`](../CLAUDE.md) and it holds. This page is the one sanctioned exception: the
directions that have been considered, so they are not re-litigated from scratch every
time someone has the idea again, and so nobody mistakes a parked idea for a shipped
feature.

Nothing here is a promise or a date. What it does say is **which tier something is in**,
because "not built yet" covers four very different situations:

| Tier | Means |
|---|---|
| **In flight** | Started. Blocked on someone else's review, or on a credential only the maintainer holds. |
| **Next** | Agreed direction, understood work, nothing in the way but time. These get picked up first. |
| **Later** | Worth doing eventually. Has not yet earned its complexity, or is waiting for a second person to actually want it. |
| **Not planned** | Considered and declined, with the reason. Reopening one of these needs a new argument, not a new request. |

An item leaves this page when it ships — [`CHANGELOG.md`](../CHANGELOG.md) is the record
of what happened, not this file. If you are looking for something that used to be here,
that is where it went.

---

## In flight

Work that exists and is waiting on a party that is not this repository.

- **Icon-theme pull requests** for the `.devprune.json` file icon, awaiting maintainer
  review upstream:
  [material-icon-theme#3567](https://github.com/material-extensions/vscode-material-icon-theme/pull/3567)
  and [vscode-icons#4223](https://github.com/vscode-icons/vscode-icons/pull/4223). Icon
  themes always win over an extension's own contribution, so this is the only route.

## Next

- **The npm channel.** The packaging is written and CI dry-runs the exact publish command
  on every push — seven packages, six platform binaries plus a dispatcher. The release
  job is gated on the `NPM_PUBLISH` variable and an `NPM_TOKEN` secret, and reports
  `skipped` rather than `success` until both exist, so switching it on is one variable
  and one secret. See [`RELEASING.md`](RELEASING.md).
- **`wingetcreate submit` in the release job.** The first WinGet submission is open
  ([microsoft/winget-pkgs#422665](https://github.com/microsoft/winget-pkgs/pull/422665)).
  Once it is merged the package identifier exists, and every release after it can raise
  its own pull request from a PAT instead of a person copying three files — see
  [`RELEASING.md`](RELEASING.md).
- **homebrew-core.** Plain `brew install dev-prune`, with no tap prefix. It has a real
  notability bar measured in stars, forks and watchers, so it is a post-popularity step
  rather than a task. The named tap covers the same install today.
- **Four more adapters: Terraform, Flutter/Dart, Mix and Nix.** `.terraform/` holds
  downloaded providers that `terraform init` restores from the lock file; `.dart_tool/`
  and `_build/` are the same shape for `pub get` and `mix deps.get`. Each has a lockfile
  that proves the directory is recoverable, which is the only bar an adapter has to
  clear. Nix is the odd one and is being looked at last, because a store path is shared
  between projects and "recoverable" there means something different.
- **`devp caches` for Composer, CocoaPods and Hex.** The three managers already covered
  by adapters whose *download* caches are not yet listed. Each entry has to ask the
  manager where its cache actually is on this platform rather than hard-coding a path —
  `composer config --global cache-dir`, and the equivalent for the other two. Bundler's
  shared gem home and Pipenv's virtualenv directory are deliberately **not** candidates:
  those are install locations, not caches, and deleting one uninstalls software.
- **A branded glyph in the VS Code status bar.** The extension currently borrows a
  built-in codicon. Its own mark needs `contributes.icons` and an icon *font* — VS Code
  will not take an SVG there — so the work is building a single-glyph font from
  `assets/devprune.svg` and referencing it by ID, which then also works anywhere a
  codicon does.
- **Restore-cost estimates.** dev-prune knows what it deleted and can time what it takes
  to put back. Recording restore durations locally would let `devp status` answer the
  question people actually hesitate over — not "how much space is this", which it already
  answers, but "how long is this to undo". Local timings only; nothing is uploaded, in
  keeping with [`PRIVACY.md`](PRIVACY.md).
- **Comparison and reference pages on the site.** The content largely exists in
  [`MARKET_ANALYSIS.md`](MARKET_ANALYSIS.md); what is missing is somewhere to put it. The
  site is a single prerendered page with no router, so this is a structural change to
  `site/`, not a writing task, and that is the reason it has not happened rather than
  disagreement about its value.

## Later

- **More adapters.** The trait, registration and test recipe are in
  [`ADDING_ADAPTERS.md`](ADDING_ADAPTERS.md), and the opt-in mechanism (`opt_in()`,
  `enable_*` settings, `build_idle_days`) already exists for anything whose deletion
  costs a recompile rather than a download. What is left after the 1.4.0 batch, in
  rough order of how ready each one is:

  - **Terraform.** `.terraform/` is a provider cache with a real lockfile beside it
    (`.terraform.lock.hcl`), and `terraform init` puts back exactly what it records.
    That is the same relationship `package-lock.json` has with `node_modules`, which
    makes this the closest thing on the list to a drop-in.
  - **Flutter and Dart.** `.dart_tool/` holds package resolution *and* build output in
    one directory, so it cannot be claimed under the plain download proof — it needs
    the opt-in treatment Gradle and Maven get, and a decision about whether the
    resolution half is worth separating.
  - **Mix's `_build/`.** The Elixir adapter deletes `deps/` today. `_build/` is a
    compiled tree, so it is opt-in territory too, and it is waiting on someone who
    actually wants it.
  - **Nix.** `result` symlinks are already refused for being symlinks; a real adapter
    would have to reason about the store, which is a different kind of problem.
- **`devp caches` coverage for the newer ecosystems.** The table resolves thirteen
  managers — npm through conan — and Composer, CocoaPods and Hex each keep a real,
  clearable download cache that is not among them (`composer clear-cache`,
  `pod cache clean --all`). Adding them is mechanical; it sits here rather than in
  Next only because each entry has to *ask* the manager where its cache is on each
  platform rather than assume, the way the existing thirteen do. Bundler's shared gem
  home and Pipenv's virtualenv directory are explicitly **not** candidates: those are
  install locations, not caches, and emptying one uninstalls software.
- **JetBrains plugin publishing.** The icon micro-plugin in `editors/jetbrains/` builds;
  the marketplace listing is the remaining step. It needs a JDK and downloads the
  IntelliJ platform on first build, so it is deliberately outside the repository gate.
- **A branded glyph in the VS Code status bar.** The extension uses built-in codicons
  because a status bar item can only render icon-font glyphs, not images. The route to a
  dev-prune mark is contributing an icon font (`contributes.icons`) built from the SVG.
- **More `--agent` targets** as editors standardise their rules files. Fifteen ship
  today; adding the sixteenth is four small changes documented in
  [`IDE_INTEGRATION.md`](IDE_INTEGRATION.md), and an editor that adopts the `AGENTS.md`
  convention needs no code at all. This entry stays open permanently — it is a standing
  invitation, not a backlog item.
- **A unified `devp analyze` storage view** across repositories and shared caches. Asked
  again in 1.4.0 and parked again, for the same reason: `devp status` already ranks the
  repositories and `devp caches` already ranks the machine-wide stores, both largest
  first with totals. A third command that adds the two lists together would be a new
  name, a new `--json` shape and a third place for the numbers to disagree, in exchange
  for a sum a reader can do. It earns its place the day someone shows a question the two
  existing reports genuinely cannot answer.
- **A read-only Docker report.** Container images and volumes are real disk usage that a
  developer machine accumulates, and dev-prune could *name* what is there. It will never
  delete any of it — see the Not planned section for why — so the most this can ever be
  is another section of `devp caches`, printing what to run yourself.
- **Man page packaging.** `devp man --dir` generates the pages; installing them
  pre-placed is a per-channel packaging question (deb, rpm, a Homebrew formula) rather
  than a CLI one, so it arrives with those channels or not at all.
- **Chocolatey.** Moderated review on every release, and unlike WinGet nothing is
  generated for it today, so it is a packaging format to write as well as a queue to
  wait in. Worth it only if Windows users ask for it by name.
- **Homebrew core.** The one channel with a real notability gate: the audit weighs GitHub
  stars, forks and watchers, and the commonly cited threshold is roughly 75. Nothing to
  build — a tap covers the functionality — so this is a waiting item, not a work item.

## Not planned

Declined with reasons, so they stay declined.

- **Build outputs and gitignore-driven deletion** — `dist/`, `.next/`, `.nuxt/`, and any
  rule of the form "delete what git ignores". No lockfile or manifest can prove an output
  directory is reproducible, and "delete whatever is gitignored" deletes `.env` files.
  This is the boundary the whole tool is built around, not a gap in it. Gradle's `build/`
  and Maven's `target/` are not an exception to it: they are opt-in, off by default, and
  gated behind their own longer idle window precisely because they are rebuilt rather
  than re-downloaded.
- **Clearing a cache from anything that runs on its own** — a prune pass, the scheduler,
  a Git hook. A cache is shared by every project on the machine, so no single lockfile
  can prove it recoverable, which is the bar every automatic deletion in dev-prune has to
  clear. `devp caches clear <manager>` exists and empties one after asking, but only ever
  because you typed it.
- **Deleting Docker images, volumes or build cache.** Nothing about them is
  lockfile-recoverable. An image layer may be irreproducible the moment an upstream tag
  moves, and a volume is data, not a cache. `docker system prune` exists, is well
  understood, and its consequences are the user's to accept.
- **32-bit builds beyond Windows** — 32-bit Linux, 32-bit macOS or 32-bit ARM anywhere.
  `i686-pc-windows-msvc` ships as of 1.4.0 because it is a plain rustup target on a
  runner that already exists and Windows is the one place 32-bit hardware is still in
  service. The others are not the same trade: macOS has been unable to run a 32-bit
  binary since Catalina, `i686-unknown-linux-musl` needs a cross musl toolchain to serve
  a desktop population that has effectively gone, and 32-bit ARM has no runner at all.
  `cargo install dev-prune` on any of those toolchains works — nothing in the source is
  64-bit-only.
- **One channel overwriting another channel's binary.** This is narrower than it sounds,
  and it is worth stating precisely because the obvious reading is wrong. `devp update`
  always updates the copy that is running: it works out which channel installed that copy
  and runs *that* channel's upgrade command — `uv tool upgrade dev-prune` for a uv
  install, `npm install -g dev-prune@latest` for an npm one, a fresh download for the
  installer's managed copy. Nothing is refused and nothing needs a second tool.

  What is declined is having one channel write over a file another channel owns. The
  binary is not the whole install: uv, pipx, npm and cargo each keep a manifest saying
  which version they put there. Overwrite the file behind their back and the manifest
  still says the old version — so the next `uv tool upgrade` reports "already up to
  date" and, on the next reinstall, quietly puts the old binary back. The file would be
  new and the package manager's record of it would be a lie. Delegating to the owning
  channel keeps the file and the record saying the same thing.
- **A trust score, a letter grade, or a "Trust level: HIGH" badge.** Suggested for
  both `devp trust` and the site. A grade is a summary that replaces the evidence it
  summarises: it reads as a rating the tool awarded itself, a reader cannot check it,
  and it would go on saying HIGH on the day a bug made it false. So `devp trust`
  prints the guarantees and the actual state of *this* machine instead — which
  schedulers are registered, whether the Git hook is installed, what the config
  currently permits — because every line of that is something you can go and verify.
  Nothing is condensed into a number.
- **Accounts, cloud sync, telemetry, a subscription tier, ads, or a web dashboard.**
  Proposed as growth surface. dev-prune is a local binary that deletes local
  directories: it has no server, sends nothing anywhere, and nothing about reclaiming
  disk space needs a login. Any of them would mean the tool learns which repositories
  exist on your machine — precisely the thing a disk-cleaning utility has no business
  knowing off your disk.
- **A desktop GUI, and "AI-powered cleanup".** The interactive parts of dev-prune are
  terminal views sitting next to the terminal work they belong to, and the VS Code
  extension already covers "I want a button" inside the editor that is open anyway.
  As for the second half: there is no judgement call in a prune pass for a model to
  make. A lockfile either proves a directory is recoverable or it does not. Selling
  that deterministic check as AI would be a lie about the one thing the tool is for.
- **Growing into a general "developer storage manager"** — Xcode DerivedData,
  simulator runtimes, Android SDK images, browser and IDE caches, the Downloads
  folder. The review framed it as the obvious next market. The problem is that
  dev-prune's whole claim is that it deletes nothing it cannot prove comes back from a
  lockfile, and none of those have one. Shipping them under the same command would
  mean the promise on the front page no longer covered everything the command does,
  which is a worse outcome than not shipping them. Machine-wide stores that *are*
  package-manager caches are reported — never deleted on a schedule — by
  `devp caches`, and that is the boundary.
- **A live "your projects are wasting 47.3 GB" figure on the homepage.** The site
  cannot see your disk, so a number there is either invented or somebody's average
  presented as your situation. `devp status` computes the real one, on your machine,
  in seconds. A landing page that fabricates the number in order to sell the tool that
  measures it honestly is arguing against itself.
- **Download counts, star counts, "trusted by N developers", or any adoption badge.**
  Proposed for the README and the site. Every one of them would be a number nobody is
  actually counting: registries report installs rather than people, a download badge
  inflates on CI reruns and therefore measures CI, and Open VSX publishes downloads —
  labelling that figure "users" would simply be false. If this project ever states
  something about its own adoption, it will be a number the reader can look up at the
  source that publishes it.
- **A bypass flag for any safety invariant.** The seven in
  [`SAFETY_INVARIANTS.md`](SAFETY_INVARIANTS.md) — the `.git` boundary, lockfile
  pre-verification, symlink refusal, atomic state writes and the rest — have no escape
  hatch and must not acquire one. A flag that turns off the proof turns dev-prune into
  `rm -rf` with extra steps, and the proof is the entire product.
