# 🖼️ `dev-prune` Visual Assets Directory

This directory contains all official brand graphics, visual banners, executable icon resources, and file-manager icon assets for **`dev-prune`** (`devp`).

---

## 🗂️ Asset Inventory & Usage Reference

| Asset File | Format | Purpose & Usage Location |
| :--- | :---: | :--- |
| **`readme-banner.png`** | PNG | 1280×640. The banner at the top of the root [`README.md`](../README.md), [`docs/README.md`](../docs/README.md), [`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md), [`docs/architecture/HLD.md`](../docs/architecture/HLD.md) and [`docs/troubleshooting/README.md`](../docs/troubleshooting/README.md) — and the file to upload as the repository's GitHub social preview, which wants exactly this size. |
| **`banner-master.png`** | PNG | 2910×1440 master. `readme-banner.png` and the site's `og-card.jpg` are both centre-crops of this file; regenerate them from here rather than upscaling either one. |
| **`hero_banner.png`** | PNG | Superseded, kept only so older permalinks and forks resolve. Nothing in this repository links to it: the lettering in it is generated gibberish (`developements`, `READM.md`) and the file is 5.8 MB. |
| **`banner.png`** | PNG | Superseded, and 8.8 MB. Kept for the same reason as `hero_banner.png`. |
| **`banner.jpg`** | JPG | The previous Open Graph card, still deployed at `site/public/assets/banner.jpg` because social platforms cache card images and links already shared point at that URL. New cards use `og-card.jpg`. |
| **`icon.png`** | PNG | High-resolution square application logo. |
| **`icon_light.png`** | PNG | The same logo on a white ground, with the mark darkened so the cyan end of the gradient survives on paper-white backgrounds. Use where a page is known to be light; `icon.png` is the dark-ground equivalent. |
| **`icon_transparent.png`** | PNG | Transparent 1536×1536 master. Every `mimetype/` size is downscaled from this file. |
| **`icon.ico`** | ICO | 6-layer multi-resolution Windows executable icon embedded directly into Windows release binaries via `winres` in [`build.rs`](../build.rs). Also embedded in the binary itself and written to the config directory by `devp config icon`. |
| **`icon.icns`** | ICNS | macOS application icon bundle for macOS distribution archives. |
| **`mimetype/`** | Directory | Hicolor-theme icons for the `application/x-devprune` MIME type, at 48px, 128px and 256px. Compiled into the binary with `include_bytes!` and installed under `$XDG_DATA_HOME/icons/hicolor/<size>x<size>/mimetypes/` by `devp config icon` so Linux file managers show them on `*.devprune.json`. Names follow the freedesktop icon-naming spec exactly (`application-x-devprune.png`) — renaming them breaks the lookup. |
| **`favicon/`** | Directory | Web favicon bundle (PNG, ICO, SVG) for the official documentation website (`site/`). |
| **`site/public/assets/og-card.jpg`** | JPG | 1200×630, 106 KB. The Open Graph and Twitter card image named by [`site/index.html`](../site/index.html). It lives under `site/public/` rather than here because only files under that directory are published to the site. |
| **`BANNER_PROMPTS.md`** | Markdown | Not an asset — the three image-generation prompts (1600×900 social banner, 1280×640 README hero, 1200×630 Open Graph card) with the palette and the constraints that keep generated art from looking like a PC optimiser. Read it before regenerating any banner. |

---

## 🎨 Asset Usage Rules

1. **Markdown Documents**: When referencing visual assets in Markdown files, use relative HTML `<img>` tags with controlled width attributes (e.g. `<img src="assets/readme-banner.png" alt="dev-prune Banner" width="800" />`) to ensure responsive rendering across light and dark themes.
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
