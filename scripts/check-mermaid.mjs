// Parse every ```mermaid block in the repository's Markdown and fail on a syntax error.
//
// GitHub renders a malformed diagram as a red error box, and nothing in the normal build
// notices. Two diagrams in this repository shipped broken for exactly that reason — an
// unquoted `O(1)` in a node label — so the check runs in CI.
//
// Usage: node scripts/check-mermaid.mjs <file.md> [more.md ...]
// Requires `mermaid` and `jsdom`. CI installs them at the repository root with
// `npm install --no-save --no-package-lock mermaid@11 jsdom`; a local `npm i` inside
// site/ also satisfies it. Both locations are searched.

import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Resolve `mermaid` and `jsdom` from the working directory first, then from site/, so the
// check runs whether they were installed at the repository root (what CI does with
// `npm install --no-save`) or only inside the landing-site workspace (what a local
// `npm i` in site/ leaves behind). Resolving from cwd alone made this fail at the
// repository root on a developer machine, which reads as a broken diagram and is not.
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const searchRoots = [process.cwd(), path.join(repoRoot, 'site'), repoRoot];

const importDep = async (name) => {
  for (const root of searchRoots) {
    try {
      const resolved = createRequire(path.join(root, 'noop.js')).resolve(name);
      return await import(pathToFileURL(resolved).href);
    } catch {
      // Try the next root; only the last failure is worth reporting.
    }
  }
  console.error(
    `cannot resolve '${name}'. Install it with:\n` +
      `  npm install --no-save --no-package-lock mermaid@11 jsdom`,
  );
  process.exit(2);
};

const { JSDOM } = await importDep('jsdom');

const dom = new JSDOM('<!doctype html><html><body></body></html>');
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.Element = dom.window.Element;
globalThis.Node = dom.window.Node;
globalThis.HTMLElement = dom.window.HTMLElement;
globalThis.SVGElement = dom.window.SVGElement;
globalThis.MutationObserver = dom.window.MutationObserver;
try {
  Object.defineProperty(globalThis, 'navigator', {
    value: dom.window.navigator,
    configurable: true,
  });
} catch {
  // Node already exposes a read-only navigator; mermaid is happy with either.
}

const { default: mermaid } = await importDep('mermaid');

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error('usage: node scripts/check-mermaid.mjs <file.md> [...]');
  process.exit(2);
}

let blocks = 0;
let failed = 0;

for (const file of files) {
  const text = fs.readFileSync(file, 'utf8');
  const fence = /```mermaid\r?\n([\s\S]*?)```/g;
  let match;
  let index = 0;
  while ((match = fence.exec(text))) {
    index += 1;
    blocks += 1;
    try {
      await mermaid.parse(match[1]);
    } catch (error) {
      failed += 1;
      const detail = String(error?.message ?? error).split('\n').slice(0, 4).join('\n  ');
      console.error(`FAIL ${file} (mermaid block #${index}):\n  ${detail}`);
    }
  }
}

console.log(`mermaid: parsed ${blocks} block(s), ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
