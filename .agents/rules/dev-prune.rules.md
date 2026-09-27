# dev-prune (`devp`) is installed on this machine

`dev-prune` reclaims disk space by deleting dependency and build directories that a
lockfile can rebuild, from Git repositories that have been idle. It knows thirty-five
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
  When a directory devp removed is needed again, put it back with `devp restore`, not
  by running `npm ci` or the manager's own install yourself; `devp history` names the
  pass that took it.
- Never `rm -rf` a bloat directory by hand. If devp pruned it and it is needed, that
  is `devp restore`; if the user wants it gone, that is `devp run . --dry-run` and
  then `devp run . -y` (with `--ignore-idle`, after asking, if the repository is
  active), which verifies the lockfile first and records the pass so it can be undone.
- Never delete a container volume, and never run `docker system prune --volumes` or
  `docker volume prune` on the user's behalf. Images and build caches can be pulled
  again; a named volume is the only copy of what is inside it. `devp caches clear
  docker` reclaims the recoverable parts and cannot touch a volume. For volume space,
  run `devp caches clear docker --include-volumes --dry-run` to list the unused ones,
  then hand the real command to the user: it takes each deletion as a typed pick and
  refuses `--yes` and a piped stdin, so only they can confirm one.
- Never empty `~/.m2/repository` or any package-manager cache uninvited. `devp caches`
  sizes them; `devp caches clear <manager>` runs only when the user asks, and Maven's
  is refused outright because it holds locally installed artifacts no remote has.
- Prefer `--json` (on `run`, `status`, `stats`, `caches`) when you need to read the
  answer rather than show it. Exit codes: 0 success, 1 failure, 2 usage error.
- Never run `devp uninstall --deep` without explicit user confirmation.

The full agent manual — every command, JSON contracts, troubleshooting tree — is the
SKILL.md exported by `devp skill` into the dev-prune config directory. Read it before
doing anything non-obvious.
