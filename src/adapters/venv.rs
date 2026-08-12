// Standard Python venv package manager adapter.
//
// Detects Python virtual environments by scanning the repo root for any
// directory containing a `pyvenv.cfg` file — the canonical marker for any
// Python virtual environment, regardless of what the folder is named
// (`.venv`, `venv`, `env`, `my_env`, `.env`, etc.).
//
// Priority: the `uv` adapter takes precedence when `uv.lock` is present.

use super::{BloatDir, EnforcePolicy, PackageManager, dir_size, run_command};
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

/// The canonical file inside every Python virtual environment.
const PYVENV_CFG: &str = "pyvenv.cfg";

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
        Ok(())
    }

    /// Recreate the environment in `.venv`.
    ///
    /// Note: the adapter prunes any directory containing `pyvenv.cfg`, whatever it is
    /// called, but restore always recreates `.venv` — the original folder name is not
    /// recorded anywhere once the directory is gone.
    fn restore(&self, path: &Path) -> Result<()> {
        #[cfg(windows)]
        {
            run_command("python", &["-m", "venv", ".venv"], path)?;
            run_command(
                ".venv\\Scripts\\python.exe",
                &["-m", "pip", "install", "-r", "requirements.txt"],
                path,
            )
        }
        #[cfg(not(windows))]
        {
            run_command("python", &["-m", "venv", ".venv"], path)?;
            run_command(
                ".venv/bin/python",
                &["-m", "pip", "install", "-r", "requirements.txt"],
                path,
            )
        }
    }

    /// Not a lockfile in the strict sense — `requirements.txt` pins whatever its author
    /// pinned — but it is the file this adapter verifies and rebuilds from, which is what
    /// the caller wants to be told about.
    fn lockfiles(&self) -> &'static [&'static str] {
        &["requirements.txt"]
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
}
