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
# The site's JSON-LD carries the version too, and nothing renders it: a stale one is
# invisible to every reader and served to every crawler. It was found two releases
# behind, which is how it got here.
expect site/index.html "\"softwareVersion\": \"$version\""

# Docs that spell out a whole asset name, so the reader can copy it. These were found
# two releases stale, which is the same failure as the fallbacks: nothing runs a doc.
expect docs/DISTRIBUTION.md "dev-prune-v$version-linux-x64.tar.gz"
expect docs/RELEASES_AND_MANUAL_INSTALL.md "dev-prune-v$version-linux-x64.tar.gz"
expect docs/troubleshooting/INSTALLATION_ISSUES.md "dev-prune-v$version-windows-x64.zip"

# npm/package.json is a template — scripts/npm-prepare.sh rewrites every version in it
# from the tag before publishing, and asserts it rewrote exactly eight. So a stale number
# here never reaches the registry; it reaches the reader, and it sat at 1.1.0 for three
# releases because nothing looked. Checking it also pins the count the prepare script
# asserts: add a platform package here and forget the `-ne 8`, and the release fails
# loudly at packaging time rather than quietly at install time.
npm_stale="$(grep -n '": "[0-9]' npm/package.json | grep -vF "\"$version\"" || true)"
if [ -n "$npm_stale" ]; then
    echo "check-version: npm/package.json must say \"$version\" everywhere:" >&2
    echo "$npm_stale" >&2
    status=1
fi

# The `--json` samples in the CLI reference each carry a "version" field. `expect` would
# pass on one correct sample among six stale ones, so this checks every line instead —
# they were found a release behind, which is how the rule got here. Leading whitespace
# is matched loosely: a fenced sample nested inside a list item is indented, and
# pinning the indent to two spaces silently skipped the install receipt.
stale="$(grep -n '^ *"version": ' docs/CLI_REFERENCE.md | grep -vF "\"version\": \"$version\"," || true)"
if [ -n "$stale" ]; then
    echo "check-version: docs/CLI_REFERENCE.md JSON samples must say \"$version\":" >&2
    echo "$stale" >&2
    status=1
fi

# The `devp -V` sample in the same file prints the version twice: once at the end of the
# ASCII banner and once on the line below it. Nothing generates that block, so it sat at
# 1.3.0 for six releases while every checked file moved on. Every `vX.Y.Z` in the file
# belongs to that one sample, so all of them are checked rather than two line numbers
# being pinned -- a pinned line number goes stale the first time the file is edited.
banner="$(grep -nE 'v[0-9]+\.[0-9]+\.[0-9]+' docs/CLI_REFERENCE.md | grep -vF "v$version" || true)"
if [ -n "$banner" ]; then
    echo "check-version: the devp -V sample in docs/CLI_REFERENCE.md must say v$version:" >&2
    echo "$banner" >&2
    status=1
fi

if [ "$status" -eq 0 ]; then
    echo "Every file that restates the version agrees with Cargo.toml."
fi
exit "$status"
