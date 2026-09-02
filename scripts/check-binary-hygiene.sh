#!/bin/sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0
#
# Antivirus verdicts on an unsigned binary come down to what it looks like, and a
# handful of substrings are what "looks like malware" mostly means: the PowerShell
# obfuscation flags, the Defender-exclusion and firewall cmdlets, the process
# injection API names. None of them has any business inside dev-prune — the 1.15.0
# hardening removed the last of them by hand, and this script is that audit turned
# into a gate, so the next stray dependency or debug helper that smuggles one back
# in fails the release build instead of failing in front of seventy scanners.
#
# Every pattern below is asserted ABSENT, and each was verified absent from the
# shipping binaries when it was added. Strings the binaries legitimately contain —
# `schtasks`, the clap-generated PowerShell completions — are deliberately not on
# the list; add a pattern only after grepping a fresh release build for it.
#
# Usage: sh scripts/check-binary-hygiene.sh <binary>...
#        (release.yml runs it over every staged executable before publishing)

set -eu

if [ "$#" -eq 0 ]; then
    echo "check-binary-hygiene: no binaries given" >&2
    echo "usage: sh scripts/check-binary-hygiene.sh <binary>..." >&2
    exit 2
fi

# One pattern per line. Grouped by what a scanner reads them as.
patterns='EncodedCommand
FromBase64String
Invoke-Expression
DownloadString
Add-Type
DllImport
-WindowStyle Hidden
Add-MpPreference
Set-MpPreference
ExclusionPath
netsh advfirewall
attrib +h
CurrentVersion\Run
vssadmin
bcdedit
CreateRemoteThread
VirtualAllocEx
WriteProcessMemory
SetWindowsHookEx
AmsiScanBuffer'

status=0
for bin in "$@"; do
    if [ ! -f "$bin" ]; then
        echo "check-binary-hygiene: no such file: $bin" >&2
        status=1
        continue
    fi
    found=""
    while IFS= read -r pat; do
        [ -n "$pat" ] || continue
        if grep -aqF -- "$pat" "$bin"; then
            found="$found
    $pat"
        fi
    done <<EOF
$patterns
EOF
    if [ -n "$found" ]; then
        echo "FAIL: $bin contains dropper-bait strings:$found" >&2
        status=1
    else
        echo "clean: $bin"
    fi
done

if [ "$status" -ne 0 ]; then
    echo >&2
    echo "A pattern above reappeared in a shipping binary. Find what introduced it" >&2
    echo "(a new dependency, a debug helper, an inlined script) and remove it — do" >&2
    echo "not remove the pattern from this list to make the release build pass." >&2
fi
exit "$status"
