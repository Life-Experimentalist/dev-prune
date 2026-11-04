# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

<#
.SYNOPSIS
    Builds the throwaway sandbox that demo.tape deletes and restores.

.DESCRIPTION
    The demo prunes dev-prune's own Rust build directory, which is the biggest single
    directory on a machine that builds it. It does that in a clone with its own registry,
    never in your working tree: the recording deletes the clone's target\ and node_modules,
    and `devp restore` reinstalls the npm half. Your real registry and your real build cache
    are not touched.

    Run it before every take, because a take deletes what the next one needs. When
    -TargetFrom is given, target\ is copied from a build you already have rather than
    rebuilt (a cold `cargo build` of this repository takes far longer than copying it).

.EXAMPLE
    pwsh scripts\demo\prepare-sandbox.ps1 -TargetFrom V:\Code\ProjectCode\dev-prune\target
#>
param(
    [string]$Root = 'V:\devp-demo',
    [string]$Source = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$TargetFrom
)

$ErrorActionPreference = 'Stop'
$env:DEV_PRUNE_CONFIG_DIR = Join-Path $Root 'config'
$env:DEV_PRUNE_NO_AUTO_SETUP = '1'

$clone = Join-Path $Root 'dev-prune'
if (-not (Test-Path $clone)) {
    New-Item -ItemType Directory -Force $Root | Out-Null
    git clone --quiet $Source $clone
}

if ($TargetFrom) {
    # Robocopy exits 1 when it copied files, which PowerShell would read as failure.
    robocopy $TargetFrom (Join-Path $clone 'target') /E /MT:16 /NFL /NDL /NJH /NP /R:1 /W:1 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed with exit code $LASTEXITCODE" }
}

Push-Location (Join-Path $clone 'site')
try { npm ci --silent } finally { Pop-Location }

# Idle days are zero so the recording prunes a project that was just built. Everything
# else the run checks (the lockfile, the rebuild command, symlinks, the .git boundary)
# is unchanged, and the README says so next to the clip.
devp link $clone
devp config set enable_cargo true
devp config set idle_days 0
devp config set build_idle_days 0
devp config set min_size_mb 0
devp config set recommendations false

Write-Host "Sandbox ready in $Root. Registry: $env:DEV_PRUNE_CONFIG_DIR"
