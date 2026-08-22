// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Builds `icons/devprune.woff` from `assets/devprune.svg`.
//
// VS Code will only render a custom glyph in the status bar, the command palette or a
// tree view through `contributes.icons`, and that contribution point takes a *font*
// and a character — never an SVG or a PNG. So the one-path logo has to become a
// one-glyph font, and this is the script that does it.
//
// The output is committed, because publishing the extension must not depend on this
// toolchain being installed. Run it only when the logo itself changes:
//
//     npm --prefix editors/vscode install
//     npm --prefix editors/vscode run icon-font
//
// A font glyph carries no colour: VS Code paints it with whatever the theme's
// foreground is, exactly as it paints its own codicons. That is the point — the mark
// stays legible on a light status bar and a dark one — but it does mean the logo's
// gradient is dropped here rather than lost by accident.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { Readable } from "node:stream";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { SVGIcons2SVGFontStream } from "svgicons2svgfont";
import svg2ttf from "svg2ttf";
import ttf2woff from "ttf2woff";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..");

const SOURCE = join(repoRoot, "assets", "devprune.svg");
const OUT_DIR = join(here, "icons");
const OUT_WOFF = join(OUT_DIR, "devprune.woff");

// Private Use Area. Any codepoint here is guaranteed never to collide with a real
// character, which matters because the character is what ends up in `package.json`.
const CODEPOINT = String.fromCodePoint(0xe000);

/// Strip the gradient and its `defs` block, leaving one solid path.
///
/// svgicons2svgfont only reads path geometry, so a `url(#...)` fill is harmless in
/// principle — but it emits a warning per icon and, more to the point, a source file
/// that still claims to be a two-stop gradient invites someone to wonder later why the
/// glyph is one colour. Making it black here says plainly that the colour is gone.
function monochrome(svg) {
	return svg
		.replace(/<defs>[\s\S]*?<\/defs>/g, "")
		.replace(/fill="url\(#[^)]*\)"/g, 'fill="#000000"');
}

const svgFont = await new Promise((resolve, reject) => {
	const chunks = [];
	const stream = new SVGIcons2SVGFontStream({
		fontName: "devprune",
		fontHeight: 1000,
		// The logo is drawn inside a 256x256 viewBox and sits well within it. Normalising
		// scales the glyph to fill the em square, so it ends up the same optical weight as
		// the codicons beside it instead of a third smaller.
		normalize: true,
		centerHorizontally: true,
		// The default logger writes one line per glyph to stdout, which turns a silent
		// build step into noise for a single icon.
		log: () => {},
	});

	stream.on("data", (chunk) => chunks.push(chunk.toString()));
	stream.on("end", () => resolve(chunks.join("")));
	stream.on("error", reject);

	const glyph = Readable.from([monochrome(readFileSync(SOURCE, "utf8"))]);
	glyph.metadata = { unicode: [CODEPOINT], name: "devprune-logo" };
	stream.write(glyph);
	stream.end();
});

const ttf = svg2ttf(svgFont, {
	copyright: "Copyright 2026 VKrishna04",
	description: "dev-prune product icon",
	version: "1.0",
});
const woff = ttf2woff(new Uint8Array(ttf.buffer));

mkdirSync(OUT_DIR, { recursive: true });
writeFileSync(OUT_WOFF, Buffer.from(woff.buffer));

console.log(
	`icon-font: wrote ${OUT_WOFF} (${Buffer.from(woff.buffer).length} bytes), ` +
		`glyph U+${CODEPOINT.codePointAt(0).toString(16).toUpperCase()}`,
);
