#!/usr/bin/env sh
# Print the CHANGELOG.md section for one version, without its heading.
#
# Used twice, and that is the point: CI runs it to prove the version in Cargo.toml has
# been written up before anyone can tag it, and the release workflow runs it to become
# the body of the GitHub release. A release note and a changelog entry that can drift
# apart eventually do.
#
# Usage: scripts/changelog-section.sh 1.0.0 [path/to/CHANGELOG.md]
# Exits 1 with a message on stderr if there is no section for that version.
set -eu

version="${1:-}"
changelog="${2:-CHANGELOG.md}"

if [ -z "$version" ]; then
    echo "usage: $0 <version> [changelog]" >&2
    exit 2
fi

if [ ! -f "$changelog" ]; then
    echo "$changelog: not found" >&2
    exit 2
fi

# Keep a Changelog headings look like `## [1.0.0] - 2026-08-12`. Matched by string
# prefix rather than by regex, so `1.0.0` cannot also match `1.0.0-rc1` and no part of
# the version has to be escaped.
section=$(
    awk -v heading="## [$version]" '
        !inside && index($0, heading) == 1 { inside = 1; next }
        inside && index($0, "## ") == 1 { exit }
        inside { print }
    ' "$changelog"
)

# Trim leading and trailing blank lines, so the release body neither starts with a gap
# nor carries the separator before the next version heading.
section=$(printf '%s\n' "$section" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;};/\n$/ba')

if [ -z "$section" ]; then
    echo "$changelog has no section for version $version." >&2
    echo "Add a '## [$version] - YYYY-MM-DD' heading with the notes for this release." >&2
    exit 1
fi

printf '%s\n' "$section"
