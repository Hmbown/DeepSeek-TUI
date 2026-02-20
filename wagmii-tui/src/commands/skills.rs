//! Skills commands: skills, skill

use std::fmt::Write;

use crate::skills::SkillRegistry;
use crate::tui::app::App;
use crate::tui::history::HistoryCell;

use super::CommandResult;

fn render_skill_warnings(registry: &SkillRegistry) -> String {
    if registry.warnings().is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let _ = writeln!(out, "\nWarnings ({}):", registry.warnings().len());
    for warning in registry.warnings() {
        let _ = writeln!(out, "  - {warning}");
    }
    out
}

/// List all available skills
pub fn list_skills(app: &mut App) -> CommandResult {
    let skills_dir = app.skills_dir.clone();
    let registry = SkillRegistry::discover(&skills_dir);
    let warnings = render_skill_warnings(&registry);

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
             <instructions here>{warnings}",
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
        "\nUse /skill <name> to run a skill\nSkills location: {}{}",
        skills_dir.display(),
        warnings
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
        let warnings = render_skill_warnings(&registry);

        if available.is_empty() {
            CommandResult::error(format!(
                "Skill '{name}' not found. No skills installed.\n\nUse /skills to see how to add skills.{warnings}"
            ))
        } else {
            CommandResult::error(format!(
                "Skill '{}' not found.\n\nAvailable skills: {}{}",
                name,
                available.join(", "),
                warnings
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use tempfile::TempDir;

    fn create_test_app_with_tmpdir(tmpdir: &TempDir) -> App {
        let options = TuiOptions {
            model: "wagmii-v3.2".to_string(),
            workspace: tmpdir.path().to_path_buf(),
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: tmpdir.path().join("skills"),
            memory_path: tmpdir.path().join("memory.md"),
            notes_path: tmpdir.path().join("notes.txt"),
            mcp_config_path: tmpdir.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    fn create_skill_dir(tmpdir: &TempDir, skill_name: &str, skill_content: &str) {
        let skill_dir = tmpdir.path().join("skills").join(skill_name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();
    }

    #[test]
    fn test_list_skills_empty_directory() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = list_skills(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("No skills found"));
        assert!(msg.contains("Skills location:"));
    }

    #[test]
    fn test_list_skills_with_skills() {
        let tmpdir = TempDir::new().unwrap();
        create_skill_dir(
            &tmpdir,
            "test-skill",
            "---\nname: test-skill\ndescription: A test skill\n---\nDo something",
        );
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = list_skills(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Available skills"));
        assert!(msg.contains("/test-skill"));
    }

    #[test]
    fn test_run_skill_without_name() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = run_skill(&mut app, None);
        assert!(result.message.is_some());
        assert!(result.message.unwrap().contains("Usage: /skill"));
    }

    #[test]
    fn test_run_skill_not_found() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = run_skill(&mut app, Some("nonexistent"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_run_skill_activates() {
        let tmpdir = TempDir::new().unwrap();
        create_skill_dir(
            &tmpdir,
            "test-skill",
            "---\nname: test-skill\ndescription: A test skill\n---\nDo something special",
        );
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = run_skill(&mut app, Some("test-skill"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Skill 'test-skill' activated"));
        assert!(msg.contains("A test skill"));
        assert!(app.active_skill.is_some());
        assert!(!app.history.is_empty());
    }
}
