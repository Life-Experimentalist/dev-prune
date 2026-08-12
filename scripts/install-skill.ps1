# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# Global AI Skill Installer for dev-prune (PowerShell)
# Usage: powershell -ExecutionPolicy Bypass -File scripts/install-skill.ps1

$ErrorActionPreference = 'Stop'

$SkillDir = Join-Path $HOME '.agents\skills\dev-prune'
if (-not (Test-Path $SkillDir)) {
    New-Item -ItemType Directory -Force -Path $SkillDir | Out-Null
}

$AppSkill = Join-Path $env:APPDATA 'dev-prune\SKILL.md'
$RepoSkill = Join-Path $PSScriptRoot '..\.agents\skills\dev-prune\SKILL.md'

if (Test-Path $AppSkill) {
    Copy-Item -Path $AppSkill -Destination (Join-Path $SkillDir 'SKILL.md') -Force
} elseif (Test-Path $RepoSkill) {
    Copy-Item -Path $RepoSkill -Destination (Join-Path $SkillDir 'SKILL.md') -Force
}

# Copy to Antigravity config directory if present
$GeminiDir = Join-Path $HOME '.gemini\config\skills\dev-prune'
if (Test-Path (Join-Path $HOME '.gemini\config')) {
    New-Item -ItemType Directory -Force -Path $GeminiDir | Out-Null
    Copy-Item -Path (Join-Path $SkillDir 'SKILL.md') -Destination (Join-Path $GeminiDir 'SKILL.md') -Force
}

Write-Host ('[OK] dev-prune AI Skill installed to ' + $SkillDir + '\SKILL.md') -ForegroundColor Green
Write-Host ''
Write-Host 'Run `devp skill` in terminal to display AI onboarding prompts anytime.' -ForegroundColor Cyan
