# Roadmap

<p align="center">
  <img src="../assets/github-readme-banner.png" alt="dev-prune — gigabytes back, nothing you can't rebuild" width="800" />
</p>

Everything else in `docs/` describes what the code does **now** — that rule is in
[`CLAUDE.md`](../CLAUDE.md) and it holds. This page is the one sanctioned exception: the
directions that have been considered, so they are not re-litigated from scratch every
time someone has the idea again, and so nobody mistakes a parked idea for a shipped
feature.

Nothing here is a promise and nothing here is a date. What it does say is **which group
something is in**, because "we have not built that" and "we are not going to build that"
each cover several very different situations, and flattening them into one list is how a
settled decision gets re-argued every quarter.

| Group | Means |
|---|---|
| [**In flight**](#in-flight) | Built, and waiting on somebody outside this repository. |
| [**Next**](#next) | Agreed direction, understood work, nothing in the way but time. |
| [**Standing orders**](#standing-orders) | Permanently open. Not a queue but a recipe, plus an invitation to use it. |
| [**On request**](#on-request) | Understood, and deliberately unbuilt until one person asks for it by name. |
| [**Waiting on a shape**](#waiting-on-a-shape) | Wanted. The design is not right yet, and getting it wrong here is expensive. |
| [**Not planned**](#not-planned) | Declined, grouped by *why* — the reasons are not interchangeable, and the reason is what says whether it can ever be reopened. |

An item leaves this page when it ships — [`CHANGELOG.md`](../CHANGELOG.md) is the record
of what happened, not this file. If you are looking for something that used to be here,
that is where it went.

---

## In flight

Work that is built and waiting on a party that is not this repository. Nothing here is
blocked on code, and nothing here blocks a release: every one of these channels reports
`skipped` and the others publish without it.

- **Icon-theme pull requests** for the `.devprune.json` file icon, awaiting maintainer
  review upstream:
  [material-icon-theme#3567](https://github.com/material-extensions/vscode-material-icon-theme/pull/3567)
  and [vscode-icons#4223](https://github.com/vscode-icons/vscode-icons/pull/4223). Icon
  themes always win over an extension's own contribution, so this is the only route.
- **Switching on the WinGet submission.** The release job that raises the `winget-pkgs`
  pull request is built and gated the same way npm is: it reports `skipped` until the
  `WINGET_PUBLISH` variable and the `WINGET_TOKEN` secret both exist. What is still
  outstanding is a person signing Microsoft's CLA once and the first submission
  ([microsoft/winget-pkgs#422809](https://github.com/microsoft/winget-pkgs/pull/422809))
  being merged, because until that identifier is in the catalog there is nothing for
  later versions to be a new version *of*. See [`RELEASING.md`](RELEASING.md).
- **homebrew-core.** Plain `brew install dev-prune`, with no tap prefix. The audit weighs
  GitHub stars, forks and watchers against a bar commonly cited as roughly 75, so it opens
  when the project is popular enough and not before. Nothing to build — the named tap
  already covers the same install — which is what makes it a waiting item rather than a
  work item.

## Next

Decided, understood, and not yet written. Nothing is in it.

The two items that stood here through 1.8.0 — an install receipt beside the managed
binary, and the interactive half of the install flow — were the second half of the
install work 1.8.0 split deliberately, the guarantees first and the convenience after.
Both shipped in 1.9.0. Nothing has moved up to replace them, and an empty queue is not a
promise that it stays empty: this is where a direction lands once it is settled, and the
groups below are where directions come from. [**On request**](#on-request) is waiting on
one person to ask by name; [**Waiting on a shape**](#waiting-on-a-shape) is waiting on a
design that is right.

## Standing orders

Permanently open. These do not complete and they do not get scheduled; the work is
written down, and the answer to "will you add X" is yes, here is the recipe.

- **More adapters.** The trait, registration and test recipe are in
  [`ADDING_ADAPTERS.md`](ADDING_ADAPTERS.md), and the opt-in mechanism (`opt_in()`,
  `enable_*` settings, `build_idle_days`) already exists for anything whose deletion
  costs a recompile rather than a download. Twenty-three ship as of 1.9.0, and the obvious
  ecosystems are covered — what is left is the awkward one. **Nix**: `result` symlinks
  are already refused for being symlinks, and a real adapter would have to reason about
  the store, which is a different kind of problem from "a lockfile says this comes back".
  Beyond that this is a standing invitation rather than a queue: name the manager and the
  recipe is written down.
- **More `--agent` targets** as editors standardise their rules files. Sixteen ship
  today; adding the seventeenth is four small changes documented in
  [`IDE_INTEGRATION.md`](IDE_INTEGRATION.md), and an editor that adopts the `AGENTS.md`
  convention needs no code at all.

## On request

Understood well enough to start, small enough not to argue about, and left unbuilt
because nobody has asked. One person asking by name moves any of these to **Next** —
that is the whole entry requirement, and the reason each is written down rather than
merely thought about.

- **Chocolatey.** Moderated review on every release, and unlike WinGet nothing is
  generated for it today, so it is a packaging format to write as well as a queue to wait
  in. Worth it only if Windows users ask for it by name.
- **JetBrains plugin publishing.** The icon micro-plugin in `editors/jetbrains/` builds;
  the marketplace listing is the remaining step. It needs a JDK and downloads the IntelliJ
  platform on first build, so it is deliberately outside the repository gate.
- **Man page packaging.** `devp man --dir` generates the pages; installing them pre-placed
  is a per-channel packaging question (deb, rpm, a Homebrew formula) rather than a CLI
  one, so it arrives with those channels or not at all.

## Waiting on a shape

Wanted, and stuck on design rather than effort. The distinction matters: everything above
could be started this afternoon by someone who decided to. These cannot, because the
first workable-looking version of each is worse than not having it, and the cost of
finding that out in production is measured in somebody's deleted work.

Nothing is in it. The one item that stood here — a user-declared prune list, as the only
sane route to build outputs — shipped in 1.10.0 as the `prunable` key, once the shape
stopped being a guess. What unblocked it was recording the rebuild command beside the
directory: whoever declares `dist/` also says what puts it back, and dev-prune verifies
that command's tool exists before it deletes anything. That is a *different* proof from a
lockfile rather than no proof at all, which was the question that kept it parked.

## Not planned

Declined, so they stay declined — but not all for the same reason, and the reason is the
part that matters. A thing declined because it would break a promise cannot be reopened
by anyone; a thing declined because the case has not appeared reopens the day it does.
The four headings below say which is which.

### Because it would break a promise the tool makes

There is no version of these that is compatible with what dev-prune tells people it is.
Reopening one means changing the promise on the front page first, and the promise is the
product.

- **Build outputs and gitignore-driven deletion** — `dist/`, `.next/`, `.nuxt/`, and any
  rule of the form "delete what git ignores". No lockfile or manifest can prove an output
  directory is reproducible, and "delete whatever is gitignored" deletes `.env` files.
  This is the boundary the whole tool is built around, not a gap in it. Gradle's `build/`
  and Maven's `target/` are not an exception to it: they are opt-in, off by default, and
  gated behind their own longer idle window precisely because they are rebuilt rather
  than re-downloaded. The one route that works is declaration rather than detection, and
  it shipped in 1.10.0: `prunable` lets whoever knows name the directory *and* the command
  that rebuilds it, which is a proof dev-prune can check before it deletes. Guessing is
  still not on the table.
- **A bypass flag for any safety invariant.** The seven in
  [`SAFETY_INVARIANTS.md`](SAFETY_INVARIANTS.md) — the `.git` boundary, lockfile
  pre-verification, symlink refusal, atomic state writes and the rest — have no escape
  hatch and must not acquire one. A flag that turns off the proof turns dev-prune into
  `rm -rf` with extra steps, and the proof is the entire product.
- **Clearing a cache from anything that runs on its own** — a prune pass, the scheduler,
  a Git hook. A cache is shared by every project on the machine, so no single lockfile
  can prove it recoverable, which is the bar every automatic deletion in dev-prune has to
  clear. `devp caches clear <manager>` exists and empties one after asking, but only ever
  because you typed it.
- **Accounts, cloud sync, telemetry, a subscription tier, ads, or a web dashboard.**
  Proposed as growth surface. dev-prune is a local binary that deletes local directories:
  it has no server, sends nothing anywhere, and nothing about reclaiming disk space needs
  a login. Any of them would mean the tool learns which repositories exist on your machine
  — precisely the thing a disk-cleaning utility has no business knowing off your disk.
- **One channel overwriting another channel's binary.** This is narrower than it sounds,
  and it is worth stating precisely because the obvious reading is wrong. `devp update`
  always updates the copy that is running: it works out which channel installed that copy
  and runs *that* channel's upgrade command — `uv tool upgrade dev-prune` for a uv
  install, `npm install -g dev-prune@latest` for an npm one, a fresh download for the
  installer's managed copy. Nothing is refused and nothing needs a second tool.

  What is declined is having one channel write over a file another channel owns. The
  binary is not the whole install: uv, pipx, npm and cargo each keep a manifest saying
  which version they put there. Overwrite the file behind their back and the manifest
  still says the old version — so the next `uv tool upgrade` reports "already up to date"
  and, on the next reinstall, quietly puts the old binary back. The file would be new and
  the package manager's record of it would be a lie. Delegating to the owning channel
  keeps the file and the record saying the same thing.

### Because it would be a claim nobody could check

Each of these is a number or a label that reads as evidence and is not. The objection is
not modesty; it is that a reader has no way to verify any of them, and every one would go
on saying the same thing on the day it became false.

- **A trust score, a letter grade, or a "Trust level: HIGH" badge.** Suggested for both
  `devp trust` and the site. A grade is a summary that replaces the evidence it
  summarises: it reads as a rating the tool awarded itself, a reader cannot check it, and
  it would go on saying HIGH on the day a bug made it false. So `devp trust` prints the
  guarantees and the actual state of *this* machine instead — which schedulers are
  registered, whether the Git hook is installed, what the config currently permits —
  because every line of that is something you can go and verify. Nothing is condensed
  into a number.
- **A live "your projects are wasting 47.3 GB" figure on the homepage.** The site cannot
  see your disk, so a number there is either invented or somebody's average presented as
  your situation. `devp status` computes the real one, on your machine, in seconds. A
  landing page that fabricates the number in order to sell the tool that measures it
  honestly is arguing against itself.
- **Download counts, star counts, "trusted by N developers", or any adoption badge.**
  Proposed for the README and the site. Every one of them would be a number nobody is
  actually counting: registries report installs rather than people, a download badge
  inflates on CI reruns and therefore measures CI, and Open VSX publishes downloads —
  labelling that figure "users" would simply be false. If this project ever states
  something about its own adoption, it will be a number the reader can look up at the
  source that publishes it.
- **"AI-powered cleanup."** There is no judgement call in a prune pass for a model to
  make. A lockfile either proves a directory is recoverable or it does not, and the check
  is the same check every time. Selling that deterministic test as AI would be a lie
  about the one thing the tool is for.

### Because it is a different tool's job

These are real needs. They are not *this* binary's needs, and the test is the same one
every time: does a lockfile prove it comes back? If any of them ever gets built, it is a
separate tool with a separate promise on its own front page — never a mode of `devp`,
because a command whose guarantee holds for some of its arguments has no guarantee.

- **Deleting Docker images, volumes or build cache.** Nothing about them is
  lockfile-recoverable. An image layer may be irreproducible the moment an upstream tag
  moves, and a volume is data, not a cache. `docker system prune` exists, is well
  understood, and its consequences are the user's to accept. Reporting what is there was
  always a different question, and it shipped in 1.9.0: `devp caches docker` sizes it,
  says what the engine calls reclaimable, and prints those commands for you to run. There
  is no flag that makes dev-prune run one, and there will not be.
- **Growing into a general "developer storage manager"** — Xcode DerivedData, simulator
  runtimes, Android SDK images, browser and IDE caches, the Downloads folder. The review
  framed it as the obvious next market. The problem is that dev-prune's whole claim is
  that it deletes nothing it cannot prove comes back from a lockfile, and none of those
  have one. Shipping them under the same command would mean the promise on the front page
  no longer covered everything the command does, which is a worse outcome than not
  shipping them. Machine-wide stores that *are* package-manager caches are reported —
  never deleted on a schedule — by `devp caches`, and that is the boundary.
- **A desktop GUI.** The interactive parts of dev-prune are terminal views sitting next
  to the terminal work they belong to, and the VS Code extension already covers "I want a
  button" inside the editor that is open anyway. A standalone window would be a second
  application to install, sign, update and support, serving a moment that already has an
  answer in both places people are already sitting.

### Because the case for it has never appeared

The door is not bolted. Each of these names the specific thing that would reopen it, and
that thing has not happened — which is different from the entries above, where nothing
could.

- **A unified `devp analyze` storage view** across repositories and shared caches. Asked
  again in 1.4.0 and parked again, for the same reason: `devp status` already ranks the
  repositories and `devp caches` already ranks the machine-wide stores, both largest first
  with totals. A third command that adds the two lists together would be a new name, a new
  `--json` shape and a third place for the numbers to disagree, in exchange for a sum a
  reader can do. **Reopens when** somebody shows a question the two existing reports
  genuinely cannot answer.
- **32-bit builds beyond Windows** — 32-bit Linux, 32-bit macOS or 32-bit ARM anywhere.
  `i686-pc-windows-msvc` ships as of 1.4.0 because it is a plain rustup target on a runner
  that already exists and Windows is the one place 32-bit hardware is still in service.
  The others are not the same trade: macOS has been unable to run a 32-bit binary since
  Catalina, `i686-unknown-linux-musl` needs a cross musl toolchain to serve a desktop
  population that has effectively gone, and 32-bit ARM has no runner at all. Nothing in
  the source is 64-bit-only, so `cargo install dev-prune` on any of those toolchains works
  today. **Reopens when** a hosted runner and a real population exist for one of them at
  the same time.
