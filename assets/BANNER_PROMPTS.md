# Banner generation prompts

> The banners shipping today are not generated from these prompts. They are rendered
> from [`site/banners/index.html`](../site/banners/index.html) by
> `npm --prefix site run banners`, which is where a change to one belongs. This file
> is kept for the palette below and for the constraints on it — the part worth having
> whichever way the artwork gets made.

Three prompts for the three sizes dev-prune actually needs, kept here so the artwork can
be regenerated without re-deciding what it should look like. Nothing in this file is
shipped — it is input to an image model. Keep the full-resolution output as
`banner-master.png`; `readme-banner.png` and `site/public/assets/og-card.jpg` were
centre-crops of it, not separate generations, so the three never drifted apart.

They share one palette, taken from `site/src/index.css` so generated art matches the
site rather than approximating it:

| Role | Hex |
|---|---|
| Ground | `#0b0f17` (near-black navy) |
| Panel / card | `#111827` |
| Border | `#1f293d` |
| Primary text | `#f9fafb` |
| Muted text | `#9ca3af` |
| Accent — cyan | `#38bdf8` |
| Accent — blue | `#3b82f6` |
| Verification green, used sparingly | `#10b981` |

## Rules that apply to all three

Written as constraints because image models drift toward stock "cleanup app" clichés,
and every one of them makes dev-prune look like a PC optimiser rather than a developer
tool:

- **No trash cans, brooms, mops, sponges, sparkles or vacuum cleaners.** Nothing that
  reads as "junk removal". The product deletes recoverable directories on purpose, it
  does not tidy.
- **No cartoon characters, no mascots, no 3D-rendered blobs, no isometric little people.**
- **No invented numbers.** No "10× faster", no "saved 400 GB", no fake star counts, no
  download badges. If a figure is not measured it does not go on a banner.
- **No fake terminal text that means nothing.** If a terminal appears, its contents must
  be a command dev-prune really has (`devp status`, `devp run --dry-run`, `devp undo`).
- Flat vector or clean technical illustration. Subtle depth is fine; heavy gradients,
  glossy plastic and lens flare are not.
- Type must be legible at the size the asset is actually viewed — a GitHub README hero
  is often rendered under 700 px wide, and a social card is frequently seen as a
  thumbnail.
- Leave the outer 5% as quiet margin. Every platform crops something.

---

## 1. Social launch banner — 1600 × 900

> A 1600×900 landscape banner for a developer command-line tool called **dev-prune**.
> Dark near-black navy ground (`#0b0f17`) with a faint grid or subtle diagonal
> structure, low contrast, never busy.
>
> Left two-thirds: the wordmark **dev-prune** in a clean geometric sans, white
> (`#f9fafb`), large and confident. Directly beneath it, smaller and in muted grey
> (`#9ca3af`), the line *"Reclaims disk space from idle repositories — and can prove it
> back."*
>
> Right third: a restrained technical illustration of a directory tree, drawn as thin
> cyan (`#38bdf8`) lines and small nodes. Some branches are dimmed to about 30% opacity
> to read as removed; the trunk and the `.git` node stay bright, clearly untouched. One
> small check mark in green (`#10b981`) sits beside the tree — the only green in the
> image — signalling verification rather than success confetti.
>
> Flat vector, sharp edges, generous negative space, no photographic texture. No trash
> cans, brooms, sparkles, mascots or cartoon imagery. No numbers, statistics, badges or
> logos of other products.

## 2. GitHub README hero — 1280 × 640

> A 1280×640 hero image for the top of a GitHub README, for a CLI tool named
> **dev-prune**. Very dark navy ground (`#0b0f17`).
>
> Centred composition. The wordmark **dev-prune** in white geometric sans across the
> upper third, with a thin cyan (`#38bdf8`) underline rule beneath it. Under that, in
> muted grey, four short words separated by middots: *Lockfile-verified · Git-aware ·
> Reversible · Automatic*.
>
> Lower half: a single terminal panel in `#111827` with a `#1f293d` one-pixel border and
> softly rounded corners, showing three monospace lines and nothing else —
> `$ devp status`, a dimmed result line, and `$ devp run --dry-run`. The prompt symbol is
> cyan, the command text is white, the result line is muted grey. One word in the result
> line may be green (`#10b981`).
>
> Everything else is empty space. Flat, technical, quiet. No icons of folders being
> thrown away, no arrows, no motion streaks, no percentages or file-size figures, no
> mascot, no gloss.

## 3. Open Graph / marketplace card — 1200 × 630

> A 1200×630 social preview card (Open Graph, also used as a marketplace listing image)
> for **dev-prune**, a developer disk-cleanup CLI. Dark navy ground (`#0b0f17`), flat
> vector.
>
> The card must survive being seen as a small thumbnail, so it carries exactly two
> things: the wordmark **dev-prune** in large white geometric sans, centred slightly
> above the middle, and one line under it in cyan (`#38bdf8`): *"The safe automatic
> cleanup tool for developers."*
>
> A single small graphic element only — a minimal cyan glyph suggesting a pruned branch:
> one stroke continuing, one stroke ending in a clean cut. Sits above the wordmark, no
> wider than a tenth of the card.
>
> Deep margins on all sides. Nothing in the corners. No terminal, no screenshot, no
> statistics, no badges, no trash-can or broom iconography, no gradient mesh, no
> photographic background.

---

## After generating

- Export PNG at exactly the stated pixel size; do not upscale a smaller render.
- Check each one at 25% zoom before committing it. If the tagline stops being readable,
  the type is too small for the platform that will show it.
- Add the file to the inventory table in [`README.md`](README.md), which is what tells
  the next person where the asset is used.
- The licence note in that README covers new artwork here too: original work, Apache-2.0,
  no third-party assets. An AI-generated image that reproduces someone's logo or a stock
  photograph is not original work — regenerate rather than retouch it.
