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

    /// The editor's name as a person knows it, for the detection report.
    fn label(self) -> &'static str {
        match self {
            AgentEditor::Cursor => "Cursor",
            AgentEditor::Windsurf => "Windsurf",
            AgentEditor::Antigravity => "Antigravity",
            AgentEditor::Cline => "Cline",
            AgentEditor::Roo => "Roo Code",
            AgentEditor::Kilocode => "Kilo Code",
            AgentEditor::Continue => "Continue",
            AgentEditor::AmazonQ => "Amazon Q Developer",
            AgentEditor::Kiro => "Kiro",
            AgentEditor::Trae => "Trae",
            AgentEditor::Junie => "JetBrains Junie",
            AgentEditor::Gemini => "Gemini CLI",
            AgentEditor::Zed => "Zed",
            AgentEditor::Copilot => "GitHub Copilot",
            AgentEditor::Aider => "Aider",
            AgentEditor::AgentsMd => "AGENTS.md readers (Codex, Jules, Amp, OpenCode)",
        }
    }

    /// The on-disk traces that mark this editor as present, for the detection
    /// report and `--detected`.
    ///
    /// Presence on disk, never process probing: every editor here leaves a
    /// well-known directory or file behind once it has run, so an `exists()`
    /// check finds it without spawning anything — which is also what lets the
    /// tests fabricate a whole machine out of a temp directory. The strings live
    /// inline rather than in `constants.rs` for the same reason as
    /// `detect_vscode_editors`'s candidate list: each one means something only as
    /// this table's row. A row may be empty on every axis — the editor is then
    /// reachable by name but never claimed as detected — so a contributor adding
    /// a target owes this table nothing.
    fn traces(self) -> Traces {
        const NONE: Traces = Traces {
            home: &[],
            repo: &[],
            extension: None,
        };
        match self {
            AgentEditor::Cursor => Traces {
                home: &[".cursor"],
                repo: &[".cursor"],
                ..NONE
            },
            AgentEditor::Windsurf => Traces {
                home: &[".codeium/windsurf", ".windsurf"],
                repo: &[".windsurf"],
                ..NONE
            },
            // `.gemini` alone is not evidence of the Gemini CLI: Antigravity keeps
            // its own state under `.gemini/antigravity*` on a machine that has
            // never run the CLI, which is why Antigravity claims that subtree and
            // Gemini claims only the CLI's own settings file.
            AgentEditor::Antigravity => Traces {
                home: &[".antigravity-ide", ".gemini/antigravity"],
                repo: &[".agent"],
                ..NONE
            },
            AgentEditor::Cline => Traces {
                repo: &[".clinerules"],
                extension: Some("saoudrizwan.claude-dev"),
                ..NONE
            },
            // Roo Code kept its original extension ID when it renamed, so the
            // prefix stops before the `-cline` suffix to survive another rename.
            AgentEditor::Roo => Traces {
                repo: &[".roo"],
                extension: Some("rooveterinaryinc.roo"),
                ..NONE
            },
            AgentEditor::Kilocode => Traces {
                repo: &[".kilocode"],
                extension: Some("kilocode.kilo-code"),
                ..NONE
            },
            AgentEditor::Continue => Traces {
                home: &[".continue"],
                repo: &[".continue"],
                extension: Some("continue.continue"),
            },
            AgentEditor::AmazonQ => Traces {
                home: &[".aws/amazonq"],
                repo: &[".amazonq"],
                extension: Some("amazonwebservices.amazon-q-vscode"),
            },
            AgentEditor::Kiro => Traces {
                home: &[".kiro"],
                repo: &[".kiro"],
                ..NONE
            },
            AgentEditor::Trae => Traces {
                home: &[".trae"],
                repo: &[".trae"],
                ..NONE
            },
            AgentEditor::Junie => Traces {
                repo: &[".junie"],
                ..NONE
            },
            AgentEditor::Gemini => Traces {
                home: &[".gemini/settings.json"],
                repo: &["GEMINI.md"],
                ..NONE
            },
            AgentEditor::Zed => Traces {
                home: &[".config/zed", "AppData/Roaming/Zed"],
                repo: &[".rules"],
                ..NONE
            },
            // `.copilot` is the Copilot CLI's home directory; the extension covers
            // the editor-hosted install.
            AgentEditor::Copilot => Traces {
                home: &[".copilot"],
                repo: &[".github/copilot-instructions.md"],
                extension: Some("github.copilot"),
            },
            AgentEditor::Aider => Traces {
                home: &[".aider.conf.yml"],
                repo: &[".aider.conf.yml"],
                ..NONE
            },
            AgentEditor::AgentsMd => Traces {
                home: &[".codex", ".config/opencode", ".opencode"],
                repo: &["AGENTS.md"],
                ..NONE
            },
        }
    }

    fn detected_under(self, home: &std::path::Path, repo: Option<&std::path::Path>) -> bool {
        let traces = self.traces();
        traces.home.iter().any(|t| home.join(t).exists())
            || traces
                .extension
                .is_some_and(|prefix| extension_present(home, prefix))
            || repo.is_some_and(|r| traces.repo.iter().any(|t| r.join(t).exists()))
    }

    /// Whether `repo` already carries this editor's rules, and whether they match
    /// the ones this binary would write — the same current-or-stale distinction
    /// `stale_skill_copies` draws for the global SKILL.md installs.
    fn rules_state(self, repo: &std::path::Path) -> RulesState {
        let (relative, style) = self.target();
        let Ok(existing) = fs::read_to_string(repo.join(relative)) else {
            return RulesState::Missing;
        };
        match style {
            Style::OwnFile | Style::CursorMdc => {
                if existing.contains(EMBEDDED_RULES_MD) {
                    RulesState::Current
                } else {
                    RulesState::Stale
                }
            }
            Style::MarkedBlock => {
                if !existing.contains(crate::constants::RULES_BLOCK_START) {
                    RulesState::Missing
                } else if existing.contains(EMBEDDED_RULES_MD) {
                    RulesState::Current
                } else {
                    RulesState::Stale
                }
            }
        }
    }

    /// The value `--agent` takes for this editor, exactly as clap will parse it —
    /// derived rather than restated, so the report cannot suggest a spelling the
    /// parser refuses.
    fn flag_value(self) -> String {
        use clap::ValueEnum;
        self.to_possible_value()
            .expect("no skipped variants")
            .get_name()
            .to_string()
    }
}

/// See [`AgentEditor::traces`].
struct Traces {
    /// Paths relative to the home directory; a file or a directory, either counts.
    home: &'static [&'static str],
    /// Paths relative to the repository root — evidence the *team* uses the editor
    /// even when this machine has never run it.
    repo: &'static [&'static str],
    /// An extension ID prefix to look for under every VS Code-family editor's
    /// extension directory.
    extension: Option<&'static str>,
}

/// Whether a repository's rules file is the one this binary would write.
enum RulesState {
    Current,
    Stale,
    Missing,
}

/// Where every VS Code-family editor unpacks its installed extensions, relative to
/// the home directory. A miss costs one failed directory read, so listing a fork
/// nobody has is free — the same reasoning as `detect_vscode_editors`.
const EXTENSION_ROOTS: &[&str] = &[
    ".vscode/extensions",
    ".vscode-insiders/extensions",
    ".vscode-oss/extensions",
    ".antigravity-ide/extensions",
    ".cursor/extensions",
    ".windsurf/extensions",
    ".trae/extensions",
    ".kiro/extensions",
];

/// Whether any installed extension's directory name starts with `prefix`.
///
/// Anchored at the publisher, not a substring match: `nvidia.nsight-copilot` must
/// not read as GitHub Copilot. Extension directories are named
/// `<publisher>.<name>-<version>`, lowercased by the editor, so the prefix is
/// compared against the lowercased name.
fn extension_present(home: &std::path::Path, prefix: &str) -> bool {
    EXTENSION_ROOTS.iter().any(|root| {
        fs::read_dir(home.join(root)).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(prefix)
            })
        })
    })
}

/// Every editor whose traces are on this machine — or, when `repo` is given, in
/// that repository. Both roots come in as parameters so tests can hand in a
/// fabricated machine instead of reading the real one.
pub fn detect_editors(home: &std::path::Path, repo: Option<&std::path::Path>) -> Vec<AgentEditor> {
    use clap::ValueEnum;
    AgentEditor::value_variants()
        .iter()
        .copied()
        .filter(|editor| editor.detected_under(home, repo))
        .collect()
}

/// Run `devp skill` to export SKILL.md, report the detected editors and display AI
/// Agent onboarding prompts; `devp skill --agent <editor>` writes per-repository
/// rules for one editor, and `devp skill --detected` writes them for every editor
/// the report would list.
pub fn run(agent: Option<AgentEditor>, detected: bool) -> Result<()> {
    if let Some(editor) = agent {
        return write_agent_rules(editor);
    }
    if detected {
        return write_detected_rules();
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
    print_detection_report();
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

/// Both writing flags need a repository to write into; the message names the flag
/// the user actually typed.
fn require_repo(cwd: &std::path::Path, flag: &str) -> Result<()> {
    if !crate::scanner::is_git_repo(cwd) {
        anyhow::bail!(
            "`{flag}` writes rules into a repository, and the current directory is not \
             one. Run it from the repository root."
        );
    }
    Ok(())
}

/// Write the condensed rules into the current repository, in `editor`'s format.
///
/// Per-repository by design: these files are meant to be committed so the whole team's
/// agents pick them up, which is exactly why nothing here is written unasked — this
/// runs only when the user types the flag.
fn write_agent_rules(editor: AgentEditor) -> Result<()> {
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    require_repo(&cwd, "--agent")?;
    write_rules_for(editor, &cwd)?;
    print_commit_note();
    Ok(())
}

/// Write rules for every editor detected on this machine or in this repository —
/// the ones plain `devp skill` reports. Same consent shape as `--agent`: the report
/// only ever prints, and typing the flag is what authorises the writes.
fn write_detected_rules() -> Result<()> {
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    require_repo(&cwd, "--detected")?;
    let home = dirs::home_dir().context("could not resolve the home directory")?;
    let editors = detect_editors(&home, Some(&cwd));
    if editors.is_empty() {
        output::print_info(
            "No editor left a trace on this machine or in this repository — nothing to \
             write. `devp skill --agent <editor>` writes rules for one by name; `devp \
             skill --help` lists them.",
        );
        return Ok(());
    }
    for editor in editors {
        write_rules_for(editor, &cwd)?;
    }
    print_commit_note();
    Ok(())
}

fn write_rules_for(editor: AgentEditor, cwd: &std::path::Path) -> Result<()> {
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
    Ok(())
}

fn print_commit_note() {
    output::print_info(
        "Commit the file if the whole team's agents should have it; it is inert data \
         and safe to share.",
    );
}

/// The read-only half of detection: which editors this machine or repository shows
/// traces of, whether their rules are already written here, and the command that
/// writes them. Printed by the bare `devp skill` run; `--detected` acts on the
/// same list.
fn print_detection_report() {
    // No home directory means no baseline to detect against; the bare run has other
    // jobs, so the report just stays silent rather than failing them.
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let repo = std::env::current_dir()
        .ok()
        .filter(|d| crate::scanner::is_git_repo(d));
    let editors = detect_editors(&home, repo.as_deref());
    if editors.is_empty() {
        return;
    }
    println!();
    output::print_header("Detected editors and agents");
    let rows: Vec<(String, &'static str, String)> = editors
        .iter()
        .map(|editor| {
            let state = match &repo {
                Some(r) => match editor.rules_state(r) {
                    RulesState::Current => "rules current here",
                    RulesState::Stale => "rules stale here",
                    RulesState::Missing => "no rules here yet",
                },
                None => "",
            };
            (
                editor.label().to_string(),
                state,
                format!("devp skill --agent {}", editor.flag_value()),
            )
        })
        .collect();
    let label_width = rows.iter().map(|(l, _, _)| l.len()).max().unwrap_or(0);
    let state_width = rows.iter().map(|(_, s, _)| s.len()).max().unwrap_or(0);
    for (label, state, command) in &rows {
        println!("  {label:<label_width$}  {state:<state_width$}  {command}");
    }
    if repo.is_some() {
        output::print_info(
            "`devp skill --detected` writes rules for all of them into this repository \
             in one pass.",
        );
    } else {
        output::print_info(
            "Run `devp skill --detected` from a repository root to write rules for all \
             of them in one pass.",
        );
    }
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

    // Detection takes its roots as parameters precisely so these tests can build a
    // machine out of a temp directory — none of them reads the real home directory.

    #[test]
    fn a_home_trace_is_enough() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".continue")).unwrap();
        let found = detect_editors(home.path(), None);
        assert!(
            found.iter().any(|e| matches!(e, AgentEditor::Continue)),
            "a `.continue` home directory did not detect Continue"
        );
    }

    #[test]
    fn a_bare_machine_detects_nothing() {
        let home = tempfile::tempdir().unwrap();
        assert!(detect_editors(home.path(), None).is_empty());
    }

    #[test]
    fn an_extension_counts_only_by_publisher_prefix() {
        // The one real near-miss on record: `nvidia.nsight-copilot` sits in the same
        // extensions directory and contains "copilot", but it is not GitHub Copilot.
        let home = tempfile::tempdir().unwrap();
        let ext = home.path().join(".vscode/extensions");
        std::fs::create_dir_all(ext.join("nvidia.nsight-copilot-2026.1.21-win32-x64")).unwrap();
        assert!(
            !detect_editors(home.path(), None)
                .iter()
                .any(|e| matches!(e, AgentEditor::Copilot)),
            "a third-party extension containing 'copilot' read as GitHub Copilot"
        );

        std::fs::create_dir_all(ext.join("github.copilot-1.350.0")).unwrap();
        assert!(
            detect_editors(home.path(), None)
                .iter()
                .any(|e| matches!(e, AgentEditor::Copilot)),
            "the real github.copilot extension was not detected"
        );
    }

    #[test]
    fn repo_traces_apply_only_when_a_repository_is_given() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("GEMINI.md"), "# rules\n").unwrap();
        assert!(
            detect_editors(home.path(), None).is_empty(),
            "a repo trace was counted with no repository in play"
        );
        assert!(
            detect_editors(home.path(), Some(repo.path()))
                .iter()
                .any(|e| matches!(e, AgentEditor::Gemini)),
            "GEMINI.md in the repository did not detect the Gemini CLI"
        );
    }

    #[test]
    fn rules_state_tells_missing_from_stale_from_current() {
        let repo = tempfile::tempdir().unwrap();
        let editor = AgentEditor::Windsurf;
        assert!(matches!(
            editor.rules_state(repo.path()),
            RulesState::Missing
        ));

        let target = repo.path().join(editor.target().0);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "an older release's rules\n").unwrap();
        assert!(matches!(editor.rules_state(repo.path()), RulesState::Stale));

        std::fs::write(&target, EMBEDDED_RULES_MD).unwrap();
        assert!(matches!(
            editor.rules_state(repo.path()),
            RulesState::Current
        ));
    }

    #[test]
    fn a_shared_file_without_the_markers_counts_as_missing() {
        // AGENTS.md full of the user's own text is not dev-prune rules going stale;
        // it is a file dev-prune has never written into.
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("AGENTS.md"), "# Their agents file\n").unwrap();
        assert!(matches!(
            AgentEditor::AgentsMd.rules_state(repo.path()),
            RulesState::Missing
        ));
    }
}
