# dev-prune for VS Code

Autocomplete, validation and hover documentation for `.devprune.json` — the
per-repository configuration file of [dev-prune](https://devprune.vkrishna04.me)
(`devp`), the CLI that reclaims disk space from idle Git repositories by deleting
dependency directories a lockfile can provably rebuild.

The extension maps `.devprune.json` to a bundled copy of the
[dev-prune JSON Schema](https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json),
so VS Code's built-in JSON language server provides — offline, with no network
fetch —:

- **Autocomplete** for every key: `project_name`, `ignore`, `disable_hooks`,
  `disable_daemon`, `override_idle_days`, `min_size_mb`, `scan_depth`.
- **Hover documentation** describing what each key does.
- **Validation** — unknown keys and out-of-range values are underlined, because a
  `.devprune.json` that does not parse makes `devp` skip the repository rather than
  guess at what it meant.
- **A status bar item showing what you could reclaim** — the reclaimable size
  from `devp status --json`, with repository and candidate counts in the tooltip.
  Clicking it opens a small menu: refresh, dry-run in a terminal, open the
  dashboard, install the AI agent skill, open the CLI reference. The same
  read-only set lives in the command palette under *dev-prune:*. Reading is all
  the extension ever does on its own — anything that deletes runs as a visible
  `devp` command in a terminal you can read before it acts.
- **A heads-up when the CLI is missing** — a workspace with a `.devprune.json` but
  no `devp` on PATH gets a one-time notification, because a config file nothing
  acts on looks exactly like a working setup. Everything that invokes the CLI
  runs solely in trusted workspaces, "Don't show again" is remembered, and one
  setting — `devprune.notifications` — turns every popup off; the status bar
  informs without interrupting.

Files that `devp config project` writes carry a `$schema` link already, so those work
without this extension; installing it covers hand-written files too. When a file has
its own `$schema` key, VS Code prefers that link — the hosted schema it points to is
republished from the same canonical file on every site build, so both paths agree.

**Seeing no squiggles on a file you know is wrong?** VS Code caches downloaded schemas
for the life of the window. If validation went quiet after a dev-prune release, run
*Developer: Reload Window* (or *JSON: Clear Schema Cache*) once — the bundled schema
this extension ships is immune, but a file with its own `$schema` link fetches remotely.

## Install

- **VS Code**: [marketplace listing](https://marketplace.visualstudio.com/items?itemName=VKrishna04.dev-prune),
  or search *dev-prune* in the Extensions view, or `code --install-extension VKrishna04.dev-prune`.
- **VSCodium / Cursor**: [OpenVSX listing](https://open-vsx.org/extension/VKrishna04/dev-prune) —
  the same search and command work there.
- **Anything without marketplace access**: every
  [GitHub release](https://github.com/Life-Experimentalist/dev-prune/releases) attaches
  the packaged extension as `dev-prune-vscode-<version>.vsix`. Download it and run:

  ```
  code --install-extension dev-prune-vscode-<version>.vsix
  ```

  `codium --install-extension …` and `cursor --install-extension …` work the same way,
  as does *Extensions view → ⋯ → Install from VSIX*. A side-loaded copy is the same
  package the marketplace serves for that version — but it never auto-updates, so
  check the releases page now and then for a newer one.

## Related

- The CLI: `curl -fsSL https://devprune.vkrishna04.me/install.sh | sh` (or
  `iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex` on Windows)
- [Configuration reference](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/CLI_REFERENCE.md)
- [What `.devprune.json` can and cannot contain](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/SAFETY_INVARIANTS.md)
  — the file holds inert data only; no key in it can name a command to run.
