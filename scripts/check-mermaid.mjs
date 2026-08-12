// Parse every ```mermaid block in the repository's Markdown and fail on a syntax error.
//
// GitHub renders a malformed diagram as a red error box, and nothing in the normal build
// notices. Two diagrams in this repository shipped broken for exactly that reason — an
// unquoted `O(1)` in a node label — so the check runs in CI.
//
// Usage: node scripts/check-mermaid.mjs <file.md> [more.md ...]
// Requires `mermaid` and `jsdom` to be resolvable (CI installs them into site/).

import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

// Resolve `mermaid` and `jsdom` from the working directory rather than from this file's
// location, so the check runs whether they were installed at the repository root (CI) or
// inside site/ (a local `npm i` in the landing-site workspace).
const requireFromCwd = createRequire(path.join(process.cwd(), 'noop.js'));
const importFromCwd = async (name) =>
  import(pathToFileURL(requireFromCwd.resolve(name)).href);

const { JSDOM } = await importFromCwd('jsdom');

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

const { default: mermaid } = await importFromCwd('mermaid');

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
