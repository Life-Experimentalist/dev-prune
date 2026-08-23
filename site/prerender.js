// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Static prerender step.
//
// `npm run build` runs three things in order: the client build, an SSR build of
// `src/entry-server.jsx`, and this script. It renders the app once and substitutes the
// markup for the `<!--app-html-->` placeholder in `dist/index.html`.
//
// The point is search engines and AI crawlers: without this, `dist/index.html` is an
// empty <div id="root"> and every crawler that does not execute JavaScript sees a blank
// page. With it, the shipped HTML is the whole page and the client merely hydrates.

import { readFileSync, writeFileSync, rmSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const templatePath = resolve(root, 'dist/index.html');
const serverEntry = resolve(root, 'dist-ssr/entry-server.js');
const PLACEHOLDER = '<!--app-html-->';

// Read rather than test-then-read: an `existsSync` before the read answers a question
// about the past, and the only thing it buys over catching the failure is a second
// syscall.
let template;
try {
  template = readFileSync(templatePath, 'utf8');
} catch {
  console.error(`prerender: ${templatePath} is missing or unreadable — run the client build first.`);
  process.exit(1);
}
if (!template.includes(PLACEHOLDER)) {
  console.error(`prerender: index.html has no ${PLACEHOLDER} placeholder to fill.`);
  process.exit(1);
}

let render;
try {
  ({ render } = await import(pathToFileURL(serverEntry).href));
} catch {
  console.error(`prerender: ${serverEntry} is missing or unloadable — run the SSR build first.`);
  process.exit(1);
}
const appHtml = render();

if (!appHtml || appHtml.length < 1000) {
  // A render that produces almost nothing means the component tree failed silently.
  // Shipping that would be worse than failing the build.
  console.error(`prerender: rendered markup is suspiciously small (${appHtml.length} bytes).`);
  process.exit(1);
}

writeFileSync(templatePath, template.replace(PLACEHOLDER, appHtml), 'utf8');
rmSync(resolve(root, 'dist-ssr'), { recursive: true, force: true });

console.log(`prerender: inlined ${appHtml.length} bytes of HTML into dist/index.html`);
