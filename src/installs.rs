// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Finding every devp on this machine.
//
// One function, [`discover`], answers "which devp binaries exist here, how did each get
// here, and which one does each caller run". `devp doctor` summarises its answer in one
// finding and `devp installs` prints all of it, so the two can never disagree about what
// is installed. The environment (PATH, home, the scheduler's and hooks' targets, the
// running binary) and the version query are passed in, which is what lets the tests
// build a machine out of a temp directory instead of depending on this one.
//
// The search is a fixed list of places, not a scan of the disk. A copy somebody put in a
// folder that is on no PATH and in none of the places below cannot be found, and the
// documentation says so rather than implying otherwise.

use std::path::{Path, PathBuf};

use crate::channel::{self, Channel};
use crate::constants;
use crate::setup;

/// `major.minor.patch`, as [`setup::parse_version`] returns it.
pub type Version = (u64, u64, u64);

/// What a caller does with an install. Several can be true of one install, and each can
/// belong to a different install, which is the situation worth reporting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Roles {
    /// First on PATH: what typing `devp` in a new shell runs.
    pub shell: bool,
    /// The scheduled task or timer starts this one.
    pub scheduler: bool,
    /// The global git hook shims call this one.
    pub hook: bool,
    /// The binary answering this very command.
    pub running: bool,
}

/// One devp binary, after symlinks and second names have been folded together.
#[derive(Debug, Clone)]
pub struct Install {
    /// The path it was found at (the first name found, in search order).
    pub path: PathBuf,
    /// The real file behind [`Self::path`], symlinks resolved.
    pub canonical: PathBuf,
    pub channel: Channel,
    /// `None` when the copy could not state a version: it did not run, did not answer
    /// in time, or is the windowless twin, which is never asked.
    pub version: Option<Version>,
    /// 1-based position of its folder among the non-empty PATH entries; `None` when the
    /// folder is on no PATH.
    pub path_position: Option<usize>,
    pub roles: Roles,
    /// Only `devpw.exe` was found in this folder. It is the scheduler's launcher, not a
    /// command anyone types, so cleanup never keeps or removes it on its own.
    pub windowless_only: bool,
}

impl Install {
    pub fn on_path(&self) -> bool {
        self.path_position.is_some()
    }
}

/// Everything about the machine that [`discover`] reads, gathered in one place.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub path_var: String,
    pub home: Option<PathBuf>,
    /// The installer's `<config>/bin` folder.
    pub managed_dir: Option<PathBuf>,
    pub running: Option<PathBuf>,
    /// The running binary's own version, known without spawning anything.
    pub running_version: Option<Version>,
    /// Where the scheduler's entry points, as registered with the OS.
    pub scheduler: Option<PathBuf>,
    /// Where the git hook shims point.
    pub hook: Option<PathBuf>,
}

impl Environment {
    pub fn from_machine() -> Self {
        Environment {
            path_var: std::env::var_os("PATH")
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_default(),
            home: dirs::home_dir(),
            managed_dir: setup::managed_exe_path()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            running: std::env::current_exe().ok(),
            running_version: setup::parse_version(constants::VERSION),
            scheduler: crate::daemon::registered_exe_path(),
            hook: crate::commands::hook::registered_exe_path(),
        }
    }
}

/// What [`discover`] found.
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub installs: Vec<Install>,
    /// Echoes of [`Environment::scheduler`] and [`Environment::hook`], so a printer can
    /// name a target that belongs to no install found here.
    pub scheduler: Option<PathBuf>,
    pub hook: Option<PathBuf>,
}

/// Discover on this machine, asking each distinct copy for its version.
pub fn discover_machine() -> Discovery {
    discover(&Environment::from_machine(), &setup::binary_version)
}

/// Find every devp reachable from `env`.
///
/// `ask` is the version query. It is called at most once per distinct real file, never
/// for the running binary (its version is in `env`) and never for `devpw`, which is
/// linked for the GUI subsystem and, asked for `--version`, waits forever. The calls run
/// in parallel so one hung copy costs one ceiling, not one per copy.
pub fn discover(env: &Environment, ask: &(dyn Fn(&Path) -> Option<Version> + Sync)) -> Discovery {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut dirs: Vec<(PathBuf, Option<usize>)> = Vec::new();
    let mut position = 0;
    for entry in env.path_var.split(sep).filter(|e| !e.is_empty()) {
        position += 1;
        let dir = PathBuf::from(entry);
        // PATH routinely lists a folder twice; the earlier entry is the one that matters.
        if !dirs.iter().any(|(seen, _)| same_dir(seen, &dir)) {
            dirs.push((dir, Some(position)));
        }
    }
    // The places a manager installs into whether or not the folder is on PATH. A copy
    // nobody can see is a copy nobody upgrades, and it becomes the one that runs the day
    // the folder reaches PATH or a script calls it by absolute path.
    let mut known = channel::install_dirs(env.home.as_deref());
    known.extend(env.managed_dir.clone());
    // Not in `install_dirs`: `devp uninstall` sweeps that list, and `~/bin` is a folder
    // people own, not one a package manager does.
    known.extend(env.home.as_deref().map(|h| h.join("bin")));
    for dir in known {
        if !dirs.iter().any(|(seen, _)| same_dir(seen, &dir)) {
            dirs.push((dir, None));
        }
    }

    let mut installs: Vec<Install> = Vec::new();
    for (dir, position) in &dirs {
        let before = installs.len();
        for name in binary_names() {
            add_candidate(&mut installs, dir, &name, *position, env, false);
        }
        // `.cmd` shims are how npm, pnpm, yarn and bun put a command on a Windows PATH;
        // the real binary sits in their package tree. One install per folder: a shim
        // beside a real `.exe` is the same install.
        if cfg!(windows) && installs.len() == before {
            for name in ["dev-prune.cmd", "devp.cmd"] {
                add_candidate(&mut installs, dir, name, *position, env, false);
            }
        }
        // The windowless twin alone in a folder is still evidence of an install, but only
        // when no real binary was found there (or in the same folder under another
        // spelling): it normally sits beside its console twin and is not a second copy.
        if cfg!(windows)
            && !installs
                .iter()
                .any(|i| i.path.parent().is_some_and(|p| same_dir(p, dir)))
        {
            add_candidate(
                &mut installs,
                dir,
                constants::WINDOWS_WINDOWLESS_BIN,
                *position,
                env,
                true,
            );
        }
    }

    // The running binary is always listed, even when it lives in a folder nothing above
    // covers (`target/debug`, a downloads folder).
    if let Some(running) = env.running.as_deref()
        && running.is_file()
        && owner_of_exact(&installs, running).is_none()
        && manager_sibling(
            &installs,
            running,
            new_install(running, None, env, false).channel,
        )
        .is_none()
    {
        let on_path_at = running.parent().and_then(|d| {
            dirs.iter()
                .find(|(seen, _)| same_dir(seen, d))
                .and_then(|(_, p)| *p)
        });
        installs.push(new_install(running, on_path_at, env, false));
    }

    // Exact match, or the manager's other name for the same install: attributing the
    // running binary to whatever else sits in its folder would mark a stale neighbour as
    // the one answering this command.
    let running_idx = env.running.as_deref().and_then(|r| {
        owner_of_exact(&installs, r)
            .or_else(|| manager_sibling(&installs, r, new_install(r, None, env, false).channel))
    });
    if let Some(i) = running_idx {
        installs[i].roles.running = true;
    }
    let scheduler_idx = env
        .scheduler
        .as_deref()
        .and_then(|t| owner_of(&installs, t));
    if let Some(i) = scheduler_idx {
        installs[i].roles.scheduler = true;
    }
    let hook_idx = env.hook.as_deref().and_then(|t| owner_of(&installs, t));
    if let Some(i) = hook_idx {
        installs[i].roles.hook = true;
    }
    let shell_idx = installs
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.windowless_only)
        .filter_map(|(idx, i)| i.path_position.map(|p| (p, idx)))
        .min()
        .map(|(_, idx)| idx);
    if let Some(i) = shell_idx {
        installs[i].roles.shell = true;
    }

    let versions: Vec<Option<Version>> = std::thread::scope(|scope| {
        let handles: Vec<_> = installs
            .iter()
            .enumerate()
            .map(|(idx, install)| {
                let known = (Some(idx) == running_idx).then_some(env.running_version);
                scope.spawn(move || {
                    if install.windowless_only {
                        None
                    } else if let Some(version) = known {
                        version
                    } else {
                        ask(&install.path)
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(None))
            .collect()
    });
    for (install, version) in installs.iter_mut().zip(versions) {
        install.version = version;
    }

    Discovery {
        installs,
        scheduler: env.scheduler.clone(),
        hook: env.hook.clone(),
    }
}

/// The names to look for in each folder.
fn binary_names() -> [String; 2] {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    [
        format!("{}{ext}", constants::BIN_NAME_LONG),
        format!("{}{ext}", constants::BIN_NAME_SHORT),
    ]
}

fn add_candidate(
    installs: &mut Vec<Install>,
    dir: &Path,
    name: &str,
    position: Option<usize>,
    env: &Environment,
    windowless_only: bool,
) {
    let candidate = dir.join(name);
    if !candidate.is_file() {
        return;
    }
    // Under both names and through symlinks one file is one install: Homebrew and the
    // install script link `devp` to `dev-prune`, and on Windows the pair are usually
    // hard links.
    if owner_of_exact(installs, &candidate).is_some() {
        return;
    }
    let install = new_install(&candidate, position, env, windowless_only);
    if manager_sibling(installs, &candidate, install.channel).is_some() {
        return;
    }
    installs.push(install);
}

/// The install already listed beside `path` when a package manager put both there.
///
/// `cargo install` links the `dev-prune` and `devp` targets separately, so the two files
/// never hold the same bytes, yet they are one install and `cargo uninstall` removes
/// both. Counting them as two copies made every cargo user look like they had a
/// duplicate. The installer's own folder is exempt: its `devp` is a copy that a failed
/// upgrade can leave stale, and that is worth reporting.
fn manager_sibling(installs: &[Install], path: &Path, channel: Channel) -> Option<usize> {
    if channel == Channel::Installer || channel == Channel::Unknown {
        return None;
    }
    let dir = path.parent()?;
    installs.iter().position(|i| {
        !i.windowless_only
            && i.channel == channel
            && i.path.parent().is_some_and(|p| same_dir(p, dir))
    })
}

fn new_install(
    path: &Path,
    position: Option<usize>,
    env: &Environment,
    windowless_only: bool,
) -> Install {
    let canonical = canonical_of(path);
    let in_managed = path
        .parent()
        .zip(env.managed_dir.as_deref())
        .is_some_and(|(a, b)| same_dir(a, b));
    let channel = if in_managed {
        // `Channel::detect_at` compares folders exactly; a PATH entry spelled in another
        // case or with a trailing separator must not turn the installer's copy into an
        // unrecognised one.
        Channel::Installer
    } else {
        // Symlinked installs (Homebrew, uv, npm) show the manager only at the target.
        match Channel::detect_at(path, env.managed_dir.as_deref()) {
            Channel::Unknown => Channel::detect_at(&canonical, env.managed_dir.as_deref()),
            known => known,
        }
    };
    Install {
        path: path.to_path_buf(),
        canonical,
        channel,
        version: None,
        path_position: position,
        roles: Roles::default(),
        windowless_only,
    }
}

/// Which install a path belongs to: the same file, or failing that the install in the
/// same folder.
///
/// The folder fallback is for the scheduler's target, which may be `devpw.exe` sitting
/// beside the console binary without being listed as an install of its own.
pub fn owner_of(installs: &[Install], target: &Path) -> Option<usize> {
    owner_of_exact(installs, target).or_else(|| {
        let dir = target.parent()?;
        installs
            .iter()
            .position(|i| i.path.parent().is_some_and(|p| same_dir(p, dir)))
    })
}

pub(crate) fn owner_of_exact(installs: &[Install], target: &Path) -> Option<usize> {
    let canonical = canonical_of(target);
    installs.iter().position(|i| {
        same_dir(&i.canonical, &canonical)
            || (i.path.parent() == target.parent() && same_binary(&i.path, target))
    })
}

/// The real path of `path`, without the `\\?\` prefix Windows adds, or `path` itself
/// when it cannot be resolved.
fn canonical_of(path: &Path) -> PathBuf {
    let Ok(real) = std::fs::canonicalize(path) else {
        return path.to_path_buf();
    };
    let text = real.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => real,
    }
}

/// Whether a PATH entry names the directory the binary lives in.
///
/// Windows paths are case-insensitive but `Path` equality is not, and PATH entries
/// routinely carry a trailing backslash the installer never wrote. Either mismatch made
/// doctor report a perfectly good install as "not on PATH" — as a problem, so `devp
/// doctor` exited 1 on a healthy machine.
pub(crate) fn same_dir(entry: &Path, dir: &Path) -> bool {
    if entry == dir {
        return true;
    }
    cfg!(windows) && {
        let norm = |p: &Path| {
            p.to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase()
        };
        norm(entry) == norm(dir)
    }
}

/// Whether two files hold the same bytes.
///
/// Doctor runs at human speed, so when the cheap size test cannot rule the pair
/// different this reads both files outright — a stale twin left by a failed upgrade can
/// share a size with its replacement, and "same version" is the whole question here.
pub(crate) fn same_binary(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return false;
    };
    if ma.len() != mb.len() {
        return false;
    }
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(ba), Ok(bb)) if ba == bb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn exe(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(exe(name));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn join_path(dirs: &[&Path]) -> String {
        let sep = if cfg!(windows) { ";" } else { ":" };
        dirs.iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(sep)
    }

    fn env_with(home: &Path, path: &[&Path]) -> Environment {
        Environment {
            path_var: join_path(path),
            home: Some(home.to_path_buf()),
            ..Environment::default()
        }
    }

    /// A version asker that answers from a table keyed by file bytes and counts calls.
    struct Asker {
        answers: HashMap<Vec<u8>, Option<Version>>,
        calls: Mutex<Vec<PathBuf>>,
    }

    impl Asker {
        fn new(answers: &[(&[u8], Option<Version>)]) -> Self {
            Asker {
                answers: answers.iter().map(|(b, v)| (b.to_vec(), *v)).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn ask(&self, path: &Path) -> Option<Version> {
            self.calls.lock().unwrap().push(path.to_path_buf());
            let bytes = std::fs::read(path).ok()?;
            self.answers.get(&bytes).copied().flatten()
        }
    }

    fn run(env: &Environment, asker: &Asker) -> Discovery {
        discover(env, &|p| asker.ask(p))
    }

    #[test]
    #[cfg(windows)]
    fn a_path_entry_matches_regardless_of_case_and_trailing_separator() {
        // Both differences are ones Windows itself ignores, and either used to turn
        // into a "not on PATH" problem — a healthy install failing `devp doctor`.
        let dir = Path::new(r"C:\Users\Someone\AppData\Roaming\dev-prune\bin");
        assert!(same_dir(
            Path::new(r"c:\users\someone\appdata\roaming\dev-prune\bin\"),
            dir
        ));
        assert!(!same_dir(Path::new(r"C:\Windows"), dir));
    }

    #[test]
    #[cfg(not(windows))]
    fn a_path_entry_on_unix_is_matched_exactly() {
        assert!(same_dir(
            Path::new("/usr/local/bin"),
            Path::new("/usr/local/bin")
        ));
        assert!(!same_dir(
            Path::new("/USR/local/bin"),
            Path::new("/usr/local/bin")
        ));
    }

    #[test]
    fn a_stale_twin_is_told_apart_from_a_current_one() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("dev-prune");
        let b = dir.path().join("devp");
        std::fs::write(&a, b"version two").unwrap();
        std::fs::write(&b, b"version two").unwrap();
        assert!(same_binary(&a, &b));

        // Same length, different bytes — the case a size-only test waves through.
        std::fs::write(&b, b"version one").unwrap();
        assert!(!same_binary(&a, &b));

        std::fs::write(&b, b"short").unwrap();
        assert!(!same_binary(&a, &b));
        assert!(!same_binary(&a, &dir.path().join("missing")));
    }

    #[test]
    fn path_entries_are_searched_and_empty_ones_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        put(&a, "devp", b"one");
        put(&b, "devp", b"two");
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut env = env_with(&tmp.path().join("nohome"), &[]);
        env.path_var = format!("{}{sep}{sep}{}", a.display(), b.display());

        let found = run(&env, &Asker::new(&[]));
        let positions: Vec<_> = found.installs.iter().map(|i| i.path_position).collect();
        assert_eq!(positions, vec![Some(1), Some(2)]);
    }

    #[test]
    fn one_binary_under_both_names_is_listed_once() {
        // `dev-prune` and `devp` in the same directory are the same binary — on Windows
        // usually literally the same file — so a single install must not read as two.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        put(&dir, "dev-prune", b"same bytes");
        put(&dir, "devp", b"same bytes");
        let env = env_with(&tmp.path().join("nohome"), &[&dir]);
        assert_eq!(run(&env, &Asker::new(&[])).installs.len(), 1);

        // A genuinely different binary under the second name is a second copy.
        put(&dir, "devp", b"a different build");
        assert_eq!(run(&env, &Asker::new(&[])).installs.len(), 2);
    }

    #[test]
    fn a_manager_that_links_each_name_separately_still_has_one_install() {
        // cargo builds `dev-prune` and `devp` as two outputs, so their bytes differ.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let bin = home.join(".cargo").join("bin");
        put(&bin, "dev-prune", b"first target");
        put(&bin, "devp", b"second target");
        let env = env_with(&home, &[&bin]);
        let found = run(&env, &Asker::new(&[]));
        assert_eq!(found.installs.len(), 1);
        assert_eq!(found.installs[0].channel, Channel::Cargo);
    }

    #[test]
    #[cfg(unix)]
    fn one_real_file_behind_a_symlink_and_two_path_entries_is_listed_once() {
        let tmp = tempfile::tempdir().unwrap();
        let real = put(&tmp.path().join("real"), "dev-prune", b"binary");
        let linked = tmp.path().join("linked");
        std::fs::create_dir_all(&linked).unwrap();
        std::os::unix::fs::symlink(&real, linked.join("devp")).unwrap();
        // The same folder spelled twice, plus a folder that only holds the link.
        let real_dir = tmp.path().join("real");
        let env = env_with(&tmp.path().join("nohome"), &[&real_dir, &linked, &real_dir]);
        let asker = Asker::new(&[(b"binary", Some((1, 2, 3)))]);

        let found = run(&env, &asker);

        assert_eq!(found.installs.len(), 1, "{:?}", found.installs);
        assert_eq!(found.installs[0].path_position, Some(1));
        assert_eq!(asker.calls.lock().unwrap().len(), 1, "one spawn per file");
    }

    #[test]
    fn two_path_spellings_of_one_folder_are_one_install_and_one_question() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        put(&dir, "devp", b"binary");
        let mut spelled_twice = dir.to_string_lossy().into_owned();
        spelled_twice.push(std::path::MAIN_SEPARATOR);
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut env = env_with(&tmp.path().join("nohome"), &[]);
        env.path_var = format!("{}{sep}{}", dir.display(), spelled_twice);
        let asker = Asker::new(&[(b"binary", Some((1, 0, 0)))]);

        let found = run(&env, &asker);

        assert_eq!(found.installs.len(), 1);
        assert_eq!(asker.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_binary_in_a_known_place_off_path_is_listed_with_on_path_false() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        put(&home.join(".cargo").join("bin"), "devp", b"cargo copy");
        let env = env_with(&home, &[]);
        let asker = Asker::new(&[(b"cargo copy", Some((1, 22, 0)))]);

        let found = run(&env, &asker);

        assert_eq!(found.installs.len(), 1);
        let install = &found.installs[0];
        assert!(!install.on_path());
        assert_eq!(install.channel, Channel::Cargo);
        assert_eq!(install.version, Some((1, 22, 0)));
        assert!(!install.roles.shell, "nothing off PATH answers a shell");
    }

    #[test]
    fn a_home_bin_folder_is_searched() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        put(&home.join("bin"), "devp", b"hand copy");
        let found = run(&env_with(&home, &[]), &Asker::new(&[]));
        assert_eq!(found.installs.len(), 1);
        assert_eq!(found.installs[0].channel, Channel::Unknown);
    }

    #[test]
    fn a_copy_that_cannot_state_its_version_is_listed_not_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        put(&dir, "devp", b"not a real devp");
        let env = env_with(&tmp.path().join("nohome"), &[&dir]);

        let found = run(&env, &Asker::new(&[(b"not a real devp", None)]));

        assert_eq!(found.installs.len(), 1);
        assert_eq!(found.installs[0].version, None);
    }

    #[test]
    fn the_installer_folder_is_recognised_even_when_path_spells_it_differently() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("cfg").join("bin");
        put(&managed, "dev-prune", b"installer copy");
        let mut env = env_with(&tmp.path().join("nohome"), &[]);
        env.managed_dir = Some(managed.clone());
        let mut with_slash = managed.to_string_lossy().into_owned();
        with_slash.push(std::path::MAIN_SEPARATOR);
        env.path_var = with_slash;

        let found = run(&env, &Asker::new(&[]));

        assert_eq!(found.installs.len(), 1);
        assert_eq!(found.installs[0].channel, Channel::Installer);
        assert!(found.installs[0].on_path());
    }

    #[test]
    fn each_channel_folder_yields_its_channel() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let scripts = if cfg!(windows) { "Scripts" } else { "bin" };
        let cases: Vec<(PathBuf, Channel)> = vec![
            (home.join(".cargo").join("bin"), Channel::Cargo),
            (
                home.join(".local")
                    .join("share")
                    .join("uv")
                    .join("tools")
                    .join("dev-prune")
                    .join(scripts),
                Channel::UvTool,
            ),
            (
                home.join(".local")
                    .join("pipx")
                    .join("venvs")
                    .join("dev-prune")
                    .join(scripts),
                Channel::Pipx,
            ),
            (home.join(".bun").join("bin"), Channel::Bun),
        ];
        for (dir, _) in &cases {
            put(dir, "dev-prune", dir.to_string_lossy().as_bytes());
        }
        let found = run(&env_with(&home, &[]), &Asker::new(&[]));
        for (dir, channel) in &cases {
            let install = found
                .installs
                .iter()
                .find(|i| i.path.starts_with(dir))
                .unwrap_or_else(|| panic!("{} not found", dir.display()));
            assert_eq!(install.channel, *channel, "{}", dir.display());
        }
    }

    #[test]
    fn the_roles_land_on_different_installs_when_they_point_at_different_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let first = put(&tmp.path().join("first"), "devp", b"first");
        let second = put(&tmp.path().join("second"), "devp", b"second");
        let home = tmp.path().join("home");
        let third = put(&home.join(".cargo").join("bin"), "devp", b"third");
        let mut env = env_with(&home, &[first.parent().unwrap(), second.parent().unwrap()]);
        env.running = Some(second.clone());
        env.running_version = Some((1, 24, 0));
        env.scheduler = Some(third.clone());
        env.hook = Some(first.clone());
        let asker = Asker::new(&[
            (b"first", Some((1, 20, 0))),
            (b"second", Some((1, 24, 0))),
            (b"third", Some((1, 22, 0))),
        ]);

        let found = run(&env, &asker);

        let by = |p: &Path| found.installs.iter().find(|i| i.path == p).unwrap();
        assert!(by(&first).roles.shell && by(&first).roles.hook);
        assert!(!by(&first).roles.running && !by(&first).roles.scheduler);
        assert!(by(&second).roles.running && !by(&second).roles.shell);
        assert!(by(&third).roles.scheduler && !by(&third).roles.shell);
        assert!(!by(&third).on_path());
        // The running binary states its version for itself and is never spawned.
        assert!(
            !asker.calls.lock().unwrap().contains(&second),
            "the running binary must not be asked"
        );
        assert_eq!(by(&second).version, Some((1, 24, 0)));
    }

    #[test]
    fn the_running_binary_is_listed_even_in_a_folder_nothing_searches() {
        let tmp = tempfile::tempdir().unwrap();
        let dev = put(
            &tmp.path().join("target").join("debug"),
            "devp",
            b"dev build",
        );
        let mut env = env_with(&tmp.path().join("nohome"), &[]);
        env.running = Some(dev.clone());
        env.running_version = Some((1, 25, 0));

        let found = run(&env, &Asker::new(&[]));

        assert_eq!(found.installs.len(), 1);
        assert!(found.installs[0].roles.running);
        assert_eq!(found.installs[0].version, Some((1, 25, 0)));
    }

    #[test]
    fn a_scheduler_target_beside_an_install_belongs_to_that_install() {
        // The scheduler may run `devpw.exe`, which sits next to the console binary and
        // is not an install of its own.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        let devp = put(&dir, "devp", b"binary");
        let mut env = env_with(&tmp.path().join("nohome"), &[&dir]);
        env.scheduler = Some(dir.join(exe("devpw")));

        let found = run(&env, &Asker::new(&[]));

        assert_eq!(found.installs.len(), 1);
        assert_eq!(found.installs[0].path, devp);
        assert!(found.installs[0].roles.scheduler);
    }

    #[test]
    #[cfg(windows)]
    fn a_lone_windowless_twin_is_listed_and_never_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(constants::WINDOWS_WINDOWLESS_BIN), b"gui").unwrap();
        let env = env_with(&tmp.path().join("nohome"), &[&dir]);
        let asker = Asker::new(&[]);

        let found = run(&env, &asker);

        assert_eq!(found.installs.len(), 1);
        assert!(found.installs[0].windowless_only);
        assert_eq!(found.installs[0].version, None);
        assert!(asker.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn an_environment_with_no_devp_anywhere_finds_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_with(&tmp.path().join("nohome"), &[tmp.path()]);
        assert!(run(&env, &Asker::new(&[])).installs.is_empty());
    }
}
