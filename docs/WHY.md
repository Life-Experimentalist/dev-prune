# Why `dev-prune` refuses

Every other document here describes what the tool does. This one is the argument it was
built from, because the argument explains the parts of the design that otherwise look
like over-engineering — the dry-run before every deletion, the seven invariants with no
override flag, the flat refusal to touch a build output.

---

## The problem is not deleting

Reclaiming disk space from a developer's machine is a one-line problem:

```bash
find ~/Code -name node_modules -type d -prune -exec rm -rf {} +
```

That command works. It is fast, it needs nothing installed, and on most machines most of
the time it does exactly the right thing. Every tool in this space is, at bottom, a nicer
front end for it — a size column, a confirmation prompt, a progress bar.

The hard problem is the other one: **knowing which of those directories you cannot
delete.** Not "should not" — *cannot*, because nothing on the machine can put it back.

That directory looks identical to the others. Same name, same size, same age. The only
thing that distinguishes it is a fact stored nowhere on disk: whether the lockfile beside
it still describes the tree inside it. You cannot see that by looking, and neither can a
file-size scanner, an `mtime` heuristic, or a person approving rows at a prompt.

---

## What that looks like in practice

One pass, on one real machine, on 2026-08-22:

| | |
|---|---|
| Repositories tracked | 80 |
| Candidate directories | 11 |
| Deleted | 9 |
| **Refused** | **2** |
| Freed | 4.35 GiB |

The nine were unremarkable. The two are the reason this project exists:

1. **`npm ci --dry-run` exited non-zero.** `package-lock.json` had drifted from
   `package.json` — `webpack@5.109.2` was installed and absent from the lock, and `ajv`
   was pinned at two different majors. The `node_modules` on disk was the *only* working
   copy of a tree the lockfile could no longer reproduce.

2. **`uv lock --locked` exited non-zero.** A package had been installed into `.venv` by
   hand and never recorded. `uv sync` would have rebuilt the environment without it, and
   the failure would have surfaced later as an import error in a file nobody had touched.

The `find` command above would have deleted all eleven. Nine of those deletions would
have been fine. Two would have cost an afternoon each, and neither would have announced
itself at the moment of deletion — the cost lands days later, in a place that looks
unrelated.

**18% of candidates were refused.** That number is the whole design. A cleaner that never
refuses anything has not demonstrated that it can.

---

## Why a prompt is not the answer

The obvious fix is to ask. Show the list, let the human decide, delete what they confirm.
That is what the good tools in this space do, and for a supervised sweep it is correct —
see [`MARKET_ANALYSIS.md`](MARKET_ANALYSIS.md) for the honest comparison with `kondo`,
which says so in its own README.

It does not survive contact with the actual use case. Nobody wants to think about disk
space; they want to not run out of it. The version of this tool that gets used is the one
that runs on a schedule, and a scheduled pass has nobody to ask. So the question becomes:
what can stand in for the human's judgement at 3 a.m.?

Not file age — the two refusals above were both older than any threshold worth setting.
Not size, not path, not `.gitignore`. The only thing that actually answers "can this come
back?" is the package manager that would have to bring it back. So dev-prune asks it,
every time, and treats a non-zero exit as a veto:

```
npm ci --dry-run --ignore-scripts    pnpm install --lockfile-only --frozen-lockfile
uv lock --locked                     cargo metadata --locked
bundle lock --check                  go mod download
```

That is slower than a prompt and much slower than `rm -rf`. It is the price of being
allowed to run unattended.

---

## What follows from it

Three parts of the design that look excessive read differently once the above is the
premise:

**The invariants have no bypass flag.** All seven of them —
[`SAFETY_INVARIANTS.md`](SAFETY_INVARIANTS.md) — are unconditional. A `--force` that
disables the lockfile check would turn the tool into the `find` command with extra steps,
and it would be reached for at exactly the moment it matters: when a repository is
refusing and you are in a hurry. There is no flag, deliberately, and there will not be
one.

**Build outputs are out of scope.** `dist/`, `.next/`, `target/` and friends are large
and they do rebuild, but no lockfile describes their contents, so nothing can *prove*
they come back — a compile can take minutes, or fail on a machine whose toolchain has
moved on. A repository that wants one gone declares it explicitly in
`project.devprune.json`, with the command that restores it, and dev-prune verifies that
command's tool exists before deleting anything. That is deliberately more work than a
detection rule, and it is the same principle: no deletion without a proof of return.

**Activity is read from `git log`, not `mtime`.** A `node_modules` nobody has opened in
six months, inside a repository committed to this morning, belongs to work in progress.
File timestamps say the directory is idle; the commit log says the project is not. The
commit log is right.

---

## The one-sentence version

Anyone can write a recursive delete. This one is interesting because it stops.

---

**Next:** [`SAFETY_INVARIANTS.md`](SAFETY_INVARIANTS.md) states the seven rules and where
each is enforced. [`MARKET_ANALYSIS.md`](MARKET_ANALYSIS.md) compares this position
against the tools that took the other route.
