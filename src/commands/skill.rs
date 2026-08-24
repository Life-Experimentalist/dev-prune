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
    /// `.roo/rules/dev-prune.md` (Roo Code)
    Roo,
    /// `.kilocode/rules/dev-prune.md` (Kilo Code)
    Kilocode,
    /// `.continue/rules/dev-prune.md` (Continue)
    Continue,
    /// `.amazonq/rules/dev-prune.md` (Amazon Q Developer)
    AmazonQ,
    /// `.kiro/steering/dev-prune.md` (Kiro)
    Kiro,
    /// `.trae/rules/dev-prune.md` (Trae)
    Trae,
    /// `.junie/guidelines.md`, as a marked block (JetBrains Junie)
    Junie,
    /// `GEMINI.md`, as a marked block (Gemini CLI)
    Gemini,
    /// `.rules`, as a marked block (Zed — read ahead of every other convention)
    Zed,
    /// `.github/copilot-instructions.md`, as a marked block
    Copilot,
    /// `CONVENTIONS.md`, as a marked block (Aider — which has to be told to read it)
    Aider,
    /// `AGENTS.md`, as a marked block — the cross-tool convention (Codex, Jules,
    /// Amp, OpenCode, Antigravity and others read it)
    AgentsMd,
}

/// How the rules go into the file.
enum Style {
    /// dev-prune owns the whole file.
    OwnFile,
    /// Cursor's `.mdc` format, which needs frontmatter above the rules.
    CursorMdc,
    /// The file belongs to somebody else, so dev-prune owns a marked block inside it
    /// and leaves every byte outside the markers exactly as found.
    MarkedBlock,
}

impl AgentEditor {
    /// The repository-relative file this editor's agent actually reads, and how to
    /// write into it.
    ///
    /// One table rather than one match arm each: an editor whose agent reads a
    /// directory of rule files is the same three lines every time, and the only thing
    /// a contributor should have to establish is the path.
    fn target(self) -> (&'static str, Style) {
        use crate::constants as c;
        match self {
            AgentEditor::Cursor => (c::CURSOR_RULES_FILE, Style::CursorMdc),
            AgentEditor::Windsurf => (c::WINDSURF_RULES_FILE, Style::OwnFile),
            AgentEditor::Antigravity => (c::ANTIGRAVITY_RULES_FILE, Style::OwnFile),
            AgentEditor::Cline => (c::CLINE_RULES_FILE, Style::OwnFile),
            AgentEditor::Roo => (c::ROO_RULES_FILE, Style::OwnFile),
            AgentEditor::Kilocode => (c::KILOCODE_RULES_FILE, Style::OwnFile),
            AgentEditor::Continue => (c::CONTINUE_RULES_FILE, Style::OwnFile),
            AgentEditor::AmazonQ => (c::AMAZON_Q_RULES_FILE, Style::OwnFile),
            AgentEditor::Kiro => (c::KIRO_STEERING_FILE, Style::OwnFile),
            AgentEditor::Trae => (c::TRAE_RULES_FILE, Style::OwnFile),
            AgentEditor::Junie => (c::JUNIE_GUIDELINES_FILE, Style::MarkedBlock),
            AgentEditor::Gemini => (c::GEMINI_MD_FILE, Style::MarkedBlock),
            AgentEditor::Zed => (c::ZED_RULES_FILE, Style::MarkedBlock),
            AgentEditor::Copilot => (c::COPILOT_INSTRUCTIONS_FILE, Style::MarkedBlock),
            AgentEditor::Aider => (c::AIDER_CONVENTIONS_FILE, Style::MarkedBlock),
            AgentEditor::AgentsMd => (c::AGENTS_MD_FILE, Style::MarkedBlock),
        }
    }

    /// What the user still has to do, for the one editor that does not read its
    /// file unprompted. Aider loads `CONVENTIONS.md` only when told to, so writing
    /// the file and saying nothing would leave rules an agent never sees.
    fn wiring(self) -> Option<&'static str> {
        match self {
            AgentEditor::Aider => Some(
                "Aider does not read this file on its own. Add `read: CONVENTIONS.md` \
                 to `.aider.conf.yml`, or start it with `aider --read CONVENTIONS.md`.",
            ),
            _ => None,
        }
    }
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

    let (relative, style) = editor.target();
    let target = cwd.join(relative);
    let content = match style {
        Style::OwnFile => EMBEDDED_RULES_MD.to_string(),
        Style::CursorMdc => format!(
            "---\ndescription: dev-prune (devp) — reclaiming disk space from idle \
             repositories safely\nalwaysApply: false\n---\n\n{EMBEDDED_RULES_MD}"
        ),
        Style::MarkedBlock => {
            let existing = fs::read_to_string(&target).unwrap_or_default();
            upsert_marked_block(&existing)
        }
    };
    write_rules_file(&target, &content)?;

    output::print_success(&format!("Rules written: {}", output::clean_path(&target)));
    if let Some(wiring) = editor.wiring() {
        output::print_info(wiring);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;

    #[test]
    fn every_editor_writes_to_its_own_file() {
        // A copy-pasted path would make one editor silently overwrite another's rules,
        // and nothing else in the program would notice.
        let mut paths: Vec<&str> = AgentEditor::value_variants()
            .iter()
            .map(|e| e.target().0)
            .collect();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), total, "two editors share a path");
    }

    #[test]
    fn the_editor_that_has_to_be_told_to_read_its_file_says_so() {
        // Rules an agent never loads are worse than no rules at all: the repository
        // looks configured and nothing is. Aider is the only target whose file is not
        // picked up by being there, so it is the only one that carries a note — and the
        // note has to name the file, because that name is what goes in the config.
        for editor in AgentEditor::value_variants() {
            if let Some(note) = editor.wiring() {
                let path = editor.target().0;
                assert_eq!(path, crate::constants::AIDER_CONVENTIONS_FILE);
                assert!(
                    note.contains(path),
                    "the note does not name the file: {note}"
                );
            }
        }
        assert!(
            AgentEditor::Aider.wiring().is_some(),
            "aider writes a file nothing reads until it is configured"
        );
    }

    #[test]
    fn a_shared_file_is_only_ever_edited_inside_the_markers() {
        // The whole reason `MarkedBlock` exists: these files belong to the user, and a
        // second run must not stack a second copy of the rules on top of the first.
        let theirs = "# Our conventions\n\nUse tabs.\n";
        let once = upsert_marked_block(theirs);
        let twice = upsert_marked_block(&once);
        assert_eq!(once, twice, "a second write duplicated the block");
        assert!(once.starts_with(theirs));
        assert_eq!(once.matches(crate::constants::RULES_BLOCK_START).count(), 1);
    }
}
