// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Standard Python venv package manager adapter.
//
// Detects Python virtual environments by scanning the repo root for any
// directory containing a `pyvenv.cfg` file — the canonical marker for any
// Python virtual environment, regardless of what the folder is named
// (`.venv`, `venv`, `env`, `my_env`, `.env`, etc.).
//
// Priority: the `uv` adapter takes precedence when `uv.lock` is present.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, run_command_with_timeout};
use anyhow::{Result, anyhow};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The canonical file inside every Python virtual environment.
const PYVENV_CFG: &str = "pyvenv.cfg";

/// Lockfiles owned by other Python managers.
///
/// A poetry, pipenv or pdm project often carries an exported `requirements.txt` as well —
/// usually stale. Rebuilding the environment from that export instead of the real
/// lockfile would silently install the wrong versions, so those projects are never
/// claimed here: poetry has its own adapter, and pipenv/pdm are left to their own tools.
const FOREIGN_PYTHON_LOCKFILES: [&str; 3] = ["poetry.lock", "Pipfile.lock", "pdm.lock"];

/// Distributions present in effectively every virtual environment without ever being
/// listed in a requirements file.
pub(super) const BASELINE_DISTRIBUTIONS: [&str; 4] =
    ["pip", "setuptools", "wheel", "pkg-resources"];

/// Adapter for standard Python venv projects.
pub struct Venv;

/// Scan the repo root for directories containing `pyvenv.cfg`.
///
/// This catches any venv folder name: `.venv`, `venv`, `env`, `my_env`, etc.
fn find_venv_dirs(path: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();

    let Ok(entries) = fs::read_dir(path) else {
        return found;
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_dir() && entry_path.join(PYVENV_CFG).exists() {
            found.push(entry_path);
        }
    }

    found
}

/// Whether `pyproject.toml` declares a `[tool.poetry]` table.
///
/// A textual check rather than a TOML parse: the table header only ever appears at the
/// start of a line, and this adapter needs a yes/no, not the table's contents.
fn is_poetry_project(path: &Path) -> bool {
    fs::read_to_string(path.join("pyproject.toml"))
        .map(|c| {
            c.lines()
                .any(|l| l.trim_start().starts_with("[tool.poetry"))
        })
        .unwrap_or(false)
}

/// A package name as PEP 503 compares them: lowercased, with runs of `-`, `_` and `.`
/// collapsed to a single `-`, so `Foo_Bar` and `foo-bar` are the same package.
pub(super) fn normalize_package_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for c in name.chars() {
        if c == '-' || c == '_' || c == '.' {
            if !last_dash {
                out.push('-');
            }
            last_dash = true;
        } else {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        }
    }
    out
}

/// The leading package name of a PEP 508 requirement string, normalised.
///
/// `requests[socks]==2.32.3 ; python_version < "3.9"` → `requests`. Returns `None` for
/// anything that does not begin with a name — URLs, local paths — because pip is the
/// only thing that can know what those install.
fn requirement_name(spec: &str) -> Option<String> {
    let name: String = spec
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    // A name never starts with `.` — that spelling is a relative path (`./pkg`, `..`).
    if name.is_empty() || name.starts_with('.') || spec.contains("://") && !spec.contains(" @ ") {
        return None;
    }
    Some(normalize_package_name(&name))
}

/// Every package name a requirements file pins, following `-r`/`-c` includes.
///
/// `None` means the file cannot be fully accounted for without running pip — an editable
/// install, a bare URL or path, or an include that cannot be read. The caller skips the
/// drift comparison in that case rather than guessing in either direction.
pub(crate) fn requirement_names(
    file: &Path,
    visited: &mut Vec<PathBuf>,
) -> Option<HashSet<String>> {
    // The depth cap breaks include cycles that the exact-path check misses (e.g. the
    // same file reached through differently-spelled relative paths).
    if visited.len() >= 8 || visited.iter().any(|p| p == file) {
        return None;
    }
    visited.push(file.to_path_buf());

    let content = fs::read_to_string(file).ok()?;
    let dir = file.parent()?;
    let mut names = HashSet::new();

    for raw in content.lines() {
        // pip treats `#` as a comment at line start or after whitespace — never inside
        // a URL fragment like `#egg=name`.
        let mut line = raw.trim();
        if let Some(idx) = line.find(" #") {
            line = &line[..idx];
        }
        let line = line.trim_end_matches('\\').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(included) = line
            .strip_prefix("-r ")
            .or_else(|| line.strip_prefix("--requirement "))
            .or_else(|| line.strip_prefix("-c "))
            .or_else(|| line.strip_prefix("--constraint "))
        {
            names.extend(requirement_names(&dir.join(included.trim()), visited)?);
            continue;
        }

        if line.starts_with('-') {
            // An editable install's package name only pip can compute. Every other
            // option (`--index-url`, `--hash`, …) names no package at all.
            if line.starts_with("-e") || line.starts_with("--editable") {
                return None;
            }
            continue;
        }

        // `name @ url` is a direct reference whose name is on the left of the `@`.
        let spec = line.split(" @ ").next().unwrap_or(line).trim();
        names.insert(requirement_name(spec)?);
    }

    Some(names)
}

/// Installed distributions and their declared dependencies, read from the
/// `*.dist-info` directories of a virtual environment's `site-packages`.
///
/// The map's keys are the installed package names; the values are the names in each
/// package's `Requires-Dist` metadata. `None` when no `site-packages` directory could
/// be found at all — an exotic layout is not evidence of anything.
pub(super) fn installed_distributions(venv: &Path) -> Option<HashMap<String, Vec<String>>> {
    let mut site_packages: Vec<PathBuf> = Vec::new();
    let windows_layout = venv.join("Lib").join("site-packages");
    if windows_layout.is_dir() {
        site_packages.push(windows_layout);
    }
    // POSIX layout: `lib/python3.X/site-packages`. `lib64` is usually a symlink to
    // `lib`; the HashMap deduplicates whatever both spellings yield.
    for lib in ["lib", "lib64"] {
        let Ok(entries) = fs::read_dir(venv.join(lib)) else {
            continue;
        };
        for entry in entries.flatten() {
            let sp = entry.path().join("site-packages");
            if sp.is_dir() {
                site_packages.push(sp);
            }
        }
    }
    if site_packages.is_empty() {
        return None;
    }

    let mut installed = HashMap::new();
    for sp in site_packages {
        let Ok(entries) = fs::read_dir(&sp) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = file_name
                .strip_suffix(".dist-info")
                .or_else(|| file_name.strip_suffix(".egg-info"))
            else {
                continue;
            };
            // `{escaped_name}-{version}`: the escaping turns `-` into `_`, so the
            // name part never contains a hyphen — but setuptools may append `-pyX.Y`
            // to an egg-info (which a last-hyphen split read as part of the name),
            // and a legacy editable install writes a bare `{name}.egg-info` with no
            // version at all. Versions always start with a digit, so the name is
            // everything before the first `-<digit>`.
            let name = stem
                .match_indices('-')
                .find(|(i, _)| {
                    stem[i + 1..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit())
                })
                .map(|(i, _)| &stem[..i])
                .unwrap_or(stem);
            installed.insert(
                normalize_package_name(name),
                declared_dependencies(&entry.path()),
            );
        }
    }
    Some(installed)
}

/// The package names in a dist-info directory's `Requires-Dist` metadata lines.
///
/// Extras-gated dependencies are included: if one is installed it is reachable from its
/// parent, and this graph exists to prove reachability, not to plan an install.
fn declared_dependencies(dist_info: &Path) -> Vec<String> {
    let Ok(metadata) = fs::read_to_string(dist_info.join("METADATA")) else {
        return Vec::new();
    };
    let mut deps = Vec::new();
    for line in metadata.lines() {
        // Headers end at the first blank line; the body is a README that could
        // contain anything, including text that looks like a header.
        if line.is_empty() {
            break;
        }
        if let Some(spec) = line.strip_prefix("Requires-Dist:")
            && let Some(name) = requirement_name(spec.trim())
        {
            deps.push(name);
        }
    }
    deps
}

/// The `major.minor` of the Python a venv was built with, from its `pyvenv.cfg`.
fn venv_python_version(venv: &Path) -> Option<(u64, u64)> {
    let tag = super::venv_runtime_tag(venv)?;
    let (major, minor) = tag.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The `major.minor` of whatever `python` is on PATH — the interpreter a restore would
/// rebuild with. `None` when there is none or it cannot say.
fn path_python_version() -> Option<(u64, u64)> {
    let output = crate::spawn::command(super::resolve_program("python"))
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Python 2 printed the version on stderr; 3.4+ prints it on stdout.
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let text = if stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        stdout
    };
    let version = text.split_whitespace().nth(1)?;
    let mut parts = version.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// Say out loud, once per pass, anything that would make a restore rebuild something
/// other than what was deleted. Warnings, not refusals: every one of these environments
/// is still rebuildable, just not byte-for-byte.
fn warn_about_restore_surprises(path: &Path, venvs: &[PathBuf]) {
    if venvs.len() > 1 {
        crate::output::print_warning(&format!(
            "`{}` has {} virtual environments, all rebuilt from one requirements.txt. \
             Each restores under its own recorded name; a plain `devp restore` with no \
             record rebuilds only `.venv`.",
            crate::output::clean_path(path),
            venvs.len()
        ));
    } else if let Some(venv) = venvs.first() {
        let name = venv.file_name().map(|n| n.to_string_lossy().into_owned());
        if let Some(name) = name
            && name != ".venv"
        {
            crate::output::print_info(&format!(
                "The environment at `{}` is named `{name}` — `devp restore --last-run` \
                     recreates that name, but a restore with no record creates `.venv`.",
                crate::output::clean_path(venv)
            ));
        }
    }

    let on_path = path_python_version();
    for venv in venvs {
        if let (Some(built_with), Some(available)) = (venv_python_version(venv), on_path)
            && built_with != available
        {
            crate::output::print_warning(&format!(
                "`{}` was built with Python {}.{}, but `python` on PATH is {}.{} — a \
                 restore would rebuild it on that interpreter instead, and pinned \
                 wheels may not exist for it.",
                crate::output::clean_path(venv),
                built_with.0,
                built_with.1,
                available.0,
                available.1
            ));
            // A warning that only names the problem leaves the reader to work out the
            // fix, and the fix is one command. `uv venv` is offered first because it
            // downloads the interpreter if the machine no longer has it; the launcher
            // and `pythonX.Y` forms only work if it is already installed.
            let dir = crate::output::clean_path(venv);
            let (major, minor) = built_with;
            #[cfg(windows)]
            let native = format!("py -{major}.{minor} -m venv \"{dir}\"");
            #[cfg(not(windows))]
            let native = format!("python{major}.{minor} -m venv \"{dir}\"");
            crate::output::print_info(&format!(
                "  Rebuild on {major}.{minor}:  uv venv --python {major}.{minor} \"{dir}\"   (or `{native}`)"
            ));
        }
    }
}

/// Installed packages that nothing in the requirements file accounts for.
///
/// A hand-written requirements file pins direct dependencies only; the environment
/// legitimately holds their whole transitive closure. So the check walks the installed
/// dependency graph from every pinned name and flags only what is *unreachable* — a
/// `pip install` that was never written back, which `pip install -r` after deletion
/// would not bring back.
/// Whether a distribution name is this tool, under any spelling pip may hand back.
///
/// PEP 503 treats `dev-prune`, `dev_prune` and `Dev.Prune` as one project, and which
/// spelling lands on disk depends on how the wheel was built rather than on anything the
/// user did. `APP_NAME` is already in normalised form, so one side needs no conversion.
pub(crate) fn is_dev_prune(name: &str) -> bool {
    normalize_package_name(name) == crate::constants::APP_NAME
}

fn unrecorded_packages(
    installed: &HashMap<String, Vec<String>>,
    pinned: &HashSet<String>,
) -> Vec<String> {
    let mut reachable: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = pinned.iter().cloned().collect();
    queue.extend(BASELINE_DISTRIBUTIONS.iter().map(|s| (*s).to_string()));

    while let Some(name) = queue.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(deps) = installed.get(&name) {
            queue.extend(deps.iter().cloned());
        }
    }

    let mut extras: Vec<String> = installed
        .keys()
        .filter(|name| !reachable.contains(*name))
        .cloned()
        .collect();
    extras.sort();
    extras
}

impl PackageManager for Venv {
    fn name(&self) -> &'static str {
        "venv"
    }

    /// Detect a plain-venv project:
    /// - `requirements.txt` must exist (otherwise it's probably not a managed venv project)
    /// - At least one directory with `pyvenv.cfg` must exist in the repo root
    /// - `uv.lock` must NOT exist (uv adapter takes priority)
    ///
    /// uv's precedence is also enforced centrally in `adapters::detect_adapters`, which
    /// covers uv projects declared only through `[tool.uv]` in `pyproject.toml`.
    fn detect(&self, path: &Path) -> bool {
        let req_txt = path.join("requirements.txt");
        let uv_lock = path.join("uv.lock");

        if !req_txt.exists() || uv_lock.exists() {
            return false;
        }

        // A poetry/pipenv/pdm project belongs to its own tool. Its requirements.txt is
        // usually an export of the real lockfile — often stale — and rebuilding from it
        // would quietly produce a different environment than the one deleted.
        if FOREIGN_PYTHON_LOCKFILES
            .iter()
            .any(|f| path.join(f).exists())
            || is_poetry_project(path)
        {
            return false;
        }

        !find_venv_dirs(path).is_empty()
    }

    /// Return all venv directories (any folder containing `pyvenv.cfg`) as bloat dirs.
    fn bloat_dirs(&self, path: &Path) -> Vec<BloatDir> {
        find_venv_dirs(path)
            .into_iter()
            .map(|venv_path| {
                let name = venv_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| venv_path.display().to_string());
                let size = dir_size(&venv_path);
                BloatDir {
                    name,
                    path: venv_path,
                    size_bytes: size,
                    shared_bytes: 0,
                }
            })
            .collect()
    }

    /// Pure inspection: reads `requirements.txt` and runs nothing, so neither half of
    /// [`EnforcePolicy`] applies.
    fn enforce_lockfile(&self, path: &Path, _policy: EnforcePolicy) -> Result<()> {
        let req_txt = path.join("requirements.txt");
        if !req_txt.exists() {
            return Err(anyhow!("requirements.txt missing"));
        }
        // An empty requirements.txt cannot rebuild the environment, so deleting the
        // venv against it would be unrecoverable rather than merely inconvenient.
        let has_requirements = fs::read_to_string(&req_txt)
            .map(|c| {
                c.lines()
                    .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            })
            .unwrap_or(false);
        if !has_requirements {
            return Err(anyhow!(
                "requirements.txt at `{}` lists no packages — the virtual environment \
                 could not be rebuilt after deletion. Populate it with `pip freeze > requirements.txt`.",
                req_txt.display()
            ));
        }

        let venvs = find_venv_dirs(path);
        warn_about_restore_surprises(path, &venvs);

        // The environment can hold packages the requirements file never recorded — a
        // `pip install foo` nobody wrote back. Those are recoverable from nowhere, which
        // is exactly what this tool promises never to delete. A file that cannot be
        // fully parsed (editable installs, URLs, unreadable includes) skips the
        // comparison rather than guessing in either direction.
        if let Some(pinned) = requirement_names(&req_txt, &mut Vec::new()) {
            for venv in venvs {
                let Some(installed) = installed_distributions(&venv) else {
                    continue;
                };
                let extras = unrecorded_packages(&installed, &pinned);
                if extras.is_empty() {
                    continue;
                }
                let shown = extras
                    .iter()
                    .take(10)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if extras.len() > 10 {
                    format!(", … and {} more", extras.len() - 10)
                } else {
                    String::new()
                };
                // The one unaccounted package this tool can name from the inside. It
                // is nearly always the same accident -- `pip install dev-prune` typed
                // with a project's environment active -- and the generic message sends
                // the user to record a tool in their application's requirements file,
                // which is the wrong repair for it. The refusal itself does not soften:
                // an unrecorded package is an unrecorded package, whichever one it is.
                if extras.len() == 1 && is_dev_prune(&extras[0]) {
                    return Err(anyhow!(
                        "`{}` holds {app}, which requirements.txt does not account for. \
                         {app} is installed inside this project's virtual environment, \
                         and a tool install belongs outside a project. Either remove it \
                         — `pip uninstall {app}`, then `uv tool install {app}` — or \
                         record it as a deliberate dev dependency with `pip freeze > \
                         requirements.txt`. Either one makes the environment prunable. \
                         Nothing was deleted.",
                        venv.display(),
                        app = crate::constants::APP_NAME
                    ));
                }
                return Err(anyhow!(
                    "`{}` holds {} package(s) that requirements.txt does not account for \
                     ({shown}{suffix}). Deleting the environment would lose them with no \
                     way back. Record them first: `pip freeze > requirements.txt`.",
                    venv.display(),
                    extras.len()
                ));
            }
        }
        Ok(())
    }

    /// Recreate the environment in `.venv` — the name used when nothing recorded the
    /// original one. `devp restore --last-run` knows better and calls
    /// [`PackageManager::restore_named`] with the folder name the prune deleted.
    fn restore(&self, path: &Path, timeout: std::time::Duration) -> Result<()> {
        self.restore_named(path, ".venv", None, timeout)
    }

    /// The interpreter this environment was built with, so a restore can rebuild on it.
    fn runtime_tag(&self, path: &Path, dir_name: &str) -> Option<String> {
        super::venv_runtime_tag(&path.join(dir_name))
    }

    /// Recreate the environment under the folder name it had before the prune, so
    /// activate scripts and IDE interpreter paths keep pointing at something real.
    fn restore_named(
        &self,
        path: &Path,
        dir_name: &str,
        runtime: Option<&str>,
        timeout: std::time::Duration,
    ) -> Result<()> {
        // The recorded name comes from the registry file; a mangled entry must not be
        // able to turn `python -m venv <name>` into a write outside the project.
        let dir_name = if dir_name.is_empty()
            || dir_name == "."
            || dir_name == ".."
            || dir_name.contains(['/', '\\'])
        {
            ".venv"
        } else {
            dir_name
        };
        // Rebuild on the interpreter the environment was *created* with when the prune
        // recorded one and this machine still has it. A venv is a copy of one specific
        // interpreter; rebuilding a 3.12 environment on 3.14 changes which wheels
        // resolve, and the failure shows up later as an import error nobody connects
        // back to a restore. `run_last_run` has already checked availability and cleared
        // the tag if it was not there, so this only re-checks the single-project path.
        let launcher = runtime
            .filter(|tag| super::python_runtime_available(tag))
            .and_then(super::python_launcher);
        match launcher {
            Some((program, prefix)) => {
                let mut args: Vec<&str> = prefix.iter().map(String::as_str).collect();
                args.extend_from_slice(&["-m", "venv", dir_name]);
                run_command_with_timeout(&program, &args, path, timeout)?;
            }
            None => {
                run_command_with_timeout("python", &["-m", "venv", dir_name], path, timeout)?;
            }
        }
        // Absolute, because a relative program path is resolved against the parent
        // process's working directory, not the `current_dir` handed to the child.
        #[cfg(windows)]
        let python = path.join(dir_name).join("Scripts").join("python.exe");
        #[cfg(not(windows))]
        let python = path.join(dir_name).join("bin").join("python");
        run_command_with_timeout(
            &python.to_string_lossy(),
            &["-m", "pip", "install", "-r", "requirements.txt"],
            path,
            timeout,
        )
    }

    /// Not a lockfile in the strict sense — `requirements.txt` pins whatever its author
    /// pinned — but it is the file this adapter verifies and rebuilds from, which is what
    /// the caller wants to be told about.
    fn lockfiles(&self) -> &'static [&'static str] {
        &["requirements.txt"]
    }

    /// The comparison `enforce_lockfile` refuses on, as data: per venv, the installed
    /// distributions unreachable from anything `requirements.txt` pins.
    fn drift(&self, path: &Path) -> Vec<super::DriftReport> {
        let Some(pinned) = requirement_names(&path.join("requirements.txt"), &mut Vec::new())
        else {
            return Vec::new();
        };
        let mut reports = Vec::new();
        for venv in find_venv_dirs(path) {
            let Some(installed) = installed_distributions(&venv) else {
                continue;
            };
            let extras = unrecorded_packages(&installed, &pinned);
            if extras.is_empty() {
                continue;
            }
            reports.push(super::DriftReport {
                directory: venv
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| venv.display().to_string()),
                unrecorded: extras,
                record_command: "pip freeze > requirements.txt",
            });
        }
        reports
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::tempdir;

    fn make_venv(dir: &Path, name: &str) {
        let venv = dir.join(name);
        fs::create_dir(&venv).unwrap();
        File::create(venv.join(PYVENV_CFG)).unwrap();
    }

    #[test]
    fn test_name() {
        assert_eq!(Venv.name(), "venv");
    }

    #[test]
    fn test_detect_positive_dot_venv() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("requirements.txt")).unwrap();
        make_venv(dir.path(), ".venv");
        assert!(Venv.detect(dir.path()));
    }

    #[test]
    fn test_detect_positive_venv() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("requirements.txt")).unwrap();
        make_venv(dir.path(), "venv");
        assert!(Venv.detect(dir.path()));
    }

    #[test]
    fn test_detect_positive_custom_name() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("requirements.txt")).unwrap();
        make_venv(dir.path(), "my_env");
        assert!(Venv.detect(dir.path()));
    }

    #[test]
    fn test_detect_positive_env() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("requirements.txt")).unwrap();
        make_venv(dir.path(), "env");
        assert!(Venv.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative_no_req() {
        let dir = tempdir().unwrap();
        make_venv(dir.path(), ".venv");
        assert!(!Venv.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative_no_env() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("requirements.txt")).unwrap();
        // A plain directory without pyvenv.cfg — not a venv
        fs::create_dir(dir.path().join("not_a_venv")).unwrap();
        assert!(!Venv.detect(dir.path()));
    }

    #[test]
    fn test_detect_negative_uv_lock() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("requirements.txt")).unwrap();
        File::create(dir.path().join("uv.lock")).unwrap();
        make_venv(dir.path(), ".venv");
        assert!(!Venv.detect(dir.path()));
    }

    #[test]
    fn test_bloat_dirs_present() {
        let dir = tempdir().unwrap();
        make_venv(dir.path(), ".venv");
        make_venv(dir.path(), "my_env");
        let dirs = Venv.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 2);
        let names: Vec<&str> = dirs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&".venv"));
        assert!(names.contains(&"my_env"));
    }

    #[test]
    fn test_bloat_dirs_absent() {
        let dir = tempdir().unwrap();
        let dirs = Venv.bloat_dirs(dir.path());
        assert!(dirs.is_empty());
    }

    #[test]
    fn test_bloat_dirs_ignores_non_venv_dirs() {
        let dir = tempdir().unwrap();
        // A dir without pyvenv.cfg should NOT be returned
        fs::create_dir(dir.path().join("src")).unwrap();
        make_venv(dir.path(), ".venv");
        let dirs = Venv.bloat_dirs(dir.path());
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, ".venv");
    }

    /// A `<name>-<version>.dist-info` under the venv's `site-packages`, the same
    /// metadata pip writes. The `Lib/` spelling is Windows' layout, which
    /// `installed_distributions` reads on every OS — so the tests can build it anywhere.
    fn install_package(root: &Path, venv: &str, name: &str, requires: &[&str]) {
        let dist_info = root
            .join(venv)
            .join("Lib")
            .join("site-packages")
            .join(format!("{name}-1.0.0.dist-info"));
        fs::create_dir_all(&dist_info).unwrap();
        let mut metadata = format!("Metadata-Version: 2.1\nName: {name}\nVersion: 1.0.0\n");
        for dep in requires {
            metadata.push_str(&format!("Requires-Dist: {dep}\n"));
        }
        fs::write(dist_info.join("METADATA"), metadata).unwrap();
    }

    #[test]
    fn enforce_refuses_when_requirements_lists_nothing() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "# nothing pinned\n\n").unwrap();
        make_venv(dir.path(), ".venv");

        let err = Venv
            .enforce_lockfile(dir.path(), EnforcePolicy::default())
            .unwrap_err();
        assert!(err.to_string().contains("lists no packages"));
    }

    #[test]
    fn enforce_refuses_a_package_the_requirements_never_recorded() {
        // `pip install requests` that nobody wrote back: recoverable from nowhere,
        // so deleting the environment must be refused, naming the package.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask==3.0.0\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "flask", &[]);
        install_package(dir.path(), ".venv", "requests", &[]);

        let err = Venv
            .enforce_lockfile(dir.path(), EnforcePolicy::default())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("requests"),
            "names the unrecorded package: {err}"
        );
        assert!(
            !err.contains("flask"),
            "must not blame the pinned one: {err}"
        );
        assert!(err.contains("pip freeze"), "says how to record it: {err}");
    }

    #[test]
    fn enforce_accepts_transitive_dependencies_of_pinned_packages() {
        // requirements.txt pins direct dependencies only; the environment legitimately
        // holds their whole closure. Reachable packages are not drift.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "requests==2.32.3\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(
            dir.path(),
            ".venv",
            "requests",
            &["urllib3 (>=1.21.1)", "charset-normalizer"],
        );
        install_package(dir.path(), ".venv", "urllib3", &[]);
        // Installed under the `_` spelling; PEP 503 normalization must still match.
        install_package(dir.path(), ".venv", "charset_normalizer", &[]);

        assert!(
            Venv.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn enforce_skips_the_comparison_when_requirements_cannot_be_parsed() {
        // An editable install's name only pip can compute. Guessing in either
        // direction is wrong, so an unparseable file skips the drift check.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("requirements.txt"),
            "-e ./local-package\nflask==3.0.0\n",
        )
        .unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "left-behind", &[]);

        assert!(
            Venv.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn drift_names_the_venv_and_the_unrecorded_packages() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "requests==2.32.3\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "requests", &[]);
        install_package(dir.path(), ".venv", "sneaky-pkg", &[]);

        let reports = Venv.drift(dir.path());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].directory, ".venv");
        assert_eq!(reports[0].unrecorded, vec!["sneaky-pkg"]);
        assert_eq!(reports[0].record_command, "pip freeze > requirements.txt");
    }

    /// A dependency of a pinned package is reachable from the requirements file, so it
    /// is recorded in the only sense that matters: `pip install -r` brings it back.
    #[test]
    fn drift_does_not_flag_transitive_dependencies_of_pinned_packages() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "requests==2.32.3\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "requests", &["urllib3"]);
        install_package(dir.path(), ".venv", "urllib3", &[]);

        assert!(Venv.drift(dir.path()).is_empty());
    }

    #[test]
    fn enforce_names_dev_prune_as_the_situation_it_actually_is() {
        // The generic message would send somebody to record a tool in their
        // application's requirements file, which is the wrong repair for the accident
        // that produced it. The refusal is unchanged; only the advice is.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask==3.0.0\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "flask", &[]);
        // pip escapes the name into the dist-info directory, so this is what is on disk.
        install_package(dir.path(), ".venv", "dev_prune", &[]);

        let err = Venv
            .enforce_lockfile(dir.path(), EnforcePolicy::default())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("pip uninstall dev-prune"),
            "offers the removal: {err}"
        );
        assert!(
            err.contains("uv tool install dev-prune"),
            "offers the tool install: {err}"
        );
        assert!(err.contains("pip freeze"), "offers recording it: {err}");
        assert!(
            err.contains("Nothing was deleted"),
            "is still a refusal: {err}"
        );
    }

    #[test]
    fn enforce_accepts_dev_prune_when_requirements_records_it() {
        // The odd but legitimate case: a project that really does depend on the tool,
        // in its own requirements file, on purpose. Recorded is recorded — there is
        // nothing special about this package once somebody has written it down.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "dev-prune==1.7.0\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "dev_prune", &[]);

        assert!(
            Venv.enforce_lockfile(dir.path(), EnforcePolicy::default())
                .is_ok()
        );
    }

    #[test]
    fn enforce_keeps_the_generic_message_when_dev_prune_is_not_alone() {
        // Naming one of two strays and saying how to fix only that one would leave the
        // user re-running the pass to be refused a second time for a package the first
        // message never mentioned.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask==3.0.0\n").unwrap();
        make_venv(dir.path(), ".venv");
        install_package(dir.path(), ".venv", "flask", &[]);
        install_package(dir.path(), ".venv", "dev_prune", &[]);
        install_package(dir.path(), ".venv", "requests", &[]);

        let err = Venv
            .enforce_lockfile(dir.path(), EnforcePolicy::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("requests"), "names the other stray: {err}");
        assert!(err.contains("2 package(s)"), "counts both: {err}");
    }

    #[test]
    fn is_dev_prune_accepts_every_spelling_pip_may_write() {
        assert!(is_dev_prune("dev-prune"));
        assert!(is_dev_prune("dev_prune"));
        assert!(is_dev_prune("Dev-Prune"));
        assert!(!is_dev_prune("dev-pruner"));
        assert!(!is_dev_prune("prune"));
    }
}
