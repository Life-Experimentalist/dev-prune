# Changelog

## 1.2.0 - 2026-08-20

- **The schema is now bundled inside the extension.** Validation, autocomplete and
  hover docs for hand-written `.devprune.json` files work offline and immediately —
  no network fetch, nothing to go stale in VS Code's remote-schema cache. Files the
  CLI writes carry a `$schema` link and keep tracking the hosted schema, which always
  matches the latest CLI.
- **Works in Restricted Mode and virtual workspaces.** The extension declares
  `untrustedWorkspaces` and `virtualWorkspaces` support explicitly — it contains no
  code, so there is nothing to restrict.

## 0.1.0 - 2026-08-19

- Initial release: maps `.devprune.json` to the hosted JSON Schema at
  <https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json> for validation,
  autocomplete and hover documentation via VS Code's built-in JSON language server.
