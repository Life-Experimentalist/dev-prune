// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

/// AI Agent Skill exporter & onboarding prompt generator.
use anyhow::{Context, Result};
use std::fs;

use crate::config::Registry;
use crate::output;

pub const EMBEDDED_SKILL_MD: &str = include_str!("../../.agents/skills/dev-prune/SKILL.md");

/// The condensed rules `--agent` writes: what the tool is, the non-negotiables, and a
/// pointer at the full SKILL.md — short enough that an editor loads it on every turn.
pub const EMBEDDED_RULES_MD: &str = include_str!("../../.agents/rules/dev-prune.rules.md");

/// Editors whose agents read per-repository rule files.
///
/// Claude Code is deliberately absent: its skill installs globally (`devp skill`,
/// `devp setup`), so there is nothing to write into individual repositories.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum AgentEditor {
    /// `.cursor/rules/dev-prune.mdc`
    Cursor,
    /// `.windsurf/rules/dev-prune.md`
    Windsurf,
    /// `.agent/rules/dev-prune.md` (Antigravity)
    Antigravity,
    /// `.clinerules/dev-prune.md`
    Cline,
    /// `.github/copilot-instructions.md`, as a marked block
    Copilot,
    /// `AGENTS.md`, as a marked block — the cross-tool convention (Codex, Jules,
    /// Amp, OpenCode, Antigravity and others read it)
    AgentsMd,
}

/// Run `devp skill` to export SKILL.md and display AI Agent onboarding prompts, or
/// `devp skill --agent <editor>` to write per-repository rules for one editor.
pub fn run(agent: Option<AgentEditor>) -> Result<()> {
    if let Some(editor) = agent {
        return write_agent_rules(editor);
    }
    output::print_header("dev-prune AI Agent Skill Integration");

    // The export is the command's one job — claiming success over a swallowed write
    // error would leave the user pointing an agent at a file that is not there.
    let skill_path = {
        let config_dir =
            Registry::config_dir().context("could not resolve the config directory")?;
        fs::create_dir_all(&config_dir)
            .with_context(|| format!("could not create {}", output::clean_path(&config_dir)))?;
        let target = config_dir.join("SKILL.md");
        fs::write(&target, EMBEDDED_SKILL_MD)
            .with_context(|| format!("could not write {}", output::clean_path(&target)))?;
        output::clean_path(&target)
    };

    output::print_success(&format!("Bundled SKILL.md exported to `{skill_path}`"));

    // Agents with an on-disk skill format get the file put where they read it, so the
    // prompts below are only needed for the ones without one.
    let agent_roots = crate::setup::agent_skill_roots();
    match crate::setup::ensure_agent_skills() {
        crate::setup::Outcome::Installed | crate::setup::Outcome::AlreadyPresent => {
            for root in &agent_roots {
                output::print_success(&format!(
                    "Skill installed for your AI agent at `{}`",
                    output::clean_path(root.join("SKILL.md"))
                ));
            }
        }
        crate::setup::Outcome::Skipped(_) => {
            output::print_info(
                "No AI agent skills directory was found — use the prompts below instead.",
            );
        }
        crate::setup::Outcome::Failed(why) => {
            output::print_warning(&format!(
                "Could not install into the agent skills directory: {why}"
            ));
        }
    }
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

/// Write the condensed rules into the current repository, in `editor`'s format.
///
/// Per-repository by design: these files are meant to be committed so the whole team's
/// agents pick them up, which is exactly why nothing here is written unasked — this
/// runs only when the user types the flag.
fn write_agent_rules(editor: AgentEditor) -> Result<()> {
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    if !crate::scanner::is_git_repo(&cwd) {
        anyhow::bail!(
            "`--agent` writes rules into a repository, and the current directory is not \
             one. Run it from the repository root."
        );
    }

    let written = match editor {
        AgentEditor::Cursor => {
            let target = cwd.join(crate::constants::CURSOR_RULES_FILE);
            let content = format!(
                "---\ndescription: dev-prune (devp) — reclaiming disk space from idle \
                 repositories safely\nalwaysApply: false\n---\n\n{EMBEDDED_RULES_MD}"
            );
            write_rules_file(&target, &content)?;
            target
        }
        AgentEditor::Windsurf => {
            let target = cwd.join(crate::constants::WINDSURF_RULES_FILE);
            write_rules_file(&target, EMBEDDED_RULES_MD)?;
            target
        }
        AgentEditor::Antigravity => {
            let target = cwd.join(crate::constants::ANTIGRAVITY_RULES_FILE);
            write_rules_file(&target, EMBEDDED_RULES_MD)?;
            target
        }
        AgentEditor::Cline => {
            let target = cwd.join(crate::constants::CLINE_RULES_FILE);
            write_rules_file(&target, EMBEDDED_RULES_MD)?;
            target
        }
        // These two formats are one shared file, so dev-prune owns a marked block
        // inside it rather than the file: replace the block if a previous run left
        // one, append it otherwise, and touch nothing outside the markers.
        AgentEditor::Copilot => {
            let target = cwd.join(crate::constants::COPILOT_INSTRUCTIONS_FILE);
            let existing = fs::read_to_string(&target).unwrap_or_default();
            write_rules_file(&target, &upsert_marked_block(&existing))?;
            target
        }
        AgentEditor::AgentsMd => {
            let target = cwd.join(crate::constants::AGENTS_MD_FILE);
            let existing = fs::read_to_string(&target).unwrap_or_default();
            write_rules_file(&target, &upsert_marked_block(&existing))?;
            target
        }
    };

    output::print_success(&format!("Rules written: {}", output::clean_path(&written)));
    output::print_info(
        "Commit the file if the whole team's agents should have it; it is inert data \
         and safe to share.",
    );
    Ok(())
}

/// Replace dev-prune's marked block in `existing`, or append one — leaving every
/// byte outside the markers exactly as found.
fn upsert_marked_block(existing: &str) -> String {
    let block = format!(
        "{}\n{EMBEDDED_RULES_MD}{}\n",
        crate::constants::RULES_BLOCK_START,
        crate::constants::RULES_BLOCK_END
    );
    match (
        existing.find(crate::constants::RULES_BLOCK_START),
        existing.find(crate::constants::RULES_BLOCK_END),
    ) {
        (Some(start), Some(end)) if end > start => {
            let after = end + crate::constants::RULES_BLOCK_END.len();
            // The trailing newline of the old block belongs to it.
            let after = if existing[after..].starts_with('\n') {
                after + 1
            } else {
                after
            };
            format!("{}{block}{}", &existing[..start], &existing[after..])
        }
        _ if existing.is_empty() => block,
        _ => format!("{}\n\n{block}", existing.trim_end_matches('\n')),
    }
}

fn write_rules_file(target: &std::path::Path, content: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", output::clean_path(parent)))?;
    }
    fs::write(target, content)
        .with_context(|| format!("could not write {}", output::clean_path(target)))
}
