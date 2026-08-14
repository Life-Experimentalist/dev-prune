// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

pub mod adapters;
pub mod commands;
pub mod config;
pub mod constants;
pub mod daemon;
pub mod engine;
pub mod json;
pub mod output;
pub mod scanner;
pub mod setup;
pub mod tui;
pub mod workspace;

use clap::{Parser, Subcommand};

/// Process exit codes, so scripts and CI can branch on the outcome.
///
/// These are part of the tool's contract and are documented in `docs/CLI_REFERENCE.md`;
/// changing one is a breaking change.
pub mod exit_code {
    /// The command did what it was asked to do. A prune that deleted nothing because
    /// nothing was idle is still a success.
    pub const OK: i32 = 0;
    /// The command failed. The reason is on stderr.
    pub const FAILURE: i32 = 1;
    /// The arguments were not usable. Emitted by clap, listed here so the set is complete.
    pub const USAGE: i32 = 2;
}

/// Restore the default disposition for `SIGPIPE`.
///
/// Rust ignores `SIGPIPE` at startup, which turns `devp status | head` into a panic —
/// "failed printing to stdout" plus a backtrace — where every other Unix tool simply
/// stops. Putting the default back makes dev-prune behave like `ls` in a pipeline.
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: `signal` with SIG_DFL is async-signal-safe and this runs before any
    // thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

/// Explain the rename, then answer the question the user was really asking.
///
/// Nobody types `--force` for fun. They type it because something was not pruned and
/// they want the tool to stop arguing — so a bare "the flag moved" note would leave
/// them exactly as stuck as before. The list below is every reason a directory gets
/// skipped, with the fix, because six of the seven are not what `--force` was for.
///
/// Goes to stderr with the rest of the diagnostics, so `--json` stays parseable.
fn print_force_help() {
    output::print_notice(
        "`--force` is now `--ignore-idle`, which is what it has always actually done. \
         The old spelling still works.",
    );
    eprintln!(
        "
  Reaching for --force usually means something did not get pruned. It is one of these:

    Not idle yet          A commit or a source edit inside idle_days (15 by default).
                          This is the one --ignore-idle is for.
    Lockfile unusable     The package manager could not confirm it. Run the command
                          dev-prune printed, then try again. No flag skips this check.
    Opted out             `ignore.devprune.json` in the root, or `\"ignore\": true`
                          in `.devprune.json`.
    Under the size floor  Smaller than min_size_mb. `--min-size 0` includes it.
    Not registered        `devp link .` first; `devp status` shows what is tracked.
    Nested or symlinked   A submodule is pruned as itself, never as part of its
                          parent, and a linked directory is refused. By design.
    Too deep              Beyond scan_depth (6 levels). `devp config set scan_depth N`.

  `devp run --dry-run` names the actual reason, per repository.

  Still stuck? Ask your AI assistant — `devp skill` hands it the full troubleshooting
  tree, including this list. It has read it. It wrote it.
"
    );
}

/// Whether a failure is just the reader at the other end of a pipe hanging up.
///
/// `devp status | head -5` is a normal thing to type, and the closed pipe it produces is
/// not an error worth printing — printing it would itself fail.
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

/// Universal, lockfile-safe workspace pruner and background dependency cleaner.
///
/// Note: `dev-prune` and `devp` are interchangeable binary aliases.
#[derive(Parser, Debug)]
#[command(name = constants::APP_NAME)]
#[command(version = constants::VERSION)]
#[command(author = constants::AUTHOR)]
#[command(long_version = constants::LONG_VERSION.as_str())]
#[command(
    about = "Universal, lockfile-safe workspace pruner and background dependency cleaner\nNote: `dev-prune` and `devp` are interchangeable binary aliases."
)]
#[command(
    after_help = "EXAMPLES:\n  devp init ~/Code          Scan directory trees & onboard workspaces\n  devp link                 Register current repository\n  devp run                  Execute prune pass across inactive repositories\n  devp status               View system status dashboard\n  devp status --top 10      Show only the ten biggest reclaims\n  devp stats                Lifetime totals, recent passes, biggest repositories\n  devp caches               Size every package manager cache (deletes nothing)\n  devp completions powershell   Emit a shell completion script\n  devp status daemon        Check background daemon status (alias for `devp config daemon status`)\n  devp status . hook        Check workspace Git hook status (alias for `devp config . hook status`)\n  devp config . daemon disable  Disable daemon background pass for current workspace\n  devp restore .            Restore missing node_modules/.venv via lockfile\n  devp undo                 Revert most recent init or link action\n\nBINARY ALIAS:\n  `dev-prune` and `devp` invoke the exact same executable.\n\ndev-prune is written by VKrishna04 and licensed Apache-2.0.\n  https://github.com/Life-Experimentalist/dev-prune"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Simulate pruning without deleting any files.
    #[arg(long, global = true)]
    dry_run: bool,

    /// Prune repositories you are still working in, ignoring the idle-day threshold.
    ///
    /// This is the *only* check it lifts. Lockfile verification, `ignore.devprune.json`,
    /// `"ignore": true`, symlink refusal and nested-repository refusal all still apply.
    #[arg(long, global = true)]
    ignore_idle: bool,

    /// Deprecated spelling of `--ignore-idle`.
    ///
    /// Renamed because "force" reads like "override the safety checks", which it never
    /// did — it only ever skipped the idle-day wait. Still accepted; prints a note.
    #[arg(long, global = true)]
    force: bool,

    /// Bypass interactive confirmation prompts.
    #[arg(long, short = 'y', global = true)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Workspace onboarding & discovery: crawl paths for Git repositories and register them.
    #[command(alias = "scan", alias = "onboard")]
    Init {
        /// Paths to scan for Git repositories (defaults to current directory).
        #[arg(default_value = ".")]
        paths: Vec<String>,
    },

    /// Register a single Git repository for pruning (defaults to current directory `.`).
    Link {
        /// Path to the Git repository to register.
        #[arg(default_value = ".")]
        path: String,

        /// Suppress output and skip repos that set `disable_hooks`. Used by the Git hook.
        #[arg(long)]
        quiet: bool,
    },

    /// Remove a repository from the dev-prune registry (does not delete workspace files).
    Unlink {
        /// Path to the Git repository to unregister.
        #[arg(default_value = ".")]
        path: String,

        /// Unregister every path that no longer exists, instead of one named repository.
        #[arg(long, conflicts_with = "path")]
        missing: bool,
    },

    /// Revert the most recent init or link action.
    Undo,

    /// Run a prune pass across all registered repositories or a target directory (`devp run .`).
    Run {
        /// Optional target workspace path. If omitted, runs across all registered repositories.
        target_path: Option<String>,

        /// Mark this as the scheduled background pass. Repositories that set
        /// `disable_daemon` in `.devprune.json` are skipped. Set by the installed scheduler.
        #[arg(long)]
        daemon: bool,

        /// Act only on these package managers (comma-separated),
        /// e.g. `--only npm,pnpm`. Unknown names are an error.
        #[arg(long, value_name = "ADAPTERS", conflicts_with = "skip")]
        only: Option<String>,

        /// Leave these package managers alone (comma-separated), e.g. `--skip cargo`.
        #[arg(long, value_name = "ADAPTERS")]
        skip: Option<String>,

        /// Ignore bloat directories smaller than this many MiB. Overrides `min_size_mb`.
        #[arg(long, value_name = "MIB")]
        min_size: Option<u64>,

        /// Prune everything except these repositories (comma-separated paths or names).
        ///
        /// The safe way to express "clean up but keep the API project": that project is
        /// never verified, never deleted and never reinstalled, instead of being pruned
        /// and then restored over the network.
        #[arg(long, value_name = "REPOS")]
        except: Option<String>,

        /// Emit one JSON document instead of the human report. Implies non-interactive.
        #[arg(long)]
        json: bool,
    },

    /// View system dashboard: registered repos, background daemon, Git hooks & space metrics.
    Status {
        /// Show only the N repositories with the most reclaimable space.
        ///
        /// The dashboard lists every registered repository, which on a machine with a
        /// hundred of them buries the handful actually worth pruning. Applies to the TUI,
        /// the plain table and `--json` alike.
        ///
        /// Zero is rejected up front: "show the top 0" can only be a typo, and an empty
        /// dashboard that looks like an empty registry is worse than a usage error.
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
        top: Option<u64>,

        /// Report lockfile drift instead of the dashboard: environments holding packages
        /// their lockfile never recorded — the installs a prune would refuse to delete
        /// because nothing could bring them back.
        ///
        /// A pure read: no package manager runs, nothing is written. Checked where a
        /// file-level comparison exists — npm, uv and venv projects.
        #[arg(long, conflicts_with = "top")]
        drift: bool,

        /// Emit the dashboard as one JSON document instead of the TUI or text table.
        #[arg(long)]
        json: bool,
    },

    /// Show lifetime space reclaimed, recent prune passes, and the biggest repositories.
    Stats {
        /// Emit the figures as one JSON document instead of the text report.
        #[arg(long)]
        json: bool,
    },

    /// Print a shell completion script for bash, zsh, fish, PowerShell or elvish.
    Completions {
        /// Shell to generate for.
        shell: clap_complete::Shell,
    },

    /// Report the size of every package manager cache on this machine (read-only, deletes nothing).
    Caches {
        /// Emit the report as one JSON document instead of the table.
        #[arg(long)]
        json: bool,
    },

    /// Manage global settings, background daemon, Git hooks, custom icons, or per-project .devprune.json.
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Restore dependencies in a project using its lockfile (npm ci, pnpm install, uv sync).
    Restore {
        /// Path to the project to restore (defaults to current directory).
        path: Option<String>,

        /// Put back exactly what the most recent prune pass deleted, in every repository
        /// it touched. The undo for a `run`.
        #[arg(long, conflicts_with = "path")]
        last_run: bool,
    },

    /// Print the installed version, check for a newer release, and show how to upgrade.
    Update {
        /// Skip the release check for this run. The check is the only thing in dev-prune
        /// that opens a network connection; `devp config set update_check false` turns
        /// it off for good.
        #[arg(long)]
        offline: bool,
    },

    /// Export SKILL.md and display ready-to-copy AI Agent onboarding & skill import prompts.
    Skill,

    /// Install whatever dev-prune integration is missing: alias, SKILL.md, Git hooks, scheduler.
    Setup {
        /// Report what is installed without changing anything.
        #[arg(long)]
        status: bool,
    },

    /// Diagnose the installation, or one repository if given a path (`devp doctor .`).
    Doctor {
        /// Repository to diagnose. Omit to check the installation itself.
        path: Option<String>,

        /// Repair what the installation check finds broken: refresh a stale or missing
        /// `devp` twin, re-export SKILL.md, re-register a scheduler or Git hooks whose
        /// binary moved, and drop registry entries whose repository is gone.
        ///
        /// Repairs only what was installed and has since broken — it never installs an
        /// integration that was never set up (that is `devp setup`), and it cannot fix a
        /// corrupt registry file, which needs a human decision.
        #[arg(long, conflicts_with = "path")]
        fix: bool,
    },

    /// Uninstall background daemon, Git hooks, and optionally wipe configuration.
    Uninstall {
        /// Perform a deep uninstall (wipe configuration folder and .devprune.json files).
        #[arg(long)]
        deep: bool,
    },
}

impl Commands {
    /// Whether this command's stdout is something another program reads.
    ///
    /// Two cases. `--json` promises stdout carries one document and nothing else, and
    /// `completions` prints a script that gets sourced — a stray line in either is a
    /// parse error rather than a nicety. `link --quiet` is the Git hook path, which runs
    /// inside somebody's commit.
    ///
    /// Everything else defers to [`output::print_attribution`], which prints only when
    /// stdout is a terminal. Neither function checks that the line is intact, and nothing
    /// downstream depends on it having been printed.
    fn suppresses_attribution(&self) -> bool {
        match self {
            Commands::Completions { .. } => true,
            Commands::Run { json, .. }
            | Commands::Status { json, .. }
            | Commands::Stats { json }
            | Commands::Caches { json } => *json,
            Commands::Link { quiet, .. } => *quiet,
            _ => false,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Display a global configuration value.
    Get {
        /// Any key `devp config show` lists — idle_days, min_size_mb, scan_depth,
        /// require_confirmation, allow_manifest_rewrite, command_timeout_secs,
        /// auto_setup, auto_daemon, check_interval_days, auto_hooks, auto_hooks_chain,
        /// update_check, update_check_interval_days, update_check_timeout_secs.
        key: String,
    },
    /// Set a global configuration value.
    Set {
        /// Configuration key.
        key: String,
        /// New value.
        value: String,
    },
    /// Show all global configuration values or sync per-repo configurations.
    Show {
        /// Force update/sync pass across all registered repos.
        #[arg(long, short)]
        update: bool,
    },
    /// Inspect or initialize per-repository config (.devprune.json) for a workspace path.
    Project {
        /// Path to the repository (defaults to current directory).
        #[arg(default_value = ".")]
        path: String,
        /// Force update/sync pass on this project config.
        #[arg(long, short)]
        update: bool,
    },
    /// Configure OS background daemon scheduler globally or for a workspace path.
    Daemon {
        /// Optional workspace path or sub-action (enable, disable, status).
        target: Option<String>,
        /// Sub-action if path was provided (enable, disable, status).
        sub_action: Option<String>,
    },
    /// Configure non-blocking global Git background auto-registration hooks globally or for a workspace path.
    Hook {
        /// Optional workspace path or sub-action (enable, disable, status).
        target: Option<String>,
        /// Sub-action if path was provided (enable, disable, status).
        sub_action: Option<String>,
        /// Install in front of the hooks directory already configured, forwarding to it,
        /// instead of refusing to take a slot another tool is using.
        #[arg(long)]
        chain: bool,
    },
    /// Register a file-manager icon for .devprune.json, and print an editor snippet.
    Icon,
    /// Walk through every global setting, confirming or changing each one.
    Wizard,
}

/// Create the `devp` executable alias next to `dev-prune`, and keep it current.
///
/// Runs on every invocation because it is two `stat` calls in the settled case, and
/// because the alias is how most people invoke this tool — it must never be the stale
/// half of an upgrade.
///
/// `DEV_PRUNE_NO_AUTO_SETUP` suppresses it, and so does looking like CI or a container,
/// because writing a second executable next to the first is a self-installation like any
/// other — and those environments cannot set the variable before the first run. `devp
/// setup` still creates the alias in either case: it governs the unattended pass, not
/// the explicit request.
pub fn ensure_devp_alias() {
    if setup::no_auto_setup_requested() || setup::unattended_environment().is_some() {
        return;
    }
    let _ = setup::ensure_alias();
}

/// Print rich version & system environment details for -v / -V / --version.
///
/// This, not clap, is what `devp --version` actually runs — [`normalize_args`] catches the
/// flag first. The author and repository are printed here because a copy of this binary
/// found on a machine with no package manager record should still be able to say where it
/// came from, and `--version` is the first thing anyone runs on an unknown executable.
pub fn print_version_info() {
    output::print_banner();
    println!("dev-prune (devp) v{}", constants::VERSION);
    println!("  Binary Aliases:  dev-prune | devp (interchangeable)");
    println!("  Author:          {}", constants::AUTHOR);
    println!("  Repository:      {}", constants::REPO_URL);
    println!("  Homepage:        {}", constants::HOMEPAGE_URL);
    println!("  Target OS:       {}", std::env::consts::OS);
    println!("  Architecture:    {}", std::env::consts::ARCH);
    println!("  Compiler:        Rust 1.85+ (edition 2024)");
    println!("  License:         Apache-2.0 (no analytics, no diagnostics)");
    println!();
    let reg_path = config::Registry::registry_path()
        .map(|p| output::clean_path(&p))
        .unwrap_or_else(|_| "unknown".to_string());
    println!("  Config Path:     {reg_path}");

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let exe_dir_str = output::clean_path(exe_dir);
            let path_var = std::env::var("PATH").unwrap_or_default();
            let is_in_path = path_var
                .split(if cfg!(windows) { ';' } else { ':' })
                .any(|p| std::path::Path::new(p) == exe_dir);

            println!("  Binary Dir:      {exe_dir_str}");
            if is_in_path {
                println!("  PATH Audit:      ✓ Executable directory is active in system PATH.");
            } else {
                println!("  PATH Audit:      ⚠ Executable directory is NOT in system PATH!");
                println!("                   Add `{exe_dir_str}` to Environment Variables.");
            }
        }
    }
}

/// Case-insensitive subcommand normalizer and status alias router.
fn normalize_args() -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && (args[1] == "-v" || args[1] == "-V" || args[1] == "--version") {
        print_version_info();
        std::process::exit(exit_code::OK);
    }
    if args.len() <= 1
        || args
            .iter()
            .any(|a| a == "-h" || a == "--help" || a == "help")
    {
        output::print_banner();
    }
    if args.len() <= 1 {
        return args;
    }

    let mut normalized = vec![args[0].clone()];
    for (i, arg) in args.iter().enumerate().skip(1) {
        if i == 1 && !arg.starts_with('-') {
            normalized.push(arg.to_lowercase());
        } else {
            normalized.push(arg.clone());
        }
    }

    // Map `devp daemon|hook|icon [ARGS...]` -> `devp config daemon|hook|icon [ARGS...]`
    //
    // These live under `config` because that is where the rest of the persistent
    // settings live, but nobody types `devp config hook install` when they mean
    // "install the hook" — and the tool's own output has always said `devp hook
    // install`. Accepting both costs one insert and removes a papercut.
    if matches!(normalized[1].as_str(), "daemon" | "hook" | "icon") {
        normalized.insert(1, "config".to_string());
    }

    // Map `devp status [PATH] daemon` -> `devp config daemon [PATH] status`
    // Map `devp status [PATH] hook`   -> `devp config hook [PATH] status`
    //
    // Exactly one optional PATH, and never a flag: `devp status --json daemon` must
    // reach clap as typed and fail there, not be rewritten with `--json` as a path.
    if normalized[1] == "status"
        && (normalized.len() == 3 || (normalized.len() == 4 && !normalized[2].starts_with('-')))
    {
        let last = normalized
            .last()
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if last == "daemon" || last == "hook" {
            let mut rewrited = vec![normalized[0].clone(), "config".to_string(), last];
            if normalized.len() > 3 {
                rewrited.push(normalized[2].clone());
            }
            rewrited.push("status".to_string());
            return rewrited;
        }
    }

    // Map `devp config [PATH] daemon [ACTION]` -> `devp config daemon [PATH] [ACTION]`
    // Map `devp config [PATH] hook [ACTION]`   -> `devp config hook [PATH] [ACTION]`
    //
    // Only when the second argument can actually be a path — a flag there means the
    // user is talking to `config` itself and the rewrite would misfile it.
    if normalized.len() >= 4 && normalized[1] == "config" && !normalized[2].starts_with('-') {
        let third = normalized[3].to_lowercase();
        if third == "daemon" || third == "hook" {
            let mut rewrited = vec![
                normalized[0].clone(),
                "config".to_string(),
                third,
                normalized[2].clone(),
            ];
            for extra in &normalized[4..] {
                rewrited.push(extra.clone());
            }
            return rewrited;
        }
    }

    normalized
}

/// Whether the automatic setup pass may run for this invocation.
///
/// Two callers are excluded on purpose. The Git hook runs `link --quiet` with no
/// terminal attached and inside someone's commit; the scheduler runs `run --daemon` the
/// same way. An integration pass nobody can see is one nobody can refuse, so both wait
/// for the next command a human types. `uninstall` is excluded for the obvious reason,
/// and `setup` because it is the pass, run deliberately.
fn auto_setup_allowed(args: &[String]) -> bool {
    let subcommand = args.get(1).map(String::as_str).unwrap_or("");
    !matches!(subcommand, "uninstall" | "setup")
        && !args.iter().any(|a| a == "--quiet" || a == "--daemon")
}

/// Run the CLI application.
pub fn run_cli() {
    restore_sigpipe();
    ensure_devp_alias();

    let args = normalize_args();
    if auto_setup_allowed(&args) {
        setup::auto_setup_if_due();
    }
    let cli = Cli::parse_from(args);

    // Both spellings mean the same thing; the old one just says so first.
    let ignore_idle = cli.ignore_idle || cli.force;
    if cli.force {
        print_force_help();
    }

    // Decided before the match, because that is where `cli.command` is consumed.
    let credit_the_author = !cli.command.suppresses_attribution();

    // Every path the user typed passes through `expand_tilde` on the way in. PowerShell
    // and cmd hand us `~/Code` verbatim, so without this the documented one-liner
    // registers a directory literally named `~`.
    let result = match cli.command {
        Commands::Init { paths } => {
            let paths: Vec<String> = paths.iter().map(|p| config::expand_tilde(p)).collect();
            commands::init::run(&paths, cli.dry_run)
        }
        Commands::Link { path, quiet } => {
            commands::link::run_link(&config::expand_tilde(&path), quiet)
        }
        Commands::Unlink { path, missing } => {
            if missing {
                commands::link::run_unlink_missing()
            } else {
                commands::link::run_unlink(&config::expand_tilde(&path))
            }
        }
        Commands::Undo => commands::undo::run(),
        Commands::Run {
            target_path,
            daemon,
            only,
            skip,
            min_size,
            except,
            json,
        } => {
            let target_path = target_path.map(|p| config::expand_tilde(&p));
            commands::run::run(commands::run::RunArgs {
                target_path: target_path.as_deref(),
                dry_run: cli.dry_run,
                force: ignore_idle,
                yes: cli.yes,
                daemon,
                only: only.as_deref(),
                skip: skip.as_deref(),
                min_size_mb: min_size,
                except: except.as_deref(),
                json,
            })
        }
        Commands::Status { top, drift, json } => {
            commands::status::run(top.map(|n| n as usize), drift, json)
        }
        Commands::Stats { json } => commands::stats::run(json),
        Commands::Completions { shell } => commands::completions::run(shell),
        Commands::Caches { json } => commands::caches::run(json),
        Commands::Config { action } => match action {
            Some(ConfigAction::Get { key }) => commands::config::run_get(&key),
            Some(ConfigAction::Set { key, value }) => commands::config::run_set(&key, &value),
            Some(ConfigAction::Show { update: true }) => commands::config::run_global_update(),
            Some(ConfigAction::Show { update: false }) | None => commands::config::run_show(),
            Some(ConfigAction::Project { path, update }) => {
                commands::config::run_path_config(&config::expand_tilde(&path), update)
            }
            Some(ConfigAction::Daemon { target, sub_action }) => {
                // A toggle word (`on`, `off`) never starts with `~`, so expanding the
                // target before the match cannot turn one into a path.
                let target = target.map(|t| config::expand_tilde(&t));
                let (path, action) = match (target.as_deref(), sub_action.as_deref()) {
                    (Some(t), Some(a)) => (Some(t), a),
                    (Some(t), None) if commands::config::is_toggle_word(t) => (None, t),
                    (Some(t), None) => (Some(t), "status"),
                    (None, Some(a)) => (None, a),
                    (None, None) => (None, "status"),
                };
                commands::config::run_daemon_toggle(path, action)
            }
            Some(ConfigAction::Hook {
                target,
                sub_action,
                chain,
            }) => {
                let target = target.map(|t| config::expand_tilde(&t));
                let (path, action) = match (target.as_deref(), sub_action.as_deref()) {
                    (Some(t), Some(a)) => (Some(t), a),
                    (Some(t), None) if commands::config::is_toggle_word(t) => (None, t),
                    (Some(t), None) => (Some(t), "status"),
                    (None, Some(a)) => (None, a),
                    // `--chain` on its own is an install instruction, not a status query.
                    (None, None) if chain => (None, "install"),
                    (None, None) => (None, "status"),
                };
                commands::config::run_hook_toggle(path, action, chain)
            }
            Some(ConfigAction::Icon) => commands::icon::run_install(),
            Some(ConfigAction::Wizard) => commands::config::run_wizard(),
        },
        Commands::Restore { path, last_run } => {
            if last_run {
                commands::restore::run_last_run()
            } else {
                commands::restore::run(&config::expand_tilde(path.as_deref().unwrap_or(".")))
            }
        }
        Commands::Update { offline } => commands::update::run(offline),
        Commands::Skill => commands::skill::run(),
        Commands::Setup { status } => commands::setup::run(status),
        Commands::Doctor { path, fix } => {
            let path = path.map(|p| config::expand_tilde(&p));
            commands::doctor::run(path.as_deref(), fix)
        }
        Commands::Uninstall { deep } => commands::uninstall::run(deep, cli.yes),
    };

    if let Err(e) = result {
        if is_broken_pipe(&e) {
            std::process::exit(exit_code::OK);
        }
        output::print_error(&format!("{e:#}"));
        std::process::exit(exit_code::FAILURE);
    }

    // Only on the way out of a successful run: nobody reading an error message needs a
    // credit under it.
    if credit_the_author {
        output::print_attribution();
    }
}
