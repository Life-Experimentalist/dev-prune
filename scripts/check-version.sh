#!/bin/sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0
#
# `version` in Cargo.toml is the single source of truth for the release version, and
# four files outside it restate the number by hand. None of them is read by anything
# that would notice being wrong:
#
#   - the install scripts pin a fallback used only when the GitHub API is unreachable,
#     so a stale one surfaces on a rate-limited runner months later, installing an old
#     release with no error;
#   - the site prints the current version on the homepage and in llms.txt, where the
#     only thing that notices a stale number is a reader — and both files have already
#     disagreed with each other in a deployed build.
#
# This check reads all four on every push. release.yml calls it too, one step after
# verifying Cargo.toml matches the tag, so at tag time "agrees with Cargo.toml" and
# "agrees with the tag" are the same statement.
#
# Usage: sh scripts/check-version.sh   (from the repository root)

set -eu

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml | head -n 1)"
if [ -z "$version" ]; then
    echo "check-version: could not read version from Cargo.toml" >&2
    exit 1
fi
echo "Cargo.toml version: $version"

status=0

# Exact strings, not a bare version match: every one of these files mentions other
# version numbers (an MSRV, a dependency, an example release), and a loose grep would
# pass on any of them.
expect() {
    if ! grep -qF "$2" "$1"; then
        echo "check-version: $1 must contain: $2" >&2
        status=1
    fi
}

expect scripts/install.sh "FALLBACK_VERSION=\"$version\""
expect scripts/install.ps1 "\$fallbackVersion = '$version'"
expect site/src/App.jsx "const VERSION = \"$version\";"
expect site/public/llms.txt "Version $version."

# Docs that spell out a whole asset name, so the reader can copy it. These were found
# two releases stale, which is the same failure as the fallbacks: nothing runs a doc.
expect docs/DISTRIBUTION.md "dev-prune-v$version-linux-x64.tar.gz"
expect docs/RELEASES_AND_MANUAL_INSTALL.md "dev-prune-v$version-linux-x64.tar.gz"
expect docs/troubleshooting/INSTALLATION_ISSUES.md "dev-prune-v$version-windows-x64.zip"

if [ "$status" -eq 0 ]; then
    echo "Every file that restates the version agrees with Cargo.toml."
fi
exit "$status"
