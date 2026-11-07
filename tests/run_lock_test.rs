// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// One devp per device: the run lock and the registry writer-version guard, end to end.
//
// The test process takes the lock itself, so the binary it spawns is a genuinely separate
// process meeting a lock someone else holds, which is the situation the lock exists for.
// Every test points `DEV_PRUNE_CONFIG_DIR` at a temporary directory and sets
// `DEV_PRUNE_NO_AUTO_SETUP`, so nothing here can touch the developer's own machine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dev_prune::config::Registry;
use dev_prune::lock::{self, Taken};
use tempfile::TempDir;

fn devp(config_dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dev-prune"));
    cmd.env("DEV_PRUNE_NO_AUTO_SETUP", "1");
    cmd.env("DEV_PRUNE_OFFLINE", "1");
    cmd.env("DEV_PRUNE_CONFIG_DIR", config_dir);
    // Not this crate's root: `status` and `run` register the repository they stand in.
    cmd.current_dir(std::env::temp_dir());
    cmd
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn hold(config_dir: &Path) -> lock::RunLock {
    match lock::try_take(config_dir).expect("lock file opens") {
        Taken::Held(held) => held,
        Taken::Busy(_) => panic!("nothing else should hold this lock"),
    }
}

fn registry_file(config_dir: &Path) -> PathBuf {
    config_dir.join("registry.json")
}

/// Rewrites the registry's writer stamp, as another install would have left it.
fn stamp_as(config_dir: &Path, writer: Option<&str>) {
    let path = registry_file(config_dir);
    Registry::default().save_to(&path).unwrap();
    let mut doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let object = doc.as_object_mut().unwrap();
    match writer {
        Some(v) => {
            object.insert("written_by".into(), v.into());
        }
        None => {
            object.remove("written_by");
        }
    }
    fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
}

#[test]
fn a_scheduled_pass_skips_while_the_lock_is_held_and_exits_zero() {
    let tmp = TempDir::new().unwrap();
    let _held = hold(tmp.path());

    let out = devp(tmp.path()).args(["run", "--daemon"]).output().unwrap();

    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("Skipped"), "{shown}");
    assert!(
        shown.contains(&format!("pid {}", std::process::id())),
        "the holder is named: {shown}"
    );
    assert!(
        shown.contains("devp "),
        "the holder's version is named: {shown}"
    );
}

#[test]
fn an_interactive_run_while_the_lock_is_held_exits_one_naming_the_holder() {
    let tmp = TempDir::new().unwrap();
    let _held = hold(tmp.path());

    let out = devp(tmp.path())
        .args(["run", "--dry-run"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let shown = text(&out);
    assert!(
        shown.contains(&format!("pid {}", std::process::id())),
        "{shown}"
    );
    assert!(shown.contains("Wait for it to finish"), "{shown}");
}

#[test]
fn an_interactive_restore_while_the_lock_is_held_exits_one() {
    let tmp = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let _held = hold(tmp.path());

    let out = devp(tmp.path())
        .args(["restore", project.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("already running a pass"),
        "{}",
        text(&out)
    );
}

#[test]
fn a_hook_triggered_registration_steps_aside_quietly() {
    let tmp = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let _held = hold(tmp.path());

    let out = devp(tmp.path())
        .args(["link", project.path().to_str().unwrap(), "--quiet"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        out.stdout.is_empty(),
        "--quiet prints nothing on stdout: {}",
        text(&out)
    );
}

#[test]
fn read_only_commands_run_while_the_lock_is_held() {
    let tmp = TempDir::new().unwrap();
    let _held = hold(tmp.path());

    for args in [["status", "--json"], ["stats", "--json"]] {
        let out = devp(tmp.path()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", text(&out));
    }
}

#[test]
fn a_finished_pass_releases_the_lock() {
    let tmp = TempDir::new().unwrap();

    let out = devp(tmp.path())
        .args(["run", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    // Taking it here proves the child let go on its way out.
    drop(hold(tmp.path()));
}

#[test]
fn a_lock_file_left_behind_by_a_dead_process_does_not_block_a_pass() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        lock::lock_path(tmp.path()),
        "pid=4294967295\nversion=0.0.1\nsince=2020-01-01T00:00:00Z\n",
    )
    .unwrap();

    let out = devp(tmp.path())
        .args(["run", "--dry-run"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

#[test]
fn a_registry_from_a_newer_devp_is_readable_but_not_overwritten() {
    let tmp = TempDir::new().unwrap();
    stamp_as(tmp.path(), Some("999.0.0"));
    let before = fs::read_to_string(registry_file(tmp.path())).unwrap();

    let status = devp(tmp.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0), "{}", text(&status));

    let set = devp(tmp.path())
        .args(["config", "set", "min_size_mb", "7"])
        .output()
        .unwrap();
    assert_eq!(set.status.code(), Some(1), "{}", text(&set));
    let shown = text(&set);
    assert!(
        shown.contains("registry.json was written by devp 999.0.0, this is"),
        "{shown}"
    );
    assert!(shown.contains("see devp doctor"), "{shown}");

    assert_eq!(
        fs::read_to_string(registry_file(tmp.path())).unwrap(),
        before,
        "neither command may rewrite the newer registry"
    );
}

#[test]
fn a_registry_without_the_stamp_is_not_rewritten_by_reading_it() {
    let tmp = TempDir::new().unwrap();
    stamp_as(tmp.path(), None);
    let before = fs::read_to_string(registry_file(tmp.path())).unwrap();

    for args in [["status", "--json"], ["stats", "--json"]] {
        let out = devp(tmp.path()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", text(&out));
    }

    assert_eq!(
        fs::read_to_string(registry_file(tmp.path())).unwrap(),
        before
    );
}

#[test]
fn an_older_registry_is_updated_and_stamped_by_a_mutating_command() {
    let tmp = TempDir::new().unwrap();
    stamp_as(tmp.path(), Some("0.0.1"));

    let out = devp(tmp.path())
        .args(["config", "set", "min_size_mb", "7"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        Registry::written_by_at(&registry_file(tmp.path())).as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}
