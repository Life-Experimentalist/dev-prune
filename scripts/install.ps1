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
$version = if ($Version) { $Version } elseif ($env:DEV_PRUNE_VERSION) { $env:DEV_PRUNE_VERSION } else { '' }
$noPath = $NoPath -or ($env:DEV_PRUNE_NO_PATH -eq '1')
$noAutoSetup = $NoAutoSetup -or ($env:DEV_PRUNE_NO_AUTO_SETUP -eq '1')
$repo = 'Life-Experimentalist/dev-prune'

# With no version pinned, ask GitHub which release is newest. The /releases/latest
# redirect carries the tag, so one HEAD request answers without parsing JSON.
# $fallbackVersion exists for offline mirrors and rate-limited CI: it must always name
# a published release, and the release workflow refuses to tag until it matches.
$fallbackVersion = '1.7.0'
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

# `C:\x\bin` and `C:\x\bin\` are the same PATH entry to Windows; a literal -contains
# treats them as different and appends a duplicate on every re-install.
function Test-OnPath {
    param([string]$PathValue, [string]$Dir)
    $norm = $Dir.TrimEnd('\')
    @(($PathValue -split ';') | ForEach-Object { $_.TrimEnd('\') }) -contains $norm
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
    if (-not (Test-OnPath $userPath $binDir)) {
        Write-Host "-> Adding $binDir to your User PATH" -ForegroundColor Cyan
        # Trim first: appending to an empty Path would leave a leading ';', and an
        # empty PATH entry on Windows means "search the current directory".
        $trimmed = $userPath.TrimEnd(';')
        $newPath = if ($trimmed) { $trimmed + ';' + $binDir } else { $binDir }
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
    if (-not (Test-OnPath $env:PATH $binDir)) {
        $env:PATH = $binDir + ';' + $env:PATH
    }
    $pathReady = $true
} else {
    $pathReady = Test-OnPath $env:PATH $binDir
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

}
