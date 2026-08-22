#!/bin/sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0
#
# `schemas/devprune.schema.json` is the config schema, and it exists three times.
#
# The binary embeds the canonical copy at compile time (`icon::EMBEDDED_SCHEMA_BYTES`),
# the VS Code extension ships its own so `.devprune.json` gets completions without a
# network round trip, and the site publishes one at the `$id` URL that every config
# file's `$schema` points at. Three files, one meaning — and nothing in the build derives
# any of them from another, so adding a config key to the canonical copy alone leaves
# editors and the published URL describing a schema that no longer exists. The failure is
# silent in all three places, which is why it is checked here rather than noticed later.
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

if [ "$status" -ne 0 ]; then
    echo >&2
    echo "Copy $canonical over each file listed above, then commit all of them." >&2
fi
exit "$status"
