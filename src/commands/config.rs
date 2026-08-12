// Handler for the `dev-prune config` command.
//
// Supports `get`, `set`, `show`, `update`, `daemon`, and `hook` sub-actions
// for managing global and per-repo workspace settings.

use anyhow::{Result, bail};
use std::path::Path;

use crate::config::{PerRepoConfig, Registry, Settings};
use crate::output;

/// One tunable in the global config: how to read it, how to write it, and what to say
/// about it.
///
/// A table rather than a `match` arm per operation. `get`, `set`, `show` and the
/// first-run walkthrough all iterate this, so a setting cannot be added to one of them
/// and quietly forgotten in the other three — which is how `min_size_mb` shipped with no
/// line in `config show`.
struct Setting {
    key: &'static str,
    /// One line, shown by the walkthrough and by `config show --help-text`.
    help: &'static str,
    get: fn(&Settings) -> String,
    set: fn(&mut Settings, &str) -> Result<()>,
}

/// Every global setting, in the order a person would want to be asked about them.
const SETTINGS: &[Setting] = &[
    Setting {
        key: "idle_days",
        help: "Days a repository must sit untouched before it is eligible for pruning.",
        get: |s| s.idle_days.to_string(),
        set: |s, v| {
            s.idle_days = v
                .parse()
                .map_err(|_| anyhow::anyhow!("idle_days must be a whole number of days"))?;
            Ok(())
        },
    },
    Setting {
        key: "min_size_mb",
        help: "Smallest bloat directory worth deleting, in MiB. 0 removes the floor.",
        get: |s| s.min_size_mb.to_string(),
        set: |s, v| {
            s.min_size_mb = v.parse().map_err(|_| {
                anyhow::anyhow!("min_size_mb must be a whole number of MiB (0 disables the floor)")
            })?;
            Ok(())
        },
    },
    Setting {
        key: "scan_depth",
        help: "How many directory levels below a repo root project discovery descends.",
        get: |s| s.scan_depth.to_string(),
        set: |s, v| {
            let depth: usize = v
                .parse()
                .map_err(|_| anyhow::anyhow!("scan_depth must be a positive integer"))?;
            // Rejected rather than clamped. `clamp_depth` exists so a hand-edited config
            // file cannot break the walk, but when someone types the number at us we owe
            // them the truth instead of silently storing something else.
            if depth == 0 {
                bail!("scan_depth must be at least 1 — 0 would find no projects at all.");
            }
            if depth > crate::constants::MAX_SCAN_DEPTH_LIMIT {
                bail!(
                    "scan_depth must be at most {} — deeper walks stall on generated trees.",
                    crate::constants::MAX_SCAN_DEPTH_LIMIT
                );
            }
            s.scan_depth = depth;
            Ok(())
        },
    },
    Setting {
        key: "require_confirmation",
        help: "Ask before deleting anything. Turning this off makes every run unattended.",
        get: |s| s.require_confirmation.to_string(),
        set: |s, v| {
            s.require_confirmation = parse_bool("require_confirmation", v)?;
            Ok(())
        },
    },
    Setting {
        key: "allow_manifest_rewrite",
        help: "Let cargo and go run the sync command that rewrites tracked manifests.",
        get: |s| s.allow_manifest_rewrite.to_string(),
        set: |s, v| {
            s.allow_manifest_rewrite = parse_bool("allow_manifest_rewrite", v)?;
            Ok(())
        },
    },
    Setting {
        key: "command_timeout_secs",
        help: "How long a lockfile command may run before it is killed.",
        get: |s| s.command_timeout_secs.to_string(),
        set: |s, v| {
            let secs: u64 = v
                .parse()
                .map_err(|_| anyhow::anyhow!("command_timeout_secs must be a positive integer"))?;
            // Zero is not "no limit": the runner compares elapsed time against it before
            // the child has had a chance to finish, so every lockfile sync would be
            // killed on the spot and nothing would ever be pruneable.
            if secs == 0 {
                bail!(
                    "command_timeout_secs must be at least 1 — 0 would kill every command \
                     the instant it starts."
                );
            }
            s.command_timeout_secs = secs;
            Ok(())
        },
    },
    Setting {
        key: "auto_setup",
        help: "Install missing integrations by itself, once per installed version.",
        get: |s| s.auto_setup.to_string(),
        set: |s, v| {
            s.auto_setup = parse_bool("auto_setup", v)?;
            Ok(())
        },
    },
    Setting {
        key: "auto_daemon",
        help: "Register the OS scheduler so passes run without being remembered.",
        get: |s| s.auto_daemon.to_string(),
        set: |s, v| {
            s.auto_daemon = parse_bool("auto_daemon", v)?;
            Ok(())
        },
    },
    Setting {
        key: "check_interval_days",
        help: "Days between scheduled background passes.",
        get: |s| s.check_interval_days.to_string(),
        set: |s, v| {
            let days: u64 = v
                .parse()
                .map_err(|_| anyhow::anyhow!("check_interval_days must be a positive integer"))?;
            // Zero would schedule a prune pass with no gap between passes.
            if days == 0 {
                bail!("check_interval_days must be at least 1.");
            }
            s.check_interval_days = days;
            Ok(())
        },
    },
    Setting {
        key: "auto_hooks",
        help: "Install the Git hooks that register repositories as you clone them.",
        get: |s| s.auto_hooks.to_string(),
        set: |s, v| {
            s.auto_hooks = parse_bool("auto_hooks", v)?;
            Ok(())
        },
    },
    Setting {
        key: "auto_hooks_chain",
        help: "If another tool owns core.hooksPath, install in front of it and forward.",
        get: |s| s.auto_hooks_chain.to_string(),
        set: |s, v| {
            s.auto_hooks_chain = parse_bool("auto_hooks_chain", v)?;
            Ok(())
        },
    },
    Setting {
        key: "update_check",
        help: "Ask GitHub for the latest release from time to time. Sends nothing but the request.",
        get: |s| s.update_check.to_string(),
        set: |s, v| {
            s.update_check = parse_bool("update_check", v)?;
            Ok(())
        },
    },
    Setting {
        key: "update_check_interval_days",
        help: "Days between automatic release checks.",
        get: |s| s.update_check_interval_days.to_string(),
        set: |s, v| {
            let days: i64 = v.parse().map_err(|_| {
                anyhow::anyhow!("update_check_interval_days must be a positive integer")
            })?;
            if days < 1 {
                bail!("update_check_interval_days must be at least 1.");
            }
            s.update_check_interval_days = days;
            Ok(())
        },
    },
    Setting {
        key: "update_check_timeout_secs",
        help: "Seconds the release check waits for GitHub. Raise it behind a slow proxy.",
        get: |s| s.update_check_timeout_secs.to_string(),
        set: |s, v| {
            let secs: u64 = v.parse().map_err(|_| {
                anyhow::anyhow!("update_check_timeout_secs must be a positive integer")
            })?;
            if secs == 0 {
                bail!("update_check_timeout_secs must be at least 1.");
            }
            s.update_check_timeout_secs = secs;
            Ok(())
        },
    },
];

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "no" | "n" | "off" | "0" => Ok(false),
        _ => bail!("{key} must be true or false"),
    }
}

/// Every stored setting that its own setter would refuse, with the reason.
///
/// `devp config set` guards the ranges, but nothing guards a hand-edited `registry.json`
/// — and the values that get in that way are the quiet ones: `scan_depth: 0` finds no
/// projects, `command_timeout_secs: 0` kills every lockfile command the instant it
/// starts. Both leave a tool that runs, reports success and prunes nothing.
///
/// Round-tripping each value through the setter that owns it is deliberate. A separate
/// list of ranges would be a second copy of the rules, free to drift from the ones
/// actually enforced.
pub fn invalid_settings(settings: &Settings) -> Vec<(&'static str, String)> {
    SETTINGS
        .iter()
        .filter_map(|setting| {
            let mut probe = settings.clone();
            (setting.set)(&mut probe, &(setting.get)(settings))
                .err()
                .map(|e| (setting.key, e.to_string()))
        })
        .collect()
}

/// The number of settings [`invalid_settings`] checks, for reports that say so.
pub fn setting_count() -> usize {
    SETTINGS.len()
}

fn find_setting(key: &str) -> Result<&'static Setting> {
    SETTINGS
        .iter()
        .find(|s| s.key == key)
        .ok_or_else(|| anyhow::anyhow!("Unknown config key: {key}. Valid keys: {}", valid_keys()))
}

fn valid_keys() -> String {
    SETTINGS
        .iter()
        .map(|s| s.key)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a `daemon` / `hook` sub-action word means.
#[derive(Debug, PartialEq, Eq)]
pub enum Toggle {
    Enable,
    Disable,
    Status,
}

/// Resolve the sub-action word users actually type.
///
/// `install` / `uninstall` are what this tool's own output and its documentation have
/// always called these operations, and `on` / `off` is the obvious guess; each pair
/// means the same thing as `enable` / `disable`, so all of them are accepted.
///
/// Anything else is an error rather than a fall-through to `status`. Silently printing
/// status for `devp config daemon enabel` looks like it worked and leaves the daemon
/// uninstalled.
pub fn parse_toggle(action: &str) -> Result<Toggle> {
    match action.to_lowercase().as_str() {
        "enable" | "install" | "on" => Ok(Toggle::Enable),
        "disable" | "uninstall" | "remove" | "off" => Ok(Toggle::Disable),
        "" | "status" | "show" => Ok(Toggle::Status),
        other => bail!(
            "Unknown action `{other}`. Expected `enable`, `disable` or `status` \
             (`install` / `uninstall` / `on` / `off` also work)."
        ),
    }
}

/// Whether a bare argument is a sub-action rather than a workspace path.
///
/// `devp config hook <word>` is ambiguous by design — `<word>` is either the action or
/// the repository to apply it to — so both the argument router and [`parse_toggle`]
/// have to agree on which words are actions.
pub fn is_toggle_word(word: &str) -> bool {
    parse_toggle(word).is_ok() && !word.is_empty()
}

/// Resolve the workspace argument of `daemon` / `hook`, which is whatever was not
/// recognised as an action.
///
/// A word that is neither an action nor a directory is a mistyped action. Treating it
/// as a path would print `Daemon Status (enabel): Enabled for workspace` — a success
/// message about a repository that does not exist.
fn resolve_workspace(path: &str) -> Result<std::path::PathBuf> {
    let raw = Path::new(path);
    if !raw.is_dir() {
        bail!(
            "`{path}` is neither an action nor an existing directory.\n\
             Expected `enable`, `disable` or `status`, or a path to a repository."
        );
    }
    Ok(raw.canonicalize().unwrap_or_else(|_| raw.to_path_buf()))
}

/// Display a single config value.
pub fn run_get(key: &str) -> Result<()> {
    let registry = Registry::load()?;
    let setting = find_setting(key)?;
    println!("{key} = {}", (setting.get)(&registry.settings));
    Ok(())
}

/// Set a config value.
pub fn run_set(key: &str, value: &str) -> Result<()> {
    let mut registry = Registry::load()?;
    let setting = find_setting(key)?;
    (setting.set)(&mut registry.settings, value)?;
    registry.save()?;

    // The stored value, not the typed one: `devp config set auto_daemon yes` stores
    // `true`, and echoing "auto_daemon = yes" would describe a file that does not exist.
    output::print_success(&format!("{key} = {}", (setting.get)(&registry.settings)));
    Ok(())
}

/// Widest key name, so every value in `config show` lines up.
fn key_column_width() -> usize {
    SETTINGS.iter().map(|s| s.key.len()).max().unwrap_or(0)
}

/// Show all config values.
pub fn run_show() -> Result<()> {
    let registry = Registry::load()?;
    let width = key_column_width();

    output::print_header("dev-prune Global Configuration");
    for setting in SETTINGS {
        println!(
            "  {:<width$} = {}",
            setting.key,
            (setting.get)(&registry.settings)
        );
    }
    println!("  {:<width$} = {}", "tracked_repos", registry.repo_count());

    let reg_path = Registry::registry_path()
        .map(|p| output::clean_path(&p))
        .unwrap_or_else(|_| "unknown".to_string());
    println!("\n  {:<width$} = {reg_path}", "registry_file");
    println!();
    output::print_info("Change any of these with `devp config set <key> <value>`.");
    output::print_info("Walk through them one at a time with `devp config wizard`.");

    Ok(())
}

/// Walk the global settings, offering each current value for confirmation.
///
/// Run once by hand as `devp config wizard`, and once automatically the first time a
/// human types a command on a fresh install — the point at which every default is about
/// to start applying to their machine, and the only point at which they can be told so
/// before rather than after.
///
/// Refuses without a terminal instead of hanging on a read that will never return.
pub fn run_wizard() -> Result<()> {
    use std::io::{self, IsTerminal, Write};

    if !io::stdin().is_terminal() {
        bail!(
            "`devp config wizard` needs a terminal to ask questions on.\n\
             Use `devp config show` to read the settings and `devp config set <key> <value>` \
             to change one."
        );
    }

    let mut registry = Registry::load()?;
    let width = key_column_width();

    output::print_header("dev-prune configuration");
    output::print_info("These are the defaults every run will use. Nothing has been changed yet.");
    println!();
    for setting in SETTINGS {
        println!(
            "  {:<width$} = {}",
            setting.key,
            (setting.get)(&registry.settings)
        );
        println!("  {:<width$}   {}", "", setting.help);
    }
    println!();

    print!("Keep all of these? [Y/n] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let keep = !matches!(answer.trim().to_lowercase().as_str(), "n" | "no");

    if keep {
        mark_reviewed();
        output::print_success("Keeping the defaults. `devp config set <key> <value>` changes any.");
        return Ok(());
    }

    println!();
    output::print_info("Enter a new value, or press Enter to keep the one shown.");
    println!();

    let mut changed = 0usize;
    for setting in SETTINGS {
        let current = (setting.get)(&registry.settings);
        loop {
            print!("  {} [{current}]: ", setting.key);
            io::stdout().flush()?;
            let mut line = String::new();
            // EOF mid-way — a closed pipe or Ctrl-D — keeps what has been answered so far
            // rather than looping forever on an empty read.
            if io::stdin().read_line(&mut line)? == 0 {
                println!();
                break;
            }
            let typed = line.trim();
            if typed.is_empty() {
                break;
            }
            match (setting.set)(&mut registry.settings, typed) {
                Ok(()) => {
                    changed += 1;
                    break;
                }
                // Re-asked rather than aborted: losing the eight answers already given
                // because the ninth was a typo is not a reasonable trade.
                Err(e) => output::print_error(&format!("{e}")),
            }
        }
    }

    registry.save()?;
    mark_reviewed();
    println!();
    if changed == 0 {
        output::print_success("Nothing changed — the defaults are in place.");
    } else {
        output::print_success(&format!(
            "Saved {changed} {}. `devp config show` lists them all.",
            output::plural(changed, "change", "changes")
        ));
    }
    Ok(())
}

/// Marker recording that the settings have been put in front of the user once.
const REVIEW_MARKER: &str = "config-reviewed";

/// Whether the first-run walkthrough is still owed.
///
/// Keyed on the marker file rather than on the version stamp: an upgrade should not
/// re-ask about settings that were confirmed once. A `devp uninstall --purge` removes the
/// config directory and with it this marker, which is what makes a genuine reinstall ask
/// again.
pub fn config_review_is_due() -> bool {
    Registry::config_dir()
        .map(|dir| !dir.join(REVIEW_MARKER).exists())
        .unwrap_or(false)
}

fn mark_reviewed() {
    if let Ok(dir) = Registry::config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(REVIEW_MARKER), crate::constants::VERSION);
    }
}

/// Suppress the first-run walkthrough without running it.
///
/// For the paths that must not stop to ask: the Git hook, the scheduler, and anything
/// with no terminal attached.
pub fn skip_config_review() {
    mark_reviewed();
}

/// Global audit pass for all registered repos.
pub fn run_global_update() -> Result<()> {
    output::print_header("dev-prune Global Configuration Audit & Sync");

    let registry = Registry::load()?;
    let mut total_audited = 0;
    let mut errors_found = 0;

    for repo_path in registry.repositories.keys() {
        total_audited += 1;
        let clean = output::clean_path(repo_path);

        match PerRepoConfig::load_with_diagnostics(repo_path) {
            Ok(Some(cfg)) => {
                if let Err(e) = cfg.save_to_repo(repo_path) {
                    output::print_error(&format!("Failed to write config for {clean}: {e}"));
                    errors_found += 1;
                } else {
                    output::print_success(&format!("Audited & synced config for {clean}"));
                }
            }
            Ok(None) => {
                let cfg = PerRepoConfig::default();
                if let Err(e) = cfg.save_to_repo(repo_path) {
                    output::print_error(&format!("Failed to write config for {clean}: {e}"));
                    errors_found += 1;
                } else {
                    output::print_success(&format!(
                        "Audited & initialized default config for {clean}"
                    ));
                }
            }
            Err(err_msg) => {
                errors_found += 1;
                output::print_error(&format!("Syntax/Schema Error in {clean}:"));
                for line in err_msg.lines() {
                    eprintln!("    {line}");
                }
                output::print_info(&format!(
                    "Hint: fix the syntax by hand, or run `devp config {clean} --update` to \
                     replace the file with a valid default."
                ));
            }
        }
    }

    if errors_found > 0 {
        // Non-zero, so a CI step or a shell `&&` chain notices. An audit that found
        // broken config files has not succeeded, however calmly it says so.
        anyhow::bail!(
            "Audit complete: {total_audited} repos checked, {errors_found} could not be read \
             or written."
        );
    }
    output::print_success(&format!(
        "Audit complete: All {total_audited} registered repositories are healthy & synced!"
    ));

    Ok(())
}

/// Inspect or create per-repository configuration (.devprune.json).
pub fn run_path_config(path_str: &str, force_update: bool) -> Result<()> {
    let raw_path = Path::new(path_str);

    let path = if raw_path.exists() {
        raw_path
            .canonicalize()
            .unwrap_or_else(|_| raw_path.to_path_buf())
    } else {
        raw_path.to_path_buf()
    };

    let clean = output::clean_path(&path);

    if !path.exists() {
        bail!("Path does not exist: {clean}");
    }

    if !crate::scanner::is_git_repo(&path) {
        // The old text said "Initializing Git repo first..." and then did no such thing.
        bail!(
            "`{clean}` is not a Git repository.\n  \
             Run `git init` there first, then `devp config {clean}` again."
        );
    }

    let mut registry = Registry::load()?;
    if !registry.repositories.contains_key(&path) {
        output::print_info(&format!(
            "{clean} is not yet registered with dev-prune. Registering now..."
        ));
        registry.add_repo(path.clone());
        registry.save()?;
    }

    let cfg_file = path.join(crate::constants::PER_REPO_CONFIG_FILE);

    if cfg_file.exists() && !force_update {
        output::print_header(&format!("dev-prune Per-Repo Config for {clean}"));
        match PerRepoConfig::load_with_diagnostics(&path) {
            Ok(cfg) => {
                let json_str = serde_json::to_string_pretty(&cfg)?;
                println!("{json_str}");
                output::print_info("File location: .devprune.json");
            }
            Err(err_msg) => {
                output::print_error(&format!("Invalid configuration in {clean}:"));
                for line in err_msg.lines() {
                    eprintln!("    {line}");
                }
                // Non-zero: the file this command was asked to show could not be read,
                // and the same file is what every prune of this repo will trip over.
                anyhow::bail!(
                    "Run `devp config {clean} --update` to reset this file back to defaults \
                     (your current overrides in it are discarded)."
                );
            }
        }
    } else {
        output::print_info(&format!("Initializing .devprune.json for {clean}..."));
        let cfg = PerRepoConfig::default();
        cfg.save_to_repo(&path)?;
        output::print_success(&format!("Created .devprune.json in {clean}"));
    }

    Ok(())
}

/// Load a workspace's `.devprune.json` for a toggle that is about to write it back.
///
/// Refuses a file that does not parse, rather than starting from the defaults. Starting
/// from the defaults meant `devp config <repo> daemon off` wrote a fresh file straight
/// over the broken one, so a single typo cost the user every other override in it.
fn load_workspace_config_for_write(repo_path: &Path) -> Result<PerRepoConfig> {
    match PerRepoConfig::load_with_diagnostics(repo_path) {
        Ok(Some(cfg)) => Ok(cfg),
        Ok(None) => Ok(PerRepoConfig::default()),
        Err(e) => bail!(
            "{e}\n  \
             Fix that file, or run `devp config {} --update` to reset it back to defaults \
             (your current overrides in it are discarded).",
            output::clean_path(repo_path)
        ),
    }
}

/// Toggle or status check for background daemon (global or local workspace).
pub fn run_daemon_toggle(path: Option<&str>, action: &str) -> Result<()> {
    if let Some(p) = path {
        let repo_path = resolve_workspace(p)?;
        let mut cfg = load_workspace_config_for_write(&repo_path)?;
        match parse_toggle(action)? {
            Toggle::Enable => {
                cfg.disable_daemon = false;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Enabled background daemon for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Disable => {
                cfg.disable_daemon = true;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Disabled background daemon for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Status => {
                let st = if cfg.disable_daemon {
                    "Disabled for workspace"
                } else {
                    "Enabled for workspace"
                };
                output::print_info(&format!(
                    "Daemon Status ({}): {}",
                    output::clean_path(&repo_path),
                    st
                ));
            }
        }
    } else {
        match parse_toggle(action)? {
            Toggle::Enable => crate::commands::daemon::run_install()?,
            Toggle::Disable => crate::commands::daemon::run_uninstall()?,
            Toggle::Status => crate::commands::daemon::run_status()?,
        }
    }
    Ok(())
}

/// Toggle or status check for background Git hooks (global or local workspace).
pub fn run_hook_toggle(path: Option<&str>, action: &str, chain: bool) -> Result<()> {
    if let Some(p) = path {
        if chain {
            bail!(
                "`--chain` changes the single global `core.hooksPath`, so it has no \
                 per-workspace form. Drop the path: `devp hook install --chain`."
            );
        }
        let repo_path = resolve_workspace(p)?;
        let mut cfg = load_workspace_config_for_write(&repo_path)?;
        match parse_toggle(action)? {
            Toggle::Enable => {
                cfg.disable_hooks = false;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Enabled background Git hooks for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Disable => {
                cfg.disable_hooks = true;
                cfg.save_to_repo(&repo_path)?;
                output::print_success(&format!(
                    "Disabled background Git hooks for workspace: {}",
                    output::clean_path(&repo_path)
                ));
            }
            Toggle::Status => {
                let st = if cfg.disable_hooks {
                    "Disabled for workspace"
                } else {
                    "Enabled for workspace"
                };
                output::print_info(&format!(
                    "Git Hook Status ({}): {}",
                    output::clean_path(&repo_path),
                    st
                ));
            }
        }
    } else {
        match parse_toggle(action)? {
            Toggle::Enable => crate::commands::hook::run_install(chain)?,
            Toggle::Disable => crate::commands::hook::run_uninstall()?,
            Toggle::Status => crate::commands::hook::run_status()?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_synonyms_all_resolve_to_enable() {
        for word in ["enable", "install", "on", "INSTALL", "On"] {
            assert_eq!(parse_toggle(word).unwrap(), Toggle::Enable, "{word}");
        }
    }

    #[test]
    fn disable_synonyms_all_resolve_to_disable() {
        for word in ["disable", "uninstall", "remove", "off", "Uninstall"] {
            assert_eq!(parse_toggle(word).unwrap(), Toggle::Disable, "{word}");
        }
    }

    #[test]
    fn status_is_the_default_and_is_also_spellable() {
        for word in ["", "status", "show"] {
            assert_eq!(parse_toggle(word).unwrap(), Toggle::Status, "{word}");
        }
    }

    #[test]
    fn a_typo_is_an_error_rather_than_a_silent_status_report() {
        // `devp config daemon enabel` must not print status and exit 0 — that reads as
        // success while the daemon stays uninstalled.
        let err = parse_toggle("enabel").unwrap_err().to_string();
        assert!(err.contains("enabel"), "{err}");
        assert!(err.contains("enable"), "{err}");
    }

    #[test]
    fn a_workspace_toggle_refuses_to_write_over_a_broken_config() {
        // The toggle rewrites the whole file. Starting from the defaults on a file it
        // could not read would silently discard every override the user had put in it.
        let tmp = tempfile::TempDir::new().unwrap();
        let broken = r#"{ "project_name": "api", "override_idle_days": 90, }"#;
        std::fs::write(
            tmp.path().join(crate::constants::PER_REPO_CONFIG_FILE),
            broken,
        )
        .unwrap();

        let err = load_workspace_config_for_write(tmp.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("Syntax error"), "{err}");
        assert!(err.contains("--update"), "{err}");

        // Untouched, so the user still has their 90 days to recover.
        let on_disk =
            std::fs::read_to_string(tmp.path().join(crate::constants::PER_REPO_CONFIG_FILE))
                .unwrap();
        assert_eq!(on_disk, broken);
    }

    #[test]
    fn a_workspace_with_no_config_yet_starts_from_the_defaults() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(
            load_workspace_config_for_write(tmp.path()).unwrap(),
            PerRepoConfig::default()
        );
    }

    #[test]
    fn every_setting_round_trips_through_its_own_getter() {
        // The table is what `get`, `set`, `show` and the wizard all read, so a getter
        // that reports a different field than its setter writes would be invisible in
        // every one of them at once.
        let mut settings = Settings::default();
        for setting in SETTINGS {
            let before = (setting.get)(&settings);
            let probe = match before.as_str() {
                "true" => "false".to_string(),
                "false" => "true".to_string(),
                // A number every numeric setting accepts: above every minimum, below
                // `scan_depth`'s ceiling.
                _ => "7".to_string(),
            };
            (setting.set)(&mut settings, &probe)
                .unwrap_or_else(|e| panic!("{} rejected `{probe}`: {e}", setting.key));
            assert_eq!(
                (setting.get)(&settings),
                probe,
                "{} reads back a different field than it writes",
                setting.key
            );
        }
    }

    #[test]
    fn every_setting_is_documented_and_uniquely_named() {
        let mut seen = std::collections::HashSet::new();
        for setting in SETTINGS {
            assert!(seen.insert(setting.key), "duplicate key {}", setting.key);
            assert!(!setting.help.is_empty(), "{} has no help", setting.key);
            // The wizard prints the help under the key; a sentence keeps that readable.
            assert!(
                setting.help.ends_with('.'),
                "{} help should read as a sentence",
                setting.key
            );
        }
    }

    #[test]
    fn the_settings_table_covers_every_field_of_settings() {
        // Serialising `Settings` names every field, so a field added without a table
        // entry — unsettable, unshown, never asked about — fails here rather than in
        // a bug report.
        let json = serde_json::to_value(Settings::default()).unwrap();
        let fields: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        for field in fields {
            assert!(
                SETTINGS.iter().any(|s| s.key == field),
                "`{field}` is a setting with no entry in SETTINGS, so `devp config set \
                 {field}` cannot reach it"
            );
        }
    }

    #[test]
    fn a_rejected_value_leaves_the_previous_one_in_place() {
        let mut settings = Settings::default();
        assert!((find_setting("scan_depth").unwrap().set)(&mut settings, "0").is_err());
        assert_eq!(settings.scan_depth, Settings::default().scan_depth);

        assert!((find_setting("command_timeout_secs").unwrap().set)(&mut settings, "0").is_err());
        assert!((find_setting("check_interval_days").unwrap().set)(&mut settings, "0").is_err());
        assert!(
            (find_setting("update_check_interval_days").unwrap().set)(&mut settings, "0").is_err()
        );
    }

    #[test]
    fn booleans_accept_the_words_people_actually_type() {
        assert!(parse_bool("k", "yes").unwrap());
        assert!(parse_bool("k", "ON").unwrap());
        assert!(!parse_bool("k", "0").unwrap());
        assert!(parse_bool("k", "maybe").is_err());
    }

    #[test]
    fn an_unknown_key_lists_the_ones_that_exist() {
        let err = match find_setting("idel_days") {
            Ok(_) => panic!("`idel_days` is not a setting"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("idle_days"), "{err}");
    }

    #[test]
    fn a_path_is_never_mistaken_for_an_action() {
        // The router uses this to decide whether a lone argument is a path or an action.
        assert!(!is_toggle_word("~/Code/my-repo"));
        assert!(!is_toggle_word("."));
        assert!(!is_toggle_word(""));
        assert!(is_toggle_word("install"));
    }
}
