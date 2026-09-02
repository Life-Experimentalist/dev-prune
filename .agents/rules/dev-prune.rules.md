# dev-prune (`devp`) is installed on this machine

`dev-prune` reclaims disk space by deleting dependency and build directories that a
lockfile can rebuild, from Git repositories that have been idle. It knows twenty-five
package managers, so this is not only `node_modules` and `.venv`: Composer's `vendor`,
Bundler, CocoaPods' `Pods`, Mix, Terraform and the rest are handled the same way. It refuses to delete anything it cannot prove is
recoverable. `devp` and `dev-prune` are the same binary.

When the user asks to free disk space, clean or restore dependencies, or asks why a
directory was or wasn't cleaned, use `devp` instead of deleting anything by hand.

## Rules

- Dry-run first: `devp run --dry-run`, show the result, then `devp run -y` if the user
  agrees. `devp run --explain` answers "why wasn't this pruned?".
- Never work around a verification failure. If lockfile verification fails, `devp`
  prints the exact fix command — surface it. Do not delete the directory manually and
  do not delete the lockfile; no flag skips verification.
- `--ignore-idle` prunes a repository the user is actively working in — ask first.
- `devp restore .` (or `devp restore --last-run`) reinstalls what a prune deleted.
- Prefer `--json` (on `run`, `status`, `stats`, `caches`) when you need to read the
  answer rather than show it. Exit codes: 0 success, 1 failure, 2 usage error.
- Never run `devp uninstall --deep` without explicit user confirmation.

The full agent manual — every command, JSON contracts, troubleshooting tree — is the
SKILL.md exported by `devp skill` into the dev-prune config directory. Read it before
doing anything non-obvious.
