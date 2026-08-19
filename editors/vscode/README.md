# dev-prune for VS Code

Autocomplete, validation and hover documentation for `.devprune.json` — the
per-repository configuration file of [dev-prune](https://devprune.vkrishna04.me)
(`devp`), the CLI that reclaims disk space from idle Git repositories by deleting
dependency directories a lockfile can provably rebuild.

This extension contains no code. It maps `.devprune.json` to the
[hosted JSON Schema](https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json),
so VS Code's built-in JSON language server provides:

- **Autocomplete** for every key: `project_name`, `ignore`, `disable_hooks`,
  `disable_daemon`, `override_idle_days`, `min_size_mb`, `scan_depth`.
- **Hover documentation** describing what each key does.
- **Validation** — unknown keys and out-of-range values are underlined, because a
  `.devprune.json` that does not parse makes `devp` skip the repository rather than
  guess at what it meant.

Files that `devp config project` writes carry a `$schema` link already, so those work
without this extension; installing it covers hand-written files too.

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
  as does *Extensions view → ⋯ → Install from VSIX*. Because the extension is a single
  declarative schema mapping, a side-loaded copy is byte-for-byte what the marketplace
  serves — there is no auto-update to miss out on beyond new schema keys, which arrive
  through the hosted schema URL anyway, without an extension update.

## Related

- The CLI: `curl -fsSL https://devprune.vkrishna04.me/install.sh | sh` (or
  `iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex` on Windows)
- [Configuration reference](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/CLI_REFERENCE.md)
- [What `.devprune.json` can and cannot contain](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/SAFETY_INVARIANTS.md)
  — the file holds inert data only; no key in it can name a command to run.
