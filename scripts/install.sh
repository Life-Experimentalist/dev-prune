#!/usr/bin/env sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# dev-prune installer — Linux, macOS, and Windows under Git Bash / MSYS2 / Cygwin
# One-liner: curl -fsSL https://devprune.vkrishna04.me/install.sh | sh
#
# What this does:
#   1. Downloads the release binary for your platform from GitHub Releases
#   2. Verifies it against the published SHA-256 checksum
#   3. Installs it to the dev-prune config bin directory, as both `dev-prune` and `devp`
#   4. Adds that directory to PATH
#   5. Runs `dev-prune setup`, which installs whatever was missing: the exported
#      SKILL.md, the global Git auto-registration hooks, and the background scheduler
#
# Step 5 is what makes dev-prune work without being thought about. It is skippable
# (--no-auto-setup), every piece of it is reversible with `devp uninstall`, and it refuses
# to take over a `core.hooksPath` that already belongs to husky or pre-commit.
#
# What it deliberately does NOT do: modify your editor settings, delete binaries other
# installers put elsewhere, or register any repositories. Run `devp init <dir>` yourself.
#
# Options (see --help). Piping into `sh` needs `-s --` to pass any of them:
#   curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --no-auto-setup
# The matching DEV_PRUNE_* environment variables work with the plain one-liner and are
# the fallback for every option.

set -eu

VERSION=""
BIN_DIR_ARG=""
NO_PATH=0
NO_AUTO_SETUP=0
FORCE=0
VERSION_EXPLICIT=0

usage() {
    cat <<'EOF'
dev-prune installer

  --version <tag>    release to install (default: the version this script ships with)
  --bin-dir <dir>    install directory (default: the dev-prune config dir's bin/)
  --no-path          do not edit any shell rc file or the Windows User PATH
  --no-auto-setup    install the binary only; skip SKILL.md, Git hooks and the scheduler
  --force            reinstall even if this version is already installed here
  --help             this message

Piping into a shell needs `-s --` before the options:
  curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --no-auto-setup

The equivalent environment variables work with the plain one-liner:
  DEV_PRUNE_VERSION  DEV_PRUNE_BIN_DIR  DEV_PRUNE_NO_PATH=1  DEV_PRUNE_NO_AUTO_SETUP=1
  DEV_PRUNE_FORCE=1

DEV_PRUNE_NO_MIGRATE_PROMPT=1 has no flag. It suppresses the one question this script
asks -- whether to move a copy another package manager owns -- and prints the command
instead. CI, and having no terminal at all, suppress it already.

Re-running this is safe. An install that is already here, current and on PATH is left
alone and exits 0; an older one is replaced in place; a newer one is not downgraded.
EOF
}

# An unknown option is an error rather than something to ignore: a typo'd --no-auto-setup
# that silently installs the scheduler anyway is worse than a message.
while [ $# -gt 0 ]; do
    case "$1" in
        --version) [ $# -ge 2 ] || { echo "--version needs a value" >&2; exit 2; }; VERSION="$2"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; shift ;;
        --bin-dir) [ $# -ge 2 ] || { echo "--bin-dir needs a value" >&2; exit 2; }; BIN_DIR_ARG="$2"; shift 2 ;;
        --bin-dir=*) BIN_DIR_ARG="${1#*=}"; shift ;;
        --no-path) NO_PATH=1; shift ;;
        --no-auto-setup) NO_AUTO_SETUP=1; shift ;;
        --force) FORCE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; echo "" >&2; usage >&2; exit 2 ;;
    esac
done

# An option beats its environment variable: it is the more explicit of the two, and it is
# the one the user typed on the command line they are looking at.
[ -n "$VERSION" ] || VERSION="${DEV_PRUNE_VERSION:-}"
[ "$NO_PATH" = "1" ] || NO_PATH="${DEV_PRUNE_NO_PATH:-0}"
[ "$NO_AUTO_SETUP" = "1" ] || NO_AUTO_SETUP="${DEV_PRUNE_NO_AUTO_SETUP:-0}"
[ "$FORCE" = "1" ] || FORCE="${DEV_PRUNE_FORCE:-0}"

# A version the user named is a version the user meant, however it was named. It is the
# one thing that makes installing *backwards* the right answer, so it is remembered
# before the next block fills VERSION in with whatever GitHub says is newest.
[ -z "$VERSION" ] || VERSION_EXPLICIT=1
REPO="Life-Experimentalist/dev-prune"

# With no version pinned, ask GitHub which release is newest. The redirect target of
# /releases/latest carries the tag, so one HEAD request answers without parsing JSON.
# FALLBACK_VERSION exists for offline mirrors and rate-limited CI: it must always name
# a published release, and the release workflow refuses to tag until it matches.
FALLBACK_VERSION="1.21.0"
if [ -z "$VERSION" ]; then
    LATEST_URL="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)"
    case "$LATEST_URL" in
        */tag/v*) VERSION="${LATEST_URL##*/tag/v}" ;;
        *) VERSION="$FALLBACK_VERSION" ;;
    esac
fi
VERSION="${VERSION#v}"

RAW_OS="$(uname -s)"
ARCH="$(uname -m)"

# Git Bash, MSYS2 and Cygwin are POSIX shells on Windows: the same script can serve
# them, but the asset, the file extension and the install directory are all Windows'.
case "$RAW_OS" in
    MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
    Darwin) OS="darwin" ;;
    Linux) OS="linux" ;;
    *) echo "Unsupported operating system: $RAW_OS" >&2; exit 1 ;;
esac

# The OS is resolved first because 32-bit is not a portable answer: a 32-bit x86 build
# is published for Windows and for nothing else. On Linux `i686` has to be refused, and
# refusing is the point -- installing the x64 asset on a machine that cannot run it
# produces "cannot execute binary file", which names nothing.
case "$ARCH" in
    x86_64|amd64) TARGET_ARCH="x64" ;;
    aarch64|arm64) TARGET_ARCH="arm64" ;;
    i386|i486|i586|i686)
        # A 32-bit MSYS or Cygwin on 64-bit Windows also reports i686. It gets the x86
        # build, which is correct for it: a 32-bit process cannot load a 64-bit image.
        if [ "$OS" = "windows" ]; then
            TARGET_ARCH="x86"
        else
            echo "Unsupported architecture: $ARCH. The 32-bit x86 build is published for Windows only." >&2
            echo "Build one from source instead: cargo install dev-prune" >&2
            exit 1
        fi
        ;;
    *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

if [ "$OS" = "windows" ]; then
    EXT=".exe"
    ARCHIVE_EXT="zip"
else
    EXT=""
    ARCHIVE_EXT="tar.gz"
fi

# Must match Registry::config_dir(), which uses the platform config dir:
# %APPDATA% on Windows, ~/Library/Application Support on macOS, $XDG_CONFIG_HOME
# (or ~/.config) on Linux. A binary installed anywhere else is a binary dev-prune's
# own alias and uninstall paths will never find.
if [ -n "$BIN_DIR_ARG" ]; then
    BIN_DIR="$BIN_DIR_ARG"
elif [ -n "${DEV_PRUNE_BIN_DIR:-}" ]; then
    BIN_DIR="$DEV_PRUNE_BIN_DIR"
elif [ "$OS" = "windows" ]; then
    # $APPDATA arrives as a Windows path (C:\Users\...); every tool below wants a
    # POSIX one. cygpath ships with Git Bash, MSYS2 and Cygwin alike.
    if command -v cygpath >/dev/null 2>&1; then
        APPDATA_UNIX="$(cygpath -u "${APPDATA:-$HOME/AppData/Roaming}")"
    else
        APPDATA_UNIX="${APPDATA:-$HOME/AppData/Roaming}"
    fi
    BIN_DIR="$APPDATA_UNIX/dev-prune/bin"
elif [ "$OS" = "darwin" ]; then
    BIN_DIR="$HOME/Library/Application Support/dev-prune/bin"
else
    BIN_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/dev-prune/bin"
fi
EXE_PATH="$BIN_DIR/dev-prune$EXT"
ALIAS_PATH="$BIN_DIR/devp$EXT"

# Where `dev-prune` resolves *before* this script writes anything, when that is not
# the copy this script manages. Running the installer over a cargo, npm or Homebrew
# install does not fail and does not remove that copy — removing another package
# manager's file behind its back is how installations become unrepairable. But the
# copy stays on PATH afterwards, and a user who does not know it is there upgrades
# one binary and keeps running the other. So it is named at the end, with the command
# that migrates it properly.
PRIOR_EXE=""
if command -v dev-prune >/dev/null 2>&1; then
    PRIOR_EXE=$(command -v dev-prune)
    # An explicit `if`, not `[ ... ] && ...`: under `set -e` an AND-list that ends
    # false is a failing command, and it would abort the install in exactly the
    # case this is here to detect.
    if [ "$PRIOR_EXE" = "$EXE_PATH" ]; then
        PRIOR_EXE=""
    fi
fi

# Whether there is a person at the other end who can answer a question.
#
# `curl ... | sh` means stdin *is* this script: a bare `read` would swallow the rest of
# the file and run a truncated install, which is the reason this script asked nothing at
# all until now. /dev/tty is the terminal itself and the pipe does not touch it, so every
# prompt here reads from it by name. Its absence is the signal, not an obstacle: a
# container build, a CI job and a provisioning run all have no terminal, and every one of
# them wants the install and none of the conversation.
a_person_is_present() {
    if [ "${DEV_PRUNE_NO_MIGRATE_PROMPT:-}" = "1" ]; then
        return 1
    fi
    # Set by every CI provider worth naming, and by nothing with a keyboard attached.
    if [ -n "${CI:-}" ]; then
        return 1
    fi
    # `[ -r /dev/tty ]` is not enough. In a container without a terminal the device node
    # exists and is readable by mode, and opening it fails with ENXIO. Opening it is the
    # only test that answers the question actually being asked. In a subshell so the
    # descriptor closes with it.
    if ! (exec 3<>/dev/tty) 2>/dev/null; then
        return 1
    fi
    return 0
}

# One yes/no question on the terminal. Default no, and every non-answer is a no: this is
# only ever asked before running another package manager, and a stray newline pasted
# after the one-liner must never be the thing that authorises that.
ask_yes_no() {
    printf '%s [y/N]: ' "$1" > /dev/tty
    _answer=""
    read -r _answer < /dev/tty || _answer=""
    case "$_answer" in
        y | Y | yes | Yes | YES) return 0 ;;
        *) return 1 ;;
    esac
}

# Printed from three endings now, so it lives in one place.
report_prior_exe() {
    if [ -n "$PRIOR_EXE" ]; then
        echo ""
        echo "[!] Another dev-prune is on your PATH as well:"
        echo "        $PRIOR_EXE"
        echo "    A different package manager owns that copy, so this script left it alone."
        # Not "this directory comes first on PATH". Nothing here can promise that: the
        # rc line this script writes does put $BIN_DIR in front, but another manager's
        # rc line sourced after it puts its own directory in front of that, and the
        # claim was printed either way. What is true is where to look.
        if [ "$NO_PATH" = "1" ]; then
            echo "    PATH was left alone (--no-path), so which one you get is up to yours."
        else
            echo "    Which one a new shell finds is PATH order, and other managers write"
            echo "    their own rc lines too. 'devp doctor' names the copy that answered."
        fi
        echo "    Moving it over means uninstalling there, through the manager that put it"
        echo "    there. 'devp install --channel installer' does that."
        # Not offered under a pin: the copy at $EXE_PATH would refuse, and an offer that
        # ends in a refusal is worse than no offer.
        if [ "$INSTALLED_LOCK" != "1" ] && [ -x "$EXE_PATH" ] &&
            a_person_is_present && ask_yes_no "    Do that now?"; then
            echo ""
            # The copy just installed, not the old one. Handing this to the *old* binary
            # was the whole failure: nothing before 1.8.0 has an `install` subcommand at
            # all, so on the machines this offer exists for it printed an unrecognised-
            # subcommand error and nothing moved. The new copy is new by definition, it
            # can name the manager that owns the file above, and it runs that manager's
            # own uninstall.
            #
            # Nothing is deleted by this script either way.
            if "$EXE_PATH" install --channel installer --yes; then
                echo ""
                echo "[OK] Done. 'devp doctor' will confirm only one copy is left."
            else
                echo ""
                echo "[!] That did not finish, and this script deleted nothing — the copy at"
                echo "        $PRIOR_EXE"
                echo "    is exactly where it was. Its own manager can still remove it, and"
                echo "    'devp doctor' names both copies and the command for each."
            fi
        elif [ "$INSTALLED_LOCK" = "1" ]; then
            echo "    Your version pin covers this too: which copy answers on PATH is which"
            echo "    version runs, so nothing moves while the pin is on. Release it with"
            echo "        devp config set version_lock false"
            echo "    'devp doctor' lists every copy on the machine at any time."
        else
            echo "    Run it whenever you like:"
            echo "        devp install --channel installer"
            echo "    'devp doctor' lists every copy on the machine at any time."
        fi
    fi
}

# The version already sitting at the path this script manages, if any. Both switches
# matter: `--version` on this CLI is handled in code rather than short-circuited by the
# argument parser, so an unguarded call could have the *old* binary register a
# scheduler or reach GitHub in the middle of installing its replacement.
INSTALLED_VERSION=""
if [ -x "$EXE_PATH" ]; then
    INSTALLED_VERSION=$(DEV_PRUNE_NO_AUTO_SETUP=1 DEV_PRUNE_OFFLINE=1 "$EXE_PATH" --version 2>/dev/null |
        grep -o '[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*' | head -n 1 || true)
fi

# ...and whether that copy is pinned. version_lock is the one setting that outranks this
# script: somebody who set it asked for the binary not to change, and a one-liner re-run
# out of habit is precisely the accident it exists to stop.
INSTALLED_LOCK=0
if [ -n "$INSTALLED_VERSION" ]; then
    if DEV_PRUNE_NO_AUTO_SETUP=1 DEV_PRUNE_OFFLINE=1 "$EXE_PATH" config get version_lock 2>/dev/null |
        grep -q true; then
        INSTALLED_LOCK=1
    fi
fi

# How $1 stands to $2: newer, older or same. The answer is printed rather than returned
# as an exit status, because under `set -e` a function that answers "no" by failing is
# a function that ends the script. Compared field by field and numerically: a string
# compare sorts 1.10.0 before 1.9.0.
version_rel() {
    _i=1
    while [ "$_i" -le 3 ]; do
        _x=$(echo "$1" | cut -d. -f"$_i")
        _y=$(echo "$2" | cut -d. -f"$_i")
        case "$_x" in '' | *[!0-9]*) _x=0 ;; esac
        case "$_y" in '' | *[!0-9]*) _y=0 ;; esac
        if [ "$_x" -gt "$_y" ]; then echo newer; return 0; fi
        if [ "$_x" -lt "$_y" ]; then echo older; return 0; fi
        _i=$((_i + 1))
    done
    echo same
}

# Whether some shell rc file already puts *this* BIN_DIR on PATH. Only consulted to
# decide whether re-running has anything left to do: a "no" falls through to the
# ordinary install, which configures PATH as part of its normal work.
rc_configured() {
    if [ "$NO_PATH" = "1" ] || [ "$OS" = "windows" ]; then
        return 0
    fi
    for _rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile"; do
        if [ -f "$_rc" ] && grep -qF "export PATH=\"$BIN_DIR:\$PATH\"" "$_rc"; then
            return 0
        fi
    done
    _fish="$HOME/.config/fish/config.fish"
    if [ -f "$_fish" ] && grep -qF "fish_add_path '$BIN_DIR'" "$_fish"; then
        return 0
    fi
    return 1
}

# Re-running the one-liner is the most common thing anyone does with it — it is what
# the README, the release page and every "just reinstall it" answer tell people to do.
# So it has to be safe *and* quiet: an install that is already correct is left exactly
# as it is and exits 0, and one that is merely out of date is replaced without asking.
# The one thing this will not do on its own is install backwards.
#
# One branch sits above all of that, including the silent in-place update: a pinned
# install is not updated, repaired or downgraded by this script at all. --force is the
# only way past it, and it has to be typed.
if [ "$INSTALLED_LOCK" = "1" ] && [ "$FORCE" != "1" ]; then
    echo ""
    echo "[!] dev-prune v${INSTALLED_VERSION} at:"
    echo "        $EXE_PATH"
    echo "    has version_lock set, so this script changed nothing."
    echo "    Release the pin and re-run:"
    echo "        devp config set version_lock false"
    echo "    Or install over it just this once with --force."
    report_prior_exe
    exit 0
fi

if [ -n "$INSTALLED_VERSION" ] && [ "$FORCE" != "1" ]; then
    REL=$(version_rel "$INSTALLED_VERSION" "$VERSION")
    if [ "$REL" = "newer" ] && [ "$VERSION_EXPLICIT" != "1" ]; then
        echo ""
        echo "[OK] dev-prune v${INSTALLED_VERSION} is already installed at:"
        echo "        $EXE_PATH"
        echo "     That is newer than the v${VERSION} this run resolved to, so nothing"
        echo "     was changed. Re-run with --force to install v${VERSION} over it."
        report_prior_exe
        exit 0
    fi
    if [ "$REL" = "same" ] && [ -e "$ALIAS_PATH" ] && rc_configured; then
        echo ""
        echo "[OK] dev-prune v${VERSION} is already installed at:"
        echo "        $EXE_PATH"
        if [ "$NO_PATH" = "1" ]; then
            echo "     'devp' is beside it. Nothing to do."
        else
            echo "     It is on PATH and 'devp' is beside it. Nothing to do."
        fi
        echo "     Re-run with --force to download and write it again."
        report_prior_exe
        exit 0
    fi
    # Same version, but something is missing — no `devp`, or nothing on PATH
    # pointing here. Falling through reinstalls and repairs it, which is the whole
    # reason someone re-runs the one-liner after an install went wrong.
fi

echo ""
if [ -n "$INSTALLED_VERSION" ] && [ "$INSTALLED_VERSION" != "$VERSION" ]; then
    echo "-> Updating dev-prune v${INSTALLED_VERSION} -> v${VERSION}"
else
    echo "-> Installing dev-prune v${VERSION}"
fi

if ! command -v curl >/dev/null 2>&1; then
    echo "Required command 'curl' not found." >&2
    echo "Install it, or build from source with: cargo install dev-prune" >&2
    exit 1
fi

# `tar` extracts .zip too when it is bsdtar (Windows 10+, macOS), but Git Bash's is
# GNU tar, which cannot. Pick whichever extractor is actually present.
if [ "$ARCHIVE_EXT" = "zip" ]; then
    if command -v unzip >/dev/null 2>&1; then
        EXTRACT="unzip"
    elif tar --version 2>/dev/null | grep -qi bsdtar; then
        EXTRACT="tar"
    else
        echo "Neither 'unzip' nor bsdtar found — cannot extract the release archive." >&2
        echo "Install unzip, use scripts/install.ps1 from PowerShell, or: cargo install dev-prune" >&2
        exit 1
    fi
elif command -v tar >/dev/null 2>&1; then
    EXTRACT="tar"
else
    echo "Required command 'tar' not found." >&2
    echo "Install it, or build from source with: cargo install dev-prune" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    SHA_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
    SHA_CMD="shasum -a 256"
else
    echo "Neither sha256sum nor shasum found — cannot verify the download." >&2
    echo "Install one of them, or build from source with: cargo install dev-prune" >&2
    exit 1
fi

ASSET="dev-prune-v${VERSION}-${OS}-${TARGET_ARCH}.${ARCHIVE_EXT}"
BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"

TMP_DIR="$(mktemp -d)"
# shellcheck disable=SC2064
trap "rm -rf '$TMP_DIR'" EXIT INT TERM

echo "-> Downloading ${ASSET}"
# Download to a file rather than piping into the extractor: a truncated or error-page
# response piped straight into `tar -xz` can leave a half-extracted tree behind, and
# there is no opportunity to check a checksum.
if ! curl -fsSL "${BASE_URL}/${ASSET}" -o "$TMP_DIR/$ASSET"; then
    echo "Download failed: ${BASE_URL}/${ASSET}" >&2
    echo "Check that v${VERSION} has a build for ${OS}-${TARGET_ARCH}." >&2
    exit 1
fi

echo "-> Verifying checksum"
if curl -fsSL "${BASE_URL}/${ASSET}.sha256" -o "$TMP_DIR/$ASSET.sha256"; then
    # `tr -d '\r'`: a checksum file that picked up CRLF line endings anywhere along the
    # way (a proxy, a Windows-side mirror) would leave a trailing \r on the hash and
    # fail every comparison with a message that looks exactly like a corrupt download.
    EXPECTED="$(tr -d '\r' < "$TMP_DIR/$ASSET.sha256" | cut -d' ' -f1)"
    ACTUAL="$($SHA_CMD "$TMP_DIR/$ASSET" | cut -d' ' -f1)"
    if [ "$EXPECTED" != "$ACTUAL" ]; then
        echo "Checksum mismatch — refusing to install." >&2
        echo "  expected: $EXPECTED" >&2
        echo "  actual:   $ACTUAL" >&2
        exit 1
    fi
    echo "[OK] Checksum verified"
else
    echo "No published checksum for this release — refusing to install an unverified binary." >&2
    echo "Install from source instead: cargo install dev-prune" >&2
    exit 1
fi

# Extract into the temp dir first so a malformed archive cannot scatter files into
# the install directory.
if [ "$EXTRACT" = "unzip" ]; then
    unzip -q -o "$TMP_DIR/$ASSET" -d "$TMP_DIR"
elif [ "$ARCHIVE_EXT" = "zip" ]; then
    tar -xf "$TMP_DIR/$ASSET" -C "$TMP_DIR"
else
    tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"
fi
if [ ! -f "$TMP_DIR/dev-prune$EXT" ]; then
    echo "Archive did not contain a 'dev-prune$EXT' binary at its root." >&2
    exit 1
fi

mkdir -p "$BIN_DIR"

# Replacing a running executable.
#
# This is the upgrade path, and writing *into* the old file is exactly what fails while
# the scheduled prune pass happens to be running: POSIX returns ETXTBSY for a busy
# image, and Windows holds an outright lock. Renaming over it does work everywhere — the
# running process keeps the inode it already opened — so stage the new binary beside the
# old one and move it into place in a single step. That also means the install is atomic:
# there is no instant at which `dev-prune` on PATH is a half-written file.
install_binary() {
    src="$1"
    dest="$2"
    staged="$dest.new"
    cp "$src" "$staged"
    chmod +x "$staged"
    if ! mv -f "$staged" "$dest"; then
        rm -f "$staged"
        echo "Could not replace $dest — is a dev-prune process still running?" >&2
        return 1
    fi
}

install_binary "$TMP_DIR/dev-prune$EXT" "$EXE_PATH"
echo "[OK] Installed: $EXE_PATH"

# `devp` as a real entry on PATH, not a shell alias.
#
# A shell alias only exists in shells that sourced the rc file that defined it — not in
# cmd, not in an IDE's task runner, not in a cron job or a scheduled task. A second
# name in the same directory is `devp` everywhere at once, and cannot fall out of sync
# with an rc file nobody re-sources.
rm -f "$ALIAS_PATH"
if [ "$OS" = "windows" ] || ! ln -s "$EXE_PATH" "$ALIAS_PATH" 2>/dev/null; then
    # Windows symlinks need Developer Mode or elevation; a copy always works.
    install_binary "$TMP_DIR/dev-prune$EXT" "$ALIAS_PATH"
fi
echo "[OK] Installed: $ALIAS_PATH"

# What this run actually did, written down beside the binary it installed.
#
# This script, install.ps1 and the binary each used to derive the same facts on their
# own, and three derivations of one truth is how they drift. `devp doctor` and `devp
# install` read this file; it outlives the shell that ran the one-liner, which no
# variable here does. It is a record, never a setting: `Channel::detect()` is still what
# classifies a copy, because no receipt can describe one that arrived through
# `cargo install`, and every reader treats a missing file as "no installer of ours wrote
# one" rather than as an error.
#
# Written by hand rather than by the binary it just installed, because it has to be true
# even when `--no-auto-setup` means that binary is never run. `src/receipt.rs` has a test
# asserting these exact field names for that reason.
#
# Backslashes and double quotes are the only two characters a path can hold that would
# stop this file being JSON, and a --bin-dir given in Windows form holds the first one
# by definition.
# shellcheck disable=SC2001  # the replacement is a regex; ${var//x/y} is not POSIX sh.
json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

write_receipt() {
    _receipt="$BIN_DIR/install.json"
    if {
        echo '{'
        echo '  "schema": 1,'
        echo "  \"version\": \"$(json_escape "$VERSION")\","
        echo '  "channel": "installer",'
        echo '  "installed_by": "install.sh",'
        echo "  \"installed_at\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\","
        echo "  \"exe\": \"$(json_escape "$EXE_PATH")\","
        echo '  "alias": true,'
        echo "  \"path_entry\": $PATH_ENTRY"
        echo '}'
    } > "$_receipt.new" 2>/dev/null && mv -f "$_receipt.new" "$_receipt" 2>/dev/null; then
        return 0
    fi
    rm -f "$_receipt.new" 2>/dev/null || true
    echo "[!] Could not write $_receipt. Harmless: it is a note about this install,"
    echo "    not a setting, and nothing reads it to decide anything."
}

RC_TOUCHED=0
# Whether *this script* is the reason the directory is on PATH, as opposed to finding it
# already there or being told to leave PATH alone. Recorded in the receipt, and the only
# one of its fields nothing else on the machine can work out afterwards.
PATH_ENTRY=false

if [ "$NO_PATH" != "1" ]; then
    # "Already configured" means the rc file exports *this* BIN_DIR, not merely that
    # some past install left its `# dev-prune` marker: a reinstall with a different
    # --bin-dir used to match the old marker and silently leave PATH pointing at a
    # directory the binary is no longer in.
    RC_LINE="export PATH=\"$BIN_DIR:\$PATH\""
    for file in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile"; do
        if [ -f "$file" ]; then
            if grep -qF "$RC_LINE" "$file"; then
                RC_TOUCHED=1
                PATH_ENTRY=true
            else
                echo "-> Adding dev-prune to PATH in $file"
                # `$PATH` must reach the rc file unexpanded: expanding it here would
                # freeze this moment's PATH into the user's shell forever.
                # shellcheck disable=SC2016
                printf '\n# dev-prune\nexport PATH="%s:$PATH"\n' "$BIN_DIR" >> "$file"
                RC_TOUCHED=1
                PATH_ENTRY=true
            fi
        fi
    done

    FISH_CFG="$HOME/.config/fish/config.fish"
    if [ -f "$FISH_CFG" ]; then
        if ! grep -qF "fish_add_path '$BIN_DIR'" "$FISH_CFG"; then
            # Quoted: the macOS default is "~/Library/Application Support/dev-prune/bin",
            # which fish would otherwise split into two arguments.
            printf "\n# dev-prune\nfish_add_path '%s'\n" "$BIN_DIR" >> "$FISH_CFG"
        fi
        RC_TOUCHED=1
        PATH_ENTRY=true
    fi

    # A fresh container, or a shell whose rc file simply is not one of the five above.
    # Silently doing nothing here is how someone ends up with a working binary that
    # every new terminal claims not to have, so say it plainly instead.
    if [ "$RC_TOUCHED" = "0" ] && [ "$OS" != "windows" ]; then
        echo "[!] No shell rc file found to edit (~/.zshrc, ~/.bashrc, ~/.bash_profile,"
        echo "    ~/.config/fish/config.fish). Add this line to whichever one you use:"
        echo "        export PATH=\"$BIN_DIR:\$PATH\""
    fi

    # On Windows an rc file only helps inside Git Bash. Register the directory in the
    # User PATH as well, so cmd, PowerShell and every GUI-launched process see it too.
    if [ "$OS" = "windows" ] && command -v powershell.exe >/dev/null 2>&1; then
        WIN_BIN_DIR="$BIN_DIR"
        if command -v cygpath >/dev/null 2>&1; then
            WIN_BIN_DIR="$(cygpath -w "$BIN_DIR")"
        fi
        echo "-> Adding $WIN_BIN_DIR to your Windows User PATH"
        # Read-modify-write straight through the registry. Not `setx`, which truncates
        # any PATH longer than 1024 characters; and not
        # [Environment]::GetEnvironmentVariable, which expands %USERPROFILE% and every
        # other reference on the way out and then writes the expanded text back as a
        # plain string. A PATH that followed the profile would come back frozen to this
        # machine, and nothing about it would look wrong afterwards.
        #
        # Prepended, not appended. This is the same order install.ps1 writes: an install
        # that lands behind whatever a previous package manager left on PATH is an
        # install the user has no way to tell happened.
        #
        # The single quotes are the point: this is PowerShell source, and
        # `$dir`/`$env:`/`$userPath` are its variables, not the shell's. The directory
        # crosses over as an environment variable so it never has to be quoted into the
        # script text rather than being interpolated into it.
        # shellcheck disable=SC2016
        if DEV_PRUNE_WIN_BIN_DIR="$WIN_BIN_DIR" powershell.exe -NoProfile -NonInteractive -Command '
            $dir = $env:DEV_PRUNE_WIN_BIN_DIR
            $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
            $userPath = ""
            if ($null -ne $key) {
                $raw = [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames
                $userPath = [string]$key.GetValue("Path", "", $raw)
            }
            if (($userPath -split ";") -notcontains $dir) {
                $trimmed = $userPath.Trim(";")
                $newPath = if ($trimmed) { $dir + ";" + $trimmed } else { $dir }
                if ($null -ne $key) {
                    $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
                    try { $kind = $key.GetValueKind("Path") } catch {}
                    if ($kind -ne [Microsoft.Win32.RegistryValueKind]::String) {
                        $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
                    }
                    $key.SetValue("Path", $newPath, $kind)
                } else {
                    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
                }
                # Tell the already-running desktop. Without this, Explorer and every
                # program it launches keep the old PATH until the next sign-in.
                #
                # Borrowed from the .NET environment API rather than sent by hand:
                # SetEnvironmentVariable is documented to notify other applications for
                # the User target, and the notification names the whole Environment key,
                # so a throwaway variable refreshes the Path set above. Written and
                # deleted in the same breath.
                #
                # Not by compiling a scrap of C# at run time to call the window-
                # message API by hand, which is how this was done before: that is the
                # defining move of a whole class of droppers, and nobody reading an
                # installer should have to talk themselves out of it. install.ps1
                # carries the longer version of this note.
                try {
                    [Environment]::SetEnvironmentVariable("DEV_PRUNE_PATH_REFRESH", "1", "User")
                    [Environment]::SetEnvironmentVariable("DEV_PRUNE_PATH_REFRESH", $null, "User")
                } catch {}
            }
        '; then
            PATH_ENTRY=true
        else
            echo "[!] Could not update the Windows User PATH; add $WIN_BIN_DIR by hand."
        fi
    fi
fi

write_receipt

# Make both names work for the rest of *this script* — the `devp setup` call below and
# anything the user chained onto the same `sh -c`.
#
# It cannot reach the interactive shell that ran `curl … | sh`: that pipeline starts a
# child process, and a child cannot change its parent's environment. This is the one
# real difference from the PowerShell installer, where `iex` runs in-process and `devp`
# is live immediately. The closing message says so rather than pretending otherwise.
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) PATH="$BIN_DIR:$PATH"; export PATH ;;
esac

# Git is not optional: it is how dev-prune recognises a repository at all.
if ! command -v git >/dev/null 2>&1; then
    echo ""
    echo "[!] git was not found on your PATH."
    echo "    dev-prune identifies repositories with Git, so it cannot do much without it."
    echo "    Install it from https://git-scm.com/downloads (or your package manager),"
    echo "    then run: devp setup"
fi

if [ "$NO_AUTO_SETUP" != "1" ]; then
    echo ""
    "$EXE_PATH" setup || echo "[!] Setup did not complete. Re-run it any time with: devp setup"
else
    echo ""
    echo "-> Skipped setup (--no-auto-setup). Run 'devp setup' when you want it."
fi

report_prior_exe

echo ""
echo "[OK] Installation complete."
echo ""
echo "    Open a new terminal, or run this to use devp in the current one:"
echo "        export PATH=\"$BIN_DIR:\$PATH\""
echo ""
echo "    Nothing is tracked yet. Register your repositories one of two ways:"
echo ""
echo "    1. Point it at the folder that holds your projects. It finds every Git"
echo "       repository inside, however deep:"
echo "         devp init ~/code"
echo ""
echo "    2. Or go into a single project and register just that one:"
echo "         cd ~/code/my-project"
echo "         devp link ."
echo ""
echo "    Then:"
echo "    devp status             # see what is reclaimable"
echo "    devp run                # reclaim it (shows the plan, asks before deleting)"
echo ""
echo "    devp setup --status     # what got installed alongside the binary"
echo "    devp uninstall          # remove all of it again"
