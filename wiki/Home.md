# dev-prune (`devp`)

<p align="center">
  <img src="https://raw.githubusercontent.com/Life-Experimentalist/dev-prune/main/assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

**Universal, lockfile-safe workspace pruner.** Reclaims disk space from idle Git
repositories by deleting dependency and build directories a lockfile can rebuild —
`node_modules`, `.venv`, `target`, `vendor` — and refuses to delete anything it cannot
prove is recoverable.

> This wiki is a map, not a mirror. The documentation lives **in the repository** —
> versioned with the code it describes — and on the website. Every link below points at
> the authoritative copy, so nothing here can drift out of date.

## Start here

| I want to… | Go to |
| :--- | :--- |
| Install it | [devprune.vkrishna04.me](https://devprune.vkrishna04.me) — one-liners for every platform |
| See every command and flag | [CLI Command Reference](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/CLI_REFERENCE.md) (also built in: `devp <command> --help`) |
| Know why it's safe | [Safety Invariants](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/SAFETY_INVARIANTS.md) — the seven rules with no bypass flag |
| Fix something | [Troubleshooting hub](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/troubleshooting/README.md) |
| Understand the architecture | [Architecture entry point](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/ARCHITECTURE.md) → HLD / LLD |
| Add a package manager | [Adding Adapters](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/ADDING_ADAPTERS.md) |
| Teach my AI assistant | Run `devp skill` — or read [SKILL.md](https://github.com/Life-Experimentalist/dev-prune/blob/main/.agents/skills/dev-prune/SKILL.md) |
| Everything else | [Documentation hub](https://github.com/Life-Experimentalist/dev-prune/blob/main/docs/README.md) — the full index |

## The 30-second version

```bash
devp init ~/Code        # register every Git repository under ~/Code
devp status             # what could be reclaimed, per repository
devp run --dry-run      # what a prune would do, and why the rest is skipped
devp run                # do it (asks first)
devp restore --last-run # the undo
```

`dev-prune` and `devp` are the same binary. Exit codes are a contract: `0` success,
`1` failure, `2` usage error.
