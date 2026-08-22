// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// End-to-end coverage for repositories that host more than one package manager.
//
// A single repository may put uv, npm and cargo side by side in its root, spread them
// across `frontend/`, `services/api/` and `tools/cli/`, or mix both. These tests drive
// the real binary over each of those layouts.

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
    cmd
}

/// A throwaway config directory under `target/`, shared by every case in this file that
/// does not name one of its own.
///
/// `CARGO_TARGET_TMPDIR` rather than `std::env::temp_dir()`: it is inside the build
/// directory, so `cargo clean` removes it and a test run leaves nothing behind anywhere
/// else on the machine.
fn scratch_config_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("monorepo-scratch-config");
    std::fs::create_dir_all(&dir).expect("create scratch config dir");
    dir
}

/// A `git` that cannot see the developer's own configuration.
///
/// dev-prune installs a *global* `core.hooksPath`, so without this a fixture's first
/// commit fires the real `post-commit` hook and registers the temporary directory in the
/// developer's real registry.
fn git(path: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(path)
        .env("GIT_CONFIG_GLOBAL", path.join("no-such-gitconfig"))
        .env("GIT_CONFIG_SYSTEM", path.join("no-such-gitconfig"));
    cmd
}

/// Create a git repository with one commit so activity checks have something to read.
fn git_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path).args(["init"]).output().unwrap();
    // Unique per repository — see the same helper in `cli_contract_test.rs`: identical
    // trees committed within one second collide, and `link` identifies a repository by
    // its root commit.
    fs::write(
        path.join("README.md"),
        format!("# {}", path.file_name().unwrap().to_string_lossy()),
    )
    .unwrap();
    git(path).args(["add", "."]).output().unwrap();
    git(path)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@test.com",
            "commit",
            "-m",
            "initial",
        ])
        .output()
        .unwrap();
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// An npm project with an installed `node_modules`.
fn npm_project(dir: &Path) {
    write(&dir.join("package.json"), "{}");
    write(&dir.join("package-lock.json"), "{}");
    write(
        &dir.join("node_modules/left-pad/index.js"),
        "module.exports=1",
    );
}

/// A cargo project with a build directory.
fn cargo_project(dir: &Path) {
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    );
    write(&dir.join("target/debug/artifact"), "binary");
}

/// A pip/venv project. Its lockfile check is pure file inspection, so this is the one
/// ecosystem that can be pruned for real without the package manager installed.
fn venv_project(dir: &Path) {
    write(&dir.join("requirements.txt"), "requests==2.32.3\n");
    write(&dir.join(".venv/pyvenv.cfg"), "home = /usr\n");
    write(&dir.join(".venv/lib/site.py"), "payload");
}

/// Run a targeted dry run against `repo` and return combined output.
fn dry_run(config_dir: &Path, repo: &Path) -> String {
    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", config_dir)
        .args(["--dry-run", "--force", "-y", "run", repo.to_str().unwrap()])
        .output()
        .expect("failed to run dev-prune");
    assert!(
        out.status.success(),
        "dev-prune exited {:?}\n{}\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn reports_three_ecosystems_sharing_one_root() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("all-in-root");
    git_repo(&repo);
    npm_project(&repo);
    cargo_project(&repo);
    venv_project(&repo);

    let output = dry_run(&tmp.path().join("config"), &repo);
    assert!(output.contains("node_modules"), "{output}");
    assert!(output.contains("target"), "{output}");
    assert!(output.contains(".venv"), "{output}");
}

#[test]
fn reports_three_ecosystems_at_three_different_depths() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("monorepo");
    git_repo(&repo);
    npm_project(&repo.join("frontend"));
    venv_project(&repo.join("services/api"));
    cargo_project(&repo.join("tools/cli"));

    let output = dry_run(&tmp.path().join("config"), &repo);
    assert!(output.contains("frontend/node_modules"), "{output}");
    assert!(output.contains("services/api/.venv"), "{output}");
    assert!(output.contains("tools/cli/target"), "{output}");
}

#[test]
fn reports_a_root_project_alongside_nested_ones() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("mixed");
    git_repo(&repo);
    cargo_project(&repo);
    npm_project(&repo.join("web"));

    let output = dry_run(&tmp.path().join("config"), &repo);
    assert!(output.contains("web/node_modules"), "{output}");
    // The root build directory is reported without a path prefix.
    assert!(
        output.lines().any(|l| l.contains("→ target ")),
        "root target missing from:\n{output}"
    );
}

#[test]
fn never_reports_dependencies_of_dependencies() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("nested-deps");
    git_repo(&repo);
    npm_project(&repo);
    // A dependency shipping its own lockfile must not be treated as a project.
    npm_project(&repo.join("node_modules/some-dep"));

    let output = dry_run(&tmp.path().join("config"), &repo);
    assert!(
        !output.contains("node_modules/some-dep"),
        "walked into node_modules:\n{output}"
    );
}

#[test]
fn prunes_every_nested_project_in_one_pass() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("py-monorepo");
    git_repo(&repo);
    venv_project(&repo.join("apps/worker"));
    venv_project(&repo.join("libs/shared"));

    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", tmp.path().join("config"))
        .args(["--force", "-y", "run", repo.to_str().unwrap()])
        .output()
        .expect("failed to run dev-prune");
    assert!(
        out.status.success(),
        "dev-prune exited {:?}\n{}\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(!repo.join("apps/worker/.venv").exists());
    assert!(!repo.join("libs/shared/.venv").exists());
    // Only the environments go; the sources that describe them stay.
    assert!(repo.join("apps/worker/requirements.txt").exists());
    assert!(repo.join("libs/shared/requirements.txt").exists());
}

#[test]
fn refuses_to_prune_a_nested_project_with_an_empty_requirements_file() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("unrecoverable");
    git_repo(&repo);
    venv_project(&repo.join("apps/worker"));
    // Nothing to reinstall from — deleting the environment would be irreversible.
    write(&repo.join("apps/worker/requirements.txt"), "# empty\n");

    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", tmp.path().join("config"))
        .args(["--force", "-y", "run", repo.to_str().unwrap()])
        .output()
        .expect("failed to run dev-prune");

    assert!(repo.join("apps/worker/.venv").exists());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("lists no packages"), "{combined}");
}

#[test]
fn restore_covers_every_nested_project() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("restore-monorepo");
    git_repo(&repo);
    npm_project(&repo.join("frontend"));
    cargo_project(&repo.join("tools/cli"));

    let out = devp()
        .env("DEV_PRUNE_CONFIG_DIR", tmp.path().join("config"))
        .args(["restore", repo.to_str().unwrap()])
        .output()
        .expect("failed to run dev-prune restore");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Both nested projects are addressed by name, whether or not their package manager
    // is installed on the machine running the test.
    assert!(combined.contains("frontend"), "{combined}");
    assert!(combined.contains("tools/cli"), "{combined}");
}
