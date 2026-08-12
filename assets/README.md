# 🖼️ `dev-prune` Visual Assets Directory

This directory contains all official brand graphics, visual banners, executable icon resources, and file-manager icon assets for **`dev-prune`** (`devp`).

---

## 🗂️ Asset Inventory & Usage Reference

| Asset File | Format | Purpose & Usage Location |
| :--- | :---: | :--- |
| **`hero_banner.png`** | PNG | Primary hero banner embedded at top of root [`README.md`](../README.md) and [`docs/architecture/HLD.md`](../docs/architecture/HLD.md). |
| **`banner.png`** | PNG | Diagnostics banner used in [`docs/README.md`](../docs/README.md) and [`docs/troubleshooting/README.md`](../docs/troubleshooting/README.md). |
| **`banner.jpg`** | JPG | Compressed web version of visual banner for external sites and social preview graphs. |
| **`icon.png`** | PNG | High-resolution square application logo. |
| **`icon_transparent.png`** | PNG | Transparent 1536×1536 master. Every `mimetype/` size is downscaled from this file. |
| **`icon.ico`** | ICO | 6-layer multi-resolution Windows executable icon embedded directly into Windows release binaries via `winres` in [`build.rs`](../build.rs). Also embedded in the binary itself and written to the config directory by `devp config icon`. |
| **`icon.icns`** | ICNS | macOS application icon bundle for macOS distribution archives. |
| **`mimetype/`** | Directory | Hicolor-theme icons for the `application/x-devprune` MIME type, at 48px, 128px and 256px. Compiled into the binary with `include_bytes!` and installed under `$XDG_DATA_HOME/icons/hicolor/<size>x<size>/mimetypes/` by `devp config icon` so Linux file managers show them on `*.devprune.json`. Names follow the freedesktop icon-naming spec exactly (`application-x-devprune.png`) — renaming them breaks the lookup. |
| **`favicon/`** | Directory | Web favicon bundle (PNG, ICO, SVG) for the official documentation website (`site/`). |

---

## 🎨 Asset Usage Rules

1. **Markdown Documents**: When referencing visual assets in Markdown files, use relative HTML `<img>` tags with controlled width attributes (e.g. `<img src="assets/hero_banner.png" alt="dev-prune Banner" width="800" />`) to ensure responsive rendering across light and dark themes.
2. **Binary Embedding**: `icon.ico` is compiled directly into `dev-prune.exe` on Windows targets using `winres::WindowsResource::set_icon("assets/icon.ico")` in [`build.rs`](../build.rs).
3. **File Manager Integration**: `devp config icon` extracts the icons it needs from the binary — nothing is downloaded and this directory does not have to exist at runtime. On Linux it registers the `application/x-devprune` MIME type and installs the `mimetype/` icons into the hicolor theme; on Windows and macOS it explains why the platform cannot do the same and stops rather than pretending. It also prints a paste-able `settings.json` snippet for editors that support icon associations. It never edits an editor's settings, never modifies `PATH`, and never touches a shell startup file. `devp uninstall` removes every path it created.
4. **Regenerating `mimetype/`**: downscale `icon_transparent.png` with Lanczos resampling to 48, 128 and 256 pixels square, keeping the alpha channel. The sizes are fixed by the hicolor theme directories they install into; adding a size means adding it to `EMBEDDED_MIME_ICONS` in [`src/commands/icon.rs`](../src/commands/icon.rs) as well.

---

## ⚖️ Licence

Every file in this directory is original work created for dev-prune. There is no
third-party artwork here, no stock imagery, no icon-set derivative, and no font
embedded in any raster. Nothing in this directory carries an attribution requirement
from anyone other than the dev-prune authors.

All of it is released under the **Apache License 2.0**, the same licence as the source
code — see [`LICENSE.md`](../LICENSE.md). That covers the copies compiled into the
binary with `include_bytes!` and the copies `devp config icon` writes onto a user's
machine: a redistributed dev-prune binary redistributes these assets under the same
terms, and no separate notice file is required beyond the one Apache-2.0 already asks for.

**Trademark note.** The licence grants rights to the artwork, not to the identity. Use
of the dev-prune name and mark to imply endorsement of, or affiliation with, a fork or
derived product is not granted here. Fork the code freely; rebrand it if you ship it as
your own.
