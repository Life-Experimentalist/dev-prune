#!/bin/sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0
#
# `rust-version` in Cargo.toml is the single source of truth for the MSRV. Everything
# that can read it does: the binary bakes it in at compile time (`constants::MSRV`),
# CI's msrv job picks its toolchain from it, and release.yml writes it into the
# install table. Markdown and JSX cannot read it, so a handful of files restate the
# number by hand — this check is what keeps them honest. Bump `rust-version` and CI
# fails here, listing every file that still says the old number.
#
# Usage: sh scripts/check-msrv.sh   (from the repository root)

set -eu

msrv="$(sed -n 's/^rust-version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml)"
if [ -z "$msrv" ]; then
    echo "check-msrv: could not read rust-version from Cargo.toml" >&2
    exit 1
fi
echo "Cargo.toml rust-version: $msrv"

status=0
for file in \
    CLAUDE.md \
    CONTRIBUTING.md \
    README.md \
    docs/CLI_REFERENCE.md \
    docs/DISTRIBUTION.md \
    docs/RELEASES_AND_MANUAL_INSTALL.md \
    site/public/llms.txt \
    site/src/App.jsx; do
    if ! grep -qF "$msrv" "$file"; then
        echo "check-msrv: $file does not mention Rust $msrv — update it to match Cargo.toml" >&2
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "Every file that restates the MSRV agrees with Cargo.toml."
fi
exit "$status"
