# Repository instructions

Conventions for anyone — human or agent — working in this repository.

`dev-prune` (second binary: `devp`) is a single-binary Rust CLI, edition 2024, MSRV
1.88. It reclaims disk space from idle Git repositories by deleting dependency and build
directories that a lockfile can rebuild, and refuses to delete anything it cannot prove
is recoverable.

---

## The gate

Nothing is done until all four pass. CI runs the same four on Linux, macOS and Windows.

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
npm --prefix site run build
```

`clippy` is `-D warnings`, including in test code — `--all-targets` is not optional.

Tests must never touch the machine they run on. `DEV_PRUNE_NO_AUTO_SETUP=1` disables
every self-installation path and `DEV_PRUNE_CONFIG_DIR` relocates the config directory;
any test that could otherwise register a scheduled task, rewrite `core.hooksPath`, or
write to the real registry sets both.

---

## Layout

| Path | What lives there |
|---|---|
| `src/commands/` | One module per subcommand. Each owns its own argument handling and output. |
| `src/adapters/` | One module per package manager, all implementing `PackageManager`. |
| `src/scanner/`, `src/engine.rs` | Repository discovery and the prune pass itself. |
| `src/tui/`, `src/output.rs`, `src/json.rs` | Human output, and the `--json` contract. |
| `src/daemon/`, `src/setup.rs` | OS schedulers and git-hook integration. |
| `src/constants.rs` | Every env var name, path fragment and magic string. Add here, not inline. |
| `tests/` | `cli_contract_test.rs` (flags and exit codes), `integration_test.rs`, `monorepo_test.rs`. |
| `docs/` | User and maintainer documentation. `docs/README.md` is the index. |
| `scripts/` | Installers, the packaging scripts, and CI helpers. |
| `.agents/skills/dev-prune/` | The AI skill definition, embedded into the binary at build time. |

Adding an adapter is documented end to end in
[`docs/ADDING_ADAPTERS.md`](docs/ADDING_ADAPTERS.md).

---

## CHANGELOG.md

**`CHANGELOG.md` is the release notes.** It is not a summary of them, and it is not
generated from commits. `scripts/changelog-section.sh <version>` extracts one version's
section and is called from three places:

1. CI, on every push — fails if `CHANGELOG.md` has no section for the version in
   `Cargo.toml`.
2. The release build, before compiling anything.
3. The release publish — the extracted section *becomes* the GitHub release body.

So the changelog entry and the release note cannot drift apart, because they are the
same text. Write the entry as the thing users will read on the release page.

### Format the tooling depends on

```markdown
## [1.1.0] - 2026-09-01

### Added

- **`devp run --except <repos>`** prunes everything *but* the repositories you name, so
  "clean up but keep the API project" no longer means pruning it and downloading it back.

### Fixed

- `devp unlink --missing` now clears the undo list too. Previously a deleted repository
  stayed in it, and the next `devp undo` reported that it had removed nothing.
```

- Heading is exactly `## [<version>] - <YYYY-MM-DD>`. The extractor matches
  `## [<version>]` as a literal prefix, so `1.0.0` never matches `1.0.0-rc1`.
- A section runs until the next `## ` heading.
- Subsections are `### Added` / `### Changed` / `### Fixed` / `### Removed`. A large
  release may group by subsystem instead (see the 1.0.0 entry) — but be consistent
  within one version.
- The version must match `Cargo.toml` before tagging.
- Dates are absolute. Never "today".

Check what will be published before tagging:

```bash
sh scripts/changelog-section.sh 1.1.0
```

### Voice

Every entry should answer three questions. An entry that answers fewer than two needs
rewriting:

1. **What changed?** — name the flag, command or behaviour.
2. **Why should I care?** — the situation it fixes, in the user's terms.
3. **How do I use it?** — the command, or a link to the reference.

Lead with what the user can now *do*. "You can now…", not "Refactored the…". An entry
that reads like a commit message is not finished. Bold the command or flag at the start
of an `Added` entry so the section scans.

Changes that only matter to contributors go under a `### For contributors` subsection,
not mixed in with user-facing ones.

### Never

- **Never rewrite or reorder existing entries.** Fix wording in place with an exact-match
  edit; do not regenerate a section. Published versions describe what shipped, and what
  shipped does not change.
- **Never overwrite `CHANGELOG.md` wholesale.** Edit it.
- **Never add an entry describing a bug that was introduced and fixed without ever being
  released.** The changelog records what changed for users between releases, not what
  happened during development.

`/document-release` (gstack) polishes changelog voice against this rubric and will not
clobber existing entries.

---

## Documentation

Docs describe what the code does *now*. There is no "planned", "coming soon" or
"currently in works" section — if something is not built, it is not in the docs.

- Every doc must be reachable from [`docs/README.md`](docs/README.md) or the root
  `README.md`. A doc nobody links to is a doc nobody reads.
- Mermaid diagrams are parsed in CI (`scripts/check-mermaid.mjs`). A malformed diagram
  renders as a red box on GitHub and nothing else notices, so the parse is the only
  thing that catches it.
- Flags, config keys and exit codes documented in `docs/CLI_REFERENCE.md` must match
  `src/`. The same facts also appear in `.agents/skills/dev-prune/SKILL.md`,
  `site/public/llms.txt` and `site/src/App.jsx` — changing one means changing all of
  them.
- The MSRV has one source of truth: `rust-version` in `Cargo.toml`. The binary
  (`constants::MSRV`), CI's msrv job and the release install table all read it from
  there; the few docs that restate the number by hand are checked against it by
  `scripts/check-msrv.sh` in CI. To bump the MSRV, change `rust-version` and let that
  check list the files to touch.
- `schemas/` holds the JSON schema for `.devprune.json`; a new config key goes there too.

---

## Code conventions

- **Comments explain why, never what.** A comment that restates the line above it is
  noise. A comment that records the failure that motivated the code is worth keeping
  forever.
- **Backwards compatibility starts at 1.0.0**, which is published and permanent on
  crates.io and PyPI. The CLI surface — flag names, exit codes, the `--json` shape, the
  config keys — is something people can now depend on, so breaking any of it is a major
  version and never a patch. Nothing *internal* is protected: no migration paths for
  versions that never shipped, and no `#[allow(deprecated)]` shims for spellings that
  only ever existed in this repository. Change those outright.
- **Exit codes are a contract**: `0` success, `1` failure, `2` usage error. `devp doctor`
  exits `0` for warnings and `1` only for genuine breakage. `tests/cli_contract_test.rs`
  enforces this.
- **Safety invariants are not negotiable.** The seven listed in
  [`docs/SAFETY_INVARIANTS.md`](docs/SAFETY_INVARIANTS.md) — `.git` boundary, lockfile
  pre-verification, symlink refusal, atomic state writes, and the rest — have no bypass
  flag and must not acquire one.
- **Every source file opens with the licence header.** Two lines — `Copyright 2026
  VKrishna04` and `SPDX-License-Identifier: Apache-2.0` — in the language's comment
  syntax, below the shebang if there is one. New files included; a scanner that finds one
  file without it reports the whole repository as mixed-licence.
- **New strings go in `src/constants.rs`.** Env var names and path fragments especially:
  they are also referenced from `npm/bin/dev-prune.js`, `scripts/install.sh` and
  `scripts/install.ps1`, and a typo in one of those is invisible until someone hits it.

---

## Releasing

One `git push` of a tag does everything. The full process, every credential the
automation needs, and which registries actually review submissions:
[`docs/RELEASING.md`](docs/RELEASING.md).
