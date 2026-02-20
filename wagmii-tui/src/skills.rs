//! Skill discovery and registry for local SKILL.md files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::logging;

// === Defaults ===

#[allow(dead_code)]
#[must_use]
pub fn default_skills_dir() -> PathBuf {
    dirs::home_dir().map_or_else(
        || PathBuf::from("/tmp/wagmii/skills"),
        |p| p.join(".wagmii").join("skills"),
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
    warnings: Vec<String>,
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
                    match fs::read_to_string(&skill_path) {
                        Ok(content) => match Self::parse_skill(&skill_path, &content) {
                            Ok(skill) => registry.skills.push(skill),
                            Err(reason) => registry.push_warning(format!(
                                "Failed to parse {}: {reason}",
                                skill_path.display()
                            )),
                        },
                        Err(err) if skill_path.exists() => {
                            registry.push_warning(format!(
                                "Failed to read {}: {err}",
                                skill_path.display()
                            ));
                        }
                        Err(_) => {}
                    }
                }
            }
        } else {
            registry.push_warning(format!("Failed to read skills directory {}", dir.display()));
        }
        registry
    }

    fn push_warning(&mut self, warning: String) {
        logging::warn(&warning);
        self.warnings.push(warning);
    }

    fn parse_skill(_path: &Path, content: &str) -> std::result::Result<Skill, String> {
        let trimmed = content.trim_start();
        let (frontmatter, body) = if trimmed.starts_with("---") {
            let start = content
                .find("---")
                .ok_or_else(|| "missing frontmatter opening delimiter".to_string())?;
            let rest = &content[start + 3..];
            let end = rest
                .find("---")
                .ok_or_else(|| "missing frontmatter closing delimiter".to_string())?;
            (&rest[..end], &rest[end + 3..])
        } else {
            return Err("missing frontmatter opening delimiter '---'".to_string());
        };

        let mut metadata = HashMap::new();
        for raw in frontmatter.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                metadata.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }

        let name = metadata
            .get("name")
            .filter(|name| !name.is_empty())
            .cloned()
            .ok_or_else(|| "missing required frontmatter field: name".to_string())?;

        let description = metadata.get("description").cloned().unwrap_or_default();

        let body = body.trim().to_string();

        Ok(Skill {
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

    /// Parse or I/O warnings encountered while discovering skills.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
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
