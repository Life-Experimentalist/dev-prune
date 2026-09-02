# Changelog

## [0.5.0] - 2026-09-02

- **A stale “devp not found” now heals itself.** The verdict was computed once,
  when the window opened — install the CLI a minute later (or have an antivirus
  quarantine give it back) and the warning stayed for the life of the window,
  and clicking it sent you to the install page for a CLI you already had. The
  extension now re-checks whenever the window regains focus while that warning
  is up, and a click re-probes first: the install page only opens when devp is
  still really missing.
- **A slow `devp status` no longer looks like a missing one.** The status call
  walks every registered repository, and on a machine mid antivirus-scan it
  could outlive the old 15-second limit — the extension then hid its status bar
  item without a word. The limit is now 60 seconds, and reaching it shows
  “devp: not responding” with Refresh one click away, instead of nothing.
- **Prune This Repository is now a palette command.** Pruning was reachable
  only through the status-bar QuickPick, and only while it showed a candidate;
  everything else the QuickPick offers already had a Command Palette twin. The
  new command runs `devp run .` in a visible terminal, same as the QuickPick
  entry — the CLI still decides whether this repository is safely prunable and
  says so in that terminal if not.

## [0.4.0] - 2026-09-01

- **`project.devprune.json` gets the same validation as `.devprune.json`.** The
  committed half of the configuration — the one a whole team inherits on clone, and
  so the one most likely to be typed by hand — was the only one the extension did
  not check. Autocomplete, hover docs and error squiggles now cover it too, at the
  repository root and anywhere below it.
- **The bundled schema knows the config keys the CLI has added since.** A
  `prunable.directories` entry — the `path`, the `rebuild` command that puts it
  back, the optional `why` — completes as you type and explains itself on hover,
  and so does the `exclude` list that lets one person keep a directory the rest of
  the team has declared rebuildable. Previously these validated as unknown
  properties in a file the CLI accepts.
- **The status bar item now wears the dev-prune mark.** Every state is prefixed with
  the product glyph rather than a borrowed codicon, so the item is findable at a
  glance in a status bar that already holds a dozen others. The glyph is registered
  through `contributes.icons` and ships as a one-character font, which means VS Code
  paints it in the theme's own foreground colour and it stays legible on light and
  dark alike. The missing-CLI state keeps its warning background — that was always
  the signal there, and the triangle beside it was saying the same thing twice.

## [0.3.0] - 2026-08-21

- **The status bar now walks your workspace through dev-prune's whole lifecycle**,
  instead of only showing a machine-wide total. One glance tells you where this
  repository stands, in order: devp not installed (click to install it) → not a
  Git repository (click to `git init`) → not registered (click to `devp link .`)
  → active (how much space dependency and build folders occupy, and which package
  managers are in use — npm, uv, cargo and so on) → idle candidate (the
  reclaimable size, with a "why so low?" explanation when pnpm or bun hardlink
  most of the bytes into their store) → cleaned (how much dev-prune has saved
  here). The warning background is reserved for the missing-CLI state.
- **Clicking the status bar opens a state-aware menu.** The action that fits the
  current state comes first — initialize Git, register the repo, prune it, or
  restore what was pruned — followed by the standing set: refresh, dry run,
  dashboard, create a `.devprune.json`, ignore this repository, install the AI
  agent skill, open the CLI reference.
- **New palette commands**: *Create .devprune.json* (writes a skeleton with the
  `$schema` line so validation lights up immediately), *Ignore This Repository*
  (sets `"ignore": true` so dev-prune never prunes it), *Register This
  Repository*, *Initialize Git Repository* and *Refresh Status Bar*.
- **The extension activates in every workspace**, not only ones that already have
  a `.devprune.json` — the lifecycle states before "configured" are the ones a
  new user actually sees. Everything that runs the CLI still waits for workspace
  trust, and anything that deletes still runs as a visible terminal command.

## [0.2.1] - 2026-08-21

- **Terminal commands run the CLI the extension actually found.** The dry run and
  the dashboard used to send a bare `devp` to the terminal, which failed whenever
  the status bar was working off a probed install directory rather than PATH. They
  now run the same detected binary, quoted for the shell.
- **The install notification only fires when a config file really exists.** The
  missing-CLI popup and warning now verify a `.devprune.json` is present in the
  workspace before claiming one is — and a workspace holding only
  `ignore.devprune.json`, the opt-out marker, gets no install nag at all.
- **Nested configs activate the extension.** A monorepo whose only
  `.devprune.json` sits below the root now gets the status bar and commands; the
  activation globs previously matched root-level files only.
- **Honest virtual-workspace declaration.** The extension spawns a local CLI,
  which cannot work on a virtual file system — the manifest now says `limited`
  (schema validation still works everywhere) instead of claiming full support.

## [0.2.0] - 2026-08-20

- **The schema is now bundled inside the extension.** Validation, autocomplete and
  hover docs for hand-written `.devprune.json` files work offline and immediately —
  no network fetch, nothing to go stale in VS Code's remote-schema cache. Files the
  CLI writes carry a `$schema` link and keep tracking the hosted schema, which always
  matches the latest CLI.
- **A status bar item shows what you could reclaim.** A trash icon with the
  reclaimable size from `devp status --json`, with repository and candidate counts
  in the tooltip. Clicking it opens a small menu: refresh, dry-run in a terminal,
  open the dashboard, install the AI agent skill, open the CLI reference. Reading
  is all the extension ever does on its own — anything that deletes runs as a
  visible `devp` command in a terminal you can read before it acts.
- **Command palette commands.** *dev-prune: Show Reclaimable Space & Actions*,
  *Dry Run in Terminal*, *Install AI Agent Skill* and *Open CLI Reference* — the
  same read-only set the status bar menu offers.
- **A missing CLI is called out instead of staying silent.** When a workspace
  contains `.devprune.json` (or `ignore.devprune.json`) but `devp` cannot be found,
  the status bar shows a persistent warning state and one notification appears at
  most once per session — a config file nothing acts on otherwise looks exactly
  like a working setup. The popup is about installing and nothing more: *Install
  devp* opens the website, *I installed it* rechecks immediately, and "Don't show
  again" silences it permanently; the status bar keeps quietly telling the truth.
  The recheck also probes devp's own install directories directly, because VS Code
  keeps the PATH it was launched with — so a CLI installed a minute ago is found
  without restarting anything, and only when it truly isn't there does the
  extension offer to reload the window.
- **All popups can be turned off.** A single setting, `devprune.notifications`
  (on by default), disables both the install prompt and the skill offer. The
  status bar is unaffected — it informs without interrupting.
- **A one-time offer to teach your AI agent.** If an agent skills directory
  (`~/.claude/skills/`) exists without the dev-prune skill in it, one notification
  offers to run `devp skill`. Shown once ever, never re-asked.
- **Works in Restricted Mode and virtual workspaces.** Schema validation works
  everywhere; everything that invokes the CLI runs solely in trusted workspaces,
  so there is nothing for Restricted Mode to restrict.

## [0.1.0] - 2026-08-19

- Initial release: maps `.devprune.json` to the hosted JSON Schema at
  <https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json> for validation,
  autocomplete and hover documentation via VS Code's built-in JSON language server.
