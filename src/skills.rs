//! Skill discovery and registry for local SKILL.md files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

// === Defaults ===

#[allow(dead_code)]
#[must_use]
pub fn default_skills_dir() -> PathBuf {
    dirs::home_dir().map_or_else(
        || PathBuf::from("/tmp/deepseek/skills"),
        |p| p.join(".deepseek").join("skills"),
    )
}

// === Types ===

/// Parsed representation of a SKILL.md definition.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// Collection of discovered skills.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: Vec<Skill>,
}

impl SkillRegistry {
    /// Discover skills from the given directory.
    #[must_use]
    pub fn discover(dir: &Path) -> Self {
        let mut registry = Self::default();
        if !dir.exists() {
            return registry;
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type()
                    && ft.is_dir()
                {
                    let skill_path = entry.path().join("SKILL.md");
                    if let Ok(content) = fs::read_to_string(&skill_path)
                        && let Some(skill) = Self::parse_skill(&skill_path, &content)
                    {
                        registry.skills.push(skill);
                    }
                }
            }
        }
        registry
    }

    fn parse_skill(_path: &Path, content: &str) -> Option<Skill> {
        let trimmed = content.trim_start();
        let (frontmatter, body) = if trimmed.starts_with("---") {
            let start = content.find("---")?;
            let rest = &content[start + 3..];
            let end = rest.find("---")?;
            (&rest[..end], &rest[end + 3..])
        } else {
            let frontmatter_end = content.find("---")?;
            (&content[..frontmatter_end], &content[frontmatter_end + 3..])
        };
        let name = frontmatter
            .lines()
            .find(|l| l.starts_with("name:"))
            .and_then(|l| l.split(':').nth(1))?
            .trim()
            .to_string();

        let description = frontmatter
            .lines()
            .find(|l| l.starts_with("description:"))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let body = body.trim().to_string();

        Some(Skill {
            name,
            description,
            body,
        })
    }

    /// Lookup a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// Return all loaded skills.
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    /// Check whether any skills were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Return the number of loaded skills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }
}

// === CLI Helpers ===

#[allow(dead_code)] // CLI utility for future use
pub fn list(skills_dir: &Path) -> Result<()> {
    if !skills_dir.exists() {
        println!("No skills directory found at {}", skills_dir.display());
        return Ok(());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(skills_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
    }

    if entries.is_empty() {
        println!("No skills found in {}", skills_dir.display());
        return Ok(());
    }

    entries.sort();
    for entry in entries {
        println!("{entry}");
    }
    Ok(())
}

#[allow(dead_code)] // CLI utility for future use
pub fn show(skills_dir: &Path, name: &str) -> Result<()> {
    let path = skills_dir.join(name).join("SKILL.md");
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    println!("{contents}");
    Ok(())
}
