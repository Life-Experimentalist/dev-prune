// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// The long-form help text: what `devp <command> --help` prints.
//
// `-h` stays short — one line per flag, no scrolling. `--help` (and `devp help
// <command>`) gets these: the full description, the behaviour that is not obvious from
// the flag list, and worked examples for every command and subcommand. The text is
// kept in one file rather than inline in the derive so the CLI definition in `lib.rs`
// stays readable, and because these paragraphs are documentation first — they carry
// the same facts as `docs/CLI_REFERENCE.md`, and changing one means changing both.

pub const INIT_LONG: &str = "\
Crawl the given directory trees for Git repositories and register every one found, up \
to 8 levels deep. Registration is one entry in dev-prune's own registry file — nothing \
in the repository is created, changed or deleted.

After registering, it runs the same integration pass as `devp setup` (installing \
whatever is missing, reporting whatever it skipped) and the same quiet release check \
as `devp update`.

Registration is what makes a repository visible to `devp run`, `devp status` and the \
background pass. Repositories are never auto-discovered at prune time: what dev-prune \
touches is exactly what you registered.";

pub const INIT_EXAMPLES: &str = "\
EXAMPLES:
  devp init                       Register repositories under the current directory
  devp init ~/Code                Register everything under ~/Code
  devp init ~/Code ~/Work/oss     Multiple trees in one pass
  devp scan ~/Code                Same command — `scan` and `onboard` are aliases
  DEV_PRUNE_NO_AUTO_SETUP=1 devp init ~/Code
                                  Register repositories, install nothing

UNDO:
  devp undo                       Reverts the most recent init or link";

pub const LINK_LONG: &str = "\
Register one Git repository for pruning. The path defaults to `.`, so inside a \
repository `devp link` is the whole command. Registration writes one entry to \
dev-prune's registry; the repository itself is untouched.

`--quiet` is the form the global Git hook invokes: it prints nothing (a hook fires \
inside someone's commit, whose terminal is not dev-prune's to write to) and it skips \
repositories whose `.devprune.json` sets `disable_hooks`, so a workspace that opted \
out of auto-registration stays out.";

pub const LINK_EXAMPLES: &str = "\
EXAMPLES:
  devp link                       Register the current directory
  devp link ~/Code/my-app         Register a repository by path
  devp link . --quiet             What the Git hook runs; silent, honours opt-outs

UNDO:
  devp undo                       Reverts the most recent init or link
  devp unlink                     Unregister (keeps every file on disk)";

pub const UNLINK_LONG: &str = "\
Remove a repository from dev-prune's registry. This deletes the registry entry and \
nothing else — no workspace file is touched, and the repository's `.devprune.json`, \
if it has one, stays where it is.

`--missing` removes every registered path whose directory no longer exists, instead \
of one named repository. Deleted clones, reformatted drives and moved workspaces all \
leave dead entries behind; `devp doctor` counts them in one warning and points here \
rather than printing one `devp unlink` line per dead path.";

pub const UNLINK_EXAMPLES: &str = "\
EXAMPLES:
  devp unlink                     Unregister the current directory
  devp unlink ~/Code/old-app      Unregister a repository by path
  devp unlink --missing           Drop every entry whose path no longer exists";

pub const UNDO_LONG: &str = "\
Revert the most recent `devp init` or `devp link`: whatever repositories that one \
action registered are unregistered again. Only registration is undone — `undo` never \
deletes files, and it is not the undo for a prune (that is `devp restore --last-run`).";

pub const UNDO_EXAMPLES: &str = "\
EXAMPLES:
  devp undo                       Unregister whatever the last init/link registered

RELATED:
  devp restore --last-run         The undo for a prune pass — reinstalls what it deleted";

pub const RUN_LONG: &str = "\
Execute a prune pass: across every registered repository with no path, or on one \
repository with `devp run <path>`. Each repository goes through the same gauntlet, \
and a directory is deleted only when every check passes:

  1. `ignore.devprune.json` in the root, or `\"ignore\": true` — skipped instantly.
  2. Idle check: last commit and newest source mtime, against `idle_days` (15 by
     default). `--ignore-idle` lifts this one check and nothing else.
  3. Project discovery: the root and up to `scan_depth` levels below it (6 by
     default), so a monorepo's every package is found.
  4. The package manager's own binary must be present — a directory whose manager
     is missing cannot be verified, so it is not touched.
  5. Lockfile verification: the manager itself confirms the lockfile can rebuild
     the directory. No flag bypasses this, and none ever will.
  6. Size floor (`min_size_mb` / `--min-size`), symlink refusal, nested-repository
     refusal.

Interactively, a selection TUI shows what would be deleted before anything is; `-y` \
skips the confirmation, and `--dry-run` reports what a pass would do without deleting \
anything at all. Adapter names for `--only`/`--skip` are: npm, pnpm, yarn, bun, uv, \
poetry, pdm, pipenv, venv, cargo, go, composer, bundler, cocoapods, mix, mix_build, \
gradle, maven, swift, terraform, dart, vcpkg, cmake_build — an unknown name is an \
error listing the valid ones, not a silently empty pass. cargo, gradle, maven, \
swift, dart, mix_build, vcpkg and cmake_build are opt-in (`devp config set \
enable_cargo true`) and idle-gated separately by `build_idle_days`, because a \
build directory takes far longer to get back than a dependency directory. \
`devp config wizard` switches them on by language, and can give any one adapter \
its own idle window.

`--except` is the safe spelling of \"clean up but keep the API project\": the named \
repositories are never verified, never deleted and never restored, which beats \
pruning them and downloading everything back. Entries match by full path or by \
directory name, case-insensitively, `~` expanded.

`--explain` answers \"why was that repository not pruned?\": every repository and \
directory is listed with its verdict, including the states a normal pass keeps quiet \
about — still active (with the actual age), opted out, under the size floor. It is \
read-only and cannot be combined with `--json`.";

pub const RUN_EXAMPLES: &str = "\
EXAMPLES:
  devp run --dry-run              What would be pruned, and why the rest would not
  devp run                        Prune across all registered repositories (asks first)
  devp run -y                     Same, no confirmation prompt
  devp run .                      Prune only the current repository
  devp run ~/Code/my-app --ignore-idle -y
                                  Prune it even though it was touched recently
  devp run --except api-service,~/Code/playground
                                  Everything except the ones you name
  devp run --only cargo,uv --dry-run
                                  Only these package managers
  devp run --skip venv --min-size 50
                                  Skip venvs; ignore directories under 50 MiB
  devp run --json --dry-run       One JSON document on stdout (schema in CLI_REFERENCE)
  devp run --explain              Why each repository would or would not be pruned

Run interactively with a terminal, `--json` also copies the document to the clipboard.
A run in which any repository failed exits 1, even if others succeeded.";

pub const STATUS_LONG: &str = "\
The dashboard: every registered repository with its state (Candidate, Active, \
Ignored, No Bloat, Path Missing, or an unreadable `.devprune.json`), its reclaimable \
space, and its last activity. In a terminal this is an interactive TUI; piped or \
redirected it prints a plain table; `--json` replaces either with one document and \
changes nothing at all.

Sizes are what deleting the directory actually gives back: bytes hardlinked into a \
pnpm or bun store are measured per file and excluded, because the store keeps them.

TUI KEYS:
  Up/Down, j/k      Move           PgUp/PgDn        Jump ten rows
  Home/End, g/G     First/last     p                Prune-select mode (candidates pre-selected)
  Space             Toggle row     a                Toggle all candidates
  Enter             Prune the selection             i    Toggle ignore in .devprune.json
  Esc               Leave mode / exit               q, Ctrl-C   Exit

SHORTCUTS:
  devp status daemon              = devp config daemon status
  devp status . hook              = devp config hook . status";

pub const STATUS_EXAMPLES: &str = "\
EXAMPLES:
  devp status                     The dashboard (TUI in a terminal, table when piped)
  devp status --top 10            Only the ten biggest repositories; totals unaffected
  devp status --drift             What would a prune refuse on, and how to record it
  devp status --json | jq '.totals.reclaimable_bytes'
                                  Machine-readable; stdout carries the document only
  devp status --top 5 --json      The trim is reported as a top-level \"top\" field

Run interactively with a terminal, `--json` also copies the document to the clipboard.";

pub const STATS_LONG: &str = "\
What dev-prune has already done, as opposed to what it could do next: lifetime space \
reclaimed, how many prune passes there have been, the most recent pass and how to \
undo it, the last passes, and the repositories that have given back the most. \
Read-only — it reads the registry and touches nothing.";

pub const STATS_EXAMPLES: &str = "\
EXAMPLES:
  devp stats                      The report
  devp stats --json | jq '.lifetime.bytes_freed'
                                  Lifetime bytes as a number

Run interactively with a terminal, `--json` also copies the document to the clipboard.";

pub const MAN_LONG: &str = "Render the manual, from the same clap definitions `--help` prints, so the manual cannot describe a flag the program does not have.

`devp man` at a terminal prints the contents page: every command grouped by what it is for, one line each, plus the flags that go before the command and the exit codes. `devp man <command>` prints that one command's page — the same text `devp <command> --help` prints, because they are the same definition.

The roff source is something `man` formats, not something a person reads, and on Windows there is no `man` to hand it to, so it appears only where something can use it: redirect or pipe the output and it is roff again, so `devp man > devp.1` and `devp man | man -l -` are unchanged. `--roff` forces roff at a terminal too.

`--dir` writes the full set (`devp.1`, `dev-prune.1`, and one `devp-<command>.1` per subcommand) into a directory, ready to copy onto `manpath`.";

pub const MAN_EXAMPLES: &str = "EXAMPLES:
  devp man                        The contents page, on any platform
  devp man run                    One command's page
  devp man | man -l -             Read it formatted by man (Linux/macOS)
  devp man --roff > devp.1        Save the roff source
  devp man run --roff > devp-run.1   Save one page's roff source
  devp man --dir ./man            Write the full set into ./man
  sudo cp man/*.1 /usr/local/share/man/man1/   Install them system-wide";

pub const COMPLETIONS_LONG: &str = "\
Print a shell completion script to stdout — the script and nothing else, because the \
output is meant to be redirected or eval'd and anything extra becomes a shell error \
on every new terminal.

The script is generated from the same argument definition the binary parses with, so \
a flag cannot exist in one and be missing from the other. It completes whichever name \
invoked it: `devp completions bash` completes `devp`, `dev-prune completions bash` \
completes `dev-prune` — generate one for each name you actually type.";

pub const COMPLETIONS_EXAMPLES: &str = "\
EXAMPLES:
  source <(devp completions bash)                       Bash, current shell
  devp completions bash > ~/.local/share/bash-completion/completions/devp
  devp completions zsh  > ~/.zfunc/_devp                Zsh (a directory on $fpath)
  devp completions fish > ~/.config/fish/completions/devp.fish
  devp completions powershell | Out-File -Append -Encoding utf8 $PROFILE
                                                        PowerShell, permanently";

pub const CACHES_LONG: &str = "\
Find every package-manager cache and store on the machine, size each one, and print \
the command that clears it — largest first, with a total. On its own it deletes \
nothing, and nothing that runs on a schedule ever will: a cache is shared by every \
repository, so no single lockfile can prove its contents recoverable, which is the \
bar every dev-prune deletion has to clear. Clearing one also turns the next `devp \
restore` into a download.

`devp caches clear <manager>` runs the command this table prints, after showing you \
what goes and asking.

Covered: npm, pnpm, yarn, bun, uv, pip, conda, cargo, go, maven, gradle, nuget, vcpkg, \
conan, composer, cocoapods and hex. Each manager is asked where its cache is (`npm \
config get cache`, `go env GOMODCACHE`, …) rather than assumed, with read-only queries \
run from your home directory; a manager that is not installed falls back to the \
conventional location, because a cache left behind by an uninstalled manager is \
exactly the multi-gigabyte directory nobody remembers.";

pub const CACHES_EXAMPLES: &str = "\
EXAMPLES:
  devp caches                     The table, largest first, with clear commands
  devp caches --json | jq '.summary.total_bytes'
                                  Machine-readable
  devp caches clear npm           Empty one, after asking
  devp caches clear all --dry-run What would go, and nothing touched

Run interactively with a terminal, `--json` also copies the document to the clipboard.";

pub const CACHES_DOCKER_LONG: &str = "\
What the engine is holding, in its own words: images, containers, local volumes and build cache, each with a count, a size, and how much of that size it believes it could give back. Then the commands that would give it back, narrowest first.

Read-only, permanently. dev-prune deletes only what a lockfile proves it can rebuild, and nothing here clears that bar: an image's registry tag can be retagged or deleted, the Dockerfile that built it may not be on this disk, and a named volume is the one thing on the machine that is not reproducible at all. So this prints the prune commands and never runs them, with or without `--yes`.

The numbers come from the engine's own `system df` rather than from a directory walk. On Docker Desktop and Podman the store lives inside a VM disk image the host cannot see, and `~/.docker` is configuration rather than data — a size taken off the filesystem would be wrong by orders of magnitude, in the reassuring direction. Asking the engine is also the only way to learn what is *reclaimable*, which is the figure that decides anything: 40 GB of images with 38 GB dangling is a different situation from 40 GB with 2 GB dangling.

An engine that is installed with its daemon stopped is reported as exactly that, in the engine's own words, rather than as an absence.";

pub const CACHES_CONTAINERS_LONG: &str = "\
The same read-only report as `devp caches docker`, for every container engine on this machine — docker, podman and nerdctl — or for the one you name.

Local Kubernetes clusters are listed by name and deliberately not sized. kind, k3d and minikube run their nodes as containers, or as a VM disk belonging to an engine already in the table, so their disk is counted there. A figure beside the cluster name would be the same gigabytes twice. Delete one with its own tool — `kind delete cluster`, `minikube delete`, `k3d cluster delete` — which is what actually releases the space. The cluster list is read out of your kubeconfig with `kubectl config get-contexts`, which contacts nothing: a context pointing at a production cluster is filtered out by name here rather than by being dialled.";

pub const CACHES_CONTAINERS_EXAMPLES: &str = "\
EXAMPLES:
  devp caches docker              Images, containers, volumes, build cache
  devp caches podman              The same, for Podman
  devp caches containers          Every engine installed, plus local clusters
  devp caches containers nerdctl  Just that one
  devp caches docker --json | jq '.summary.reclaimable_bytes'
                                  Machine-readable

Nothing here deletes anything. The prune commands are printed for you to run.";

pub const CACHES_CLEAR_LONG: &str = "\
Empty one manager's cache, or every one of them. What is about to go is listed and \
sized first, and unless `--yes` answers for you, it asks.

This is a convenience, not automation. No scheduler, no Git hook and no `devp run` \
will ever clear a cache — this only runs when you type it.

Wherever the manager ships its own subcommand, that is what runs: `npm cache clean \
--force`, `pnpm store prune`, `go clean -modcache`. The manager knows what is still \
referenced, which a directory delete cannot work out, and its own bookkeeping stays \
consistent. cargo, gradle, vcpkg and hex ship nothing equivalent, so those \
are cleared by removing the directory this command resolved and sized — never a string \
handed to a shell.

Maven is reported and never cleared. `~/.m2/repository` is an install target as \
well as a download cache — `mvn install:install-file` puts artifacts there that no \
remote can hand back — so dev-prune sizes it and prints `rm -rf ~/.m2/repository` \
for you to run. `clear maven` says so and stops; `clear all` skips it.

Two flags narrow what `all` means, so you do not have to pick the caches by hand. \
`--over-cap` keeps only the managers that have outgrown the ceiling you set in \
`cache_max_gb`; with no cap set anywhere it clears nothing and says so. `--unused` keeps \
only the managers that no registered repository uses — a cache with nothing behind it \
was filled for projects that are not on this disk any more. It counts only repositories \
dev-prune knows about, so `devp link` anything you keep outside the registry first, and \
it refuses to run at all when there are no registered repositories to check against.

Nothing else in a cache is lost; every manager re-downloads what it needs. What it costs \
is time, in every project on the machine, on the next install and the next `devp \
restore`. The freed size reported afterwards is measured rather than assumed, because \
a `prune` keeps what is still in use.";

pub const CACHES_CLEAR_EXAMPLES: &str = "\
EXAMPLES:
  devp caches clear npm           One manager, after confirming
  devp caches clear cargo         Both cargo rows: the registry cache and its sources
  devp caches clear all --dry-run Everything that would go, and nothing touched
  devp caches clear all --over-cap
                                  Only the ones past their cache_max_gb
  devp caches clear all --unused  Only the ones no registered repository uses
  devp caches clear all --yes     No prompt, for a script
  devp caches clear go --json --yes
                                  Machine-readable (`--json` requires `--yes`)

Exit code 1 if any cache could not be cleared; the rows are printed either way.
Exit code 2 for `maven`, which is reported but never cleared.";

pub const TRUST_LONG: &str = "\
What dev-prune is allowed to do on this machine, on one screen. Read-only — it reads \
the registry and the OS and changes nothing.

Two sections, and the split is the point. The first is guaranteed by the code: the \
seven safety invariants plus the two questions asked as often as any of them — there \
is no telemetry endpoint, and build output is never deleted. Those rows read the same \
on every machine and have no setting and no flag behind them. The second is read live \
off this machine: whether the scheduler is installed, whether the Git hooks register \
repositories on their own, how many repositories are registered, and the settings that \
widen what may happen without you asking for it.

There is no letter grade. A report that says `trust level: MEDIUM` tells you nothing \
you can act on, so the widened settings are named instead — `devp config show` has \
every one of them, and `devp config set <key> <value>` puts one back.

The long form of the guarantees is docs/SAFETY_INVARIANTS.md.";

pub const TRUST_EXAMPLES: &str = "\
EXAMPLES:
  devp trust                      The report
  devp trust --json | jq '.summary.widened'
                                  Just the settings that widen what may happen
  devp trust --json | jq -e '.summary.widened_count == 0'
                                  Exit 1 from jq if this machine has widened anything

Run interactively with a terminal, `--json` also copies the document to the clipboard.";

pub const CONFIG_LONG: &str = "\
Everything configurable lives under here: global settings (get/set/show/wizard), the \
per-repository `.devprune.json` (project), the OS background scheduler (daemon), the \
global Git auto-registration hooks (hook), and the file-manager icon registration \
(icon).

SHORTHANDS — `daemon`, `hook` and `icon` work without the leading `config`, and the \
action words people reach for are accepted:
  devp hook install       = devp config hook enable
  devp hook uninstall     = devp config hook disable
  devp daemon on / off    = devp config daemon enable / disable
  devp icon               = devp config icon
Accepted action words: enable/install/on, disable/uninstall/remove/off, status/show. \
Anything else is rejected — a mistyped action never silently degrades into a status \
report.";

pub const CONFIG_EXAMPLES: &str = "\
EXAMPLES:
  devp config show                Every global setting and its value
  devp config get idle_days       One setting
  devp config set idle_days 30    Change it (rejects out-of-range values)
  devp config recommended         Turn on everything the first run recommends
  devp config wizard              Walk through every setting, Enter keeps the current
  devp config project .           Inspect or create this repo's .devprune.json
  devp config daemon status       Is the background pass scheduled?
  devp config . daemon disable    Opt this repository out of the background pass
  devp config hook enable --chain Take core.hooksPath, forwarding to the tool holding it
  devp config icon                Register the .devprune.json icon and schema";

pub const CONFIG_GET_LONG: &str = "\
Print one global setting's current value. The keys, defaults and meanings:

  idle_days                  15     Days untouched before a repo is a candidate
  min_size_mb                0      Smallest directory worth deleting (0 = no floor)
  scan_depth                 6      Directory levels below a repo root discovery descends (1-32)
  require_confirmation       true   Whether a prune pass asks before deleting
  allow_manifest_rewrite     false  Whether verification may repair a drifted lockfile
  command_timeout_secs       600    Ceiling on any one package-manager command
  auto_setup                 true   Whether the integration pass may run unattended
  auto_daemon                true   …and may register the OS scheduler
  check_interval_days        2      How often the scheduler runs a pass
  auto_hooks                 true   …and may install the global Git hooks
  auto_hooks_chain           false  …and may chain onto another tool's core.hooksPath
  update_check               true   Whether the periodic release check runs
  update_check_interval_days 7      Minimum gap between two release checks
  update_check_timeout_secs  5      How long that one request may hang

Three have a per-repository override in `.devprune.json`, where they win for that \
tree only: `idle_days` (spelled `override_idle_days` there), `min_size_mb` and \
`scan_depth`. The rest are deliberately global — a committed `.devprune.json` must \
not be able to grant a repository `allow_manifest_rewrite` over its own manifests.";

pub const CONFIG_GET_EXAMPLES: &str = "\
EXAMPLES:
  devp config get idle_days
  devp config get update_check";

pub const CONFIG_SET_LONG: &str = "\
Change one global setting. A value outside the accepted range is rejected with the \
range in the message, never silently clamped — `scan_depth 0` and `scan_depth 40` are \
both refused outright. Keys are the same list `devp config get --help` shows.";

pub const CONFIG_SET_EXAMPLES: &str = "\
EXAMPLES:
  devp config set idle_days 30
  devp config set min_size_mb 50
  devp config set command_timeout_secs 1200
  devp config set update_check false    Turn the release check off for good";

pub const CONFIG_SHOW_LONG: &str = "\
Print every global setting with its current value. With `--update`, also run a sync \
pass across all registered repositories, refreshing each one's `.devprune.json` \
scaffolding without touching values you have changed.";

pub const CONFIG_SHOW_EXAMPLES: &str = "\
EXAMPLES:
  devp config show
  devp config show --update";

pub const CONFIG_PROJECT_LONG: &str = "\
Inspect a repository's `.devprune.json`, or create it when missing. The file holds \
the per-repository overrides — `override_idle_days`, `min_size_mb`, `scan_depth`, \
`ignore`, `disable_daemon`, `disable_hooks` — and carries a `$schema` line so any \
editor with JSON Schema support validates and completes it.

`--update` refreshes the file's scaffolding (schema pointer, missing keys) while \
keeping every value you have set. A file that does not parse is refused, not reset — \
fix it, or pass `--update` deliberately.

Writing the file also records it in the repository's `.git/info/exclude`, so the \
config — one machine's preference, not part of the project — never shows up in \
`git status`. The shared, tracked `.gitignore` is never modified.

`--team` addresses `project.devprune.json` instead: same keys, same schema, and \
deliberately not excluded, because it is the half meant to be committed. Every key it \
names wins over `.devprune.json`; every key it leaves out is still yours to answer. It \
is created empty apart from the schema line for that reason. Nothing dev-prune writes \
on your behalf ever edits it.

Both files can also carry `prunable.directories`: directories no lockfile describes, \
each with the `rebuild` command that puts it back. Unlike every other key, the two \
files' lists add up rather than one winning — a team declaration never discards your \
own. Before deleting one, dev-prune checks that it is inside the repository, that Git \
is tracking nothing in it, and that the rebuild command's tool is on this machine.

`prunable.exclude` lists declared paths to leave alone on this machine, whoever \
declared them — how you keep a directory the committed file calls rebuildable without \
editing a file the whole team shares. Spelled the same way as a `path`, and honoured \
from whichever file names it, because a veto only ever deletes less. Naming one path in \
both lists of the *same* file is a typo rather than a decision — the exclusion still \
wins, so the declaration never runs — and `devp doctor` says so.

Both files are read from the repository root and nowhere else, because the paths inside \
them are relative to that root. A copy one directory down parses and does nothing at \
all; `devp doctor` names it rather than moving it, since moving it would change what \
every path inside it means.";

pub const CONFIG_PROJECT_EXAMPLES: &str = "\
EXAMPLES:
  devp config project .           Show (or create) this repository's config
  devp config project ~/Code/api
  devp config project . --update  Refresh scaffolding, keep your values
  devp config project . --team    Create the committed project.devprune.json";

pub const CONFIG_DAEMON_LONG: &str = "\
The OS background scheduler — Task Scheduler on Windows, launchd on macOS, systemd \
timers on Linux — which runs `devp run --daemon` every `check_interval_days` days.

Globally: `enable` registers the schedule, `disable` removes it, `status` reports \
it. With a path first, the same words act on one repository via `disable_daemon` in \
its `.devprune.json`: the machine-wide pass keeps running, that repository sits it \
out.";

pub const CONFIG_DAEMON_EXAMPLES: &str = "\
EXAMPLES:
  devp daemon status              Machine-wide scheduler state
  devp daemon enable              Register the schedule (also: install, on)
  devp daemon disable             Remove it (also: uninstall, remove, off)
  devp config . daemon disable    This repository opts out of the background pass
  devp config ~/Code/api daemon enable";

pub const CONFIG_HOOK_LONG: &str = "\
The global Git hooks (via `core.hooksPath`) that auto-register any repository you \
commit in — `devp link --quiet`, silent, honouring opt-outs. `enable` installs them, \
`disable` removes them (restoring what was there), `status` reports them. With a \
path first, the same words act on one repository via `disable_hooks`.

Git has exactly one global `core.hooksPath` and no way to chain two, so a tool that \
holds it (husky, pre-commit, lefthook) shuts every other one out. `--chain` is the \
way through: dev-prune takes the slot and writes, per hook, a shim that does its own \
work and then execs the same-named hook in the displaced directory — their hooks \
keep firing, their exit codes still block commits. `devp hook uninstall` puts the \
original back. The chain snapshots the other tool's hooks at install time; `devp \
hook status` reports any that have drifted, and `devp hook install --chain` rebuilds.";

pub const CONFIG_HOOK_EXAMPLES: &str = "\
EXAMPLES:
  devp hook status                Installed? Chained? Drifted?
  devp hook install               Take the free core.hooksPath slot
  devp hook install --chain       Take a slot husky/pre-commit/lefthook holds, forwarding
  devp hook uninstall             Restore the previous core.hooksPath
  devp config . hook disable      This repository opts out of auto-registration";

pub const CONFIG_ICON_LONG: &str = "\
Register `*.devprune.json` with the OS file manager and write the icon files and the \
JSON Schema into the config directory. On Linux this is a complete registration \
(shared-mime-info plus hicolor icons — Nautilus, Dolphin, Thunar, Nemo, PCManFM). On \
Windows, Explorer resolves icons by last extension only, so the config folder gets \
its own icon instead of hijacking every `.json` on the machine. On macOS a UTI must \
come from an application bundle, which a single binary is not.

It also prints an editor snippet to paste yourself — it never edits your editor \
settings, your PATH, or your shell startup files.";

pub const CONFIG_ICON_EXAMPLES: &str = "\
EXAMPLES:
  devp icon                       Same command, without the leading `config`";

pub const CONFIG_RECOMMENDED_LONG: &str = "\
Turn on everything the first run recommends, without sitting through the first run.

The recommendations are the adapters and behaviours that are off by default because \
they are not universally wanted, not because they are risky: Cargo, Gradle, Maven, \
Swift, Dart, Mix builds, vcpkg and CMake builds. Accepting them all is one command \
here and one keypress in `devp config wizard`, and both read the same list, so the \
two can never drift apart.

One recommendation is held back: `allow_manifest_rewrite` lets `cargo` and `go` tidy \
up their own manifests during a restore, which edits files in your working tree. \
That is worth having and it is worth knowing about first, so it arrives only when \
you type --with-cautious. Everything printed is also printed by `devp config show`, \
which lists whatever you have not taken yet.

Nothing here is irreversible: `devp config set <key> false` puts any of it back, and \
this command never marks the settings as reviewed — the walkthrough you skipped is \
still owed to you, and will still open.";

pub const CONFIG_RECOMMENDED_EXAMPLES: &str = "\
EXAMPLES:
  devp config recommended                  Everything recommended without a caveat
  devp config recommended --with-cautious  That, plus allow_manifest_rewrite
  devp config show                         What is still outstanding
  devp config set enable_cargo false       Put one back";

pub const CONFIG_WIZARD_LONG: &str = "\
Open every global setting in a full-screen configurator, with the `devp trust` \
declaration in front of it: what this tool is allowed to do is on screen before any \
of it is configurable.

Arrows move; Space changes the highlighted setting — a toggle flips, a number opens \
a field, `disabled_adapters` opens the adapter checklist; `r` puts one back. The \
list ends in a Finish line: two presses of Enter there open the last screen, which \
lists exactly what will be written, before it is written. Two presses rather than \
one, because one Enter is what people press to dismiss a screen they have stopped \
reading. `q` leaves without saving anything, from anywhere.

`devp config recommended` is the one-command version of the suggestions screen, for \
when you know what you want and do not want to walk the list.

It runs itself once on a first install — so the defaults are something you agreed \
to, not something you inherited — and again after an upgrade adds a setting this \
machine has never been shown, which it marks NEW and opens on. Settings you have \
already confirmed are never re-asked.

It never runs unattended: no TTY means skipped, not guessed at. `--no-tui`, and the \
DEV_PRUNE_NO_TUI environment variable, ask one question per line instead — for \
terminals the full-screen view cannot drive, and for agents, which hold a real \
terminal and will never press a key. To configure this tool from a script, use \
`devp config set <key> <value>`, which needs no terminal at all.";

pub const CONFIG_WIZARD_EXAMPLES: &str = "\
EXAMPLES:
  devp config wizard
  devp config wizard --no-tui              One question per line
  devp config set disabled_adapters go     Leave Go projects alone entirely
  devp config set disabled_adapters -      Every adapter active again";

pub const RESTORE_LONG: &str = "\
Put dependencies back: detect each project's lockfile and run its manager's install \
(`npm ci`, `pnpm install`, `uv sync`, `cargo fetch`, …). Mirrors pruning — every \
project in the tree is restored, each by its own manager, so a monorepo comes back \
whole.

`--last-run` restores exactly what the most recent prune pass deleted, across every \
repository it touched, and nothing else: the undo for a `devp run`. Each prune \
records what it removed (a dry run records nothing), and the flag fails cleanly if \
no pass has been recorded yet. It cannot be combined with a path — silently ignoring \
the path would restore the wrong thing.";

pub const RESTORE_EXAMPLES: &str = "\
EXAMPLES:
  devp restore                    Restore the current directory's projects
  devp restore ~/Code/my-app
  devp restore --last-run         Undo the last prune pass, everywhere it acted";

pub const UPDATE_LONG: &str = "\
Print the installed version, ask GitHub's public API for the latest release, and \
show the upgrade command for how this copy was installed. `--install` runs the upgrade through the \
package manager that owns this copy (cargo, npm, bun, pnpm, yarn, uv, pipx, or the \
installer script). `--channels` prints the command for every channel instead of only \
this one, and touches nothing. `auto_update` is on by default and does the verified-download half by \
itself at the end of a prune pass when a newer release is known — never the \
package-manager half, and nothing at all on WinGet, Scoop and Homebrew, where the \
manager owns the upgrade; `devp config set auto_update false` stops it. An upgrade never interrupts the \
scheduler: the scheduled pass runs a managed copy that refreshes itself from the \
new binary on its next run.

`devp config set version_lock true` outranks all of it. While the pin is on this \
copy stays on the version it is: `auto_update` does not run however it is set, \
`--install` refuses, `devp install --channel` refuses because moving channels \
installs the latest release, and the install scripts leave the binary alone. \
No flag bypasses it -- `devp config set version_lock false` is the way back.

The same check also runs quietly from `devp run` and `devp status`, at most once \
every `update_check_interval_days` (7 by default), printing one line only when a \
newer version exists. It is the only thing in dev-prune that opens a network \
connection, sends no body and no identifier, and `devp config set update_check \
false` turns it off for good; `--offline` skips it for one run without changing the \
setting.";

pub const UPDATE_EXAMPLES: &str = "\
EXAMPLES:
  devp update                     Version, latest release, upgrade command
  devp update --install           Upgrade now, through the owning channel
  devp update --channels          Every channel's upgrade command, no network
  devp update --offline           No network this run";

pub const INSTALL_LONG: &str = "\
Move this installation from one package manager to another.

`devp update` always upgrades the copy that is running, through whichever channel \
installed it. This command changes *which* channel owns it: it installs through the \
manager you name, then removes the old copy through the manager that put it there — in \
that order, so a failed install leaves the working copy untouched.

Removing the old copy through its own manager, rather than deleting the file, is the \
point: uv, pipx, npm, cargo and the rest each keep a record of what they installed, and \
a manager whose record still says dev-prune is there will put the old binary back.

bun, pnpm and yarn install the same npm package and are each their own channel, not \
npm. A copy `bun add -g dev-prune` put in place is upgraded with bun and removed with \
bun; running npm against it installs a second copy under npm's prefix and leaves the \
first one stale and still on PATH.

Nothing is migrated, because nothing needs to be. Settings, the repository registry and \
the undo history live in the config directory, which no package manager owns.

With no `--channel` it prints which manager installed this copy and what `--channel` \
accepts. `--dry-run` prints the commands without running any of them.";

pub const INSTALL_EXAMPLES: &str = "\
EXAMPLES:
  devp install                              Which channel owns this copy
  devp install --channel winget --dry-run   Print the plan, change nothing
  devp install --channel uv                 Move onto uv, and remove the old copy
  devp install --channel cargo --yes        Skip the confirmation prompt";

pub const SKILL_LONG: &str = "\
Teach your AI assistant this tool. Exports SKILL.md — the full agent-facing manual: \
every command, the JSON contracts, the safety invariants, the troubleshooting tree — \
into the config directory, installs it into any detected agent skills directory \
(`~/.claude/skills/dev-prune/`, the same install `devp setup` performs), and prints \
ready-to-copy onboarding prompts for assistants without one (Gemini Antigravity, \
Cursor, Windsurf, Copilot, OpenClaw).

`--agent <EDITOR>` instead writes per-repository rules into the current repository, in \
the file that editor's agent reads. Ten editors get a file of their own — cursor, \
windsurf, antigravity, cline, roo, kilocode, continue, amazon-q, kiro, trae — and six \
share a file with other tools, so dev-prune owns a marked block inside it: agents-md \
(`AGENTS.md`, the cross-tool convention Codex, Jules, Amp and OpenCode read), copilot \
(`.github/copilot-instructions.md`), gemini (`GEMINI.md`), junie \
(`.junie/guidelines.md`), zed (`.rules`) and aider (`CONVENTIONS.md`, the one file its \
editor does not read by finding it — writing it prints the `read: CONVENTIONS.md` line \
that makes Aider load it). Every byte outside the markers is left as found. \
`devp skill --help` lists each value with its exact path. Claude Code needs no \
per-repository file — its skill installs globally.";

pub const SKILL_EXAMPLES: &str = "\
EXAMPLES:
  devp skill                      Export SKILL.md, print onboarding prompts
  devp skill --agent cursor       Write .cursor/rules/dev-prune.mdc here
  devp skill --agent agents-md    Upsert the marked block in AGENTS.md";

pub const SETUP_LONG: &str = "\
Install whatever integration is missing and leave the rest alone: the `devp` alias, \
the managed binary directory on your PATH (a user PATH entry on Windows, \
`~/.local/bin` symlinks elsewhere — what keeps `devp` working after the venv or npx \
cache it came from disappears), the exported SKILL.md and its agent-directory \
install, the file-manager icon registration, the global Git hooks, and the OS \
scheduler. Safe to run repeatedly: it is the same pass the install scripts run, the \
same one `devp init` runs, and the same one that runs by itself on the first command \
after an upgrade.

It skips rather than forces: Git hooks when `git` is missing or another tool holds \
`core.hooksPath` (take the slot with `devp hook install --chain`), the alias when \
the running process is `devp` itself on Windows, and anything switched off by \
`auto_setup`, `auto_hooks`, `auto_daemon` or `DEV_PRUNE_NO_AUTO_SETUP=1`.

When a VS Code-family editor is on your PATH (VS Code, VSCodium, Cursor, Windsurf, \
Positron, Kiro, or an Insiders build) and the dev-prune extension is not installed, \
one run also asks — once ever, only at a terminal — whether to install it into each \
editor found. Each editor installs from its own registry; when a fork's registry does \
not carry the extension, the `.vsix` from the extension's own newest release is \
installed instead. Decline and it never asks again; install it yourself later with \
`code --install-extension VKrishna04.dev-prune`.";

pub const SETUP_EXAMPLES: &str = "\
EXAMPLES:
  devp setup                      Install what is missing, report what was skipped
  devp setup --status             Report only; change nothing
  DEV_PRUNE_NO_AUTO_SETUP=1 devp init ~/Code
                                  Register repositories, install nothing";

pub const DOCTOR_LONG: &str = "\
One read-only pass that answers \"why is this not doing what I expect\". Without a \
path it checks the installation: binary and twin, PATH, registry health, every \
stored setting revalidated, SKILL.md, icons, hooks, scheduler, the package-manager \
binaries your repositories actually need, and the release-check state. With a path \
it checks that repository and ends by naming the single reason a prune would or \
would not touch it.

`--fix` is diagnosis first, then treatment — and it mends installed-but-broken only: \
a stale or missing `devp` twin, a missing SKILL.md export, hooks or a scheduler \
whose recorded binary moved, a drifted hook chain, and registry entries whose \
repository is gone. Each repair is the corresponding `devp setup` pass re-run, so a \
repair can never do more than setup itself would. It never performs a first-time \
install, and never touches an unreadable registry — a parse failure is for you to \
look at, not for a tool to guess at.

EXIT CODES: 0 when everything works, warnings included — a missing scheduler should \
not fail a script. 1 only for genuine breakage: an unreadable registry, an \
out-of-range setting, a dead registered path, a directory that is not a Git \
repository. `--fix` exits 1 when any repair failed or was out of reach.";

pub const DOCTOR_EXAMPLES: &str = "\
EXAMPLES:
  devp doctor                     Check the installation
  devp doctor .                   Why would a prune touch (or skip) this repository?
  devp doctor ~/Code/api
  devp doctor --fix               Repair what the installation check finds broken";

pub const UNINSTALL_LONG: &str = "\
Remove dev-prune from the machine: the OS scheduler, the global Git hooks (only if \
`core.hooksPath` still points at dev-prune), the file-type icons, the installed \
agent skill, the PATH entry (or `~/.local/bin` symlinks), and the binaries — the \
managed pair, the copy you invoked, and, with your confirmation, every other copy \
found on PATH or in the well-known install directories (`~/.cargo/bin`, \
`~/.local/bin`, npm's global directory, pip's Scripts directories). A copy owned by \
a package manager is listed with its manager, and after removal the manager's own \
uninstall command is printed so its records can be cleared too.

On Windows, where a running executable cannot delete itself, a detached helper \
removes the last files a few seconds after the command exits — no reboot, no closed \
terminal.

Without `--deep` the configuration survives, so a reinstall picks up where you left \
off. With `--deep` the global config folder and every registered repository's \
`.devprune.json` go too; that asks for confirmation, and refuses outright with no \
terminal to ask on unless `-y` is passed. Exits 1 if anything could not be removed, \
naming each leftover.";

pub const UNINSTALL_EXAMPLES: &str = "\
EXAMPLES:
  devp uninstall                  Remove the program; keep config for a reinstall
  devp uninstall --deep           Also wipe config and per-repo .devprune.json (asks)
  devp uninstall --deep -y        Non-interactive; also confirms the stray-copy sweep";
