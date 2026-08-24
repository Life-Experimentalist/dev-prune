// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// End-to-end coverage for the machine-readable and filtering surface of the CLI:
// `--json`, `--only` / `--skip`, `--min-size` / `min_size_mb`, and the release check.
//
// These drive the real binary. Every one of them points `DEV_PRUNE_CONFIG_DIR` at a
// temporary directory and sets `DEV_PRUNE_NO_AUTO_SETUP`, so the suite can neither read
// nor write the developer's own registry, scheduler or git hooks.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn devp(config_dir: &Path) -> Command {
    let exe = Path::new(env!("CARGO_BIN_EXE_dev-prune"));
    let mut cmd = Command::new(exe);
    cmd.env("DEV_PRUNE_NO_AUTO_SETUP", "1");
    // Each test gets a fresh config dir, so the interval-gated release check would be
    // "due" on every invocation — and a test suite must not touch the network.
    cmd.env("DEV_PRUNE_OFFLINE", "1");
    cmd.env("DEV_PRUNE_CONFIG_DIR", config_dir);

    // `doctor` warns when the running executable's own directory is not on PATH. The
    // test binary lives in `target/debug`, which is on PATH only because cargo puts it
    // there so Windows can find DLLs. Linux and macOS get `LD_LIBRARY_PATH`/`DYLD_*`
    // instead, so without this the doctor tests would assert one report on one platform
    // and a different one on the other two.
    if let Some(dir) = exe.parent() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let inherited = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}{sep}{inherited}", dir.display()));
    }
    cmd
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Fill a file with `bytes` of payload so size floors have something to measure.
fn write_sized(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

/// A `git` that cannot see the developer's own configuration.
///
/// dev-prune installs a *global* `core.hooksPath`. Without this isolation, committing in a
/// fixture repository fires the real `post-commit` hook, which registers the temporary
/// directory in the developer's real registry — leaving a dead entry behind after the
/// fixture is deleted. `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` pointing at files that do
/// not exist is how git is told to read neither.
fn git(path: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(path)
        .env("GIT_CONFIG_GLOBAL", path.join("no-such-gitconfig"))
        .env("GIT_CONFIG_SYSTEM", path.join("no-such-gitconfig"));
    cmd
}

fn git_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path).args(["init"]).output().unwrap();
    // The directory name, not a constant: two repositories with the same tree, author and
    // commit message made in the same second get the *same* commit hash, and `link`
    // recognises a repository by its root commit. Identical fixtures made a moved-repo
    // relink fire between unrelated test repositories, on whichever machine was fast
    // enough to create them inside one second.
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

/// A pip/venv project. Its lockfile check is pure file inspection, so this is the one
/// ecosystem that can be analysed without the package manager installed.
fn venv_project(dir: &Path, payload_bytes: usize) {
    write(&dir.join("requirements.txt"), "requests==2.32.3\n");
    write(&dir.join(".venv/pyvenv.cfg"), "home = /usr\n");
    write_sized(&dir.join(".venv/lib/site.py"), payload_bytes);
}

fn cargo_project(dir: &Path, payload_bytes: usize) {
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    );
    write(&dir.join("Cargo.lock"), "version = 3\n");
    write_sized(&dir.join("target/debug/artifact"), payload_bytes);
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Parse stdout as JSON, failing with the whole output rather than a bare parse error.
fn parse_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout_of(out))
        .unwrap_or_else(|e| panic!("stdout was not JSON ({e}):\n{}", combined(out)))
}

/// A registered repository holding a venv project and a cargo project.
fn fixture(tmp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);
    venv_project(&repo.join("api"), 4096);
    cargo_project(&repo.join("cli"), 4096);

    let out = devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "link failed:\n{}", combined(&out));

    // Cargo is opt-in, and this fixture's whole point is to offer two ecosystems. The
    // switch is set through the CLI rather than by writing the file, so the fixture
    // also proves `config set` reaches the adapter gate.
    let out = devp(&config)
        .args(["config", "set", "enable_cargo", "true"])
        .output()
        .unwrap();
    assert!(out.status.success(), "enable failed:\n{}", combined(&out));
    (config, repo)
}

#[test]
fn cargo_finds_nothing_until_its_switch_is_on() {
    // The opt-in has to hold at the point of discovery, not at the point of deletion:
    // an adapter that detects and then refuses is one that shows up in every total.
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);
    cargo_project(&repo.join("cli"), 4096);
    let out = devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "link failed:\n{}", combined(&out));

    let out = devp(&config)
        .args(["--force", "run", "--json", "--dry-run", "--only", "cargo"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(
        parse_json(&out)["results"].as_array().unwrap().is_empty(),
        "cargo was pruned without enable_cargo:\n{}",
        combined(&out)
    );
}

// ---------------------------------------------------------------------------
// --json
// ---------------------------------------------------------------------------

#[test]
fn status_json_is_parseable_and_carries_its_schema_version() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    let out = devp(&config).args(["status", "--json"]).output().unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let doc = parse_json(&out);
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["command"], "status");
    assert_eq!(doc["totals"]["repositories"], 1);
    assert_eq!(doc["repositories"].as_array().unwrap().len(), 1);
    assert!(
        doc["repositories"][0]["path"]
            .as_str()
            .unwrap()
            .contains(repo.file_name().unwrap().to_str().unwrap())
    );
    // Every setting a caller might branch on is present.
    for key in ["idle_days", "min_size_mb", "update_check", "auto_setup"] {
        assert!(doc["settings"].get(key).is_some(), "missing setting {key}");
    }
}

#[test]
fn status_json_prints_nothing_but_the_document() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config).args(["status", "--json"]).output().unwrap();
    let text = stdout_of(&out);
    // No banner, no ANSI colour, no version notice — the first byte is the document.
    assert!(text.starts_with('{'), "stdout began with: {text:.80}");
    assert!(!text.contains('\u{1b}'), "escape codes leaked into JSON");
}

#[test]
fn run_json_reports_every_candidate_with_a_stable_status_tag() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["--force", "run", "--json", "--dry-run"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let doc = parse_json(&out);
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["command"], "run");
    assert_eq!(doc["dry_run"], true);

    let results = doc["results"].as_array().unwrap();
    assert!(!results.is_empty(), "no results in {doc}");
    for r in results {
        assert_eq!(r["status"], "skipped_dry_run", "unexpected status in {r}");
        assert!(r["adapter"].is_string());
        assert!(r["repository"].is_string());
        assert!(r["directory"].is_string());
        assert!(r["bytes"].as_u64().is_some());
    }

    // A dry run frees nothing and reclaims everything it found.
    assert_eq!(doc["summary"]["bytes_freed"], 0);
    assert_eq!(doc["summary"]["directories_pruned"], 0);
    assert!(doc["summary"]["bytes_reclaimable"].as_u64().unwrap() > 0);
}

#[test]
fn run_json_refuses_to_delete_without_an_explicit_go_ahead() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    // No `--dry-run`, no `--yes`: there is no prompt and no TUI in JSON mode, so this
    // must fail loudly rather than delete silently or do nothing silently.
    let out = devp(&config).args(["run", "--json"]).output().unwrap();
    // A missing flag combination is a usage error, not a runtime failure: scripts
    // branch on the difference, so the exact code is pinned.
    assert_eq!(out.status.code(), Some(2));
    assert!(
        combined(&out).contains("--dry-run"),
        "error did not say how to proceed:\n{}",
        combined(&out)
    );
}

// ---------------------------------------------------------------------------
// --only / --skip
// ---------------------------------------------------------------------------

#[test]
fn only_restricts_the_pass_to_the_named_adapters() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["--force", "run", "--json", "--dry-run", "--only", "cargo"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let doc = parse_json(&out);
    let adapters: Vec<&str> = doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["adapter"].as_str().unwrap())
        .collect();
    assert!(!adapters.is_empty(), "filtered everything out: {doc}");
    assert!(
        adapters.iter().all(|a| *a == "cargo"),
        "saw {adapters:?}, expected only cargo"
    );
}

#[test]
fn skip_excludes_the_named_adapters_and_keeps_the_rest() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["--force", "run", "--json", "--dry-run", "--skip", "cargo"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let doc = parse_json(&out);
    let adapters: Vec<&str> = doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["adapter"].as_str().unwrap())
        .collect();
    assert!(!adapters.is_empty(), "filtered everything out: {doc}");
    assert!(!adapters.contains(&"cargo"), "saw {adapters:?}");
    assert!(adapters.contains(&"venv"), "saw {adapters:?}");
}

#[test]
fn an_unknown_adapter_name_is_an_error_listing_the_real_ones() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["run", "--dry-run", "--only", "nmp"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "a typo silently pruned nothing");
    let text = combined(&out);
    assert!(text.contains("nmp"), "{text}");
    assert!(
        text.contains("npm"),
        "the valid names were not offered:\n{text}"
    );
}

#[test]
fn only_and_skip_cannot_be_combined() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["run", "--dry-run", "--only", "cargo", "--skip", "venv"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}

// ---------------------------------------------------------------------------
// Size floor
// ---------------------------------------------------------------------------

#[test]
fn the_size_floor_hides_directories_that_are_not_worth_reinstalling() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    // The fixture's directories are a few KiB, so a 1 MiB floor excludes all of them.
    let out = devp(&config)
        .args(["--force", "run", "--json", "--dry-run", "--min-size", "1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let doc = parse_json(&out);
    assert_eq!(
        doc["results"].as_array().unwrap().len(),
        0,
        "floor did not apply: {doc}"
    );
    assert_eq!(doc["summary"]["bytes_reclaimable"], 0);
}

#[test]
fn the_global_floor_and_status_agree_with_the_run_that_follows_it() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["config", "set", "min_size_mb", "1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    // `status` must not advertise space that `run` will then decline to reclaim.
    let status = parse_json(&devp(&config).args(["status", "--json"]).output().unwrap());
    assert_eq!(status["settings"]["min_size_mb"], 1);
    assert_eq!(status["totals"]["reclaimable_bytes"], 0);

    let run = parse_json(
        &devp(&config)
            .args(["--force", "run", "--json", "--dry-run"])
            .output()
            .unwrap(),
    );
    assert_eq!(run["summary"]["bytes_reclaimable"], 0);
}

#[test]
fn a_repository_can_opt_out_of_a_global_floor() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    devp(&config)
        .args(["config", "set", "min_size_mb", "1"])
        .output()
        .unwrap();
    // `0` here is a value, not an absence: it turns the floor off for this repository.
    write(&repo.join(".devprune.json"), "{\n  \"min_size_mb\": 0\n}\n");

    let doc = parse_json(
        &devp(&config)
            .args(["--force", "run", "--json", "--dry-run"])
            .output()
            .unwrap(),
    );
    assert!(
        doc["summary"]["bytes_reclaimable"].as_u64().unwrap() > 0,
        "the per-repo override was ignored: {doc}"
    );
}

// ---------------------------------------------------------------------------
// restore --last-run
// ---------------------------------------------------------------------------

#[test]
fn last_run_says_so_when_no_pass_has_been_recorded() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["restore", "--last-run"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "claimed to restore nothing");
    let text = combined(&out);
    assert!(text.contains("No prune pass"), "{text}");
    // And points at the command that does work without a record.
    assert!(text.contains("devp restore"), "{text}");
}

#[test]
fn last_run_and_a_path_are_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    // Silently ignoring the path would restore the wrong thing — everything the last
    // pass touched instead of the one project the user named.
    let out = devp(&config)
        .args(["restore", "--last-run", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", combined(&out));
}

#[test]
fn a_targeted_run_records_what_it_deleted() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    // venv is the one ecosystem whose verification is pure file inspection, so this
    // really deletes a directory without needing a package manager installed.
    let out = devp(&config)
        .args([
            "--ignore-idle",
            "run",
            repo.to_str().unwrap(),
            "--yes",
            "--only",
            "venv",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(!repo.join("api/.venv").exists(), "nothing was deleted");

    let registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(config.join("registry.json")).unwrap()).unwrap();

    // A targeted run used to update neither the lifetime totals nor anything `restore`
    // could read.
    assert!(
        registry["total_freed_bytes"].as_u64().unwrap() > 0,
        "{registry}"
    );
    let dirs = registry["last_prune"]["dirs"].as_array().unwrap();
    assert_eq!(dirs.len(), 1, "{registry}");
    assert_eq!(dirs[0]["adapter"], "venv");
    assert_eq!(dirs[0]["bloat_dir"], "api/.venv");
    assert!(dirs[0]["size_freed"].as_u64().unwrap() > 0);
}

#[test]
fn a_dry_run_records_nothing_to_restore() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    devp(&config)
        .args(["--ignore-idle", "run", repo.to_str().unwrap(), "--dry-run"])
        .output()
        .unwrap();

    let out = devp(&config)
        .args(["restore", "--last-run"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "a dry run left something to undo");
    assert!(
        combined(&out).contains("No prune pass"),
        "{}",
        combined(&out)
    );
}

#[test]
fn explain_conflicts_with_json_as_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["run", "--explain", "--json"])
        .output()
        .unwrap();
    // A clap conflict, so the usage exit code — the two flags describe two different
    // output contracts and picking one silently would betray the other.
    assert_eq!(out.status.code(), Some(2), "{}", combined(&out));
}

#[test]
fn explain_reports_the_active_state_and_deletes_nothing() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    // The fixture commit is seconds old, so the repository is active — the state a
    // normal pass swallows and `--explain` exists to surface.
    let out = devp(&config).args(["run", "--explain"]).output().unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    let text = combined(&out);
    assert!(text.contains("active"), "{text}");
    assert!(text.contains("--ignore-idle"), "{text}");
    assert!(
        repo.join("api/.venv").exists(),
        "--explain deleted something"
    );
}

#[test]
fn explain_lists_candidates_without_touching_them() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    let out = devp(&config)
        .args(["--ignore-idle", "run", "--explain"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(combined(&out).contains("would prune"), "{}", combined(&out));
    assert!(
        repo.join("api/.venv").exists() && repo.join("cli/target").exists(),
        "--explain deleted something"
    );
}

#[test]
fn explain_names_a_directory_under_the_size_floor() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    // The fixture directories hold a few KiB, so a 1 MiB floor puts them under it —
    // and unlike a normal pass, `--explain` says so instead of listing nothing.
    let out = devp(&config)
        .args(["--ignore-idle", "run", "--explain", "--min-size", "1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(combined(&out).contains("size floor"), "{}", combined(&out));
    assert!(
        repo.join("api/.venv").exists(),
        "--explain deleted something"
    );
}

// ---------------------------------------------------------------------------
// skill --agent
// ---------------------------------------------------------------------------

#[test]
fn skill_agent_writes_cursor_rules_into_the_repo() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);

    let out = devp(&config)
        .args(["skill", "--agent", "cursor"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let written = fs::read_to_string(repo.join(".cursor/rules/dev-prune.mdc")).unwrap();
    assert!(
        written.starts_with(
            "---
"
        ),
        "mdc frontmatter missing"
    );
    assert!(written.contains("devp run --dry-run"), "{written}");
}

#[test]
fn skill_agent_copilot_owns_a_block_and_never_the_rest_of_the_file() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);
    let target = repo.join(".github").join("copilot-instructions.md");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(
        &target,
        "# Our own instructions

Keep these.
",
    )
    .unwrap();

    let run = || {
        devp(&config)
            .args(["skill", "--agent", "copilot"])
            .current_dir(&repo)
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    // A second run replaces the block instead of stacking another copy.
    assert!(run().status.success());

    let written = fs::read_to_string(&target).unwrap();
    assert!(written.starts_with("# Our own instructions"), "{written}");
    assert!(written.contains("Keep these."), "{written}");
    assert_eq!(written.matches("<!-- dev-prune:rules:start -->").count(), 1);
    assert_eq!(written.matches("<!-- dev-prune:rules:end -->").count(), 1);
}

#[test]
fn skill_agent_outside_a_repository_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let not_a_repo = tmp.path().join("plain");
    fs::create_dir_all(&not_a_repo).unwrap();

    let out = devp(&config)
        .args(["skill", "--agent", "windsurf"])
        .current_dir(&not_a_repo)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!not_a_repo.join(".windsurf").exists());
}

#[test]
fn skill_agent_with_an_unknown_editor_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let out = devp(&config)
        .args(["skill", "--agent", "emacs"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(combined(&out).contains("cursor"), "{}", combined(&out));
}

// ---------------------------------------------------------------------------
// auto_config and doctor --fix on repo configs
// ---------------------------------------------------------------------------

#[test]
fn auto_config_writes_a_default_config_on_link() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);

    // Off by default: linking writes nothing into the repository.
    devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!repo.join(".devprune.json").exists());

    devp(&config)
        .args(["config", "set", "auto_config", "true"])
        .output()
        .unwrap();
    devp(&config)
        .args(["unlink", repo.to_str().unwrap()])
        .output()
        .unwrap();
    let out = devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let written = fs::read_to_string(repo.join(".devprune.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert!(parsed.get("$schema").is_some());
}

#[test]
fn auto_config_never_overwrites_an_existing_config() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);
    fs::write(repo.join(".devprune.json"), "{ this is not json").unwrap();

    devp(&config)
        .args(["config", "set", "auto_config", "true"])
        .output()
        .unwrap();
    devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(
        fs::read_to_string(repo.join(".devprune.json")).unwrap(),
        "{ this is not json"
    );
}

#[test]
fn doctor_fix_replaces_a_broken_repo_config_and_keeps_the_original() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);
    devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    fs::write(repo.join(".devprune.json"), "{ not json at all").unwrap();

    // The report names the breakage and offers --fix, without touching the file.
    let report = devp(&config).arg("doctor").output().unwrap();
    let text = combined(&report);
    assert!(text.contains("Unreadable config"), "{text}");
    assert!(text.contains("--fix"), "{text}");
    assert_eq!(
        fs::read_to_string(repo.join(".devprune.json")).unwrap(),
        "{ not json at all"
    );

    let out = devp(&config).args(["doctor", "--fix"]).output().unwrap();
    let text = combined(&out);
    assert!(text.contains(".devprune.json.broken"), "{text}");

    // Defaults written, the broken original preserved beside them verbatim.
    let healed = fs::read_to_string(repo.join(".devprune.json")).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&healed).is_ok());
    assert_eq!(
        fs::read_to_string(repo.join(".devprune.json.broken")).unwrap(),
        "{ not json at all"
    );
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

#[test]
fn doctor_reports_a_healthy_installation_without_touching_it() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config).arg("doctor").output().unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    let text = combined(&out);
    for section in ["Installation", "Configuration", "Integrations", "Verdict"] {
        assert!(text.contains(section), "no {section} section:\n{text}");
    }

    // A diagnostic that creates the thing it is diagnosing cannot be trusted about it.
    assert!(
        !config.join("registry.json").exists(),
        "doctor wrote a registry"
    );
}

#[test]
fn doctor_fails_on_a_registry_it_cannot_parse() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    write(&config.join("registry.json"), "{ not json");

    let out = devp(&config).arg("doctor").output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));
    let text = combined(&out);
    assert!(text.contains("registry.json"), "{text}");
    assert!(text.contains("1 problem"), "{text}");
}

#[test]
fn doctor_flags_a_setting_that_only_a_hand_edit_could_produce() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    // `devp config set` refuses this; writing the file directly does not.
    write(
        &config.join("registry.json"),
        r#"{"version":"1.0","settings":{"idle_days":15,"check_interval_days":2,
           "auto_daemon":true,"scan_depth":0},"repositories":{}}"#,
    );

    let out = devp(&config).arg("doctor").output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));
    assert!(combined(&out).contains("scan_depth"), "{}", combined(&out));
}

#[test]
fn doctor_on_a_path_names_the_reason_it_would_not_be_pruned() {
    let tmp = TempDir::new().unwrap();
    let (config, repo) = fixture(&tmp);

    // A repository committed to seconds ago is active, which is the first gate a prune
    // pass hits after registration.
    let out = devp(&config)
        .args(["doctor", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    let text = combined(&out);
    assert!(text.contains("--ignore-idle"), "{text}");
    assert!(text.contains("venv"), "no project listing:\n{text}");
    assert!(
        text.contains("requirements.txt"),
        "no lockfile line:\n{text}"
    );
}

#[test]
fn doctor_on_a_non_repository_says_so_rather_than_scanning_it() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let plain = tmp.path().join("not-a-repo");
    fs::create_dir_all(&plain).unwrap();

    let out = devp(&config)
        .args(["doctor", plain.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));
    assert!(
        combined(&out).contains("Git repository"),
        "{}",
        combined(&out)
    );
}

#[test]
fn doctor_on_a_missing_path_is_an_error_not_an_empty_report() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let out = devp(&config)
        .args(["doctor", tmp.path().join("nope").to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", combined(&out));
    assert!(
        combined(&out).contains("Path not found"),
        "{}",
        combined(&out)
    );
}

/// A registry can accumulate entries for directories that were later deleted. Doctor must
/// call that out without burying every other finding under one line per dead path, and
/// must not call it breakage: the prune pass reports those entries and carries on.
#[test]
fn doctor_collapses_dead_registry_entries_into_one_actionable_warning() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    for name in ["gone-a", "gone-b", "gone-c"] {
        let repo = tmp.path().join(name);
        git_repo(&repo);
        devp(&config)
            .args(["link", repo.to_str().unwrap()])
            .output()
            .unwrap();
        fs::remove_dir_all(&repo).unwrap();
    }

    let out = devp(&config).args(["doctor"]).output().unwrap();
    let text = combined(&out);

    assert_eq!(
        out.status.code(),
        Some(0),
        "stale entries are not breakage\n{text}"
    );
    assert!(
        text.contains("3 registered paths no longer exist"),
        "expected one collapsed line, got:\n{text}"
    );
    assert!(
        text.contains("devp unlink --missing"),
        "the warning must name the command that fixes all three\n{text}"
    );
}

#[test]
fn unlink_missing_removes_every_dead_entry_and_leaves_the_live_ones() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let live = tmp.path().join("live");
    let dead = tmp.path().join("dead");
    for repo in [&live, &dead] {
        git_repo(repo);
        devp(&config)
            .args(["link", repo.to_str().unwrap()])
            .output()
            .unwrap();
    }
    fs::remove_dir_all(&dead).unwrap();

    let out = devp(&config)
        .args(["unlink", "--missing"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(
        combined(&out).contains("Removed 1 registry entry"),
        "{}",
        combined(&out)
    );

    let registry = fs::read_to_string(config.join("registry.json")).unwrap();
    assert!(
        registry.contains("live"),
        "the live repository must survive"
    );
    assert!(!registry.contains("dead"), "the dead entry must be gone");
}

/// Registry keys are canonicalized paths, and a deleted directory can no longer be
/// canonicalized — so unregistering it by name has to fall back to a lexical match
/// instead of telling the user the path they are looking at "is not registered".
#[test]
fn unlink_by_name_still_works_after_the_directory_is_deleted() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let repo = tmp.path().join("erased");
    git_repo(&repo);
    devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    fs::remove_dir_all(&repo).unwrap();

    let out = devp(&config)
        .args(["unlink", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(combined(&out).contains("Unlinked:"), "{}", combined(&out));

    let registry = fs::read_to_string(config.join("registry.json")).unwrap();
    assert!(!registry.contains("erased"), "the entry must be gone");
}

/// A binary that demonstrably just ran is not broken; being off PATH is a warning with
/// the command that fixes it, never an exit-1. The harness normally prepends the test
/// binary's directory to PATH — this test strips it back out to simulate the off-PATH
/// install.
#[test]
fn doctor_off_path_is_a_warning_not_a_failure() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let exe = Path::new(env!("CARGO_BIN_EXE_dev-prune"));
    let exe_dir = exe.parent().unwrap();
    let sep = if cfg!(windows) { ';' } else { ':' };
    let matches_exe_dir = |entry: &str| {
        let norm = |s: &str| {
            let s = s.trim_end_matches(['/', '\\']).replace('\\', "/");
            if cfg!(windows) { s.to_lowercase() } else { s }
        };
        norm(entry) == norm(&exe_dir.to_string_lossy())
    };
    let stripped: String = std::env::var("PATH")
        .unwrap_or_default()
        .split(sep)
        .filter(|p| !matches_exe_dir(p))
        .collect::<Vec<_>>()
        .join(&sep.to_string());

    let out = devp(&config)
        .env("PATH", stripped)
        .arg("doctor")
        .output()
        .unwrap();
    let text = combined(&out);
    assert_eq!(
        out.status.code(),
        Some(0),
        "off-PATH must warn, not fail\n{text}"
    );
    assert!(
        text.contains("dev-prune setup"),
        "the warning must name the command that fixes it\n{text}"
    );
}

// ---------------------------------------------------------------------------
// status --drift and doctor --fix
// ---------------------------------------------------------------------------

/// `--drift` answers a different question than the dashboard, so combining it with a
/// dashboard-shaping flag is a usage error, not a silent pick between the two.
#[test]
fn status_drift_and_top_are_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["status", "--drift", "--top", "5"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", combined(&out));
}

/// The repository check has nothing `--fix` could safely repair, so the pair is refused
/// up front rather than the flag being silently ignored.
#[test]
fn doctor_fix_and_a_path_are_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["doctor", "--fix", "."])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", combined(&out));
}

/// Drift is a report, not a failure: unrecorded packages exit `0`, and the `--json`
/// document names the package, the directory, the adapter and the recording command.
#[test]
fn status_drift_reports_unrecorded_packages_and_exits_zero() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");
    let repo = tmp.path().join("repo");
    git_repo(&repo);

    let proj = repo.join("api");
    write(&proj.join("requirements.txt"), "requests==2.32.3\n");
    write(&proj.join(".venv/pyvenv.cfg"), "home = /usr\n");
    for dist in ["requests-2.32.3.dist-info", "sneaky_pkg-1.0.dist-info"] {
        fs::create_dir_all(proj.join(".venv/Lib/site-packages").join(dist)).unwrap();
    }

    let out = devp(&config)
        .args(["link", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "link failed:\n{}", combined(&out));

    let out = devp(&config)
        .args(["status", "--drift", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    let doc = parse_json(&out);
    assert_eq!(doc["command"], "status --drift");
    let drift = doc["drift"].as_array().expect("drift must be an array");
    let venv = drift
        .iter()
        .find(|d| d["adapter"] == "venv")
        .unwrap_or_else(|| panic!("no venv drift entry in {doc}"));
    assert_eq!(venv["directory"], ".venv");
    assert!(
        venv["unrecorded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "sneaky-pkg"),
        "sneaky-pkg must be listed:\n{doc}"
    );
    assert_eq!(venv["record_command"], "pip freeze > requirements.txt");
    assert!(doc["summary"]["unrecorded_packages"].as_u64().unwrap() >= 1);

    // The human report carries the same recording command, and still exits 0.
    let out = devp(&config).args(["status", "--drift"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", combined(&out));
    assert!(
        combined(&out).contains("pip freeze > requirements.txt"),
        "{}",
        combined(&out)
    );
}

/// `doctor --fix` clears the stale bookkeeping the diagnosis flagged — and because the
/// suite sets `DEV_PRUNE_NO_AUTO_SETUP`, anything that would write outside the config
/// directory is skipped and named rather than installed.
#[test]
fn doctor_fix_clears_dead_registry_entries_without_installing_anything() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let live = tmp.path().join("live");
    let dead = tmp.path().join("dead");
    for repo in [&live, &dead] {
        git_repo(repo);
        devp(&config)
            .args(["link", repo.to_str().unwrap()])
            .output()
            .unwrap();
    }
    fs::remove_dir_all(&dead).unwrap();

    let out = devp(&config).args(["doctor", "--fix"]).output().unwrap();
    let text = combined(&out);
    assert!(text.contains("Repairs"), "no repairs section:\n{text}");

    let registry = fs::read_to_string(config.join("registry.json")).unwrap();
    assert!(!registry.contains("dead"), "the dead entry must be gone");
    assert!(
        registry.contains("live"),
        "the live repository must survive"
    );
}

// ---------------------------------------------------------------------------
// Release check
// ---------------------------------------------------------------------------

#[test]
fn the_release_check_is_on_by_default_and_can_be_turned_off() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["config", "get", "update_check"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));
    assert!(stdout_of(&out).contains("update_check = true"));

    devp(&config)
        .args(["config", "set", "update_check", "false"])
        .output()
        .unwrap();
    let out = devp(&config)
        .args(["config", "get", "update_check"])
        .output()
        .unwrap();
    assert!(stdout_of(&out).contains("update_check = false"));
}

#[test]
fn update_offline_still_prints_the_upgrade_commands() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["update", "--offline"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let text = combined(&out);
    assert!(text.contains("Installed version"));
    assert!(text.contains("cargo install dev-prune"));
    assert!(
        text.contains("--offline"),
        "did not say the check was skipped"
    );
}

#[test]
fn a_pinned_copy_is_told_it_is_pinned_instead_of_how_to_upgrade() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let set = devp(&config)
        .args(["config", "set", "version_lock", "true"])
        .output()
        .unwrap();
    assert!(set.status.success(), "{}", combined(&set));

    let out = devp(&config)
        .args(["update", "--offline"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    // The upgrade command is the one command that undoes the pin, so `devp update`
    // stops offering it and names the pin instead.
    let text = combined(&out);
    assert!(text.contains("version_lock"), "{text}");
    assert!(!text.contains("cargo install dev-prune"), "{text}");
}

#[test]
fn a_pinned_copy_refuses_to_replace_its_own_binary() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    devp(&config)
        .args(["config", "set", "version_lock", "true"])
        .output()
        .unwrap();

    let out = devp(&config)
        .args(["update", "--install"])
        .output()
        .unwrap();
    // Not doing what was asked is a failure, the same as the offline refusal beside it:
    // the exit code is what a provisioning script reads, and it must not read as done.
    let text = combined(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("version_lock"), "{text}");
}

#[test]
fn doctor_reports_a_pin_without_calling_it_a_problem() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    devp(&config)
        .args(["config", "set", "version_lock", "true"])
        .output()
        .unwrap();

    let out = devp(&config).arg("doctor").output().unwrap();
    let text = combined(&out);
    // A deliberate pin is a fact to report, not breakage: exit 0, and named where
    // somebody looking for "why has this not updated" would actually look.
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("version_lock"), "{text}");
}

// ---------------------------------------------------------------------------
// Cache report
// ---------------------------------------------------------------------------
//
// Deliberately no end-to-end run of `devp caches` here. It walks every package manager
// cache on the machine it runs on — 28 seconds and 27 GiB on the developer laptop this
// was written on — so a test that invoked it would take longer than the rest of the
// suite put together, and would measure the runner's disk rather than dev-prune. The
// document it emits is pinned in `json.rs`, where it is deterministic and free; what is
// worth asserting out here is the flag surface and the exit codes.

#[test]
fn caches_advertises_its_json_form_and_that_it_deletes_nothing() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config).args(["caches", "--help"]).output().unwrap();
    assert!(out.status.success(), "{}", combined(&out));

    let text = combined(&out);
    assert!(text.contains("--json"), "{text}");
    // The one promise this command makes. If the wording ever drifts away from saying
    // so, the safety claim in the docs is no longer visible where people read it.
    assert!(
        text.contains("deletes nothing") || text.contains("read-only"),
        "the help text no longer says it is read-only:\n{text}"
    );
}

#[test]
fn an_unknown_caches_flag_is_a_usage_error_not_a_report() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("config");

    let out = devp(&config)
        .args(["caches", "--delete-them-all"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", combined(&out));
}

#[test]
fn a_disabled_check_keeps_run_and_status_off_the_network() {
    let tmp = TempDir::new().unwrap();
    let (config, _repo) = fixture(&tmp);

    devp(&config)
        .args(["config", "set", "update_check", "false"])
        .output()
        .unwrap();

    // With the check off, the registry must carry no evidence of one having run.
    devp(&config).args(["status"]).output().unwrap();
    let registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(config.join("registry.json")).unwrap()).unwrap();
    assert!(registry["last_update_check"].is_null(), "{registry}");
}

/// `--yes` alone must be a usage error, not a silent no-op.
///
/// It exists to skip one confirmation, and the confirmation belongs to `--fix-ownership`.
/// Accepting it on its own would let `devp trust --yes` look like it had agreed to
/// something when it had done nothing at all.
#[test]
fn trust_yes_requires_fix_ownership() {
    let dir = TempDir::new().unwrap();
    let out = devp(dir.path()).args(["trust", "--yes"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "usage errors exit 2");
}

/// `--fix-ownership` writes to git's config; `--json` promises one document on stdout.
/// Asking for both is a contradiction, and clap should say so rather than pick one.
#[test]
fn trust_cannot_repair_and_emit_json_at_once() {
    let dir = TempDir::new().unwrap();
    let out = devp(dir.path())
        .args(["trust", "--fix-ownership", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "usage errors exit 2");
}

/// With nothing registered there is nothing Git could be refusing, so the repair reports
/// that and exits 0 — a clean machine is not a failure.
#[test]
fn trust_fix_ownership_is_clean_on_an_empty_registry() {
    let dir = TempDir::new().unwrap();
    let out = devp(dir.path())
        .args(["trust", "--fix-ownership", "--yes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Nothing to fix"), "unexpected output: {text}");
}
