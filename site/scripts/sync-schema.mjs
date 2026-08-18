// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

// Copies the canonical JSON Schema into the site's public tree before every build.
//
// The published URL (https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json)
// once drifted from `schemas/devprune.schema.json`: it kept advertising keys the CLI
// had removed (`custom_bloat_dirs`, `post_prune_command`) and, because the schema sets
// `additionalProperties: false`, flagged real keys like `min_size_mb` as errors in
// every editor that resolved it. Copying at build time makes the hosted file and the
// repository file the same bytes, so that cannot happen again.
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, "..", "..", "schemas", "devprune.schema.json");
const dest = join(here, "..", "public", "schemas", "v1", "devprune.schema.json");

mkdirSync(dirname(dest), { recursive: true });
copyFileSync(src, dest);
console.log("schema synced into site/public/schemas/v1/");
