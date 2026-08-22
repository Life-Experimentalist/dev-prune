#!/bin/sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0
#
# Turn this repository's copies of the WinGet manifests into the copies winget-pkgs
# accepts, in an output directory of your choosing.
#
# Two differences, both of which have rejected a submission before:
#
# 1. **No licence header.** Every file in this repository opens with the Apache-2.0 two-
#    liner, and `packaging/winget/` is no exception. A winget-pkgs manifest must open with
#    its `# yaml-language-server:` schema comment, so everything above that line is
#    dropped here rather than by hand at submission time.
# 2. **UTF-8 with a byte-order mark.** winget-pkgs validation rejects a manifest without
#    one. It is invisible in every editor, which is exactly why it is worth a script.
#
# Line endings are left LF; winget-pkgs accepts either and the BOM is what it checks.
#
# This is the same transform the release job runs, so a manifest submitted by hand and a
# manifest submitted by CI are byte-identical. `docs/RELEASING.md` used to spell the two
# rules out as prose for a person to follow, which is how the first submission went out
# with the wrong `Commands` list: a checklist nobody executes is not a check.
#
# Usage: sh scripts/winget-manifests.sh <output-directory>
#
# Run `sh scripts/render-packaging.sh <version>` first if the manifests do not already
# describe the version you are submitting.

set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: sh scripts/winget-manifests.sh <output-directory>" >&2
    exit 2
fi

out="$1"
src="packaging/winget"

if [ ! -d "$src" ]; then
    echo "winget-manifests: $src does not exist — run scripts/render-packaging.sh first" >&2
    exit 1
fi

mkdir -p "$out"

count=0
for file in "$src"/*.yaml; do
    [ -e "$file" ] || continue
    name="$(basename "$file")"
    dest="$out/$name"

    if ! grep -q '^# yaml-language-server:' "$file"; then
        echo "winget-manifests: $file has no '# yaml-language-server:' line" >&2
        exit 1
    fi

    # The BOM, then the file from its schema comment onward. `printf` rather than a
    # literal: a BOM pasted into a shell script is a BOM in the shell script.
    printf '\357\273\277' > "$dest"
    sed -n '/^# yaml-language-server:/,$p' "$file" >> "$dest"
    count=$((count + 1))
    echo "wrote $dest"
done

if [ "$count" -eq 0 ]; then
    echo "winget-manifests: no manifests found in $src" >&2
    exit 1
fi

version="$(sed -n 's/^PackageVersion: *//p' "$src/VKrishna04.dev-prune.yaml" | head -n 1)"
echo "$count manifest(s) for version ${version:-unknown} ready in $out"
