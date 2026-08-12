#!/usr/bin/env sh
# Turn the release assets into publishable npm packages.
#
# Produces one package per platform, each holding the real binary and marked with `os`
# and `cpu` so npm installs exactly the matching one, plus the `dev-prune` dispatcher
# that depends on all six as optionalDependencies. This is the esbuild/biome layout, and
# it is why `npx dev-prune` works with nothing installed beforehand.
#
# The platform manifests are generated rather than checked in: six near-identical files
# whose only job is to carry the version are six files that drift.
#
# Usage: scripts/npm-prepare.sh <version> <assets-dir> <out-dir>
#   assets-dir  holds dev-prune-v<version>-<os>-<arch>.{tar.gz,zip} from the build job
#   out-dir     is created; each subdirectory is ready for `npm publish`
set -eu

version="${1:?usage: $0 <version> <assets-dir> <out-dir>}"
assets="${2:?usage: $0 <version> <assets-dir> <out-dir>}"
out="${3:?usage: $0 <version> <assets-dir> <out-dir>}"

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

mkdir -p "$out"

# Fed by heredoc rather than by a pipe: a piped `while` runs in a subshell, where the
# `exit 1` on a missing asset would end the subshell and let the script carry on to
# publish a set of packages with a hole in it.
#
# Columns: asset-os asset-arch node-platform node-arch archive-kind
# The asset names come from .github/workflows/release.yml; the node names come from
# `process.platform` and `process.arch`, which is what the launcher resolves against.
while read -r asset_os asset_arch node_os node_arch kind; do
    [ -n "$asset_os" ] || continue

    pkg="dev-prune-$node_os-$node_arch"
    dir="$out/$pkg"
    mkdir -p "$dir/bin"

    if [ "$kind" = zip ]; then
        archive="$assets/dev-prune-v$version-$asset_os-$asset_arch.zip"
        exe="dev-prune.exe"
        [ -f "$archive" ] || { echo "missing asset: $archive" >&2; exit 1; }
        unzip -o -q "$archive" -d "$dir/bin"
    else
        archive="$assets/dev-prune-v$version-$asset_os-$asset_arch.tar.gz"
        exe="dev-prune"
        [ -f "$archive" ] || { echo "missing asset: $archive" >&2; exit 1; }
        tar -xzf "$archive" -C "$dir/bin"
    fi

    [ -f "$dir/bin/$exe" ] || { echo "$archive did not contain $exe" >&2; exit 1; }

    # npm preserves the mode bits it finds in the tarball, so the executable bit has to
    # be set here rather than at install time.
    chmod +x "$dir/bin/$exe"

    # Apache-2.0 section 4(a) asks for a copy of the licence alongside the thing being
    # redistributed, and each of these is an independently installable package holding a
    # binary. The dispatcher carrying one is not enough: `--no-optional` or a direct
    # install puts the executable on a machine without it.
    cp "$repo_root/LICENSE.md" "$dir/LICENSE.md"

    cat > "$dir/package.json" <<EOF
{
  "name": "$pkg",
  "version": "$version",
  "description": "Prebuilt dev-prune binary for $node_os $node_arch. Installed automatically by the 'dev-prune' package; not meant to be depended on directly.",
  "os": ["$node_os"],
  "cpu": ["$node_arch"],
  "files": ["bin", "LICENSE.md"],
  "license": "Apache-2.0",
  "author": "VKrishna04",
  "homepage": "https://devprune.vkrishna04.me",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/Life-Experimentalist/dev-prune.git"
  },
  "bugs": {
    "url": "https://github.com/Life-Experimentalist/dev-prune/issues"
  }
}
EOF
    echo "prepared $pkg"
done <<'TARGETS'
linux   x64    linux   x64    tar
linux   arm64  linux   arm64  tar
darwin  x64    darwin  x64    tar
darwin  arm64  darwin  arm64  tar
windows x64    win32   x64    zip
windows arm64  win32   arm64  zip
TARGETS

# The dispatcher. Its own version and every optionalDependency version are rewritten
# from the tag, so a release cannot ship a manifest pinned to the previous one.
dispatcher="$out/dev-prune"
mkdir -p "$dispatcher"
cp -R "$repo_root/npm/bin" "$dispatcher/bin"
cp "$repo_root/README.md" "$dispatcher/README.md"
cp "$repo_root/LICENSE.md" "$dispatcher/LICENSE.md"

sed -e "s/\"version\": \"[^\"]*\"/\"version\": \"$version\"/" \
    -e "s/\(\"dev-prune-[a-z0-9]*-[a-z0-9]*\": \)\"[^\"]*\"/\1\"$version\"/" \
    "$repo_root/npm/package.json" > "$dispatcher/package.json"

# Prove the rewrite happened rather than trusting the sed. A dispatcher still pointing
# at the previous version installs the previous binary, which is a far quieter failure
# than not installing at all.
stale=$(grep -c "\"$version\"" "$dispatcher/package.json" || true)
if [ "$stale" -ne 7 ]; then
    echo "expected 7 occurrences of $version in the dispatcher manifest, found $stale" >&2
    cat "$dispatcher/package.json" >&2
    exit 1
fi
echo "prepared dev-prune (dispatcher) at $version"
