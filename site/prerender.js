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

import { readFileSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const templatePath = resolve(root, 'dist/index.html');
const serverEntry = resolve(root, 'dist-ssr/entry-server.js');
const PLACEHOLDER = '<!--app-html-->';

if (!existsSync(templatePath)) {
  console.error(`prerender: ${templatePath} is missing — run the client build first.`);
  process.exit(1);
}
if (!existsSync(serverEntry)) {
  console.error(`prerender: ${serverEntry} is missing — run the SSR build first.`);
  process.exit(1);
}

const template = readFileSync(templatePath, 'utf8');
if (!template.includes(PLACEHOLDER)) {
  console.error(`prerender: index.html has no ${PLACEHOLDER} placeholder to fill.`);
  process.exit(1);
}

const { render } = await import(pathToFileURL(serverEntry).href);
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
