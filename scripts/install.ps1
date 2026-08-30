# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# dev-prune Windows installer
# One-liner: iwr -useb https://devprune.vkrishna04.me/install.ps1 | iex
#
# What this does:
#   1. Downloads the release binary from GitHub Releases
#   2. Verifies it against the published SHA-256 checksum
#   3. Installs it to %APPDATA%\dev-prune\bin as both dev-prune.exe and devp.exe, and
#      clears any Mark of the Web so SmartScreen has nothing to challenge
#   4. Adds that directory to the User PATH
#   5. Runs `dev-prune setup`, which installs the parts that were missing: the exported
#      SKILL.md, the global Git auto-registration hooks, and the scheduled task
#
# Step 5 is what makes dev-prune work without being thought about. It is skippable
# (-NoAutoSetup), every piece of it is reversible with `devp uninstall`, and it refuses to
# take over a `core.hooksPath` that already belongs to husky or pre-commit.
#
# What it deliberately does NOT do: modify your editor settings, delete binaries other
# installers put elsewhere, or register any repositories. Run `devp init <dir>` yourself.
#
# Parameters (see -Help). To pass any of them, the one-liner has to become:
#   & ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -NoAutoSetup
# `iwr | iex` runs the script as a bare expression, which has nowhere to put arguments.
# The matching DEV_PRUNE_* environment variables below work with the plain one-liner and
# are the fallback for every parameter.

param(
    # Release tag to install, without the leading `v`.
    [string]$Version,
    # Where the two executables go. Must be a directory you can write to.
    [string]$BinDir,
    # Leave the User PATH alone. `devp` then only resolves if $BinDir is already on it.
    [switch]$NoPath,
    # Install the binary and nothing else: no SKILL.md, no Git hooks, no scheduled task.
    # `devp setup` installs them later; `devp setup --status` shows what is missing.
    [switch]$NoAutoSetup,
    # Download and write the binary even when the same version is already installed here.
    [switch]$Force,
    [switch]$Help
)

# Everything below runs inside one script block so a download truncated mid-stream can
# never execute half an installer: `iwr | iex` evaluates whatever bytes arrived, and a
# cut that happens to land on a statement boundary parses cleanly and runs up to it.
# With the brace, any truncation below this line is an unclosed block — a parse error,
# and nothing at all has run. The body is deliberately not re-indented.
& {

$ErrorActionPreference = 'Stop'

# PowerShell 5.1 on an un-updated Windows still offers TLS 1.0/1.1 by default and
# github.com refuses both, which surfaces as "Could not create SSL/TLS secure channel".
# `-bor` adds 1.2 without taking away anything newer the OS already negotiates.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

if ($Help) {
    Write-Host @'
dev-prune installer

  -Version <tag>   release to install (default: the version this script ships with)
  -BinDir <dir>    install directory (default: %APPDATA%\dev-prune\bin)
  -NoPath          do not touch the User PATH
  -NoAutoSetup     install the binary only; skip SKILL.md, Git hooks and the scheduler
  -Force           reinstall even if this version is already installed here
  -Help            this message

Passing a parameter needs the script as a script block:
  & ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -NoAutoSetup

The equivalent environment variables work with the plain `iwr ... | iex` one-liner:
  DEV_PRUNE_VERSION  DEV_PRUNE_BIN_DIR  DEV_PRUNE_NO_PATH=1  DEV_PRUNE_NO_AUTO_SETUP=1
  DEV_PRUNE_FORCE=1

DEV_PRUNE_NO_MIGRATE_PROMPT=1 has no parameter. It suppresses the one question this
script asks -- whether to move a copy another package manager owns -- and prints the
command instead. CI, and a host with no desktop behind it, suppress it already.

Re-running this is safe. An install that is already here, current and on PATH is left
alone and exits 0; an older one is replaced in place; a newer one is not downgraded.
'@
    return
}

# A parameter beats its environment variable: it is the more explicit of the two, and it
# is the one the user typed on the command line they are looking at.
$version = if ($Version) { $Version } elseif ($env:DEV_PRUNE_VERSION) { $env:DEV_PRUNE_VERSION } else { '' }
$noPath = $NoPath -or ($env:DEV_PRUNE_NO_PATH -eq '1')
$noAutoSetup = $NoAutoSetup -or ($env:DEV_PRUNE_NO_AUTO_SETUP -eq '1')
$force = $Force -or ($env:DEV_PRUNE_FORCE -eq '1')
# A version the user named is a version the user meant, however it was named. It is the
# one thing that makes installing *backwards* the right answer, so it is remembered
# before the next block fills $version in with whatever GitHub says is newest.
$versionExplicit = [bool]$version
$repo = 'Life-Experimentalist/dev-prune'

# With no version pinned, ask GitHub which release is newest. The /releases/latest
# redirect carries the tag, so one HEAD request answers without parsing JSON.
# $fallbackVersion exists for offline mirrors and rate-limited CI: it must always name
# a published release, and the release workflow refuses to tag until it matches.
$fallbackVersion = '1.13.0'
if (-not $version) {
    try {
        $resp = Invoke-WebRequest -Uri "https://github.com/$repo/releases/latest" -Method Head -MaximumRedirection 5 -UseBasicParsing -ErrorAction Stop
        $finalUrl = if ($resp.BaseResponse.ResponseUri) { $resp.BaseResponse.ResponseUri.AbsoluteUri } else { $resp.BaseResponse.RequestMessage.RequestUri.AbsoluteUri }
        if ($finalUrl -match '/tag/v([^/]+)$') { $version = $Matches[1] } else { $version = $fallbackVersion }
    } catch {
        $version = $fallbackVersion
    }
}
$version = $version.TrimStart('v')

# Must match Registry::config_dir(), which resolves to %APPDATA%\dev-prune on Windows.
$binDir = if ($BinDir) {
    $BinDir
} elseif ($env:DEV_PRUNE_BIN_DIR) {
    $env:DEV_PRUNE_BIN_DIR
} else {
    Join-Path (Join-Path $env:APPDATA 'dev-prune') 'bin'
}
$exePath = Join-Path $binDir 'dev-prune.exe'
$aliasPath = Join-Path $binDir 'devp.exe'

# The two facts about this run that nothing on the machine can work out afterwards, so
# the receipt at the end has to be told them. Both start false and are only ever raised
# by the step that makes them true.
$aliasInstalled = $false
$pathEntry = $false

# Where `dev-prune` resolves *before* this script writes anything, when that is not
# the copy this script manages. Running the installer over a cargo, WinGet or Scoop
# install does not fail and does not remove that copy - removing another package
# manager's file behind its back is how installations become unrepairable. But the
# copy stays on PATH afterwards, so it is named at the end with the command that
# migrates it properly.
$priorExe = (Get-Command dev-prune -CommandType Application -ErrorAction SilentlyContinue |
    Select-Object -First 1).Source
if ($priorExe -eq $exePath) { $priorExe = $null }

# `C:\x\bin` and `C:\x\bin\` are the same PATH entry to Windows; a literal -contains
# treats them as different and appends a duplicate on every re-install.
function Test-OnPath {
    param([string]$PathValue, [string]$Dir)
    $norm = $Dir.TrimEnd('\')
    @(($PathValue -split ';') | ForEach-Object { $_.TrimEnd('\') }) -contains $norm
}

# Is it the *first* entry, which is the only position that decides anything.
#
# Presence was the question this script used to ask, and presence is not the question. A
# machine that installed dev-prune here once and through another manager later has this
# directory on PATH and the other copy in front of it; every re-run of the one-liner
# then agreed there was nothing to do while the wrong binary kept answering.
function Test-FirstOnPath {
    param([string]$PathValue, [string]$Dir)
    $first = @(($PathValue -split ';') | Where-Object { $_ }) | Select-Object -First 1
    if ($null -eq $first) { return $false }
    return $first.TrimEnd('\') -eq $Dir.TrimEnd('\')
}

# The same PATH string with this directory in front and its other occurrences dropped.
#
# Everything else keeps its spelling and its order: entries here may be written with
# %USERPROFILE% or a trailing separator, and a PATH this script rewrote into its own
# idea of tidy is a PATH somebody has to read a diff of.
function Move-ToPathFront {
    param([string]$PathValue, [string]$Dir)
    $norm = $Dir.TrimEnd('\')
    $rest = @(($PathValue -split ';') | Where-Object { $_.TrimEnd('\') -ne $norm })
    $joined = ($rest -join ';').Trim(';')
    if ($joined) { return $Dir + ';' + $joined }
    return $Dir
}

# Which file a bare `dev-prune` finds in this session, resolved by hand.
#
# Get-Command would be shorter and is what found $priorExe above, before anything was
# written. It is not what should answer afterwards: it consults a discovery cache, and a
# cached miss outliving a PATH change is exactly the kind of almost-true this function
# exists to stop the script printing.
function Resolve-OnPath {
    param([string]$Name)
    foreach ($dir in ($env:PATH -split ';')) {
        if (-not $dir) { continue }
        try {
            $candidate = Join-Path $dir $Name
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        } catch {
            # An unusable PATH entry is the user's to worry about, not this script's.
            continue
        }
    }
    return $null
}

# Whether there is a person at the other end who can answer a question.
#
# `iwr ... | iex` runs inside the caller's own process with their console still attached,
# so unlike the POSIX script there is no pipe standing between Read-Host and the keyboard
# and no /dev/tty to reach for. What can still be missing is the person: -NonInteractive,
# a scheduled task, a CI runner, a provisioning script. Every one of those wants the
# install and none of the conversation.
function Test-PersonPresent {
    if ($env:DEV_PRUNE_NO_MIGRATE_PROMPT -eq '1') { return $false }
    # Set by every CI provider worth naming, and by nothing with a keyboard attached.
    if ($env:CI) { return $false }
    # False in a service or a scheduled task running with no desktop behind it.
    try {
        if (-not [Environment]::UserInteractive) { return $false }
    } catch {
        return $false
    }
    return $true
}

# One yes/no question. Default no, and every non-answer is a no: this is only ever asked
# before running another package manager, and a stray newline pasted after the one-liner
# must never be the thing that authorises that.
#
# A -NonInteractive host throws from Read-Host rather than returning anything, which is
# the same answer as a shrug, so the catch says no as well.
function Read-YesNo {
    param([string]$Prompt)
    $answer = ''
    try {
        $answer = Read-Host "$Prompt [y/N]"
    } catch {
        return $false
    }
    if (-not $answer) { return $false }
    return @('y', 'yes') -contains $answer.Trim().ToLowerInvariant()
}

# What this run actually did, written down beside the binary it installed.
#
# This script, install.sh and the binary each used to derive the same facts on their own,
# and three derivations of one truth is how they drift. `devp doctor` and `devp install`
# read this file; it outlives the session that ran the one-liner, which no variable here
# does. It is a record, never a setting: `Channel::detect()` is still what classifies a
# copy, because no receipt can describe one that arrived through `cargo install`, and
# every reader treats a missing file as "no installer of ours wrote one" rather than as
# an error.
#
# Written here by hand rather than by the binary it just installed, because it has to be
# true even when -NoAutoSetup means that binary is never run. `src/receipt.rs` has a test
# asserting these exact field names for that reason.
function Write-Receipt {
    $receipt = Join-Path $binDir 'install.json'
    $staged = "$receipt.new"
    try {
        # [ordered], not @{}: PowerShell 5.1 hashtables have no order to convert, and a
        # receipt whose keys move around between installs is a diff nobody can read.
        $body = [ordered]@{
            schema       = 1
            version      = $version
            channel      = 'installer'
            installed_by = 'install.ps1'
            installed_at = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ', [Globalization.CultureInfo]::InvariantCulture)
            exe          = $exePath
            alias        = $aliasInstalled
            path_entry   = $pathEntry
        } | ConvertTo-Json
        # WriteAllText with an explicit no-BOM encoder. Set-Content -Encoding utf8 on
        # PowerShell 5.1 writes a byte order mark, and three bytes in front of a '{' are
        # not JSON to serde_json or to anything else.
        [IO.File]::WriteAllText($staged, $body, (New-Object Text.UTF8Encoding($false)))
        # Staged and renamed, so a reader never sees a half-written file.
        Move-Item -LiteralPath $staged -Destination $receipt -Force -ErrorAction Stop
    } catch {
        Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
        Write-Host "[!] Could not write $receipt. Harmless: it is a note about this install," -ForegroundColor Yellow
        Write-Host "    not a setting, and nothing reads it to decide anything." -ForegroundColor Yellow
    }
}

# Printed from three endings now, so it lives in one place.
function Write-PriorExeNotice {
    if ($priorExe) {
        Write-Host ""
        Write-Host "[!] Another dev-prune is on your PATH as well:" -ForegroundColor Yellow
        Write-Host "        $priorExe"
        Write-Host "    A different package manager owns that copy, so this script left it alone."
        # Resolved, not asserted. The line here used to claim that $binDir came first on
        # PATH, which this script only ever made true when the entry was absent - on a
        # machine where it was present but behind another manager's directory the
        # prepend was skipped, the older copy kept answering, and the claim was printed
        # regardless. Saying which file actually answers cannot go wrong that way.
        $nowExe = Resolve-OnPath 'dev-prune.exe'
        if ($nowExe -and ($nowExe -eq $exePath)) {
            Write-Host "    In this session 'dev-prune' now resolves to the copy in $binDir."
            if ($noPath) {
                Write-Host "    New terminals are yours to arrange (-NoPath left PATH alone)."
            }
        } elseif ($nowExe) {
            Write-Host "    In this session 'dev-prune' still resolves to:"
            Write-Host "        $nowExe"
            if ($noPath) {
                Write-Host "    -NoPath left PATH alone, so the order is yours to set."
            }
        }
        Write-Host "    Moving it over means uninstalling there, through the manager that put it"
        Write-Host "    there. 'devp install --channel installer' does that."
        # Not offered under a pin: the copy at $exePath would refuse, and an offer that
        # ends in a refusal is worse than no offer.
        if ((-not $installedLock) -and (Test-Path -LiteralPath $exePath) -and
            (Test-PersonPresent) -and (Read-YesNo "    Do that now?")) {
            Write-Host ""
            # The copy just installed, not the old one. Handing this to the *old* binary
            # was the whole failure: nothing before 1.8.0 has an `install` subcommand at
            # all, so on the machines this offer exists for it printed an unrecognised-
            # subcommand error and nothing moved. The new copy is new by definition, it
            # can name the manager that owns the file above, and it runs that manager's
            # own uninstall.
            #
            # Nothing is deleted by this script either way.
            $moved = $false
            try {
                # A native command's stderr becomes an ErrorRecord under 'Stop', and this
                # one runs another package manager: its progress must not be mistaken for
                # failure. The exit code is the answer. Function-scoped, so the caller's
                # 'Stop' is untouched.
                $ErrorActionPreference = 'Continue'
                & $exePath install --channel installer --yes
                $moved = ($LASTEXITCODE -eq 0)
            } catch {
                # It could not be started at all, which the message below covers.
                $moved = $false
            }
            Write-Host ""
            if ($moved) {
                Write-Host "[OK] Done. 'devp doctor' will confirm only one copy is left." -ForegroundColor Green
            } else {
                Write-Host "[!] That did not finish, and this script deleted nothing - the copy at" -ForegroundColor Yellow
                Write-Host "        $priorExe"
                Write-Host "    is exactly where it was. Its own manager can still remove it, and"
                Write-Host "    'devp doctor' names both copies and the command for each."
            }
        } elseif ($installedLock) {
            Write-Host "    Your version pin covers this too: which copy answers on PATH is which"
            Write-Host "    version runs, so nothing moves while the pin is on. Release it with"
            Write-Host "        devp config set version_lock false"
            Write-Host "    'devp doctor' lists every copy on the machine at any time."
        } else {
            Write-Host "    Run it whenever you like:"
            Write-Host "        devp install --channel installer"
            Write-Host "    'devp doctor' lists every copy on the machine at any time."
        }
    }
}

# The version already sitting at the path this script manages, if any. Both switches
# matter: `--version` on this CLI is handled in code rather than short-circuited by the
# parameter parser, so an unguarded call could have the *old* binary register a
# scheduled task or reach GitHub in the middle of installing its replacement. They are
# put back afterwards because `devp setup` runs later in this same process and reads
# them too.
$installedVersion = ''
# ...and whether that copy is pinned. version_lock is the one setting that outranks this
# script: somebody who set it asked for the binary not to change, and a one-liner re-run
# out of habit is precisely the accident it exists to stop. Probed in the same guarded
# block, because it is the same binary answering under the same two switches.
$installedLock = $false
if (Test-Path -LiteralPath $exePath) {
    $prevNoSetup = $env:DEV_PRUNE_NO_AUTO_SETUP
    $prevOffline = $env:DEV_PRUNE_OFFLINE
    $prevEap = $ErrorActionPreference
    try {
        $env:DEV_PRUNE_NO_AUTO_SETUP = '1'
        $env:DEV_PRUNE_OFFLINE = '1'
        # A binary too old, too new or too broken to answer is not a reason to stop.
        $ErrorActionPreference = 'Continue'
        $probe = & $exePath --version 2>$null | Out-String
        if ($probe -match '(\d+\.\d+\.\d+)') { $installedVersion = $Matches[1] }
        if ($installedVersion) {
            $lockProbe = & $exePath config get version_lock 2>$null | Out-String
            if ($lockProbe -match 'true') { $installedLock = $true }
        }
    } catch {
    } finally {
        $ErrorActionPreference = $prevEap
        $env:DEV_PRUNE_NO_AUTO_SETUP = $prevNoSetup
        $env:DEV_PRUNE_OFFLINE = $prevOffline
    }
}

# Whether a *new* terminal would find this install, which is the only question that
# matters here: the current session's $env:PATH may only contain $binDir because an
# earlier run of this script put it there for the session. -NoPath means the user is
# managing PATH themselves, so it stops being this script's question.
$persistedPath = ''
try { $persistedPath = [Environment]::GetEnvironmentVariable('Path', 'User') } catch {}
if ($null -eq $persistedPath) { $persistedPath = '' }
$pathConfigured = $noPath -or (Test-OnPath $persistedPath $binDir)

# Put this directory in front for the rest of this session, before any ending is chosen.
# Every ending below reports which copy `dev-prune` resolves to, and the three that
# change nothing else - already installed, newer already installed, pinned - are exactly
# the endings somebody re-runs the one-liner to reach when the wrong copy keeps
# answering. The persisted User PATH is a separate question, settled further down and
# only when the install goes ahead.
if ((-not $noPath) -and (Test-Path -LiteralPath $exePath)) {
    $env:PATH = Move-ToPathFront $env:PATH $binDir
}

# A release tag that is not three numbers - a `-rc1`, say - simply does not take part
# in the comparison, and the ordinary install runs.
$targetVer = $null
try { $targetVer = [version]$version } catch {}
$installedVer = if ($installedVersion) { [version]$installedVersion } else { $null }

# Re-running the one-liner is the most common thing anyone does with it - it is what the
# README, the release page and every "just reinstall it" answer tell people to do. So it
# has to be safe *and* quiet: an install that is already correct is left exactly as it
# is, and one that is merely out of date is replaced without asking. The one thing this
# will not do on its own is install backwards.
#
# One branch sits above all of that, including the silent in-place update: a pinned
# install is not updated, repaired or downgraded by this script at all. -Force is the
# only way past it, and it has to be typed.
if ($installedLock -and -not $force) {
    Write-Host ""
    Write-Host "[!] dev-prune v$installedVersion at:" -ForegroundColor Yellow
    Write-Host "        $exePath"
    Write-Host "    has version_lock set, so this script changed nothing."
    Write-Host "    Release the pin and re-run:"
    Write-Host "        devp config set version_lock false"
    Write-Host "    Or install over it just this once with -Force."
    Write-PriorExeNotice
    return
}

if ($installedVer -and $targetVer -and -not $force) {
    if ($installedVer -gt $targetVer -and -not $versionExplicit) {
        Write-Host ""
        Write-Host "[OK] dev-prune v$installedVersion is already installed at:" -ForegroundColor Green
        Write-Host "        $exePath"
        Write-Host "     That is newer than the v$version this run resolved to, so nothing"
        Write-Host "     was changed. Re-run with -Force to install v$version over it."
        Write-PriorExeNotice
        return
    }
    if ($installedVer -eq $targetVer -and (Test-Path -LiteralPath $aliasPath) -and $pathConfigured) {
        Write-Host ""
        Write-Host "[OK] dev-prune v$version is already installed at:" -ForegroundColor Green
        Write-Host "        $exePath"
        if ($noPath) {
            Write-Host "     'devp.exe' is beside it. Nothing to do."
        } else {
            Write-Host "     It is on PATH and 'devp.exe' is beside it. Nothing to do."
        }
        Write-Host "     Re-run with -Force to download and write it again."
        Write-PriorExeNotice
        return
    }
    # Same version, but something is missing - no devp.exe, or nothing on the User PATH
    # pointing here. Falling through reinstalls and repairs it, which is the whole
    # reason someone re-runs the one-liner after an install went wrong.
}

Write-Host ""
if ($installedVersion -and $installedVersion -ne $version) {
    Write-Host "-> Updating dev-prune v$installedVersion -> v$version" -ForegroundColor Cyan
} else {
    Write-Host "-> Installing dev-prune v$version" -ForegroundColor Cyan
}

# PROCESSOR_ARCHITECTURE reports the *process* architecture, and on Windows-on-ARM this
# script frequently runs inside an emulated x64 PowerShell — which would read AMD64 and
# silently install the x64 build. Under any emulation, PROCESSOR_ARCHITEW6432 carries
# the machine's real architecture; it is unset when the process is native.
$nativeArch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
# `x86` here means the machine itself has no 64-bit mode: a 32-bit PowerShell running on
# 64-bit Windows reads AMD64 out of PROCESSOR_ARCHITEW6432 above and gets the x64 build,
# which is the right one for it. So the x86 asset goes only to hardware that can run
# nothing else — which is the whole reason it is published.
#
# Reaching `default` now means 32-bit ARM, or something Windows has not shipped yet.
# Handing either of those an x64 zip produces "not a valid Win32 application", which
# names nothing; install.sh refuses unknown architectures rather than guessing, and this
# matches it.
$arch = switch ($nativeArch) {
    'AMD64' { 'x64' }
    'ARM64' { 'arm64' }
    'x86'   { 'x86' }
    default {
        # ASCII only inside the quotes. This script is fetched and `iex`'d, and a
        # decoder that guesses ANSI turns a UTF-8 em dash into "a-euro-rightquote" —
        # and PowerShell accepts a typographic right quote as a string terminator, so
        # the parse dies three words later. Comments survive it; strings do not.
        throw "Unsupported architecture: $nativeArch. dev-prune publishes x64, ARM64 and 32-bit x86 builds for Windows. There is no 32-bit ARM release. Build one from source instead: cargo install dev-prune"
    }
}
$asset = "dev-prune-v$version-windows-$arch.zip"
$baseUrl = "https://github.com/$repo/releases/download/v$version"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("dev-prune-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
    $zipPath = Join-Path $tmpDir $asset

    Write-Host "-> Downloading $asset" -ForegroundColor Cyan
    Invoke-WebRequest -Uri "$baseUrl/$asset" -OutFile $zipPath -UseBasicParsing

    Write-Host "-> Verifying checksum" -ForegroundColor Cyan
    $shaPath = Join-Path $tmpDir "$asset.sha256"
    try {
        Invoke-WebRequest -Uri "$baseUrl/$asset.sha256" -OutFile $shaPath -UseBasicParsing
    } catch {
        throw "No published checksum for v$version ($arch). Refusing to install an unverified binary. Install from source instead: cargo install dev-prune"
    }
    $expected = ((Get-Content -Path $shaPath -Raw).Trim() -split '\s+')[0]
    $actual = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash
    if ($expected -ne $actual) {
        throw "Checksum mismatch - refusing to install.`n  expected: $expected`n  actual:   $actual"
    }
    Write-Host "[OK] Checksum verified" -ForegroundColor Green

    # Strip any Mark of the Web here rather than leaving it for the user to discover.
    #
    # On a normal run there is nothing to remove: Invoke-WebRequest writes no
    # Zone.Identifier and Expand-Archive does not propagate one. It matters when this
    # script is not what fetched the archive - a proxy that stamps downloads, a group
    # policy, or someone running a saved copy of this installer against a zip their
    # browser downloaded. A marked binary is what raises the "Windows protected your PC"
    # dialog, and one line here is cheaper than the support answer.
    Unblock-File -Path $zipPath -ErrorAction SilentlyContinue

    # Extract into the temp dir first so a malformed archive cannot scatter files
    # into the install directory.
    Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force
    $extracted = Join-Path $tmpDir 'dev-prune.exe'
    if (-not (Test-Path $extracted)) {
        throw "Archive did not contain dev-prune.exe at its root."
    }

    if (-not (Test-Path $binDir)) {
        New-Item -ItemType Directory -Path $binDir -Force | Out-Null
    }

    # Replacing a running executable.
    #
    # This is the upgrade path, and on Windows it is the part that breaks: the OS holds
    # a lock on a running image, so `Copy-Item -Force` over dev-prune.exe fails outright
    # while the scheduled task's prune pass happens to be mid-run. A rename is allowed
    # on a locked image, though - the handle follows the file, not the name - so move
    # the old one aside first and delete it on a best-effort basis afterwards. The
    # displaced copy exits on its own and the leftover is swept up by the next install.
    function Install-Binary {
        param([string]$Source, [string]$Destination)

        if (Test-Path $Destination) {
            $stale = "$Destination.old"
            Remove-Item -Path $stale -Force -ErrorAction SilentlyContinue
            try {
                Move-Item -Path $Destination -Destination $stale -Force -ErrorAction Stop
            } catch {
                # Not even a rename worked, which on Windows means something other than
                # a running image is holding it - an antivirus scan, a locked profile.
                # Fall through to the copy so the real error is the one reported.
            }
        }
        Copy-Item -Path $Source -Destination $Destination -Force -ErrorAction Stop
        Remove-Item -Path "$Destination.old" -Force -ErrorAction SilentlyContinue
    }

    Install-Binary -Source $extracted -Destination $exePath
    Write-Host "[OK] Installed: $exePath" -ForegroundColor Green

    # `devp` as a real executable, not a shell alias.
    #
    # A PowerShell profile function only exists in PowerShell; a doskey macro only
    # exists in the cmd session that defined it. A second copy of the binary on PATH is
    # `devp` everywhere at once - cmd, PowerShell, Git Bash, an IDE's terminal, a
    # scheduled task - and it cannot fall out of sync with a profile nobody re-sources.
    try {
        Install-Binary -Source $extracted -Destination $aliasPath
        $aliasInstalled = $true
        Write-Host "[OK] Installed: $aliasPath" -ForegroundColor Green
    } catch {
        # `dev-prune.exe` is already in place at this point, and it re-installs its own
        # alias on the next run, so a locked `devp.exe` is a warning rather than a stop.
        Write-Host "[!] Could not write $aliasPath ($($_.Exception.Message))." -ForegroundColor Yellow
        Write-Host "    Close any running devp and run: dev-prune setup" -ForegroundColor Yellow
    }

    # Copy-Item carries an alternate data stream across with the file, so a mark on the
    # archive would have survived into both installed copies. Clear them for the same
    # reason the archive was cleared above. `devp.exe` may legitimately not exist here.
    Unblock-File -Path $exePath, $aliasPath -ErrorAction SilentlyContinue
} finally {
    Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}

if (-not $noPath) {
    # Raw registry access, not [Environment]::GetEnvironmentVariable: that call hands
    # back the *expanded* value, and writing it back would bake every %USERPROFILE%-
    # style entry into a literal path for good. Reading with
    # DoNotExpandEnvironmentNames and keeping the value's REG_EXPAND_SZ/REG_SZ kind
    # leaves the user's own entries exactly as they spelled them.
    $envKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    $userPath = ''
    if ($null -ne $envKey) {
        $userPath = [string]$envKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    }
    if (-not (Test-FirstOnPath $userPath $binDir)) {
        if (Test-OnPath $userPath $binDir) {
            Write-Host "-> Moving $binDir to the front of your User PATH" -ForegroundColor Cyan
        } else {
            Write-Host "-> Adding $binDir to your User PATH" -ForegroundColor Cyan
        }
        # First, not merely present, and for the same reason the in-session assignment
        # below puts it first: this directory holds nothing but dev-prune.exe and
        # devp.exe, so it can shadow nothing else, and a machine that already has a copy
        # from cargo or Scoop earlier on PATH would otherwise keep running that one in
        # every terminal opened after this. The install would look like it worked - it
        # does work in the session it ran in - and be silently undone by PATH order in
        # the next one.
        #
        # Testing only for presence was the bug behind exactly that report: a second
        # channel installed later sits in front of an entry an earlier run left here,
        # and every re-run of the one-liner then agreed there was nothing to do.
        $newPath = Move-ToPathFront $userPath $binDir
        if ($null -ne $envKey) {
            $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
            try { $kind = $envKey.GetValueKind('Path') } catch {}
            if ($kind -ne [Microsoft.Win32.RegistryValueKind]::String) {
                $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
            }
            $envKey.SetValue('Path', $newPath, $kind)
            # The raw write does not broadcast WM_SETTINGCHANGE the way the .NET
            # environment API did; without it Explorer and new terminals keep the old
            # PATH until the next sign-in.
            $sig = '[DllImport("user32.dll",SetLastError=true,CharSet=CharSet.Auto)]public static extern IntPtr SendMessageTimeout(IntPtr hWnd,uint Msg,UIntPtr wParam,string lParam,uint fuFlags,uint uTimeout,out UIntPtr lpdwResult);'
            $broadcast = Add-Type -MemberDefinition $sig -Name 'NativeBroadcast' -Namespace DevPruneInstall -PassThru
            $result = [UIntPtr]::Zero
            [void]$broadcast::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
        } else {
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        }
    }
    if ($null -ne $envKey) { $envKey.Close() }
    # Make both names work in *this* session too, without waiting for a new terminal.
    #
    # `iwr ... | iex` runs inside the caller's own process, so this assignment is
    # visible to the very next line they type. It is what makes the documented
    # `iwr ... | iex; devp init ~/Code` sequence work as written; the User PATH set
    # above is only for terminals opened later.
    $env:PATH = Move-ToPathFront $env:PATH $binDir
    $pathReady = $true
    # The User PATH names this directory now, whether this run wrote the entry or found
    # one a previous run left. Either way an installer put it there: nothing else on the
    # machine writes to a directory that holds only these two executables.
    $pathEntry = $true
} else {
    $pathReady = Test-OnPath $env:PATH $binDir
}

Write-Receipt

# Git is not optional: it is how dev-prune recognises a repository at all.
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host ""
    Write-Host "[!] git was not found on your PATH." -ForegroundColor Yellow
    Write-Host "    dev-prune identifies repositories with Git, so it cannot do much without it."
    Write-Host "    Install it from https://git-scm.com/downloads (or: winget install Git.Git),"
    Write-Host "    then run: devp setup"
}

# Smart App Control, which is a different thing from the SmartScreen dialog.
#
# SmartScreen weighs a file's reputation and only looks at files carrying a Mark of the
# Web, so the Unblock-File calls above settle it. Smart App Control does not care about the
# mark: in enforcement mode it refuses to start any executable without a valid Authenticode
# signature, however it arrived on the machine. dev-prune's releases are not signed, so on
# such a machine the binary installs and then will not run.
#
# It ships enabled only on clean installs of Windows 11 22H2 and later, and is always off
# on machines upgraded from an earlier build. That is why the identical one-liner installs
# cleanly on one laptop and is blocked on the next, and it is worth saying out loud here -
# the symptom otherwise looks exactly like a corrupt download.
#
# 0 = off, 1 = enforcement, 2 = evaluation. The key does not exist on Windows 10.
$sacEnforcing = $false
try {
    $ci = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' `
        -Name 'VerifiedAndReputablePolicyState' -ErrorAction Stop
    $sacEnforcing = ($ci.VerifiedAndReputablePolicyState -eq 1)
} catch {
    # No key, no permission, or Windows 10. Either way there is nothing to warn about.
}

if ($sacEnforcing) {
    Write-Host ""
    Write-Host "[!] Smart App Control is in enforcement mode on this machine." -ForegroundColor Yellow
    Write-Host "    It blocks every executable that is not code-signed, no matter where it came"
    Write-Host "    from, and dev-prune's releases are not signed. If dev-prune refuses to start,"
    Write-Host "    that is why: the download is not corrupt and Unblock-File will not change it."
    Write-Host "    Turning Smart App Control off is one-way - Windows cannot turn it back on"
    Write-Host "    without a reset or reinstall - so it is a deliberate decision, not a quick fix:"
    Write-Host "      Windows Security > App & browser control > Smart App Control"
    Write-Host "    https://devprune.vkrishna04.me/docs/troubleshooting"
}

if (-not $noAutoSetup) {
    Write-Host ""
    # A binary Windows refuses to start throws here rather than returning an exit code, and
    # an uncaught throw would end the install in a stack trace over an already-successful
    # install. The binary is on disk either way; only the integrations are outstanding.
    try {
        & $exePath setup
    } catch {
        Write-Host "[!] '$exePath setup' could not run: $($_.Exception.Message)" -ForegroundColor Yellow
        if ($sacEnforcing) {
            Write-Host "    Smart App Control, above, is the likely reason." -ForegroundColor Yellow
        }
        Write-Host "    The binary is installed. Run 'devp setup' once it can start." -ForegroundColor Yellow
    }
} else {
    Write-Host ""
    Write-Host "-> Skipped setup (-NoAutoSetup). Run 'devp setup' when you want it." -ForegroundColor Cyan
}

Write-PriorExeNotice

Write-Host ""
if ($pathReady) {
    Write-Host "[OK] Installation complete. 'devp' works in this terminal right now:" -ForegroundColor Green
} else {
    Write-Host "[OK] Installation complete. Add $binDir to your PATH, then:" -ForegroundColor Green
}
Write-Host ""
Write-Host "    Nothing is tracked yet. Register your repositories one of two ways:" -ForegroundColor Cyan
Write-Host ""
Write-Host "    1. Point it at the folder that holds your projects. It finds every Git"
Write-Host "       repository inside, however deep:"
Write-Host "         devp init ~\Code"
Write-Host ""
Write-Host "    2. Or go into a single project and register just that one:"
Write-Host "         cd ~\Code\my-project"
Write-Host "         devp link ."
Write-Host ""
Write-Host "    Then:"
Write-Host "    devp status             # see what is reclaimable"
Write-Host "    devp run                # reclaim it (shows the plan, asks before deleting)"
Write-Host ""
Write-Host "    devp setup --status     # what got installed alongside the binary"
Write-Host "    devp uninstall          # remove all of it again"

}
