// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Renders the banner set from site/banners/index.html.
//
// Image models cannot spell `node_modules` reliably at 1600px, and every
// regeneration of a hand-made PNG is a fresh chance to get the palette wrong.
// A browser sets the type perfectly every time, reads the same tokens the site
// reads, and re-renders the whole set from one command when the tagline moves.
//
// Uses playwright-core against the Chrome already installed on the machine, so
// nothing downloads a second browser.

import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");
const page_url = pathToFileURL(resolve(repoRoot, "site/banners/index.html")).href;
const outDir = resolve(repoRoot, "assets");

// id in the page -> filename that ships. Renaming here renames the asset, so
// the references in README.md and site/index.html move with it.
const ASSETS = [
  { id: "social", file: "social-banner.png" },
  { id: "readme", file: "github-readme-banner.png" },
  // The site serves the OG card as a JPEG on purpose — the reason is on the
  // og:image tag in site/index.html. Rendering both from the same element in
  // the same pass is the only way the card scrapers fetch cannot drift from
  // the one in assets/; the JPEG used to be updated by hand, and was not.
  {
    id: "og",
    file: "og-card.png",
    also: { path: "site/public/assets/og-card.jpg", quality: 92 },
  },
];

let chromium;
try {
  ({ chromium } = await import("playwright-core"));
} catch {
  console.error(
    "playwright-core is not installed. Run:\n\n  npm --prefix site install\n",
  );
  process.exit(1);
}

// Chrome first, then Edge — both are Chromium and both are already present on
// a Windows dev box. Only fall back to a downloaded browser if neither is.
async function launch() {
  for (const channel of ["chrome", "msedge"]) {
    try {
      return await chromium.launch({ channel });
    } catch {
      /* try the next one */
    }
  }
  return await chromium.launch();
}

const browser = await launch();
// Wider than the widest banner plus its page padding, so nothing overflows
// the viewport and gets clipped at the document edge.
const page = await browser.newPage({
  viewport: { width: 1760, height: 1000 },
  deviceScaleFactor: 1,
});

await page.goto(page_url, { waitUntil: "networkidle" });
// Webfonts decide the width of every line; screenshotting before they land
// produces a banner set in Segoe UI that looks almost right.
await page.evaluate(() => document.fonts.ready);

await mkdir(outDir, { recursive: true });

for (const { id, file, also } of ASSETS) {
  const el = page.locator(`#${id}`);
  const box = await el.boundingBox();
  await el.screenshot({ path: resolve(outDir, file) });
  console.log(`  ${file.padEnd(28)} ${box.width} x ${box.height}`);

  if (also) {
    const dest = resolve(repoRoot, also.path);
    await mkdir(dirname(dest), { recursive: true });
    await el.screenshot({ path: dest, type: "jpeg", quality: also.quality });
    console.log(`  ${also.path.padEnd(28)} ${box.width} x ${box.height}`);
  }
}

await browser.close();
console.log(`\nWrote ${ASSETS.length} banners to assets/`);
