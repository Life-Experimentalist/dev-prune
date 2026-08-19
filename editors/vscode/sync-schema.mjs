// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Refresh the bundled schema from the canonical copy before packaging.
// Runs as `vscode:prepublish`, so every `.vsix` vsce produces — CI or by hand —
// carries the schema the CLI at that commit actually parses. The bundled copy
// is also committed, but this script is what stops it drifting: packaging
// regenerates it, the same reason site/scripts/sync-schema.mjs exists.
import { copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const canonical = join(here, "..", "..", "schemas", "devprune.schema.json");
const bundled = join(here, "schemas", "devprune.schema.json");

copyFileSync(canonical, bundled);
console.log(`bundled schema refreshed from ${canonical}`);
