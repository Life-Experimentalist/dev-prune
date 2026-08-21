# Future possibilities

Everything else in `docs/` describes what the code does **now** — that rule is in
[`CLAUDE.md`](../CLAUDE.md) and it holds. This page is the one sanctioned exception: a
single list of the directions that have been considered and parked, so they are not
re-litigated from scratch each time, and so nobody mistakes a parked idea for a shipped
feature. Nothing here is promised, ordered, or scheduled.

---

## Distribution channels

The channels that already work are in [`DISTRIBUTION.md`](DISTRIBUTION.md); the gated
ones and what gates them are in [`RELEASING.md`](RELEASING.md). The short version:

- **WinGet** — `winget-pkgs` has **no popularity requirement**; any stable, versioned
  installer qualifies. What it needs is a manifest PR per release and an MSI/installer
  story, plus the automation to keep manifests current. Parked until the release
  process has room for one more registry, not until a star count.
- **Scoop** — same situation: a JSON manifest in a bucket, no notability bar. A
  personal bucket could ship today; the `extras` bucket wants sustained maintenance.
- **Homebrew (core)** — this is the one with a real notability gate: formulae are
  expected to be "notable" (the audit checks GitHub stars, forks and watchers — the
  commonly-cited threshold is roughly 75). A personal tap works at any size;
  homebrew-core waits for the project to clear that bar.
- **Chocolatey** — moderated review per release; parked for the same maintenance-cost
  reason as WinGet.

## More adapters

The trait, registration and test recipe are in
[`ADDING_ADAPTERS.md`](ADDING_ADAPTERS.md); the opt-in mechanism (`opt_in()`,
`enable_*` settings, `build_idle_days`) now exists for anything whose deletion is a
recompile rather than a re-download. Natural candidates, in rough order of demand:

- **Composer** (PHP, `vendor/` + `composer.lock`)
- **Bundler** (Ruby, `vendor/bundle` + `Gemfile.lock`)
- **pdm / pipenv** (Python — same `.venv` family as uv/poetry, same conflict rules)
- **CocoaPods** (`Pods/` + `Podfile.lock`)
- **Nix** (`result` symlinks are already refused as symlinks; a real adapter would
  reason about the store)
- **Mix** (Elixir, `deps/` + `_build/` + `mix.lock`), **Swift SPM** (`.build/`)

Permanently out of scope, not future: `dist/`, `.next/`, `.nuxt/` and any
gitignore-driven deletion rule. Those are outputs no lockfile or manifest can prove
recoverable, and "delete whatever is gitignored" deletes `.env` files.

## Editor and agent integrations

- **A real dev-prune icon in the VS Code status bar.** The extension uses built-in
  codicons today because status bar items can only render icon-font glyphs, not
  images. The path to a branded glyph is contributing an icon font
  (`contributes.icons`) built from `assets/devprune.svg`.
- **Icon-theme PRs** for `.devprune.json` file icons are submitted and awaiting
  review: [material-icon-theme#3567](https://github.com/material-extensions/vscode-material-icon-theme/pull/3567)
  and [vscode-icons#4223](https://github.com/vscode-icons/vscode-icons/pull/4223) —
  icon themes always win over extension contributions, so this is the only route.
- **JetBrains plugin publishing** — the icon micro-plugin in `editors/jetbrains/`
  builds; publishing waits on the marketplace listing (checklist in
  [`IDE_INTEGRATION.md`](IDE_INTEGRATION.md)).
- **SchemaStore submission** for `.devprune.json`, which would light up IntelliSense
  in JetBrains, Visual Studio, Neovim and Zed with nothing installed.
- **More `--agent` targets** as editors standardise their rules files — the
  contribution recipe is four small changes, documented in
  [`IDE_INTEGRATION.md`](IDE_INTEGRATION.md). Editors adopting the `AGENTS.md`
  convention need no code at all.

## CLI and engine

- **`devp caches --clear <name>`** has been considered and rejected so far: a cache is
  shared by every project on the machine, so no lockfile can prove it recoverable —
  printing each vendor's own clear command keeps the accountability where it belongs.
- **Per-adapter idle gates beyond `build_idle_days`** (a general
  `idle_days.<adapter>` map) — parked until someone actually needs a third tier.
- **Man page packaging** — `devp man --dir` generates the pages; shipping them
  pre-installed is a per-channel packaging question (deb/rpm/Homebrew formula
  territory) rather than a CLI one.
