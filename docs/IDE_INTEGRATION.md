# IDE & Editor Integration

How `.devprune.json` gets IntelliSense, validation and a recognizable icon in editors —
what already works today with no extension installed, what lives in `editors/`, and the
maintainer checklist for publishing each piece. Written in the same spirit as
[RELEASING.md](RELEASING.md): the parts a human must do are marked as such.

---

## The name is `.devprune.json`, permanently

`.dev-prune.json` was considered and rejected. The filename shipped in 1.0.0 and is
part of the backwards-compatibility contract: it is written into users' repositories
and their `.gitignore` files, `ignore.devprune.json` derives from it, the Linux MIME
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
that have not merged our SchemaStore entry, and the **icon inside IDE file trees**.

---

## SchemaStore (covers JetBrains, Visual Studio, Neovim, Zed, and more)

One merged PR to [SchemaStore](https://github.com/SchemaStore/schemastore) gives every
subscribed editor schema-by-filename, no `$schema` key needed.

**Status: submitted** — [SchemaStore/schemastore#6226](https://github.com/SchemaStore/schemastore/pull/6226)
(2026-08-19). The steps below record what it contains, for the day it needs updating.

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

---

## VS Code family (VS Code, VSCodium, Cursor, Windsurf)

[`editors/vscode/`](../editors/vscode/) holds a zero-code extension: a `package.json`
with a `jsonValidation` contribution mapping `.devprune.json` to a **bundled** copy of
the schema, plus the marketplace icon. Bundling (added in extension 1.2.0) means
hand-written files validate offline, instantly, with nothing to go stale in VS Code's
remote-schema cache; files the CLI writes carry an in-file `$schema` link, which VS
Code prefers, so those keep tracking the hosted URL. The bundled copy cannot drift:
`sync-schema.mjs` runs as `vscode:prepublish`, so every packaging pass — CI or by
hand — refreshes it from `schemas/devprune.schema.json` first. `npx @vscode/vsce
package` in that directory produces the `.vsix` (verified working end to end in a live
VS Code), and the release workflow packages it on every tag and attaches it to the
GitHub release as `dev-prune-vscode-<version>.vsix` — the side-load path
(`code`/`codium`/`cursor --install-extension <file>`) for editors that cannot reach a
marketplace.

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

Both need a maintainer-submitted PR with an SVG icon.

**Status: published on both** (2026-08-19) —
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=VKrishna04.dev-prune)
and [OpenVSX](https://open-vsx.org/extension/VKrishna04/dev-prune), publisher `VKrishna04`.

**If validation goes quiet** — a file full of stale keys showing zero problems — the
cause is almost always VS Code's remote-schema cache: the JSON language server keeps a
downloaded schema for the life of the window, so a window opened before a schema fix
deployed validates against the pre-fix copy indefinitely. *Developer: Reload Window* or
*JSON: Clear Schema Cache* clears it. This is exactly why the extension bundles the
schema — only files with their own `$schema` link still fetch remotely.

**Publishing a new version (maintainer):**

1. Bump `version` in `editors/vscode/package.json`.
2. `npx @vscode/vsce publish` from `editors/vscode/` (Azure DevOps PAT with
   Marketplace → Manage scope), or upload the `.vsix` in the
   [manage UI](https://marketplace.visualstudio.com/manage/publishers/VKrishna04).
3. `npx ovsx publish dev-prune-vscode-<version>.vsix -p <token>` for OpenVSX, which is
   what VSCodium and some Cursor builds resolve against — both uploads matter.

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

**Remaining gap:** the material-icon-theme and vscode-icons PRs (the only way
`.devprune.json` gets its own icon in VS Code file trees, since icon themes always win
over extension contributions) have not been submitted yet. The vector they require now
exists, so they are unblocked.
