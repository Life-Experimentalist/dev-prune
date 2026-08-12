#!/usr/bin/env sh
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

# Global AI Skill Installer for dev-prune (Bash / Zsh)
set -e

SKILL_DIR="$HOME/.agents/skills/dev-prune"
mkdir -p "$SKILL_DIR"

APP_SKILL="$HOME/.config/dev-prune/SKILL.md"
REPO_SKILL="$(dirname "$0")/../.agents/skills/dev-prune/SKILL.md"

if [ -f "$APP_SKILL" ]; then
    cp "$APP_SKILL" "$SKILL_DIR/SKILL.md"
elif [ -f "$REPO_SKILL" ]; then
    cp "$REPO_SKILL" "$SKILL_DIR/SKILL.md"
fi

# Copy to Antigravity config directory if present
GEMINI_DIR="$HOME/.gemini/config/skills/dev-prune"
if [ -d "$HOME/.gemini/config" ]; then
    mkdir -p "$GEMINI_DIR"
    cp "$SKILL_DIR/SKILL.md" "$GEMINI_DIR/SKILL.md"
fi

echo "✓ dev-prune AI Skill installed to $SKILL_DIR/SKILL.md"
echo ""
echo "Run 'devp skill' in terminal to display AI onboarding prompts anytime."
