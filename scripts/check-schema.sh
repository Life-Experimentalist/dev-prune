#!/bin/sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0
#
# `schemas/devprune.schema.json` is the config schema, and it exists three times.
#
# The binary embeds the canonical copy at compile time (`icon::EMBEDDED_SCHEMA_BYTES`),
# the VS Code extension bundles one so hand-written files validate offline, and the site
# serves one at `https://devprune.vkrishna04.me/schemas/v1/devprune.schema.json` — the URL
# in every generated file's `$schema` key, and the URL the SchemaStore catalog entry
# points at, which is how JetBrains, Visual Studio, Neovim and Zed resolve `.devprune.json`
# by filename with no extension installed. So the site copy is not the site's copy. It is
# what every subscribed editor on every machine downloads.
#
# Two sync scripts already regenerate the copies from the canonical file — site/scripts/
# sync-schema.mjs as the site's `prebuild`, editors/vscode/sync-schema.mjs as the
# extension's `vscode:prepublish`. Both run at build and packaging time, which is *after*
# the commit. So a change to the canonical schema can be committed, reviewed and merged
# with the other two files still describing the previous one, and the repository is only
# made honest again by the next deploy. This check moves that from "eventually" to "before
# it merges": run the sync, commit what it produced.
#
# Line endings are normalised before comparing; git may check any of these out either way.
#
# Usage: sh scripts/check-schema.sh   (from the repository root)

set -eu

canonical="schemas/devprune.schema.json"
copies="editors/vscode/schemas/devprune.schema.json site/public/schemas/v1/devprune.schema.json"

if [ ! -f "$canonical" ]; then
    echo "check-schema: $canonical is missing" >&2
    exit 1
fi

reference="$(mktemp)"
trap 'rm -f "$reference"' EXIT INT TERM
tr -d '\r' < "$canonical" > "$reference"
echo "canonical: $canonical"

status=0
for copy in $copies; do
    if [ ! -f "$copy" ]; then
        echo "MISSING  $copy" >&2
        status=1
    elif tr -d '\r' < "$copy" | diff -q "$reference" - > /dev/null 2>&1; then
        echo "ok       $copy"
    else
        echo "DIFFERS  $copy" >&2
        status=1
    fi
done

# The catalog entry at SchemaStore names one URL and nothing in this repository can change
# it — a PR to their repository can. So the schema's own `$id`, the constant the CLI writes
# into every generated `$schema` key, and the path the site publishes at all have to keep
# agreeing with that one URL. Move the published path without a matching SchemaStore PR and
# every JetBrains, Neovim and Zed user silently loses validation; this is the check that
# says so at the moment the path moves rather than in a bug report months later.
url="$(sed -n 's/.*"\$id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$canonical")"
constant="$(sed -n 's/^pub const JSON_SCHEMA_URL: &str = "\(.*\)";$/\1/p' src/constants.rs)"
published="site/public/schemas/v1/devprune.schema.json"

if [ -z "$url" ]; then
    echo "check-schema: $canonical has no \$id" >&2
    status=1
elif [ "$url" != "$constant" ]; then
    echo "DIFFERS  \$id is $url, src/constants.rs says $constant" >&2
    status=1
elif [ "${url##*/schemas/}" != "${published##*/schemas/}" ]; then
    echo "DIFFERS  \$id is $url, but the site publishes it at $published" >&2
    status=1
else
    echo "ok       \$id, src/constants.rs and the published path all say $url"
fi

if [ "$status" -ne 0 ]; then
    echo >&2
    echo "Copy $canonical over each file listed above, then commit all of them." >&2
    echo "If the URL moved, the SchemaStore catalog entry moves with it — see" >&2
    echo "docs/IDE_INTEGRATION.md." >&2
fi
exit "$status"
