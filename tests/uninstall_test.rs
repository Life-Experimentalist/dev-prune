// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// End-to-end coverage for `devp uninstall` — the one command whose job is deletion,
// and therefore the one whose tests most need to prove they touch nothing real.
//
// Isolation is layered. `DEV_PRUNE_CONFIG_DIR` puts the managed installation — the
// binaries, hooks, registry — inside a fresh temp directory. `DEV_PRUNE_NO_AUTO_SETUP`
// makes uninstall hands-off about everything setup would have installed: the real
// scheduler is not touched, no agent skill directory is deleted, and the stray-copy
// sweep searches only `PATH` — which every test here pins to its own directories, so
// the sweep can find exactly the strays the test planted and nothing on the
// developer's machine. `git` is deliberately absent from that `PATH`: the hook step
// must cope with a machine that has no git, and coping is what these tests observe.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// The uninstall command, sandboxed as described in the header.
fn devp_uninstall(config_dir: &Path, extra_path_dirs: &[&Path]) -> Command {
    let exe = Path::new(env!("CARGO_BIN_EXE_dev-prune"));
    let mut cmd = Command::new(exe);
    cmd.env("DEV_PRUNE_NO_AUTO_SETUP", "1");
    cmd.env("DEV_PRUNE_OFFLINE", "1");
    cmd.env("DEV_PRUNE_CONFIG_DIR", config_dir);
    // The Linux file-type unregistration honours XDG_DATA_HOME; point it inside the
    // sandbox so the test can never sweep a real desktop database entry.
    cmd.env("XDG_DATA_HOME", config_dir.join("xdg-data"));
    let mut paths: Vec<PathBuf> = vec![exe.parent().unwrap().to_path_buf()];
    paths.extend(extra_path_dirs.iter().map(|p| p.to_path_buf()));
    cmd.env("PATH", std::env::join_paths(paths).unwrap());
    cmd.stdin(std::process::Stdio::null());
    cmd.arg("uninstall");
    cmd
}

/// A command against the same sandbox with the developer's own PATH intact — for the
/// fixture steps (like `link`) that need a working `git`.
fn devp_with_real_path(config_dir: &Path) -> Command {
    let exe = Path::new(env!("CARGO_BIN_EXE_dev-prune"));
    let mut cmd = Command::new(exe);
    cmd.env("DEV_PRUNE_NO_AUTO_SETUP", "1");
    cmd.env("DEV_PRUNE_OFFLINE", "1");
    cmd.env("DEV_PRUNE_CONFIG_DIR", config_dir);
    cmd
}

fn combined(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// A fake managed installation: `<config>/bin` holding the pair of binaries.
fn fake_managed_install(config_dir: &Path) -> PathBuf {
    let bin = config_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for stem in ["dev-prune", "devp"] {
        fs::write(bin.join(exe_name(stem)), b"not a real binary").unwrap();
    }
    bin
}

/// A real registry in the sandbox, created the way a user would create one — by
/// linking a repository. Hand-written JSON would drift from the schema.
fn seed_registry(config_dir: &Path, repo_parent: &Path) {
    let repo = repo_parent.join("seedrepo");
    git_repo(&repo);
    let out = devp_with_real_path(config_dir)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "seed link failed:\n{}",
        combined(&out)
    );
    assert!(config_dir.join("registry.json").exists());
}

/// An isolated git repository the sandbox registry can hold.
fn git_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    let no_config = path.join("no-such-gitconfig");
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(path)
            .env("GIT_CONFIG_GLOBAL", &no_config)
            .env("GIT_CONFIG_SYSTEM", &no_config)
            .output()
            .unwrap()
    };
    run(&["init"]);
    fs::write(path.join("README.md"), "fixture").unwrap();
    run(&["add", "."]);
    run(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "init",
    ]);
}

#[test]
fn a_light_uninstall_removes_the_managed_binaries_and_keeps_the_config() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let bin = fake_managed_install(&config);
    seed_registry(&config, tmp.path());

    let out = devp_uninstall(&config, &[]).output().unwrap();
    assert!(
        out.status.success(),
        "light uninstall failed:\n{}",
        combined(&out)
    );

    for stem in ["dev-prune", "devp"] {
        assert!(
            !bin.join(exe_name(stem)).exists(),
            "{stem} survived a light uninstall"
        );
    }
    // Light keeps the configuration for a future reinstall — that promise is the
    // whole difference between the two modes.
    assert!(
        config.join("registry.json").exists(),
        "light uninstall deleted the registry:\n{}",
        combined(&out)
    );
    assert!(
        combined(&out).contains("preserved"),
        "success message missing:\n{}",
        combined(&out)
    );
}

#[test]
fn a_deep_uninstall_without_confirmation_refuses_and_deletes_nothing() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let bin = fake_managed_install(&config);
    seed_registry(&config, tmp.path());

    // stdin is null, so the confirmation cannot be answered — the command must bail
    // out before deleting anything, not fall through to "no answer means yes".
    let out = devp_uninstall(&config, &[]).arg("--deep").output().unwrap();
    assert!(
        !out.status.success(),
        "deep uninstall proceeded without confirmation:\n{}",
        combined(&out)
    );
    assert!(
        combined(&out).contains("--yes"),
        "the refusal must name the flag that overrides it:\n{}",
        combined(&out)
    );
    assert!(bin.join(exe_name("devp")).exists());
    assert!(config.join("registry.json").exists());
}

#[test]
fn a_deep_uninstall_removes_the_config_directory_and_per_repo_configs() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    fake_managed_install(&config);

    // A registered repository with a `.devprune.json` — deep uninstall promises to
    // remove that file but must not touch anything else in the repository.
    let repo = tmp.path().join("repo");
    git_repo(&repo);
    fs::write(repo.join(".devprune.json"), "{}").unwrap();
    let out = devp_with_real_path(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "link failed:\n{}", combined(&out));

    let out = devp_uninstall(&config, &[])
        .args(["--deep", "--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "deep uninstall failed:\n{}",
        combined(&out)
    );
    assert!(!config.exists(), "config directory survived --deep");
    assert!(
        !repo.join(".devprune.json").exists(),
        "per-repo config survived --deep"
    );
    assert!(
        repo.join("README.md").exists(),
        "repo contents were touched"
    );
    assert!(
        repo.join(".git").exists(),
        "the repository itself was touched"
    );
}

#[test]
fn the_sweep_lists_strays_but_deletes_nothing_without_yes() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    fake_managed_install(&config);

    let straybin = tmp.path().join("straybin");
    fs::create_dir_all(&straybin).unwrap();
    let stray = straybin.join(exe_name("devp"));
    fs::write(&stray, b"a stray copy").unwrap();

    let out = devp_uninstall(&config, &[&straybin]).output().unwrap();
    let text = combined(&out);
    assert!(
        text.contains("more cop"),
        "the stray was not announced:\n{text}"
    );
    assert!(
        text.contains("straybin"),
        "the stray location was not shown:\n{text}"
    );
    // Not a terminal, no --yes: the sweep must say how to opt in and leave the file.
    assert!(
        text.contains("--yes"),
        "no pointer to --yes for a non-interactive run:\n{text}"
    );
    assert!(stray.exists(), "the sweep deleted without confirmation");
    // A declined sweep is a decision, not a failure.
    assert!(
        out.status.success(),
        "declining the sweep changed the exit code:\n{text}"
    );
}

#[test]
fn the_sweep_with_yes_deletes_strays_and_their_shims() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    fake_managed_install(&config);

    let straybin = tmp.path().join("straybin");
    fs::create_dir_all(&straybin).unwrap();
    // On Windows npm leaves `.cmd`/`.ps1` shims and an extensionless sh shim beside
    // the real thing; each is a separate file the sweep must find by name.
    let names: &[&str] = if cfg!(windows) {
        &["devp.exe", "devp.cmd", "devp.ps1", "dev-prune.exe"]
    } else {
        &["devp", "dev-prune"]
    };
    for name in names {
        fs::write(straybin.join(name), b"a stray copy").unwrap();
    }
    // A bystander with a different name must never be touched, whatever happens.
    let bystander = straybin.join("devp-helper.sh");
    fs::write(&bystander, b"not ours").unwrap();

    let out = devp_uninstall(&config, &[&straybin])
        .arg("--yes")
        .output()
        .unwrap();
    let text = combined(&out);
    assert!(
        out.status.success(),
        "uninstall with a sweep failed:\n{text}"
    );
    for name in names {
        assert!(
            !straybin.join(name).exists(),
            "stray `{name}` survived a confirmed sweep:\n{text}"
        );
    }
    assert!(
        bystander.exists(),
        "the sweep deleted a file that is not dev-prune's"
    );
    assert!(text.contains("stray cop"), "no removal report:\n{text}");
}

#[test]
fn the_sweep_never_touches_a_development_build() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    fake_managed_install(&config);

    // The running test binary lives in `target/debug` and is on the sweep's PATH via
    // its own parent directory. Deleting it would destroy the build being tested —
    // the dev-build guard is what stands between the sweep and that outcome.
    let out = devp_uninstall(&config, &[]).arg("--yes").output().unwrap();
    assert!(
        out.status.success(),
        "uninstall failed:\n{}",
        combined(&out)
    );
    assert!(
        Path::new(env!("CARGO_BIN_EXE_dev-prune")).exists(),
        "the sweep deleted the development build under test"
    );
}

#[test]
fn hands_off_uninstall_reports_what_it_leaves_alone() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    fake_managed_install(&config);

    let out = devp_uninstall(&config, &[]).output().unwrap();
    let text = combined(&out);
    // `DEV_PRUNE_NO_AUTO_SETUP` is set in every one of these tests; the command must
    // say that the scheduler and skills were deliberately skipped, not silently
    // pretend it removed them.
    assert!(
        text.contains("DEV_PRUNE_NO_AUTO_SETUP"),
        "the hands-off skip was silent:\n{text}"
    );
}
