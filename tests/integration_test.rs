// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// The binary under test, with unattended integration installs switched off.
///
/// Without this, running the test suite installs a scheduled task and repoints the
/// developer's global `core.hooksPath` — the suite must never touch the machine it
/// runs on.
fn devp() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dev-prune"));
    cmd.env("DEV_PRUNE_NO_AUTO_SETUP", "1");
    // A fresh config dir makes the release check "due"; tests stay off the network.
    cmd.env("DEV_PRUNE_OFFLINE", "1");
    // The floor, not the choice: a case that cares about registry state sets its own
    // `DEV_PRUNE_CONFIG_DIR` after this and wins. What this stops is the case that does
    // not care and therefore writes to the developer's real registry — which is how six
    // temporary fixtures ended up permanently registered on the author's machine, listed
    // as `Path missing` forever, with nothing anywhere reporting a fault.
    cmd.env("DEV_PRUNE_CONFIG_DIR", scratch_config_dir());

    // And the same reasoning for the working directory, which stopped being neutral when
    // `status` and `run` began registering the repository they stand in — the only way a
    // repository made by `git init` is ever seen, since Git has no `post-init` hook.
    // Inherited from cargo that is this crate's own root, so a case that registers
    // nothing and then prunes would have found it adopted, with `site/node_modules`
    // inside it. A case that cares sets its own after this, and wins.
    cmd.current_dir(std::env::temp_dir());
    cmd
}

/// A throwaway config directory under `target/`, shared by every case in this file that
/// does not name one of its own.
///
/// `CARGO_TARGET_TMPDIR` rather than `std::env::temp_dir()`: it is inside the build
/// directory, so `cargo clean` removes it and a test run leaves nothing behind anywhere
/// else on the machine.
fn scratch_config_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration-scratch-config");
    std::fs::create_dir_all(&dir).expect("create scratch config dir");
    dir
}

#[test]
fn test_cli_help() {
    let output = devp()
        .arg("--help")
        .output()
        .expect("Failed to execute dev-prune --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Universal, lockfile-safe workspace pruner"));
}

#[test]
fn test_cli_version_audit() {
    let output = devp()
        .arg("-V")
        .output()
        .expect("Failed to execute dev-prune -V");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dev-prune (devp) v"));
    assert!(stdout.contains("Target OS:"));
    assert!(stdout.contains("PATH Audit:"));
}

#[test]
fn test_cli_init_and_status() {
    // Not the OS temp directory, for once. A scan skips any repository under a directory
    // named `tmp` — correctly, since that is where scratch clones live — and on Linux the
    // OS temp directory *is* `/tmp`, so a fixture there was skipped and `init` had nothing
    // to register. macOS (`/var/folders/...`) and Windows (`...\AppData\Local\Temp`) do
    // not match the rule, which is why this only ever failed on one of the three.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/init-fixtures");
    fs::create_dir_all(&base).unwrap();
    let tmp = TempDir::new_in(&base).unwrap();
    let repo_dir = tmp.path().join("my-test-repo");
    fs::create_dir_all(&repo_dir).unwrap();

    // Initialize mock git repo
    Command::new("git")
        .args(["init"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    // Run init command with --dry-run targeting the temp repo dir
    let output = devp()
        .env("DEV_PRUNE_CONFIG_DIR", tmp.path().join("config"))
        .args(["--dry-run", "init", repo_dir.to_str().unwrap()])
        .output()
        .expect("Failed to run init");

    if !output.status.success() {
        panic!(
            "CLI failed with exit code {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `--dry-run` reports what it *would* do. The old wording said "Registered:" and
    // then wrote nothing, which is the one thing a dry run must not claim.
    assert!(stdout.contains("Would register:"), "{stdout}");
    assert!(stdout.contains("Nothing was written"), "{stdout}");
    assert!(
        !tmp.path().join("config").join("registry.json").exists(),
        "a dry run must not create the registry"
    );

    // Without the flag it registers for real, and says so.
    let real = devp()
        .env("DEV_PRUNE_CONFIG_DIR", tmp.path().join("config"))
        .env("DEV_PRUNE_NO_AUTO_SETUP", "1")
        .args(["init", repo_dir.to_str().unwrap()])
        .output()
        .expect("Failed to run init");
    assert!(real.status.success());
    let stdout = String::from_utf8_lossy(&real.stdout);
    assert!(stdout.contains("Registered:"), "{stdout}");
    assert!(tmp.path().join("config").join("registry.json").exists());
}

#[test]
fn test_cli_link_unlink_undo() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path().join("link-repo");
    fs::create_dir_all(&repo_dir).unwrap();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    let config_dir = tmp.path().join("config");

    // 1. Link repository
    let link_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["link", repo_dir.to_str().unwrap()])
        .output()
        .expect("Failed link");
    assert!(link_out.status.success());
    let stdout = String::from_utf8_lossy(&link_out.stdout);
    assert!(stdout.contains("Linked:"));

    // 2. Undo action directly
    let undo_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["undo"])
        .output()
        .expect("Failed undo");
    assert!(undo_out.status.success());
    let stdout_undo = String::from_utf8_lossy(&undo_out.stdout);
    assert!(
        stdout_undo.contains("Unregistered 1 repository."),
        "{stdout_undo}"
    );

    // 3. Link again & then Unlink
    devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["link", repo_dir.to_str().unwrap()])
        .output()
        .unwrap();

    let unlink_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["unlink", repo_dir.to_str().unwrap()])
        .output()
        .expect("Failed unlink");
    assert!(unlink_out.status.success());
    let stdout_unlink = String::from_utf8_lossy(&unlink_out.stdout);
    assert!(stdout_unlink.contains("Unlinked:"));
}

#[test]
fn test_cli_config_get_set_show() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    // 1. Get idle_days
    let get_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["config", "get", "idle_days"])
        .output()
        .unwrap();
    assert!(get_out.status.success());
    assert!(String::from_utf8_lossy(&get_out.stdout).contains("idle_days"));

    // 2. Set idle_days to 30
    let set_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["config", "set", "idle_days", "30"])
        .output()
        .unwrap();
    assert!(set_out.status.success());
    assert!(String::from_utf8_lossy(&set_out.stdout).contains("idle_days = 30"));

    // 3. Show global config
    let show_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(show_out.status.success());
    assert!(String::from_utf8_lossy(&show_out.stdout).contains("Global Configuration"));
}

#[test]
fn test_cli_run_dry_run_across_ecosystems() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    // Create npm repo
    let npm_repo = tmp.path().join("npm-repo");
    fs::create_dir_all(&npm_repo).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&npm_repo)
        .output()
        .unwrap();
    fs::write(npm_repo.join("package.json"), "{}").unwrap();
    fs::write(npm_repo.join("package-lock.json"), "{}").unwrap();
    let node_modules = npm_repo.join("node_modules");
    fs::create_dir(&node_modules).unwrap();
    fs::write(node_modules.join("dummy.js"), "content").unwrap();

    // Link npm repo
    devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["link", npm_repo.to_str().unwrap()])
        .output()
        .unwrap();

    // Run --dry-run --force across registered repos
    let run_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["--dry-run", "--force", "-y", "run"])
        .output()
        .unwrap();

    assert!(run_out.status.success());
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    assert!(stdout.contains("DRY RUN"));
}

#[test]
fn test_cli_restore_no_lockfiles() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = tmp.path().join("empty-repo");
    fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let restore_out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["restore", repo.to_str().unwrap()])
        .output()
        .unwrap();

    // When no lockfile exists, restore prints header and returns error output
    let stdout = String::from_utf8_lossy(&restore_out.stdout);
    let stderr = String::from_utf8_lossy(&restore_out.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("dev-prune restore")
            || combined.contains("No matching package manager lockfiles")
    );
}

/// `daemon`, `hook` and `icon` live under `config`, but the tool's own output and its
/// docs call them by their bare names. Both spellings must reach the same handler.
#[test]
fn test_bare_hook_subcommand_routes_to_config_hook() {
    for args in [vec!["hook"], vec!["config", "hook"], vec!["status", "hook"]] {
        let output = devp()
            .args(&args)
            .output()
            .unwrap_or_else(|e| panic!("failed to run {args:?}: {e}"));
        assert!(output.status.success(), "{args:?} exited non-zero");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("dev-prune Git Hooks Status"),
            "{args:?} did not reach the hook status handler:\n{stdout}"
        );
    }
}

#[test]
fn test_bare_daemon_subcommand_routes_to_config_daemon() {
    for args in [vec!["daemon"], vec!["config", "daemon"]] {
        let output = devp()
            .args(&args)
            .output()
            .unwrap_or_else(|e| panic!("failed to run {args:?}: {e}"));
        assert!(output.status.success(), "{args:?} exited non-zero");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("dev-prune daemon status"),
            "{args:?} did not reach the daemon status handler:\n{stdout}"
        );
    }
}

/// `setup --status` reports every integration and changes nothing.
#[test]
fn test_setup_status_reports_without_installing() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let output = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["setup", "--status"])
        .output()
        .expect("Failed to run setup --status");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for row in [
        "devp alias:",
        "SKILL.md:",
        "Git hooks:",
        "Background scheduler:",
        "auto_setup",
    ] {
        assert!(stdout.contains(row), "missing `{row}` in:\n{stdout}");
    }
    // Reporting is not installing: nothing may have been exported.
    assert!(
        !config_dir.join("SKILL.md").exists(),
        "`setup --status` must not write anything"
    );
}

/// The opt-out is what keeps dev-prune off machines that did not ask for it — CI
/// images, containers, and this suite. It must hold on the command that installs most.
#[test]
fn test_auto_setup_opt_out_installs_nothing() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // `devp()` sets DEV_PRUNE_NO_AUTO_SETUP.
    let output = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["init", repo.to_str().unwrap()])
        .output()
        .expect("Failed to run init");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Integrations"),
        "init installed integrations despite the opt-out:\n{stdout}"
    );
    assert!(
        !config_dir.join("SKILL.md").exists(),
        "the opt-out must cover the SKILL.md export too"
    );
    // The registry itself is dev-prune's own business and is still written.
    assert!(config_dir.join("registry.json").exists());
}

/// A repository dev-prune refused to examine must never be reported as "nothing to do".
///
/// The analysis pass produces a `config_error` for an unreadable `.devprune.json`, and
/// that result used to be dropped along with every other non-candidate state — so the run
/// ended on "No idle repositories or pruneable bloat directories found." and exit 0 while
/// having quietly skipped the repository the user asked it to handle.
#[test]
fn test_a_repo_with_a_broken_config_is_reported_and_fails_the_run() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = tmp.path().join("broken-repo");
    fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let broken = r#"{ "project_name": "api", "override_idle_days": 90, }"#;
    fs::write(repo.join(".devprune.json"), broken).unwrap();

    devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();

    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["--force", "-y", "run"])
        .output()
        .expect("Failed to run");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "a skipped repo must fail the run");
    assert!(
        !stdout.contains("No idle repositories or pruneable bloat directories found."),
        "the run claimed there was nothing to do:\n{stdout}"
    );
    assert!(stdout.contains("Could Not Be Examined"), "{stdout}");
    assert!(stdout.contains("--update"), "{stdout}");

    // Nothing may have been written over the file the user still has to fix.
    assert_eq!(
        fs::read_to_string(repo.join(".devprune.json")).unwrap(),
        broken
    );
}

/// The same repository in the JSON document: one parseable object, `config_error`
/// carrying the parse failure, a non-zero `summary.errors`, and a non-zero exit.
#[test]
fn test_a_broken_config_is_a_config_error_in_the_json_document() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = tmp.path().join("broken-repo");
    fs::create_dir_all(&repo).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    fs::write(repo.join(".devprune.json"), "{ not json").unwrap();

    devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();

    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["--force", "-y", "run", "--json"])
        .output()
        .expect("Failed to run");

    assert!(!out.status.success());
    // stdout is the document and nothing else, so a parser never has to strip a banner.
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout was not one JSON document");
    assert_eq!(doc["results"][0]["status"], "config_error");
    assert!(
        doc["results"][0]["message"]
            .as_str()
            .unwrap()
            .contains("Syntax error")
    );
    assert_eq!(doc["summary"]["errors"], 1);
}

/// A mistyped action must fail loudly. Falling through to a status report would print a
/// success-looking message while leaving the daemon uninstalled.
#[test]
fn test_mistyped_toggle_action_is_rejected() {
    let output = devp()
        .args(["config", "daemon", "enabel"])
        .output()
        .expect("Failed to execute dev-prune");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("enabel"), "{stderr}");
}

/// A refused declaration has to reach the person running the pass.
///
/// It was computed correctly and emitted correctly in `--json`, and then dropped by
/// every human-facing reporter, which is the one place a refusal exists to be seen. The
/// whole suite stayed green while the only user-visible half of the feature did nothing.
#[test]
fn test_a_refused_declaration_is_reported_without_failing_the_run() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = tmp.path().join("declaring-repo");
    fs::create_dir_all(repo.join("tools").join("vendor")).unwrap();
    fs::write(
        repo.join("tools").join("vendor").join("blob.bin"),
        vec![0u8; 4096],
    )
    .unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    // The rebuild tool is absent on every machine, so the refusal is the same everywhere.
    fs::write(
        repo.join(".devprune.json"),
        r#"{"prunable":{"directories":[{"path":"tools/vendor","rebuild":"definitely-not-a-real-tool-xyz build"}]}}"#,
    )
    .unwrap();

    devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();

    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", &config_dir)
        .args(["--force", "-y", "run"])
        .output()
        .expect("Failed to run");

    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        printed.contains("definitely-not-a-real-tool-xyz"),
        "the refusal never reached the user:\n{printed}"
    );
    assert!(
        !printed.contains("No idle repositories or pruneable bloat directories found."),
        "the run claimed there was nothing to say:\n{printed}"
    );
    assert!(
        out.status.success(),
        "a refused declaration is a warning, not a failure:\n{printed}"
    );
}
