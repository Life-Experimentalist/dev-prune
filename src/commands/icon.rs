// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Copyright 2026 VKrishna04
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! File-manager icon registration for `.devprune.json`.
//!
//! The target here is the OS file manager, not the editor. What is actually achievable
//! differs per platform, and this module is deliberate about saying so rather than
//! implying more than it does:
//!
//! - **Linux** — a real, complete registration. A `shared-mime-info` package declares
//!   the glob `*.devprune.json` as `application/x-devprune`, and matching icons go into
//!   the hicolor theme. Nautilus, Dolphin, Thunar, Nemo and PCManFM all honour this.
//! - **Windows** — Explorer resolves a file's icon from the last extension only, so
//!   `*.devprune.json` is indistinguishable from any other `.json` to it. Claiming the
//!   icon would mean claiming *every* JSON file on the machine, which is not ours to
//!   take. The config folder gets its own icon via `desktop.ini`; individual files keep
//!   the system JSON icon.
//! - **macOS** — a UTI has to be exported by an application bundle's `Info.plist`, and
//!   a single CLI binary is not a bundle. Not supported.
//!
//! Editors are handled by printing a snippet the user can paste; nothing here edits an
//! editor's settings file.
//!
//! Everything written lands in either dev-prune's own config directory or the user's XDG
//! data directory. PATH, shell startup files and the binary are untouched.

use anyhow::Result;
use std::fs;
use std::path::Path;
// Only the XDG registration and its tests build paths; on Windows and macOS there is
// nothing here that needs an owned one.
#[cfg(any(target_os = "linux", test))]
use std::path::PathBuf;

use crate::config::Registry;
use crate::output;

pub const EMBEDDED_ICO_BYTES: &[u8] = include_bytes!("../../assets/icon.ico");
pub const EMBEDDED_SCHEMA_BYTES: &[u8] = include_bytes!("../../schemas/devprune.schema.json");

/// Mimetype icons, one per hicolor size directory. Downscaled from `assets/icon.png` and
/// shipped under the same Apache-2.0 licence as the rest of the repository — see
/// `assets/README.md`.
pub const EMBEDDED_MIME_ICONS: &[(u32, &[u8])] = &[
    (
        48,
        include_bytes!("../../assets/mimetype/application-x-devprune-48.png"),
    ),
    (
        128,
        include_bytes!("../../assets/mimetype/application-x-devprune-128.png"),
    ),
    (
        256,
        include_bytes!("../../assets/mimetype/application-x-devprune-256.png"),
    ),
];

/// The 256px icon doubles as the config-folder icon on Linux.
pub const EMBEDDED_PNG_BYTES: &[u8] =
    include_bytes!("../../assets/mimetype/application-x-devprune-256.png");

/// The MIME type `*.devprune.json` is registered as.
pub const MIME_TYPE: &str = "application/x-devprune";

/// Icon name derived from the MIME type, as the icon-naming spec requires: the type with
/// its `/` replaced by `-`. A file manager looks up exactly this name and no other.
pub const MIME_ICON_NAME: &str = "application-x-devprune";

/// Icon association snippet for editors using the Material Icon Theme.
///
/// The value has to be the *name* of an icon the theme already ships — it is resolved to
/// `<extension>/icons/<name>.svg`, so a filesystem path can never match. `tune` is a
/// sliders glyph that reads as "settings file" and is distinct from plain `json`.
const EDITOR_SNIPPET: &str = r#"    "material-icon-theme.files.associations": {
      "*.devprune.json": "tune"
    }"#;

/// Whether everything [`sync_app_directory`] writes is already on disk.
///
/// The setup pass needs a "is this missing?" question it can answer without doing any
/// work, because it runs on every install, upgrade and `devp init`. Rewriting a few
/// hundred kilobytes of PNG each time would be harmless but wasteful, and on Linux it
/// would also re-run `update-mime-database`, which is not free.
///
/// Content is not compared, only presence — the assets are compiled into the binary, so
/// the upgrade stamp already forces a rewrite when the version changes.
pub fn is_registered() -> bool {
    let Ok(config_dir) = Registry::config_dir() else {
        return false;
    };
    let present = config_dir.join("icon.ico").exists()
        && config_dir.join("icon.png").exists()
        && config_dir.join("bin").join("devprune.schema.json").exists();

    #[cfg(target_os = "linux")]
    let present = present
        && xdg_data_home()
            .is_some_and(|home| xdg_owned_paths(&home).iter().all(|path| path.exists()));

    present
}

/// Write the icon assets and JSON Schema into the config directory, then register the
/// file type with the OS file manager as far as the platform allows.
pub fn sync_app_directory() -> Result<()> {
    let Ok(config_dir) = Registry::config_dir() else {
        return Ok(());
    };

    if !config_dir.exists() {
        let _ = fs::create_dir_all(&config_dir);
    }

    let bin_dir = config_dir.join("bin");
    if !bin_dir.exists() {
        let _ = fs::create_dir_all(&bin_dir);
    }

    let ico_path = config_dir.join("icon.ico");
    let _ = fs::write(&ico_path, EMBEDDED_ICO_BYTES);

    let png_path = config_dir.join("icon.png");
    let _ = fs::write(&png_path, EMBEDDED_PNG_BYTES);

    let schema_path = bin_dir.join("devprune.schema.json");
    let _ = fs::write(&schema_path, EMBEDDED_SCHEMA_BYTES);

    apply_folder_icon(&config_dir, &ico_path, &png_path);
    register_file_type();

    // Scope note: this command used to also copy the running binary into `bin/`, append
    // `dev-prune` to the User PATH, and write a `devp` function into `$PROFILE` /
    // `.zshrc` / `.bashrc` / `config.fish`. Editing a user's shell startup is not what
    // "register file icons" means, `devp uninstall` had no way to undo any of it, and
    // it was redundant three times over: the installers already set PATH, and
    // `ensure_devp_alias()` creates `devp` beside the real binary on every run.

    Ok(())
}

/// Register custom file icon associations for `*.devprune.json`.
pub fn run_install() -> Result<()> {
    output::print_header("dev-prune File Icon Registration");

    sync_app_directory()?;

    if let Ok(config_dir) = Registry::config_dir() {
        output::print_success(&format!(
            "Icons and JSON Schema written to `{}`",
            output::clean_path(&config_dir)
        ));
    }

    report_file_manager_support();

    output::print_header("Editors");
    println!("dev-prune does not edit your editor settings. If you want the icon there");
    println!("too, paste this into your `settings.json` (Material Icon Theme):");
    println!();
    println!("{EDITOR_SNIPPET}");
    println!();
    output::print_info(
        "Schema validation needs no setting at all — every `.devprune.json` dev-prune \
         writes carries a `$schema` link.",
    );

    Ok(())
}

/// Say plainly what the current platform's file manager will and will not do.
fn report_file_manager_support() {
    output::print_header("File Manager");

    #[cfg(target_os = "linux")]
    {
        output::print_success(&format!(
            "Registered `*.devprune.json` as `{MIME_TYPE}` with icons in the hicolor theme."
        ));
        output::print_info(
            "Some file managers cache icons for the session — log out and back in if the \
             old icon is still showing.",
        );
    }

    #[cfg(windows)]
    {
        output::print_info(
            "Explorer picks a file's icon from its last extension, so `*.devprune.json` \
             looks the same to it as any other `.json`. Giving it our icon would mean \
             taking over every JSON file on the machine, which dev-prune will not do.",
        );
        output::print_success("The dev-prune config folder has its own icon.");
    }

    #[cfg(target_os = "macos")]
    output::print_info(
        "Finder resolves file icons through UTIs exported by an application bundle. \
         dev-prune ships a single binary, not a bundle, so file and folder icons stay \
         at the system default.",
    );
}

fn apply_folder_icon(config_dir: &Path, ico_path: &Path, _png_path: &Path) {
    #[cfg(windows)]
    {
        let ini_file = config_dir.join("desktop.ini");
        let ini_content = format!(
            "[.ShellClassInfo]\r\nIconResource={},0\r\n[ViewState]\r\nMode=\r\nVid=\r\nFolderType=Generic\r\n",
            ico_path.display()
        );
        let _ = fs::write(&ini_file, ini_content);

        let clean_ini = output::clean_path(&ini_file);
        let clean_dir = output::clean_path(config_dir);
        let _ = std::process::Command::new("attrib")
            .args(["+h", "+s", &clean_ini])
            .output();
        let _ = std::process::Command::new("attrib")
            .args(["+s", &clean_dir])
            .output();
    }

    #[cfg(target_os = "linux")]
    {
        let dot_dir = config_dir.join(".directory");
        let content = format!("[Desktop Entry]\nIcon={}\n", _png_path.display());
        let _ = fs::write(&dot_dir, content);
    }

    #[cfg(target_os = "macos")]
    {
        let _ = _png_path;
        let _ = ico_path;
        let _ = config_dir;
    }
}

// ---------------------------------------------------------------------------
// Linux: shared-mime-info + hicolor icon theme
// ---------------------------------------------------------------------------

/// The XDG MIME package describing the glob.
///
/// Weight 60 puts it above the default 50 so it beats the built-in `*.json` rule; without
/// that, `application/json` wins and the icon never appears.
pub fn mime_package_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- Written by dev-prune. Removed again by `devp uninstall`. -->
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="{MIME_TYPE}">
    <comment>dev-prune project configuration</comment>
    <sub-class-of type="application/json"/>
    <glob pattern="*.devprune.json" weight="60"/>
    <icon name="{MIME_ICON_NAME}"/>
    <generic-icon name="text-x-generic"/>
  </mime-type>
</mime-info>
"#
    )
}

/// Root of the user's XDG data directory (`$XDG_DATA_HOME`, else `~/.local/share`).
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn xdg_data_home() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    dirs::home_dir().map(|h| h.join(".local").join("share"))
}

/// Every file the Linux registration owns, so install and uninstall cannot drift apart.
#[cfg(any(target_os = "linux", test))]
fn xdg_owned_paths(data_home: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        data_home
            .join("mime")
            .join("packages")
            .join("dev-prune.xml"),
    ];
    for (size, _) in EMBEDDED_MIME_ICONS {
        paths.push(
            data_home
                .join("icons")
                .join("hicolor")
                .join(format!("{size}x{size}"))
                .join("mimetypes")
                .join(format!("{MIME_ICON_NAME}.png")),
        );
    }
    paths
}

#[cfg(not(target_os = "linux"))]
fn register_file_type() {}

#[cfg(target_os = "linux")]
fn register_file_type() {
    let Some(data_home) = xdg_data_home() else {
        return;
    };

    let package = data_home
        .join("mime")
        .join("packages")
        .join("dev-prune.xml");
    if let Some(parent) = package.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = fs::write(&package, mime_package_xml()) {
        output::print_warning(&format!(
            "Could not write {}: {e}",
            output::clean_path(&package)
        ));
        return;
    }

    for (size, bytes) in EMBEDDED_MIME_ICONS {
        let dir = data_home
            .join("icons")
            .join("hicolor")
            .join(format!("{size}x{size}"))
            .join("mimetypes");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join(format!("{MIME_ICON_NAME}.png")), bytes);
    }

    // These refresh the caches file managers actually read. Both are best-effort: a
    // machine without shared-mime-info still gets the files, and the next login picks
    // them up.
    refresh_cache("update-mime-database", &[data_home.join("mime")]);
    refresh_cache(
        "gtk-update-icon-cache",
        &[data_home.join("icons").join("hicolor")],
    );
}

#[cfg(target_os = "linux")]
fn refresh_cache(program: &str, args: &[PathBuf]) {
    let _ = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Undo `register_file_type`. Called by `devp uninstall`; a no-op off Linux.
pub fn unregister_file_type() {
    #[cfg(target_os = "linux")]
    {
        let Some(data_home) = xdg_data_home() else {
            return;
        };
        let mut removed = false;
        for path in xdg_owned_paths(&data_home) {
            if fs::remove_file(&path).is_ok() {
                removed = true;
            }
        }
        if removed {
            refresh_cache("update-mime-database", &[data_home.join("mime")]);
            refresh_cache(
                "gtk-update-icon-cache",
                &[data_home.join("icons").join("hicolor")],
            );
            output::print_info("Removed the `*.devprune.json` file type registration.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_name_follows_the_icon_naming_spec() {
        // A file manager looks up the MIME type with `/` swapped for `-` and nothing else.
        assert_eq!(MIME_ICON_NAME, MIME_TYPE.replace('/', "-"));
    }

    #[test]
    fn the_mime_package_outranks_the_builtin_json_glob() {
        let xml = mime_package_xml();
        // The stock `*.json` rule is weight 50. Anything at or below that loses.
        assert!(xml.contains(r#"weight="60""#), "{xml}");
        assert!(xml.contains(r#"pattern="*.devprune.json""#));
        assert!(xml.contains(&format!(r#"<icon name="{MIME_ICON_NAME}"/>"#)));
    }

    #[test]
    fn the_mime_package_is_well_formed_enough_to_have_one_type() {
        let xml = mime_package_xml();
        assert_eq!(xml.matches("<mime-type").count(), 1);
        assert_eq!(xml.matches("</mime-type>").count(), 1);
        assert!(xml.trim_end().ends_with("</mime-info>"));
    }

    #[test]
    fn install_and_uninstall_agree_on_which_files_are_ours() {
        let root = PathBuf::from("/tmp/xdg");
        let owned = xdg_owned_paths(&root);
        // One MIME package plus one icon per shipped size — nothing else is touched.
        assert_eq!(owned.len(), 1 + EMBEDDED_MIME_ICONS.len());
        assert!(owned.iter().all(|p| p.starts_with(&root)));
    }

    #[test]
    fn the_shipped_icons_are_real_pngs() {
        for (size, bytes) in EMBEDDED_MIME_ICONS {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{size}px is not a PNG");
        }
    }

    #[test]
    fn the_editor_snippet_names_a_theme_icon_rather_than_a_path() {
        // The original bug: a filesystem path here resolves to nothing, so no icon.
        assert!(EDITOR_SNIPPET.contains(r#""*.devprune.json": "tune""#));
        assert!(!EDITOR_SNIPPET.contains(".png"));
    }
}
