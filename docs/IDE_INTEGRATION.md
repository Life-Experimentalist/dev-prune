# IDE & Editor Integration

How `.devprune.json` gets IntelliSense, validation and a recognizable icon in editors —
what already works today with no extension installed, what lives in `editors/`, and the
maintainer checklist for publishing each piece. Written in the same spirit as
[RELEASING.md](RELEASING.md): the parts a human must do are marked as such.

---

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

---

## The name is `.devprune.json`, permanently

`.dev-prune.json` was considered and rejected. The filename shipped in 1.0.0 and is
part of the backwards-compatibility contract: it is written into users' repositories
and their git exclude files, `ignore.devprune.json` derives from it, the Linux MIME
type is `application/x-devprune`, and the freedesktop icon names
(`application-x-devprune.png`) encode it. Every integration below must use
`.devprune.json` exactly.

---

## What works today, with nothing installed

- **The schema is the single source of truth.** [`schemas/devprune.schema.json`](../schemas/devprune.schema.json)
  defines every key with a `description` (rendered as hover tooltips), types, ranges,
  and `additionalProperties: false`. A new config key means editing that file in the
  same commit as `src/config.rs` — [CLAUDE.md](../CLAUDE.md) says so.
- **It is hosted at a stable URL:** <https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json>.
  The site build copies the canonical file into `site/public/schemas/v1/` on every
  build (`site/scripts/sync-schema.mjs`, wired as `prebuild`), so the hosted copy
  cannot drift from the one the CLI parses. It once did — the hosted file advertised
  removed keys and rejected real ones — which is why the copy is automated rather than
  remembered.
- **Every file the CLI writes links the schema.** `devp config project` writes a
  `$schema` key pointing at the local copy `devp setup` installs (falling back to the
  hosted URL), so VS Code and every JetBrains IDE already give autocomplete and
  validation on generated files with no extension involved.
- **OS file managers show the icon.** `devp icon` registers `*.devprune.json` with
  Explorer / Finder / Nautilus — a real `shared-mime-info` type plus hicolor icons on
  Linux. This is file-manager integration, not IDE integration, and it stays: nothing
  in `devp setup` patches any editor's settings, so there is nothing to remove on that
  front.

The gap the pieces below close: **hand-written** files (no `$schema` key) in editors
that do not subscribe to SchemaStore, and the **icon inside IDE file trees**.

---

## SchemaStore (covers JetBrains, Visual Studio, Neovim, Zed, and more)

One merged PR to [SchemaStore](https://github.com/SchemaStore/schemastore) gives every
subscribed editor schema-by-filename, no `$schema` key needed.

**Status: live** — [SchemaStore/schemastore#6226](https://github.com/SchemaStore/schemastore/pull/6226)
merged and published in the catalog, so every subscribed editor now resolves
`.devprune.json` by filename with no `$schema` key. The steps below record what the entry
contains, for the day it needs updating.

1. Fork `SchemaStore/schemastore`.
2. Add this entry to `src/api/json/catalog.json` (alphabetical by `name`):

   ```json
   {
     "name": ".devprune.json",
     "description": "Per-repository configuration for dev-prune, the lockfile-verified workspace cleaner",
     "fileMatch": [".devprune.json"],
     "url": "https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json"
   }
   ```

3. Their CI validates that the URL serves a parseable schema. Externally-hosted schemas
   are accepted; hosting stays ours, so schema fixes never wait on a SchemaStore PR.
4. Do **not** add `ignore.devprune.json` to `fileMatch`: that file's contents are
   irrelevant by design (presence alone opts the repository out) and it is usually
   empty, which a JSON schema would flag as an error.

**The merged entry predates `project.devprune.json` and does not list it.** Its
`fileMatch` is `[".devprune.json"]` only, so a subscribed editor gives the committed
file no IntelliSense by filename. Nothing is broken by that: `devp config project
<PATH> --team` writes the `$schema` line into the file it creates, and every editor
here prefers an in-file `$schema` over any catalog match. It costs a hand-written
`project.devprune.json` its autocomplete until someone adds the name — which is a
one-line PR against the entry above, and the reason that entry is recorded here.

The URL in that entry is the one thing here this repository cannot change on its own —
moving the published path needs a second SchemaStore PR, and until it merges every
subscribed editor is asking for a file that is no longer served. `scripts/check-schema.sh`
runs in CI and refuses a mismatch between the schema's `$id`, `constants::JSON_SCHEMA_URL`
and the path `site/public/` publishes at, so the path cannot move quietly. It checks the
same thing about content: the extension's bundled copy and the published copy must both
equal `schemas/devprune.schema.json`. The two sync scripts regenerate them, but only at
build and packaging time — which is after the commit, so without this check a schema
change can merge with the other two copies still describing the previous one.

---

## VS Code family (VS Code, VSCodium, Cursor, Windsurf)

[`editors/vscode/`](../editors/vscode/) holds a near-zero-code extension: a
`package.json` with a `jsonValidation` contribution mapping `.devprune.json` to a
**bundled** copy of the schema, the marketplace icon, and one small `extension.js`
whose sole job is a one-time "devp is not on PATH" notification when a workspace
contains `.devprune.json` but the CLI is missing (trusted workspaces only). Bundling (added in extension 0.2.0) means
hand-written files validate offline, instantly, with nothing to go stale in VS Code's
remote-schema cache; files the CLI writes carry an in-file `$schema` link, which VS
Code prefers, so those keep tracking the hosted URL. The bundled copy cannot drift:
`sync-schema.mjs` runs as `vscode:prepublish`, so every packaging pass — CI or by
hand — refreshes it from `schemas/devprune.schema.json` first. `npx @vscode/vsce
package` in that directory produces the `.vsix` (verified working end to end in a live
VS Code).

**The extension releases on its own tags, not with the CLI.** It has its own version in
`editors/vscode/package.json`, its own changelog in `editors/vscode/CHANGELOG.md` and
its own workflow, `.github/workflows/release-extension.yml`, triggered by
`vscode-v<version>`. The CLI ships often and the extension rarely, so riding along with
every `v*` tag meant republishing an identical package over itself most of the time —
and shipping an extension fix meant cutting a CLI release with nothing in it. Its
release page carries `dev-prune-vscode-<version>.vsix`, the side-load path
(`code`/`codium`/`cursor --install-extension <file>`) for editors that cannot reach a
marketplace, and it is the exact file both marketplaces are published from.

That release is deliberately **not** marked *latest*. `devp update` asks GitHub for the
latest release and reads the version out of its tag; a `vscode-v0.4.0` sitting there
would leave every installed copy unable to compare its own version. The extension
fallback in `devp setup` walks the release list for the newest `vscode-v*` instead.

**Deliberate deviation from the obvious plan:** the extension does *not* use
`contributes.languages` to claim the filename with an icon. Declaring a new language id
for `.devprune.json` would detach the file from VS Code's built-in JSON language
server — killing exactly the autocomplete and validation the extension exists to
provide — and the language-level icon only shows when the active file-icon theme has no
mapping of its own, which the default theme always does. The icon in the *file tree*
comes from icon themes, and the way real config files get one is upstream PRs:

- [material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) —
  add a `.devprune.json` file association (this is the icon most VS Code users see).
- [vscode-icons](https://github.com/vscode-icons/vscode-icons) — same.

Both PRs are submitted (2026-08-20) and await maintainer review:
[material-icon-theme#3567](https://github.com/material-extensions/vscode-material-icon-theme/pull/3567)
(leaf logo recolored to the Material palette, green A400 → cyan A400, as their
guidelines require) and
[vscode-icons#4223](https://github.com/vscode-icons/vscode-icons/pull/4223)
(brand gradient kept, filed against
[icon request #4222](https://github.com/vscode-icons/vscode-icons/issues/4222)).

**Status: published on both** —
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=VKrishna04.dev-prune)
and [OpenVSX](https://open-vsx.org/extension/VKrishna04/dev-prune), publisher
`VKrishna04`, extension ID `VKrishna04.dev-prune`. Each carries whatever the last
`vscode-v*` tag published; the two listings are the source of truth for that, not this
page.

**If validation goes quiet** — a file full of stale keys showing zero problems — the
cause is almost always VS Code's remote-schema cache: the JSON language server keeps a
downloaded schema for the life of the window, so a window opened before a schema fix
deployed validates against the pre-fix copy indefinitely. *Developer: Reload Window* or
*JSON: Clear Schema Cache* clears it. This is exactly why the extension bundles the
schema — only files with their own `$schema` link still fetch remotely.

**Publishing a new version (maintainer):**

1. Bump `version` in `editors/vscode/package.json`. Nothing derives it from the CLI's
   version and nothing should — they are two products with two changelogs.
2. Add a `## [<version>] - <YYYY-MM-DD>` section to `editors/vscode/CHANGELOG.md`. The
   workflow extracts it with the same `scripts/changelog-section.sh` the CLI uses, and
   it *becomes* the release body, so write it for the person reading the release page.
3. Commit, then tag and push:

   ```bash
   git tag -a vscode-v0.4.0 -m "vscode-v0.4.0"
   git push origin vscode-v0.4.0
   ```

The workflow refuses the tag if it disagrees with `package.json` — the marketplaces
take the version from the manifest and ignore the tag entirely, so a mismatch would
produce a release page named one thing and two listings named another. Both uploads
matter: VS Code and the forks on Microsoft's gallery read one registry, VSCodium,
Cursor, Windsurf and the rest read Open VSX.

By hand, if the workflow is not an option: `npx @vscode/vsce publish` from
`editors/vscode/` (Azure DevOps PAT with Marketplace → Manage scope), or upload the
`.vsix` in the [manage UI](https://marketplace.visualstudio.com/manage/publishers/VKrishna04);
then `npx ovsx publish dev-prune-vscode-<version>.vsix -p <token>` for Open VSX.

---

## AI-agent rules: `devp skill --agent <editor>`

Editors whose agents read per-repository rule files get theirs written by the CLI, not
by an extension. `devp skill --agent <editor>` writes the embedded rules
(`.agents/rules/dev-prune.rules.md`, compiled into the binary) into the file that
editor's agent actually reads:

| Editor | File |
|---|---|
| `cursor` | `.cursor/rules/dev-prune.mdc` (with Cursor's rule frontmatter) |
| `windsurf` | `.windsurf/rules/dev-prune.md` |
| `antigravity` | `.agent/rules/dev-prune.md` (Gemini Antigravity) |
| `cline` | `.clinerules/dev-prune.md` |
| `roo` | `.roo/rules/dev-prune.md` (Roo Code) |
| `kilocode` | `.kilocode/rules/dev-prune.md` (Kilo Code) |
| `continue` | `.continue/rules/dev-prune.md` |
| `amazon-q` | `.amazonq/rules/dev-prune.md` (Amazon Q Developer) |
| `kiro` | `.kiro/steering/dev-prune.md` |
| `trae` | `.trae/rules/dev-prune.md` |
| `junie` | `.junie/guidelines.md` — a marked block (JetBrains Junie) |
| `gemini` | `GEMINI.md` — a marked block (Gemini CLI) |
| `zed` | `.rules` — a marked block; Zed reads it ahead of every other convention |
| `copilot` | `.github/copilot-instructions.md` — a marked block |
| `agents-md` | `AGENTS.md` — a marked block; read by Codex, Jules, Amp, OpenCode and others |
| `aider` | `CONVENTIONS.md` — a marked block; the one file its editor does not read by finding it |

The first ten own their file outright. The last six share one with other tools, so
dev-prune writes only inside its `<!-- dev-prune:rules:start -->`…`<!-- dev-prune:rules:end -->`
markers — a re-run replaces that block and leaves every byte outside it as found.

Aider is the exception to the rule that writing the file is enough. It reads
`CONVENTIONS.md` only when told to, so `devp skill --agent aider` prints the wiring
the file still needs: `read: CONVENTIONS.md` in `.aider.conf.yml`, or
`aider --read CONVENTIONS.md` at the command line. Rules an agent never loads are
worse than no rules at all — the repository looks configured and nothing is.

Claude Code is deliberately absent from that table: its skill installs globally
(`devp skill`, `devp setup`), so there is nothing to write per repository. It has a
section of its own [below](#claude-code-the-plugin-marketplace).

**Contributing a new editor** is four small changes in
[`src/commands/skill.rs`](../src/commands/skill.rs) and
[`src/constants.rs`](../src/constants.rs):

1. Find where that editor's agent looks for project rules (its docs will name one
   file or directory — that fact is the whole contribution).
2. Add the path as a constant in `src/constants.rs`.
3. Add a variant to `AgentEditor` in `src/commands/skill.rs` with a doc comment naming
   the file (clap turns the variant into the `--agent` value), and one row in
   `AgentEditor::target()` pairing it with the constant. Pick `Style::OwnFile` if the
   editor reads a directory of rule files, or `Style::MarkedBlock` if it reads one
   file other tools also write to — the block writer touches nothing outside the
   markers.
4. Mention the new value in `SKILL_LONG` in `src/help.rs` and in
   [`docs/CLI_REFERENCE.md`](CLI_REFERENCE.md) §13 — plus `site/public/llms.txt` and
   the skill's own `SKILL.md`, which restate the list.

If the editor instead reads the cross-tool `AGENTS.md` convention, no code is needed —
it is already covered by `--agent agents-md`.

---

## Claude Code: the plugin marketplace

Every other editor on this page needs `devp` on the machine before its agent learns
anything, because the rules file is written by the binary. Claude Code is the one that
can go the other way round, because this repository is also a plugin marketplace:

```text
/plugin marketplace add Life-Experimentalist/dev-prune
/plugin install dev-prune@dev-prune
```

Two commands, no account, and nothing queued for review. A Claude Code marketplace is a
Git repository with a `.claude-plugin/marketplace.json` in it, so the plugin is
installable the moment that file is on `main`, and `/plugin update` picks up a change the
moment one lands. Nobody is submitting anything to anybody.

What it installs is one skill and nothing else — no hooks, no MCP server, no agents, no
commands. `claude plugin details dev-prune` reports the whole cost:

| | |
| :--- | :--- |
| Skills | `dev-prune` — the same `SKILL.md` the binary embeds |
| Always on | ~110 tokens: the skill's name and description, so the agent knows it exists |
| On invoke | ~20k tokens, paid only when the skill actually fires |

`.claude-plugin/plugin.json` points its `skills` field at `./.agents/skills/`, which is
where the skill already lives — the same file `devp skill` exports and the same one
`include_str!` compiles into the binary. There is no second copy to drift. Everything
else under `.agents/skills/` is git-ignored, so a clone carries exactly one skill.

**The version field matters more than it looks.** `plugin.json` carries the release
version and Claude Code caches an installed plugin under it, so a stale one is not
cosmetic: the cache key never changes, and an install that already has that version
believes it is current forever. `scripts/check-version.sh` reads it on every push for
that reason, the same way it reads the skill's own version stamp.

**If `devp` is installed too, the skill is now on disk twice** — once at
`~/.claude/skills/dev-prune/` from `devp skill` or `devp setup`, once from the plugin.
That is not a collision; Claude Code namespaces plugin skills. But only one of the two
tracks the binary you have: `devp skill` re-exports the version you installed, while the
plugin follows `main`. Keep whichever matches how you got dev-prune.

---

## JetBrains IDEs

SchemaStore delivers the IntelliSense; [`editors/jetbrains/`](../editors/jetbrains/) is
the icon-only micro-plugin — a single Kotlin `LanguageFileType` over the JSON language,
registered for the exact filenames, so JSON features survive while the project tree
shows the dev-prune icon. Its README covers building; it needs a JDK and downloads the
IntelliJ platform on first build, so it is not part of the repository gate.

**Publishing (maintainer):** build `./gradlew buildPlugin`, then upload
`build/distributions/*.zip` at [plugins.jetbrains.com](https://plugins.jetbrains.com/)
(JetBrains account; first upload creates the listing, human review takes a few days).
Before first upload it needs a real vector icon — see the gap below.

---

## The vector logo

[`assets/devprune.svg`](../assets/devprune.svg) is a true vector — a single traced
path with the brand gradient (`#27ff63` → `#01f6fb`) as a real `linearGradient`, 2.2 KB,
produced 2026-08-20 by tracing the 256px raster with potrace and sampling the gradient
endpoints from the 512px one. It is the source for the JetBrains `pluginIcon.svg`
(40×40) and the JetBrains file-tree icon (`icons/devprune.svg`, 16×16 — IntelliJ's
`IconLoader` rasterizes SVG crisply at every HiDPI factor, which the 48px PNG it
replaced could not).

The `assets/favicon/*.svg` files are still the 1024×1024 raster wrapped in an `<svg>`
tag — fine for favicons, but anything new should start from `assets/devprune.svg`.

The material-icon-theme and vscode-icons PRs (the only way `.devprune.json` gets its
own icon in VS Code file trees, since icon themes always win over extension
contributions) are submitted and awaiting review — see the links in the VS Code
section above.
