//! Skills commands: skills, skill

use std::fmt::Write;

use crate::skills::SkillRegistry;
use crate::tui::app::App;
use crate::tui::history::HistoryCell;

use super::CommandResult;

/// List all available skills
pub fn list_skills(app: &mut App) -> CommandResult {
    let skills_dir = app.skills_dir.clone();
    let registry = SkillRegistry::discover(&skills_dir);

    if registry.is_empty() {
        let msg = format!(
            "No skills found.\n\n\
             Skills location: {}\n\n\
             To add skills, create directories with SKILL.md files:\n  \
             {}/my-skill/SKILL.md\n\n\
             Format:\n  \
             ---\n  \
             name: my-skill\n  \
             description: What this skill does\n  \
             allowed-tools: read_file, list_dir\n  \
             ---\n\n  \
             <instructions here>",
            skills_dir.display(),
            skills_dir.display()
        );
        return CommandResult::message(msg);
    }

    let mut output = format!("Available skills ({}):\n", registry.len());
    output.push_str("─────────────────────────────\n");
    for skill in registry.list() {
        let _ = writeln!(output, "  /{} - {}", skill.name, skill.description);
    }
    let _ = write!(
        output,
        "\nUse /skill <name> to run a skill\nSkills location: {}",
        skills_dir.display()
    );

    CommandResult::message(output)
}

/// Run a specific skill - activates skill for next user message
pub fn run_skill(app: &mut App, name: Option<&str>) -> CommandResult {
    let name = match name {
        Some(n) => n.trim(),
        None => {
            return CommandResult::error("Usage: /skill <name>");
        }
    };

    let skills_dir = app.skills_dir.clone();
    let registry = SkillRegistry::discover(&skills_dir);

    if let Some(skill) = registry.get(name) {
        let instruction = format!(
            "You are now using a skill. Follow these instructions:\n\n# Skill: {}\n\n{}\n\n---\n\nNow respond to the user's request following the above skill instructions.",
            skill.name, skill.body
        );

        app.add_message(HistoryCell::System {
            content: format!("Activated skill: {}\n\n{}", skill.name, skill.description),
        });

        app.active_skill = Some(instruction);

        CommandResult::message(format!(
            "Skill '{}' activated.\n\nDescription: {}\n\nType your request and the skill instructions will be applied.",
            skill.name, skill.description
        ))
    } else {
        let available: Vec<String> = registry.list().iter().map(|s| s.name.clone()).collect();

        if available.is_empty() {
            CommandResult::error(format!(
                "Skill '{name}' not found. No skills installed.\n\nUse /skills to see how to add skills."
            ))
        } else {
            CommandResult::error(format!(
                "Skill '{}' not found.\n\nAvailable skills: {}",
                name,
                available.join(", ")
            ))
        }
    }
}
