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
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

if ($Help) {
    Write-Host @'
dev-prune installer

  -Version <tag>   release to install (default: the version this script ships with)
  -BinDir <dir>    install directory (default: %APPDATA%\dev-prune\bin)
  -NoPath          do not touch the User PATH
  -NoAutoSetup     install the binary only; skip SKILL.md, Git hooks and the scheduler
  -Help            this message

Passing a parameter needs the script as a script block:
  & ([scriptblock]::Create((iwr -useb https://devprune.vkrishna04.me/install.ps1))) -NoAutoSetup

The equivalent environment variables work with the plain `iwr ... | iex` one-liner:
  DEV_PRUNE_VERSION  DEV_PRUNE_BIN_DIR  DEV_PRUNE_NO_PATH=1  DEV_PRUNE_NO_AUTO_SETUP=1
'@
    return
}

# A parameter beats its environment variable: it is the more explicit of the two, and it
# is the one the user typed on the command line they are looking at.
$version = if ($Version) { $Version } elseif ($env:DEV_PRUNE_VERSION) { $env:DEV_PRUNE_VERSION } else { '1.0.0' }
$version = $version.TrimStart('v')
$noPath = $NoPath -or ($env:DEV_PRUNE_NO_PATH -eq '1')
$noAutoSetup = $NoAutoSetup -or ($env:DEV_PRUNE_NO_AUTO_SETUP -eq '1')
$repo = 'Life-Experimentalist/dev-prune'

Write-Host ""
Write-Host "-> Installing dev-prune v$version" -ForegroundColor Cyan

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

$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
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
    # A profile that has never had a User-scoped Path returns $null here, and calling
    # .TrimEnd() on $null is a terminating error.
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($null -eq $userPath) { $userPath = '' }
    if (($userPath -split ';') -notcontains $binDir) {
        Write-Host "-> Adding $binDir to your User PATH" -ForegroundColor Cyan
        # Trim first: appending to an empty Path would leave a leading ';', and an
        # empty PATH entry on Windows means "search the current directory".
        $trimmed = $userPath.TrimEnd(';')
        $newPath = if ($trimmed) { $trimmed + ';' + $binDir } else { $binDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    }
    # Make both names work in *this* session too, without waiting for a new terminal.
    #
    # `iwr ... | iex` runs inside the caller's own process, so this assignment is
    # visible to the very next line they type. It is what makes the documented
    # `iwr ... | iex; devp init ~/Code` sequence work as written; the User PATH set
    # above is only for terminals opened later.
    if (($env:PATH -split ';') -notcontains $binDir) {
        $env:PATH = $binDir + ';' + $env:PATH
    }
    $pathReady = $true
} else {
    $pathReady = ($env:PATH -split ';') -contains $binDir
}

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
Write-Host "    devp run --dry-run      # preview a prune pass"
Write-Host ""
Write-Host "    devp setup --status     # what got installed alongside the binary"
Write-Host "    devp uninstall          # remove all of it again"
