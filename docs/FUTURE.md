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

- **VS Code Marketplace and Open VSX listings** for the editor extension. The `.vsix` is
  built by CI and attached to every GitHub release, so side-loading works today
  (`code --install-extension dev-prune-vscode-<version>.vsix`); the marketplace upload
  needs a publisher token. Checklist in [`IDE_INTEGRATION.md`](IDE_INTEGRATION.md).
- **The npm channel.** The packaging is written and CI dry-runs the exact publish command
  on every push — seven packages, six platform binaries plus a dispatcher. The release
  job is gated on the `NPM_PUBLISH` variable and an `NPM_TOKEN` secret, and reports
  `skipped` rather than `success` until both exist. See [`RELEASING.md`](RELEASING.md).
- **Icon-theme pull requests** for the `.devprune.json` file icon, awaiting maintainer
  review upstream:
  [material-icon-theme#3567](https://github.com/material-extensions/vscode-material-icon-theme/pull/3567)
  and [vscode-icons#4223](https://github.com/vscode-icons/vscode-icons/pull/4223). Icon
  themes always win over an extension's own contribution, so this is the only route.
- **JetBrains plugin publishing.** The icon micro-plugin in `editors/jetbrains/` builds;
  the marketplace listing is the remaining step.

## Next

- **A Homebrew tap.** `brew tap` has no notability bar — that gate belongs to
  homebrew-core, further down this page. A personal tap is a formula repository plus one
  release job that rewrites the formula's `url` and `sha256` from the published macOS
  and Linux assets, which the release already produces and checksums.
- **A Scoop bucket.** Same shape: a JSON manifest in a personal bucket, no notability
  requirement, regenerated per release. The `extras` bucket wants sustained maintenance
  and is a separate decision.
- **WinGet.** `winget-pkgs` has **no popularity requirement** either — any stable,
  versioned installer qualifies. What it needs is a manifest pull request per release,
  which means the manifest generation has to be automated first or it will be forgotten
  on the release where it matters.
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
  costs a recompile rather than a download. Candidates, in rough order of demand:
  **Composer** (PHP, `vendor/` + `composer.lock`), **Bundler** (Ruby, `vendor/bundle` +
  `Gemfile.lock`), **pdm / pipenv** (Python — same `.venv` family as uv and Poetry, same
  conflict rules), **CocoaPods** (`Pods/` + `Podfile.lock`), **Mix** (Elixir, `deps/` +
  `_build/` + `mix.lock`), **Swift SPM** (`.build/`), and **Nix** (`result` symlinks are
  already refused for being symlinks; a real adapter would have to reason about the
  store).
- **A branded glyph in the VS Code status bar.** The extension uses built-in codicons
  because a status bar item can only render icon-font glyphs, not images. The route to a
  dev-prune mark is contributing an icon font (`contributes.icons`) built from the SVG.
- **More `--agent` targets** as editors standardise their rules files. The contribution
  is four small changes, documented in [`IDE_INTEGRATION.md`](IDE_INTEGRATION.md); an
  editor that adopts the `AGENTS.md` convention needs no code at all.
- **A single `devp trust` report** — every path dev-prune has written on this machine, in
  one place: the scheduled task, `core.hooksPath`, the config directory, the installed
  binaries. The information is not hidden today (`devp doctor` reports it, `devp
  uninstall` removes it, [`BACKGROUND_AUTOMATION.md`](BACKGROUND_AUTOMATION.md) documents
  it), so this is a presentation change and it competes with simply making `doctor`'s
  output better.
- **A unified `devp analyze` storage view** across repositories and shared caches. Parked
  behind the same question: `devp status` and `devp caches` between them already produce
  these numbers, so this earns its place only if the two genuinely fail to answer the
  question someone is asking.
- **A read-only Docker report.** Container images and volumes are real disk usage that a
  developer machine accumulates, and dev-prune could *name* what is there. It will never
  delete any of it — see the Not planned section for why — so the most this can ever be
  is another section of `devp caches`, printing what to run yourself.
- **Per-adapter idle gates** beyond `build_idle_days` — a general `idle_days.<adapter>`
  map. Waiting for a third tier that someone actually needs.
- **Man page packaging.** `devp man --dir` generates the pages; installing them
  pre-placed is a per-channel packaging question (deb, rpm, a Homebrew formula) rather
  than a CLI one, so it arrives with those channels or not at all.
- **Chocolatey.** Moderated review on every release. Same maintenance-cost reasoning as
  WinGet, one tier further out because the review is human.
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
- **Deleting Docker images, volumes or build cache.** Nothing about them is
  lockfile-recoverable. An image layer may be irreproducible the moment an upstream tag
  moves, and a volume is data, not a cache. `docker system prune` exists, is well
  understood, and its consequences are the user's to accept.
- **`devp caches --clear <name>`.** A package manager's cache is shared by every project
  on the machine, so no single lockfile can prove it recoverable. Printing each vendor's
  own clear command keeps the accountability where it belongs.
- **32-bit builds** — x86 (i686) or 32-bit ARM, on any platform. Every published target
  is 64-bit; see [`DISTRIBUTION.md`](DISTRIBUTION.md). Nothing in the source is
  64-bit-only, so `cargo install dev-prune` on a 32-bit toolchain remains open to anyone
  who needs it, but a target nobody is asking for is a build matrix entry, an asset name,
  two installer branches and a release surface to keep correct forever.
- **Updating across install channels.** `devp update` refuses to replace a binary that a
  different channel owns — a uv-installed dev-prune is not overwritten by the npm copy's
  updater. One channel owns one binary; anything else means two package managers
  disagreeing about what is installed, silently.
- **A bypass flag for any safety invariant.** The seven in
  [`SAFETY_INVARIANTS.md`](SAFETY_INVARIANTS.md) — the `.git` boundary, lockfile
  pre-verification, symlink refusal, atomic state writes and the rest — have no escape
  hatch and must not acquire one. A flag that turns off the proof turns dev-prune into
  `rm -rf` with extra steps, and the proof is the entire product.
