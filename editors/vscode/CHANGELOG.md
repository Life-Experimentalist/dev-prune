# Changelog

## 0.2.1 - 2026-08-21

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

## 0.2.0 - 2026-08-20

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

## 0.1.0 - 2026-08-19

- Initial release: maps `.devprune.json` to the hosted JSON Schema at
  <https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json> for validation,
  autocomplete and hover documentation via VS Code's built-in JSON language server.
