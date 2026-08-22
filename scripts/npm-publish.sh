#!/usr/bin/env sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# Publish the packages that scripts/npm-prepare.sh produced.
#
# This exists as a script rather than as a loop inside release.yml so that CI can run the
# exact same command with --dry-run. The v1.0.0 release failed here on a line CI could
# never have exercised: it packed the directories but never published them, so the one
# argument that was wrong was the one nothing tested.
#
# Usage: scripts/npm-publish.sh <version> <packages-dir> <dist-tag> [--dry-run|--local]
set -eu

usage="usage: $0 <version> <packages-dir> <dist-tag> [--dry-run|--local]"
version="${1:?$usage}"
packages="${2:?$usage}"
dist_tag="${3:?$usage}"
mode="${4:-}"

# An absolute path can never be mistaken for anything but a directory. A relative
# `npm-dist/dev-prune-linux-x64` matches npm's `<owner>/<repo>` GitHub shorthand, so npm
# skips the filesystem entirely and runs `git ls-remote
# ssh://git@github.com/npm-dist/dev-prune-linux-x64.git`, which fails with `Permission
# denied (publickey)` and looks nothing like the path bug it is. That is what broke the
# first v1.0.0 npm publish.
packages=$(CDPATH='' cd -- "$packages" && pwd)

# Set as positional parameters rather than as a string, so the flags reach npm as
# separate words without an unquoted expansion.
case "$mode" in
    --dry-run)
        # --provenance is dropped: it signs the tarball against the workflow's OIDC
        # identity, which a CI packaging job neither has nor should have.
        set -- --dry-run
        ;;
    --local)
        # Publishing from a workstation, which is how the eight names are created in the
        # first place: npm will only let you attach a trusted publisher to a package that
        # already exists, so the very first publish cannot come from OIDC and cannot come
        # from CI without a long-lived token. An interactive `npm login` session answers
        # the 2FA challenge itself, so no token needs to exist at all.
        #
        # --provenance is impossible here rather than merely unwanted: the attestation is
        # signed against a CI OIDC identity, and a laptop has none.
        set --
        ;;
    "")
        set -- --provenance
        ;;
    *)
        echo "unknown option: $mode" >&2
        exit 2
        ;;
esac

for pkg in \
    dev-prune-linux-x64 dev-prune-linux-arm64 \
    dev-prune-darwin-x64 dev-prune-darwin-arm64 \
    dev-prune-win32-x64 dev-prune-win32-arm64 dev-prune-win32-ia32 \
    dev-prune
do
    dir="$packages/$pkg"
    [ -d "$dir" ] || { echo "missing package directory: $dir" >&2; exit 1; }

    # Re-running a release is a normal thing to do after fixing one failed job. Without
    # this, the second run dies on EPUBLISHCONFLICT at whichever package succeeded the
    # first time and never reaches the ones that did not.
    if [ "$mode" != "--dry-run" ] && npm view "$pkg@$version" version >/dev/null 2>&1; then
        echo "$pkg@$version is already on the registry — skipping."
        continue
    fi

    echo "publishing $pkg@$version under --tag $dist_tag"
    # --access public is redundant for an unscoped package that already exists, and
    # mandatory for one that does not: `--provenance` refuses to sign a package it cannot
    # confirm is public, and a package with no published versions has no access setting to
    # read. All seven were new at 1.0.0, and dev-prune-win32-ia32 was new at 1.4.0.
    npm publish "$dir" "$@" --tag "$dist_tag" --access public
done
