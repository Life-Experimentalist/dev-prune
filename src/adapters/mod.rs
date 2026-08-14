// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Package manager adapter trait and registry.
//
// This module defines the [`PackageManager`] trait that all ecosystem adapters
// must implement. It also provides helper functions for adapter detection and
// directory size calculation.
//
// ## Adding a New Adapter
//
// 1. Create a new file in `src/adapters/` (e.g., `maven.rs`)
// 2. Implement the [`PackageManager`] trait
// 3. Register it in [`get_all_adapters()`]
// 4. Add tests
//
// See [../../docs/ADDING_ADAPTERS.md] for a detailed guide.

pub mod bun;
pub mod cargo_adapter;
pub mod go;
pub mod npm;
pub mod pnpm;
pub mod uv;
pub mod venv;
pub mod yarn;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context as _, Result};
use walkdir::WalkDir;

/// Information about a bloat directory that can be pruned.
#[derive(Debug, Clone)]
pub struct BloatDir {
    /// Human-readable name (e.g., "node_modules").
    pub name: String,
    /// Full path to the bloat directory.
    pub path: PathBuf,
    /// Bytes that deleting this directory actually gives back to the disk.
    pub size_bytes: u64,
    /// Bytes reachable through hardlinks from outside this directory — pnpm's and
    /// bun's store links. Deleting the directory does not free these; the store
    /// keeps them. Zero for managers that copy instead of link.
    pub shared_bytes: u64,
}

impl fmt::Display for BloatDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.path.display())
    }
}

/// Packages sitting in a manager's environment directory that its lockfile does not
/// record — installs a post-prune restore would not bring back.
#[derive(Debug, Clone)]
pub struct DriftReport {
    /// The environment directory that drifted (e.g. `.venv`, `node_modules`).
    pub directory: String,
    /// The unrecorded package names, sorted.
    pub unrecorded: Vec<String>,
    /// The command that writes them into the lockfile.
    pub record_command: &'static str,
}

/// The core trait that every package manager adapter must implement.
///
/// Each adapter is responsible for:
/// - **Detecting** whether it applies to a given project directory
/// - **Listing** the bloat directories it manages
/// - **Enforcing** lockfile consistency before deletion
/// - **Restoring** dependencies from lockfiles
pub trait PackageManager: Send + Sync {
    /// Human-readable name for this adapter (e.g., "npm", "pnpm", "uv").
    fn name(&self) -> &'static str;

    /// Check if this adapter applies to the given project directory.
    ///
    /// Typically checks for the presence of a specific lockfile or config file.
    fn detect(&self, project_path: &Path) -> bool;

    /// List all bloat directories this adapter manages in the given project.
    ///
    /// Only returns directories that actually exist on disk.
    fn bloat_dirs(&self, project_path: &Path) -> Vec<BloatDir>;

    /// Prove the lockfile can rebuild what is about to be deleted.
    ///
    /// This is a **safety-critical** method. It MUST succeed before any bloat
    /// directory is deleted. If this fails, deletion for this adapter is aborted.
    ///
    /// See [`EnforcePolicy`] for the one rule every adapter follows.
    fn enforce_lockfile(&self, project_path: &Path, policy: EnforcePolicy) -> Result<()>;

    /// Restore dependencies from the lockfile (for `dev-prune restore`).
    ///
    /// `timeout` is threaded explicitly for the same reason [`EnforcePolicy`] is: the
    /// restore path used to burn the compiled-in default regardless of
    /// `command_timeout_secs`, and a full `npm ci` on a large tree needs the raised
    /// timeout far more often than a verify does.
    fn restore(&self, project_path: &Path, timeout: std::time::Duration) -> Result<()>;

    /// [`PackageManager::restore`], told the name the pruned directory had.
    ///
    /// Most managers have exactly one possible directory name and ignore this. venv does
    /// not: it prunes any folder carrying a `pyvenv.cfg` — `venv`, `env`, `my_env` — and
    /// without the recorded name it would rebuild the environment as `.venv`, leaving
    /// every activate script, IDE interpreter path and Makefile pointing at nothing.
    fn restore_named(
        &self,
        project_path: &Path,
        dir_name: &str,
        timeout: std::time::Duration,
    ) -> Result<()> {
        let _ = dir_name;
        self.restore(project_path, timeout)
    }

    /// The file this manager rebuilds its bloat directory from.
    ///
    /// Two callers. Conflict resolution breaks ties between managers that share a bloat
    /// directory — npm, pnpm, yarn and bun all own the same `node_modules` — by comparing
    /// these files' timestamps. `devp doctor` names them, because a missing one is the
    /// most common reason a project is not pruneable.
    ///
    /// More than one entry means the manager accepts any of them (bun's binary and text
    /// lockfiles). An empty slice means the manager has no single file to point at.
    fn lockfiles(&self) -> &'static [&'static str] {
        &[]
    }

    /// Installed-but-unrecorded packages, as data instead of a refusal.
    ///
    /// The same comparison [`PackageManager::enforce_lockfile`] refuses a prune on,
    /// surfaced early so `devp status --drift` can point at the problem before a prune
    /// is ever attempted. Runs nothing and writes nothing. An empty answer means
    /// "nothing detected", not "proven clean" — most managers have no cheap way to
    /// compare and say nothing here.
    fn drift(&self, project_path: &Path) -> Vec<DriftReport> {
        let _ = project_path;
        Vec::new()
    }
}

/// Adapters that all manage `node_modules` and therefore cannot coexist.
const JS_MANAGERS: [&str; 4] = ["npm", "pnpm", "yarn", "bun"];

/// Bookkeeping files that each JavaScript manager writes into `node_modules` when it
/// installs. Finding one identifies the manager that actually produced the tree on
/// disk, which is stronger evidence than a lockfile's timestamp.
///
/// pnpm and yarn are checked before npm: a project migrated away from npm can still
/// carry npm's `.package-lock.json` inside a tree the new manager rebuilt around it.
/// Bun has no marker we rely on, so a bun conflict falls through to the later rules.
const JS_INSTALL_MARKERS: [(&str, &[&str]); 3] = [
    ("pnpm", &[".pnpm", ".modules.yaml"]),
    ("yarn", &[".yarn-state.yml", ".yarn-integrity"]),
    ("npm", &[".package-lock.json"]),
];

/// Returns all registered package manager adapters.
///
/// To add a new adapter, create your struct and add it to this list.
pub fn get_all_adapters() -> Vec<Box<dyn PackageManager>> {
    vec![
        Box::new(npm::Npm),
        Box::new(pnpm::Pnpm),
        Box::new(yarn::Yarn),
        Box::new(bun::Bun),
        Box::new(uv::Uv),
        Box::new(venv::Venv),
        Box::new(cargo_adapter::Cargo),
        Box::new(go::Go),
    ]
}

/// Detect which adapters apply to a given project directory.
///
/// Several adapters detecting at once is normal and supported — a directory holding
/// `package-lock.json`, `uv.lock` and `Cargo.toml` legitimately has three managers,
/// each owning a different bloat directory. Adapters that would fight over the *same*
/// directory are reduced to one first; see [`resolve_conflicts`].
pub fn detect_adapters(project_path: &Path) -> Vec<Box<dyn PackageManager>> {
    let mut detected: Vec<Box<dyn PackageManager>> = get_all_adapters()
        .into_iter()
        .filter(|adapter| adapter.detect(project_path))
        .collect();
    resolve_conflicts(project_path, &mut detected);
    detected
}

/// Reduce every set of adapters that shares a bloat directory down to a single owner.
fn resolve_conflicts(project_path: &Path, detected: &mut Vec<Box<dyn PackageManager>>) {
    resolve_js_conflict(project_path, detected);
    resolve_python_conflict(detected);
}

/// Reduce several JavaScript managers claiming the same `node_modules` down to one.
///
/// A directory carrying more than one JS lockfile is usually a half-finished migration
/// or a stray file nobody deleted. Running the wrong manager's `enforce_lockfile` would
/// rewrite a lockfile the project does not use, so pick deliberately, strongest signal
/// first:
///
/// 1. The `packageManager` field of `package.json` — the maintainers said so outright.
/// 2. The bookkeeping files inside `node_modules` — whoever built the tree we are about
///    to delete is the manager whose lockfile has to be able to rebuild it.
/// 3. The most recently written lockfile — a last resort when nothing else distinguishes
///    them.
fn resolve_js_conflict(project_path: &Path, detected: &mut Vec<Box<dyn PackageManager>>) {
    if detected
        .iter()
        .filter(|a| JS_MANAGERS.contains(&a.name()))
        .count()
        < 2
    {
        return;
    }

    let winner = declared_package_manager(project_path)
        .filter(|name| detected.iter().any(|a| a.name() == name))
        .or_else(|| installed_manager(project_path, detected))
        .or_else(|| newest_lockfile_owner(project_path, detected));

    let Some(winner) = winner else { return };
    detected.retain(|a| !JS_MANAGERS.contains(&a.name()) || a.name() == winner);
}

/// Give uv sole ownership of the Python environment whenever it applies.
///
/// uv and the plain-venv adapter both point at the same virtual environment directory.
/// uv is the more capable of the two — it has a real lockfile and can rebuild the
/// environment exactly — so it takes the project whenever it recognises one, and the
/// `requirements.txt` + `pyvenv.cfg` adapter picks up everything else.
fn resolve_python_conflict(detected: &mut Vec<Box<dyn PackageManager>>) {
    if detected.iter().any(|a| a.name() == "uv") {
        detected.retain(|a| a.name() != "venv");
    }
}

/// The manager that actually installed the `node_modules` tree currently on disk.
fn installed_manager(project_path: &Path, detected: &[Box<dyn PackageManager>]) -> Option<String> {
    let node_modules = project_path.join("node_modules");
    if !node_modules.is_dir() {
        return None;
    }

    JS_INSTALL_MARKERS
        .iter()
        .find(|(name, markers)| {
            detected.iter().any(|a| a.name() == *name)
                && markers.iter().any(|m| node_modules.join(m).exists())
        })
        .map(|(name, _)| (*name).to_string())
}

/// Read the Corepack `packageManager` field (e.g. `"pnpm@9.1.0"`) from `package.json`.
fn declared_package_manager(project_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(project_path.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let declared = json.get("packageManager")?.as_str()?;
    let name = declared.split('@').next().unwrap_or_default();
    JS_MANAGERS
        .iter()
        .find(|m| **m == name)
        .map(|m| (*m).to_string())
}

/// The detected JS manager whose lockfile has the most recent modification time.
fn newest_lockfile_owner(
    project_path: &Path,
    detected: &[Box<dyn PackageManager>],
) -> Option<String> {
    detected
        .iter()
        .filter(|a| JS_MANAGERS.contains(&a.name()))
        .filter_map(|a| {
            let newest = a
                .lockfiles()
                .iter()
                .filter_map(|f| std::fs::metadata(project_path.join(f)).ok()?.modified().ok())
                .max()?;
            Some((newest, a.name().to_string()))
        })
        // Ties keep the earlier adapter in `get_all_adapters()` order, so the choice is
        // deterministic when two lockfiles share a timestamp.
        .fold(None::<(std::time::SystemTime, String)>, |best, cur| {
            match best {
                Some(b) if b.0 >= cur.0 => Some(b),
                _ => Some(cur),
            }
        })
        .map(|(_, name)| name)
}

/// Calculate the total size of a directory recursively (in bytes).
pub fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

/// A directory's size split by what deleting it would actually free.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirSizeBreakdown {
    /// Bytes `remove_dir_all` gives back to the disk.
    pub freed_bytes: u64,
    /// Bytes that survive the deletion because a hardlink outside the directory —
    /// for pnpm and bun, the global store — still points at them.
    pub shared_bytes: u64,
}

/// [`dir_size`], but hardlink-aware.
///
/// pnpm and bun do not copy packages into `node_modules`; they hardlink them from a
/// machine-wide store, so summing file sizes counts bytes the store keeps after the
/// delete and promises space a prune cannot deliver. Here a physical file is counted
/// once no matter how many names it has inside the tree, and counts as freed only
/// when every one of its links is inside the tree. A store that fell back to copying
/// — a different volume, a filesystem without hardlinks — leaves the link count at
/// one, so copied installs still count in full. A file whose link count cannot be
/// read is counted as freed, which errs toward the plain [`dir_size`] figure.
pub fn dir_size_with_hardlinks(path: &Path) -> DirSizeBreakdown {
    let mut out = DirSizeBreakdown::default();
    if !path.exists() {
        return out;
    }
    // (volume, file id) → (bytes, links on disk, links seen inside this walk)
    let mut linked: HashMap<(u64, u64), (u64, u64, u64)> = HashMap::new();
    for entry in WalkDir::new(path).follow_links(false).into_iter().flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        match file_link_identity(entry.path(), &meta) {
            Some((dev, ino, nlink)) if nlink > 1 => {
                linked.entry((dev, ino)).or_insert((meta.len(), nlink, 0)).2 += 1;
            }
            _ => out.freed_bytes += meta.len(),
        }
    }
    for (bytes, nlink, seen) in linked.into_values() {
        if seen >= nlink {
            out.freed_bytes += bytes;
        } else {
            out.shared_bytes += bytes;
        }
    }
    out
}

/// (volume, file id, hardlink count) for one file, where the platform can say.
#[cfg(unix)]
fn file_link_identity(_path: &Path, meta: &std::fs::Metadata) -> Option<(u64, u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    Some((meta.dev(), meta.ino(), meta.nlink()))
}

/// Windows keeps the link count behind an opened handle, not in the directory entry
/// (std exposes it only on an unstable feature), so this costs one metadata-only open
/// per file. Only the adapters that actually hardlink — pnpm and bun — pay it.
#[cfg(windows)]
fn file_link_identity(path: &Path, _meta: &std::fs::Metadata) -> Option<(u64, u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    // access_mode(0) asks for attribute access only, so a file another process holds
    // open without read sharing — an antivirus scan, an editor — does not fail here.
    let file = std::fs::OpenOptions::new().access_mode(0).open(path).ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `file` keeps the handle open for the whole call, and `info` is a
    // plain-data out-parameter the API fills before returning.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return None;
    }
    Some((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        u64::from(info.nNumberOfLinks),
    ))
}

#[cfg(not(any(unix, windows)))]
fn file_link_identity(_path: &Path, _meta: &std::fs::Metadata) -> Option<(u64, u64, u64)> {
    None
}

/// Resolve a program name into something `Command::new` can actually spawn.
///
/// On Windows the JS package managers (`npm`, `pnpm`, `yarn`, `bun`) are shipped as
/// `.cmd` shims. `CreateProcess` only ever appends `.exe`, so `Command::new("npm")`
/// fails with `NotFound` even when npm is installed and on `PATH`. Search `PATH`
/// ourselves for the shim extensions and hand back the full path.
///
/// Names that already contain a path separator (e.g. `.venv\Scripts\python.exe`)
/// are returned unchanged, as are all names on non-Windows platforms.
pub fn resolve_program(program: &str) -> String {
    #[cfg(windows)]
    {
        if Path::new(program).components().count() > 1 {
            return program.to_string();
        }
        let Some(path_var) = std::env::var_os("PATH") else {
            return program.to_string();
        };
        for dir in std::env::split_paths(&path_var) {
            for ext in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{program}.{ext}"));
                if candidate.is_file() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
    }
    program.to_string()
}

/// Check whether a package manager binary is present and runnable.
///
/// Answers are cached for the life of the process. Every adapter asks this before it
/// enforces a lockfile, so a monorepo with ten projects on the same manager otherwise
/// pays for ten `npm --version` process spawns — around half a second each on Windows —
/// to learn the same fact ten times. A run is short-lived, so nothing installed or
/// removed mid-run can be missed for long.
pub fn binary_available(program: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    // Held across the probe on purpose: two threads asking about the same missing
    // binary should spawn one process, not two. Nothing else takes this lock.
    let mut guard = match cache.lock() {
        Ok(g) => g,
        // A poisoned lock only means some other thread panicked mid-probe; the answer
        // is still worth having, so fall back to probing without the cache.
        Err(_) => return probe_binary(program),
    };
    if let Some(known) = guard.get(program) {
        return *known;
    }
    let available = probe_binary(program);
    guard.insert(program.to_string(), available);
    available
}

/// The actual `<program> --version` spawn behind [`binary_available`].
fn probe_binary(program: &str) -> bool {
    std::process::Command::new(resolve_program(program))
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The exit status and drained pipes of a finished command.
struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

/// Spawn a command, drain both of its pipes and wait for it, bounded by `timeout`.
///
/// Shared by the two public wrappers below. `devp caches` needs a command's *output* —
/// `npm config get cache` answers a question rather than performing an action — and a
/// second copy of the draining and polling below would be a second place for the
/// deadlock it exists to prevent to come back.
fn spawn_capture(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: std::time::Duration,
) -> Result<CommandOutput> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Instant;

    let resolved = resolve_program(program);
    let mut child = Command::new(&resolved)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to execute: {program} {}", args.join(" ")))?;

    // Drain both pipes on their own threads. A package manager easily emits more
    // than the ~64 KiB OS pipe buffer; if nobody reads it the child blocks on write
    // and never exits, which would turn every large install into a timeout kill.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!(
                        "Command timed out after {}s: {} {}\n\
                         To increase the timeout, run: `devp config set command_timeout_secs <seconds>`",
                        timeout.as_secs(),
                        program,
                        args.join(" ")
                    );
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    };

    let stderr = stderr_reader
        .join()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let stdout = stdout_reader
        .join()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();

    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

/// Helper: run a command with a configurable timeout.
pub fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: std::time::Duration,
) -> Result<()> {
    let out = spawn_capture(program, args, cwd, timeout)?;
    if out.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} {} failed (exit code {:?}):\n{}",
            program,
            args.join(" "),
            out.status.code(),
            out.stderr.trim()
        )
    }
}

/// Run a command and hand back what it printed on stdout, bounded by `timeout`.
///
/// For commands that answer a question instead of doing work. A non-zero exit is an
/// error like anywhere else, so a caller never mistakes an error message on stderr for
/// the answer it asked for.
pub fn capture_command_with_timeout(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: std::time::Duration,
) -> Result<String> {
    let out = spawn_capture(program, args, cwd, timeout)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        anyhow::bail!(
            "{} {} failed (exit code {:?}):\n{}",
            program,
            args.join(" "),
            out.status.code(),
            out.stderr.trim()
        )
    }
}

/// Helper: attempt a command but return `true`/`false` instead of `Err`.
pub fn try_run_command(program: &str, args: &[&str], cwd: &Path) -> bool {
    std::process::Command::new(resolve_program(program))
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// How much newer the manifest must be than the lockfile before
/// [`refuse_if_manifest_newer`] calls it drift.
///
/// A clone or checkout writes both files within moments of each other, in whichever
/// order the tree walk happens to visit them — a strict comparison would refuse half of
/// all fresh clones. A hand edit that never got a lockfile sync is separated by minutes
/// or days, which a minute of tolerance still catches.
const MANIFEST_MTIME_TOLERANCE: std::time::Duration = std::time::Duration::from_secs(60);

/// When the package manager is missing, a lockfile is only proof if the manifest has
/// not been edited since it was written.
///
/// With the binary present the verify command answers this properly; without it, mtimes
/// are the only signal there is. The manifest is inferred from the lockfile's file
/// name; an unrecognised name changes nothing.
fn refuse_if_manifest_newer(lockfile: &Path, program: &str, cwd: &Path) -> Result<()> {
    let manifest_name = match lockfile.file_name().and_then(|n| n.to_str()) {
        Some("Cargo.lock") => "Cargo.toml",
        Some("package-lock.json")
        | Some("yarn.lock")
        | Some("pnpm-lock.yaml")
        | Some("bun.lockb")
        | Some("bun.lock") => "package.json",
        Some("uv.lock") | Some("poetry.lock") | Some("pdm.lock") => "pyproject.toml",
        Some("go.sum") => "go.mod",
        Some("composer.lock") => "composer.json",
        _ => return Ok(()),
    };
    let manifest = cwd.join(manifest_name);
    let (Ok(manifest_meta), Ok(lock_meta)) =
        (std::fs::metadata(&manifest), std::fs::metadata(lockfile))
    else {
        return Ok(());
    };
    if let (Ok(manifest_mtime), Ok(lock_mtime)) = (manifest_meta.modified(), lock_meta.modified()) {
        if manifest_mtime > lock_mtime + MANIFEST_MTIME_TOLERANCE {
            anyhow::bail!(
                "`{program}` is not available, and `{manifest_name}` has been edited more \
                 recently than `{}` — the lockfile may no longer record the current \
                 dependencies, and without `{program}` that cannot be verified. Install \
                 {program} and run its lockfile sync, then prune again.",
                lockfile.display()
            );
        }
    }
    Ok(())
}

/// Two-tier lockfile enforcement with configurable timeout.
pub fn lock_sync_or_verify_with_timeout(
    lockfile: &Path,
    program: &str,
    sync_args: &[&str],
    cwd: &Path,
    timeout: std::time::Duration,
) -> Result<()> {
    let lockfile_exists = lockfile.exists();

    if !binary_available(program) {
        if lockfile_exists {
            refuse_if_manifest_newer(lockfile, program, cwd)?;
            return Ok(());
        } else {
            anyhow::bail!(
                "`{program}` is not available and no lockfile was found at `{}`. \
                 Cannot safely delete dependencies — install {program} first, \
                 or commit a lockfile.",
                lockfile.display()
            );
        }
    }

    // Binary is available — run the sync with timeout.
    run_command_with_timeout(program, sync_args, cwd, timeout)
}

/// What an adapter is allowed to do while enforcing a lockfile on this pass.
///
/// The two things that used to be hardcoded per adapter, and were wrong in both places:
/// every adapter burned the compiled-in timeout regardless of `command_timeout_secs`,
/// and only cargo and go consulted `allow_manifest_rewrite`.
#[derive(Debug, Clone, Copy)]
pub struct EnforcePolicy {
    /// Whether a sync command that writes files Git tracks may run anyway.
    ///
    /// The user's `allow_manifest_rewrite`. Off by default: a prune pass can come from
    /// the scheduler, and a background process that leaves a dirty working tree behind
    /// is a surprise no matter which file it wrote.
    pub allow_rewrite: bool,
    /// Ceiling on any one package-manager command — the user's `command_timeout_secs`.
    pub timeout: std::time::Duration,
}

impl Default for EnforcePolicy {
    fn default() -> Self {
        Self {
            allow_rewrite: crate::constants::DEFAULT_ALLOW_MANIFEST_REWRITE,
            timeout: std::time::Duration::from_secs(crate::constants::DEFAULT_COMMAND_TIMEOUT_SECS),
        }
    }
}

impl EnforcePolicy {
    /// A policy from the user's own settings.
    pub fn from_settings(settings: &crate::config::Settings) -> Self {
        Self {
            allow_rewrite: settings.allow_manifest_rewrite,
            timeout: std::time::Duration::from_secs(settings.command_timeout_secs),
        }
    }
}

/// The one rule every adapter enforces, given the manager's two spellings of the check.
///
/// - lockfile present → `verify_args`, which resolves the graph against the lockfile and
///   **fails** rather than writing when the two have drifted apart
/// - lockfile absent → `write_args`, because `restore` needs a lockfile to exist at all
///   and there is nothing there to preserve
/// - `allow_rewrite` → `write_args` either way: the informed opt-in, for the user who
///   would rather have a stale lockfile refreshed than have the prune refused
///
/// This used to be the cargo/go rule only. Every other adapter ran its writing sync
/// unconditionally — `npm install --package-lock-only`, `pnpm install --lockfile-only`,
/// `uv lock` and `yarn install --mode update-lockfile` all rewrite a lockfile Git tracks
/// when it has drifted from the manifest. That is a smaller edit than `go mod tidy`
/// makes, but it is still an unattended pass modifying a tracked file, and it made
/// `allow_manifest_rewrite` mean two different things depending on the ecosystem.
pub fn enforce_two_tier(
    lockfile: &Path,
    program: &str,
    verify_args: &[&str],
    write_args: &[&str],
    cwd: &Path,
    policy: EnforcePolicy,
) -> Result<()> {
    if policy.allow_rewrite {
        return lock_sync_or_verify_with_timeout(
            lockfile,
            program,
            write_args,
            cwd,
            policy.timeout,
        );
    }
    lock_verify_or_generate(
        lockfile,
        program,
        verify_args,
        write_args,
        cwd,
        policy.timeout,
    )
}

/// Lockfile enforcement for ecosystems whose "sync" command rewrites source manifests.
///
/// `cargo generate-lockfile` re-resolves every dependency and overwrites `Cargo.lock`;
/// `go mod tidy` edits both `go.mod` and `go.sum` and can drop requirements. Running
/// either as a precondition for deletion would silently modify tracked source files,
/// which contradicts the lockfile-safety guarantee. So:
///
/// - lockfile present → run the read-only `verify_args` (never writes)
/// - lockfile absent  → run `generate_args`, since a lockfile must exist for `restore`
pub fn lock_verify_or_generate(
    lockfile: &Path,
    program: &str,
    verify_args: &[&str],
    generate_args: &[&str],
    cwd: &Path,
    timeout: std::time::Duration,
) -> Result<()> {
    let lockfile_exists = lockfile.exists();

    if !binary_available(program) {
        if lockfile_exists {
            refuse_if_manifest_newer(lockfile, program, cwd)?;
            return Ok(());
        }
        anyhow::bail!(
            "`{program}` is not available and no lockfile was found at `{}`. \
             Cannot safely delete dependencies — install {program} first, \
             or commit a lockfile.",
            lockfile.display()
        );
    }

    if lockfile_exists {
        run_command_with_timeout(program, verify_args, cwd, timeout)
    } else {
        run_command_with_timeout(program, generate_args, cwd, timeout)
    }
}

/// Two-tier lockfile enforcement using default timeout.
pub fn lock_sync_or_verify(
    lockfile: &Path,
    program: &str,
    sync_args: &[&str],
    cwd: &Path,
) -> Result<()> {
    lock_sync_or_verify_with_timeout(
        lockfile,
        program,
        sync_args,
        cwd,
        std::time::Duration::from_secs(crate::constants::DEFAULT_COMMAND_TIMEOUT_SECS),
    )
}

/// Information describing status of a required package manager binary.
#[derive(Debug, Clone)]
pub struct BinaryCheckStatus {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
}

/// Scan only the package manager binaries needed by candidate repos.
pub fn scan_required_binaries(adapter_names: &[String]) -> Vec<BinaryCheckStatus> {
    let mut unique: Vec<String> = adapter_names
        .iter()
        .filter(|&n| n != "-" && n != "venv")
        .cloned()
        .collect();
    unique.sort();
    unique.dedup();

    unique
        .into_iter()
        .map(|name| {
            let output = std::process::Command::new(resolve_program(&name))
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let first_line = ver.lines().next().unwrap_or(&ver).to_string();
                    BinaryCheckStatus {
                        name,
                        available: true,
                        version: if first_line.is_empty() {
                            None
                        } else {
                            Some(first_line)
                        },
                    }
                }
                _ => BinaryCheckStatus {
                    name,
                    available: false,
                    version: None,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_bloat_dir_display() {
        let bd = BloatDir {
            name: "node_modules".to_string(),
            path: PathBuf::from("/test/node_modules"),
            size_bytes: 1024,
            shared_bytes: 0,
        };
        assert!(bd.to_string().contains("node_modules"));
    }

    #[test]
    fn test_hardlink_size_counts_a_plain_file_in_full() {
        let tmp = TempDir::new().unwrap();
        let tree = tmp.path().join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("copied.txt"), "12345").unwrap();
        let size = dir_size_with_hardlinks(&tree);
        assert_eq!(size.freed_bytes, 5);
        assert_eq!(size.shared_bytes, 0);
    }

    #[test]
    fn test_hardlink_size_excludes_a_file_the_store_keeps() {
        // The pnpm shape: the store's copy lives outside the tree being deleted, so
        // deleting the tree frees nothing for this file.
        let tmp = TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let tree = tmp.path().join("tree");
        fs::create_dir(&store).unwrap();
        fs::create_dir(&tree).unwrap();
        fs::write(store.join("pkg.js"), "0123456789").unwrap();
        fs::hard_link(store.join("pkg.js"), tree.join("pkg.js")).unwrap();
        let size = dir_size_with_hardlinks(&tree);
        assert_eq!(size.freed_bytes, 0);
        assert_eq!(size.shared_bytes, 10);
    }

    #[test]
    fn test_hardlink_size_counts_an_internal_pair_once() {
        // Both names live inside the tree, so the delete removes the last link and
        // the bytes really are freed — but only once, not per name.
        let tmp = TempDir::new().unwrap();
        let tree = tmp.path().join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("a.js"), "abcdefg").unwrap();
        fs::hard_link(tree.join("a.js"), tree.join("b.js")).unwrap();
        let size = dir_size_with_hardlinks(&tree);
        assert_eq!(size.freed_bytes, 7);
        assert_eq!(size.shared_bytes, 0);
    }

    #[test]
    fn test_dir_size_empty() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(dir_size(tmp.path()), 0);
    }

    #[test]
    fn test_dir_size_with_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file1.txt"), "hello").unwrap();
        fs::write(tmp.path().join("file2.txt"), "world!").unwrap();
        assert_eq!(dir_size(tmp.path()), 11); // 5 + 6
    }

    #[test]
    fn test_dir_size_nonexistent() {
        assert_eq!(dir_size(Path::new("/nonexistent/path")), 0);
    }

    #[test]
    fn test_get_all_adapters_not_empty() {
        let adapters = get_all_adapters();
        assert!(adapters.len() >= 6);
    }

    #[test]
    fn test_detect_adapters_npm() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        let adapters = detect_adapters(tmp.path());
        let names: Vec<&str> = adapters.iter().map(|a| a.name()).collect();
        assert!(names.contains(&"npm"));
    }

    #[test]
    fn test_detect_adapters_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let adapters = detect_adapters(tmp.path());
        assert!(adapters.is_empty());
    }

    /// Names of the adapters that detect in `dir`, sorted.
    fn detected_names(dir: &Path) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = detect_adapters(dir).iter().map(|a| a.name()).collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn test_detect_adapters_multiple_ecosystems_coexist() {
        // Different managers owning different directories must all survive detection.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        fs::write(tmp.path().join("uv.lock"), "").unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        fs::write(tmp.path().join("go.mod"), "module x").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["cargo", "go", "npm", "uv"]);
    }

    #[test]
    fn test_js_conflict_resolved_by_package_manager_field() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package.json"),
            r#"{"packageManager":"yarn@4.1.0"}"#,
        )
        .unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        fs::write(tmp.path().join("pnpm-lock.yaml"), "").unwrap();
        fs::write(tmp.path().join("yarn.lock"), "").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["yarn"]);
    }

    #[test]
    fn test_js_conflict_resolved_by_what_installed_node_modules() {
        // npm's lockfile is written last, but the tree on disk was built by pnpm — and
        // that tree is what is about to be deleted.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("pnpm-lock.yaml"), "").unwrap();
        fs::create_dir_all(tmp.path().join("node_modules/.pnpm")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["pnpm"]);
    }

    #[test]
    fn test_js_conflict_prefers_yarn_state_over_leftover_npm_bookkeeping() {
        // A repo migrated npm → yarn keeps npm's hidden lockfile inside node_modules.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        fs::write(tmp.path().join("yarn.lock"), "").unwrap();
        let nm = tmp.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join(".package-lock.json"), "{}").unwrap();
        fs::write(nm.join(".yarn-state.yml"), "").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["yarn"]);
    }

    #[test]
    fn test_declared_package_manager_outranks_what_is_installed() {
        // Corepack pins the project to pnpm; the npm tree on disk is the accident.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package.json"),
            r#"{"packageManager":"pnpm@9.1.0"}"#,
        )
        .unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        fs::write(tmp.path().join("pnpm-lock.yaml"), "").unwrap();
        let nm = tmp.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join(".package-lock.json"), "{}").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["pnpm"]);
    }

    #[test]
    fn test_uv_takes_precedence_over_plain_venv() {
        // A uv project declared through `[tool.uv]` alone, with a requirements.txt and a
        // virtual environment left over from before the migration.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("pyproject.toml"),
            "[project]\nname = \"x\"\n\n[tool.uv]\n",
        )
        .unwrap();
        fs::write(tmp.path().join("requirements.txt"), "requests\n").unwrap();
        let venv = tmp.path().join(".venv");
        fs::create_dir_all(&venv).unwrap();
        fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["uv"]);
    }

    #[test]
    fn test_plain_venv_handles_projects_uv_does_not_claim() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("requirements.txt"), "requests\n").unwrap();
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["venv"]);
    }

    #[test]
    fn test_js_conflict_falls_back_to_newest_lockfile() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        // Written second, and touched again, so pnpm is unambiguously the newer of the
        // two even on filesystems with coarse timestamp granularity.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(tmp.path().join("pnpm-lock.yaml"), "").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["pnpm"]);
    }

    #[test]
    fn test_js_conflict_ignores_an_unrecognised_package_manager_field() {
        // A `packageManager` naming something we have no adapter for must not wipe out
        // the detection entirely — fall through to the lockfile timestamps.
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package.json"),
            r#"{"packageManager":"deno@2.0.0"}"#,
        )
        .unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(tmp.path().join("yarn.lock"), "").unwrap();

        assert_eq!(detected_names(tmp.path()), vec!["yarn"]);
    }

    #[test]
    fn test_js_conflict_does_not_disturb_a_single_manager() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detected_names(tmp.path()), vec!["pnpm"]);
    }

    #[test]
    fn test_js_adapters_declare_their_lockfiles() {
        for adapter in get_all_adapters() {
            if JS_MANAGERS.contains(&adapter.name()) {
                assert!(
                    !adapter.lockfiles().is_empty(),
                    "{} shares node_modules and must declare its lockfiles for \
                     conflict resolution",
                    adapter.name()
                );
            }
        }
    }

    #[test]
    fn test_adapter_names_unique() {
        let adapters = get_all_adapters();
        let names: Vec<&str> = adapters.iter().map(|a| a.name()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "Adapter names must be unique");
    }
}
