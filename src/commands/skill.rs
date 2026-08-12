// Copyright 2026 VKrishna04
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/// AI Agent Skill exporter & onboarding prompt generator.
use anyhow::Result;
use std::fs;

use crate::config::Registry;
use crate::output;

pub const EMBEDDED_SKILL_MD: &str = include_str!("../../.agents/skills/dev-prune/SKILL.md");

/// Run `devp skill` to export SKILL.md and display AI Agent onboarding prompts.
pub fn run() -> Result<()> {
    output::print_header("dev-prune AI Agent Skill Integration");

    let skill_path = if let Ok(config_dir) = Registry::config_dir() {
        if !config_dir.exists() {
            let _ = fs::create_dir_all(&config_dir);
        }
        let target = config_dir.join("SKILL.md");
        let _ = fs::write(&target, EMBEDDED_SKILL_MD);
        output::clean_path(&target)
    } else {
        "~/.config/dev-prune/SKILL.md".to_string()
    };

    output::print_success(&format!("Bundled SKILL.md exported to `{skill_path}`"));
    println!();
    output::print_header("🤖 AI Agent Onboarding Prompts (Copy & Paste to your AI Assistant)");
    println!();
    output::print_info("Prompt 1: Initial Workspace Discovery & Onboarding");
    println!("```markdown");
    println!(
        "Read the dev-prune AI skill at file://{skill_path} and run `devp init` to scan, register, and onboard all Git repositories in my workspace."
    );
    println!("```");
    println!();
    output::print_info(
        "Prompt 2: Universal Skill Import (Antigravity, Claude Code, Cursor, Windsurf, Copilot, OpenClaw)",
    );
    println!("```markdown");
    println!(
        "I have installed `dev-prune` on my machine. Read the skill at file://{skill_path} and import it into your agent skills folder so you can autonomously maintain lockfiles and prune bloat directories."
    );
    println!("```");

    Ok(())
}
