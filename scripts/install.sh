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

usage() {
    cat <<'EOF'
dev-prune installer

  --version <tag>    release to install (default: the version this script ships with)
  --bin-dir <dir>    install directory (default: the dev-prune config dir's bin/)
  --no-path          do not edit any shell rc file or the Windows User PATH
  --no-auto-setup    install the binary only; skip SKILL.md, Git hooks and the scheduler
  --help             this message

Piping into a shell needs `-s --` before the options:
  curl -fsSL https://devprune.vkrishna04.me/install.sh | sh -s -- --no-auto-setup

The equivalent environment variables work with the plain one-liner:
  DEV_PRUNE_VERSION  DEV_PRUNE_BIN_DIR  DEV_PRUNE_NO_PATH=1  DEV_PRUNE_NO_AUTO_SETUP=1
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
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; echo "" >&2; usage >&2; exit 2 ;;
    esac
done

# An option beats its environment variable: it is the more explicit of the two, and it is
# the one the user typed on the command line they are looking at.
[ -n "$VERSION" ] || VERSION="${DEV_PRUNE_VERSION:-1.1.0}"
VERSION="${VERSION#v}"
[ "$NO_PATH" = "1" ] || NO_PATH="${DEV_PRUNE_NO_PATH:-0}"
[ "$NO_AUTO_SETUP" = "1" ] || NO_AUTO_SETUP="${DEV_PRUNE_NO_AUTO_SETUP:-0}"
REPO="Life-Experimentalist/dev-prune"

echo ""
echo "-> Installing dev-prune v${VERSION}"

RAW_OS="$(uname -s)"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64|amd64) TARGET_ARCH="x64" ;;
    aarch64|arm64) TARGET_ARCH="arm64" ;;
    *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

# Git Bash, MSYS2 and Cygwin are POSIX shells on Windows: the same script can serve
# them, but the asset, the file extension and the install directory are all Windows'.
case "$RAW_OS" in
    MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
    Darwin) OS="darwin" ;;
    Linux) OS="linux" ;;
    *) echo "Unsupported operating system: $RAW_OS" >&2; exit 1 ;;
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

RC_TOUCHED=0

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
            else
                echo "-> Adding dev-prune to PATH in $file"
                # `$PATH` must reach the rc file unexpanded: expanding it here would
                # freeze this moment's PATH into the user's shell forever.
                # shellcheck disable=SC2016
                printf '\n# dev-prune\nexport PATH="%s:$PATH"\n' "$BIN_DIR" >> "$file"
                RC_TOUCHED=1
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
        # Read-modify-write through .NET rather than `setx`, which truncates any PATH
        # longer than 1024 characters.
        #
        # The single quotes are the point: this is PowerShell source, and
        # `$dir`/`$env:`/`$userPath` are its variables, not the shell's. The directory
        # crosses over as an environment variable so it never has to be quoted into the
        # script text.
        # shellcheck disable=SC2016
        DEV_PRUNE_WIN_BIN_DIR="$WIN_BIN_DIR" powershell.exe -NoProfile -NonInteractive -Command '
            $dir = $env:DEV_PRUNE_WIN_BIN_DIR
            $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
            if ($null -eq $userPath) { $userPath = "" }
            if (($userPath -split ";") -notcontains $dir) {
                $trimmed = $userPath.TrimEnd(";")
                $newPath = if ($trimmed) { $trimmed + ";" + $dir } else { $dir }
                [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            }
        ' || echo "[!] Could not update the Windows User PATH; add $WIN_BIN_DIR by hand."
    fi
fi

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
echo "    devp run --dry-run      # preview a prune pass"
echo ""
echo "    devp setup --status     # what got installed alongside the binary"
echo "    devp uninstall          # remove all of it again"
