//! Skill Index - Scans and indexes ~/.claude/skills/ for Knowledge Hub
//!
//! Provides:
//! - Frontmatter parsing (name, description, aka)
//! - In-memory search by name/aka/keyword
//! - Context block generation for agent prompt injection

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Metadata extracted from a skill's YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aka: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    /// Absolute path to the skill file
    pub path: PathBuf,
}

/// YAML frontmatter structure (for deserialization)
#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    aka: Option<Vec<String>>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

/// In-memory skill index
#[derive(Debug)]
pub struct SkillIndex {
    skills: Vec<SkillMeta>,
    /// name/aka → index into skills vec
    lookup: HashMap<String, usize>,
}

impl SkillIndex {
    /// Build index by scanning a skills directory
    pub fn build(skills_dir: &Path) -> Self {
        let mut skills = Vec::new();
        let mut lookup = HashMap::new();

        if !skills_dir.exists() {
            warn!(path = %skills_dir.display(), "Skills directory not found");
            return Self { skills, lookup };
        }

        // Scan for skill files
        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Failed to read skills directory");
                return Self { skills, lookup };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // Case 1: directory with SKILL.md or skill.md
            if path.is_dir() {
                let skill_file = find_skill_file(&path);
                if let Some(file_path) = skill_file {
                    if let Some(meta) = parse_skill_file(&file_path) {
                        let idx = skills.len();
                        register_lookup(&mut lookup, &meta, idx);
                        skills.push(meta);
                    }
                }
                // Also check subdirectories (e.g., services/auth/)
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_dir() {
                            if let Some(file_path) = find_skill_file(&sub_path) {
                                if let Some(meta) = parse_skill_file(&file_path) {
                                    let idx = skills.len();
                                    register_lookup(&mut lookup, &meta, idx);
                                    skills.push(meta);
                                }
                            }
                        }
                    }
                }
            }
            // Case 2: standalone .md file (e.g., stardew-game.md)
            else if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(meta) = parse_skill_file(&path) {
                    let idx = skills.len();
                    register_lookup(&mut lookup, &meta, idx);
                    skills.push(meta);
                }
            }
        }

        debug!(count = skills.len(), "Skill index built");
        Self { skills, lookup }
    }

    /// List all indexed skills
    pub fn list(&self) -> &[SkillMeta] {
        &self.skills
    }

    /// Search skills by keyword (matches name, aka, description)
    pub fn search(&self, query: &str) -> Vec<&SkillMeta> {
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        if keywords.is_empty() {
            return self.skills.iter().collect();
        }

        // Exact name/aka match first
        if let Some(&idx) = self.lookup.get(&query_lower) {
            return vec![&self.skills[idx]];
        }

        // Score-based fuzzy search
        let mut scored: Vec<(usize, &SkillMeta)> = self
            .skills
            .iter()
            .filter_map(|skill| {
                let mut score = 0usize;
                let name_lower = skill.name.to_lowercase();
                let desc_lower = skill
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase();
                let aka_joined = skill
                    .aka
                    .as_ref()
                    .map(|a| a.join(" ").to_lowercase())
                    .unwrap_or_default();

                for kw in &keywords {
                    if name_lower.contains(kw) {
                        score += 10;
                    }
                    if aka_joined.contains(kw) {
                        score += 8;
                    }
                    if desc_lower.contains(kw) {
                        score += 3;
                    }
                }

                if score > 0 {
                    Some((score, skill))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, s)| s).collect()
    }

    /// Build a [Context] block from matching skills
    pub fn build_context(&self, query: &str) -> String {
        let matches = self.search(query);
        if matches.is_empty() {
            return format!("[Context]\nNo matching skills found for: {}\n", query);
        }

        let mut context = String::from("[Context]\n");

        for skill in matches.iter().take(3) {
            // Read the skill file to extract key sections
            if let Ok(content) = std::fs::read_to_string(&skill.path) {
                // Extract sections after frontmatter
                let body = strip_frontmatter(&content);

                // Find INDEX section and key tables
                let sections = extract_key_sections(&body);

                context.push_str(&format!("## {} ({})\n", skill.name,
                    skill.description.as_deref().unwrap_or("")));

                if !sections.is_empty() {
                    context.push_str(&sections);
                }
                context.push('\n');
            } else {
                context.push_str(&format!("- {}: {}\n",
                    skill.name,
                    skill.description.as_deref().unwrap_or("(no description)")
                ));
            }
        }

        context
    }

    /// Get a skill by exact name
    pub fn get(&self, name: &str) -> Option<&SkillMeta> {
        let key = name.to_lowercase();
        self.lookup.get(&key).map(|&idx| &self.skills[idx])
    }

    /// Read a skill's full content
    pub fn read(&self, name: &str) -> Option<String> {
        self.get(name)
            .and_then(|skill| std::fs::read_to_string(&skill.path).ok())
    }
}

// ========== Helper Functions ==========

fn find_skill_file(dir: &Path) -> Option<PathBuf> {
    // Try SKILL.md first, then skill.md
    let candidates = ["SKILL.md", "skill.md"];
    for name in &candidates {
        let path = dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn parse_skill_file(path: &Path) -> Option<SkillMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    let frontmatter = extract_frontmatter(&content)?;

    let fm: Frontmatter = serde_yaml::from_str(&frontmatter).ok()?;

    let name = fm.name.or_else(|| {
        // Derive name from directory or file name
        path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
    })?;

    Some(SkillMeta {
        name,
        description: fm.description,
        aka: fm.aka,
        allowed_tools: fm.allowed_tools,
        path: path.to_path_buf(),
    })
}

fn extract_frontmatter(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    let after_first = &trimmed[3..];
    let end = after_first.find("---")?;
    Some(after_first[..end].to_string())
}

fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }
    let after_first = &trimmed[3..];
    if let Some(end) = after_first.find("---") {
        after_first[end + 3..].to_string()
    } else {
        content.to_string()
    }
}

fn register_lookup(lookup: &mut HashMap<String, usize>, meta: &SkillMeta, idx: usize) {
    lookup.insert(meta.name.to_lowercase(), idx);
    if let Some(ref akas) = meta.aka {
        for alias in akas {
            lookup.insert(alias.to_lowercase(), idx);
        }
    }
}

/// Extract key sections from skill body (路径, 服务端口, INDEX tables)
fn extract_key_sections(body: &str) -> String {
    let mut result = String::new();
    let mut in_section = false;
    let mut section_name = String::new();
    let key_sections = ["路径", "path", "服务端口", "port", "index", "连接", "config", "配置"];

    for line in body.lines() {
        if line.starts_with("# ") || line.starts_with("## ") {
            let heading = line.trim_start_matches('#').trim().to_lowercase();
            in_section = key_sections.iter().any(|&s| heading.contains(s));
            if in_section {
                section_name = line.to_string();
                result.push_str(line);
                result.push('\n');
            }
        } else if in_section {
            // Stop at next heading of same or higher level
            if line.starts_with("# ") || (line.starts_with("---") && !section_name.is_empty()) {
                in_section = false;
            } else {
                result.push_str(line);
                result.push('\n');
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frontmatter() {
        let content = "---\nname: test\ndescription: A test\n---\n# Body";
        let fm = extract_frontmatter(content).unwrap();
        assert!(fm.contains("name: test"));
    }

    #[test]
    fn test_strip_frontmatter() {
        let content = "---\nname: test\n---\n# Body\nContent";
        let body = strip_frontmatter(content);
        assert!(body.contains("# Body"));
        assert!(!body.contains("name: test"));
    }

    #[test]
    fn test_parse_frontmatter_with_aka() {
        let yaml = "name: backend-deploy\ndescription: 后端部署\naka: [部署, deploy]\nallowed-tools: Bash";
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(fm.name.unwrap(), "backend-deploy");
        assert_eq!(fm.aka.unwrap().len(), 2);
    }
}
