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
    /// Phase 2: dependency declarations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<SkillRequires>,
    /// Phase 3: executable action declarations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<SkillAction>>,
    /// Phase 4: pre-flight context hooks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_hooks: Option<Vec<ContextHook>>,
}

/// Skill dependency declaration (Phase 2: cross-domain context aggregation)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillRequires {
    /// Prerequisite skill names (resolved recursively, max 2 layers)
    #[serde(default)]
    pub skills: Vec<String>,
    /// Infrastructure server IDs (looked up from servers.yaml)
    #[serde(default)]
    pub infra: Vec<String>,
    /// KB category prefixes (e.g., "memory:ops") for filtered search
    #[serde(default)]
    pub kb: Vec<String>,
}

/// Action declaration in frontmatter (Phase 3: executable skills)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAction {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub requires_approval: bool,
}

/// Context hook — pre-flight probe executed before workflow (Phase 4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextHook {
    pub tool: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub save_as: String,
}

/// A single step in a workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Step identifier for fallback references
    #[serde(default)]
    pub id: Option<String>,
    pub name: Option<String>,
    pub tool: String,
    #[serde(default)]
    pub params: serde_json::Value,
    /// Error handling: "stop" (default) | "skip" | "retry" | "fallback:step_id"
    #[serde(default = "default_on_error")]
    pub on_error: String,
    #[serde(default)]
    pub save_as: Option<String>,
    /// Max retry attempts when on_error = "retry" (exponential backoff 1s, 2s, 4s...)
    #[serde(default)]
    pub max_retries: u32,
}

fn default_on_error() -> String { "stop".to_string() }

/// Parsed workflow block from ```workflow code fence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBlock {
    pub id: String,
    #[serde(default = "default_workflow_type")]
    pub r#type: String,
    pub steps: Vec<WorkflowStep>,
}

fn default_workflow_type() -> String { "sequential".to_string() }

/// Workflow execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum WorkflowResult {
    /// Dry-run preview of steps
    #[serde(rename = "preview")]
    Preview { steps: Vec<WorkflowStepPreview> },
    /// Execution completed successfully
    #[serde(rename = "success")]
    Success { steps_completed: usize, results: Vec<WorkflowStepResult> },
    /// Execution failed at a step
    #[serde(rename = "failed")]
    Failed { steps_completed: usize, error_step: usize, error: String, results: Vec<WorkflowStepResult> },
    /// Waiting for user approval
    #[serde(rename = "pending_approval")]
    PendingApproval { action_id: String, skill: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepPreview {
    pub name: Option<String>,
    pub tool: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepResult {
    pub name: Option<String>,
    pub tool: String,
    pub success: bool,
    pub output: String,
}

/// Resolve `${var}` references in a JSON value using a context map
pub fn resolve_vars(value: &serde_json::Value, context: &std::collections::HashMap<String, String>) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            let mut result = s.clone();
            for (key, val) in context {
                result = result.replace(&format!("${{{}}}", key), val);
            }
            serde_json::Value::String(result)
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), resolve_vars(v, context)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| resolve_vars(v, context)).collect())
        }
        other => other.clone(),
    }
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
    /// Phase 2: dependency declarations
    #[serde(default)]
    requires: Option<SkillRequires>,
    /// Phase 3: executable action declarations
    #[serde(default)]
    actions: Option<Vec<SkillAction>>,
    /// Phase 4: pre-flight context hooks
    #[serde(default, rename = "context_hooks")]
    context_hooks: Option<Vec<ContextHook>>,
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
        requires: fm.requires,
        actions: fm.actions,
        context_hooks: fm.context_hooks,
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

/// Parse workflow blocks from skill content body.
/// Extracts ```workflow code fences and parses YAML content into WorkflowBlock structs.
pub fn parse_workflow_blocks(content: &str) -> Vec<WorkflowBlock> {
    let mut blocks = Vec::new();
    let mut in_workflow = false;
    let mut header_line = String::new();
    let mut yaml_buf = String::new();

    for line in content.lines() {
        if !in_workflow {
            let trimmed = line.trim();
            if trimmed.starts_with("```workflow") {
                in_workflow = true;
                // Extract header attributes: ```workflow id="deploy_backend" type="sequential"
                header_line = trimmed["```workflow".len()..].trim().to_string();
                yaml_buf.clear();
            }
        } else if line.trim() == "```" {
            // End of workflow block — parse it
            in_workflow = false;
            if let Some(block) = parse_single_workflow(&header_line, &yaml_buf) {
                blocks.push(block);
            } else {
                warn!(header = %header_line, "Failed to parse workflow block");
            }
        } else {
            yaml_buf.push_str(line);
            yaml_buf.push('\n');
        }
    }

    blocks
}

/// Parse a single workflow block from header attributes + YAML body
fn parse_single_workflow(header: &str, yaml_body: &str) -> Option<WorkflowBlock> {
    // Parse header key="value" pairs
    let id = extract_attr(header, "id")?;
    let workflow_type = extract_attr(header, "type").unwrap_or_else(|| "sequential".to_string());

    // Parse YAML body for steps
    #[derive(Deserialize)]
    struct WorkflowYaml {
        #[serde(default)]
        steps: Vec<WorkflowStep>,
    }

    let parsed: WorkflowYaml = serde_yaml::from_str(yaml_body).ok()?;

    if parsed.steps.is_empty() || parsed.steps.len() > 10 {
        warn!(id = %id, steps = parsed.steps.len(), "Workflow must have 1-10 steps");
        return None;
    }

    Some(WorkflowBlock {
        id,
        r#type: workflow_type,
        steps: parsed.steps,
    })
}

/// Extract key="value" from header string
fn extract_attr(header: &str, key: &str) -> Option<String> {
    let pattern = format!("{}=\"", key);
    let start = header.find(&pattern)? + pattern.len();
    let rest = &header[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
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

// ========== Skill Engine: Ingest + Materialize ==========

/// Parsed section from a SKILL.md file
#[derive(Debug)]
pub struct ParsedSection {
    pub title: String,
    pub content: String,
    pub sort_order: i32,
}

/// Parse a skill file body into sections by `#` headings
pub fn parse_sections(body: &str) -> Vec<ParsedSection> {
    let mut sections = Vec::new();
    let mut current_title = String::new();
    let mut current_content = String::new();
    let mut sort_order = 0i32;

    for line in body.lines() {
        // Detect heading (# or ##, but not ### which is subsection)
        if (line.starts_with("# ") || line.starts_with("## ")) && !line.starts_with("### ") {
            // Save previous section if exists
            if !current_title.is_empty() {
                sections.push(ParsedSection {
                    title: current_title.clone(),
                    content: current_content.trim().to_string(),
                    sort_order,
                });
                sort_order += 1;
            }
            current_title = line.to_string();
            current_content = String::new();
        } else if line == "---" && !current_title.is_empty() {
            // Horizontal rule as section separator - flush current
            sections.push(ParsedSection {
                title: current_title.clone(),
                content: current_content.trim().to_string(),
                sort_order,
            });
            sort_order += 1;
            current_title = String::new();
            current_content = String::new();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Don't forget the last section
    if !current_title.is_empty() {
        sections.push(ParsedSection {
            title: current_title,
            content: current_content.trim().to_string(),
            sort_order,
        });
    }

    sections
}

/// Ingest all SKILL.md files into the database.
/// Scans ~/.claude/skills/, parses frontmatter + sections, writes to skill_topics + skill_blocks.
pub fn ingest_skills(db: &crate::db::MissionDB, skills_dir: &Path) -> usize {
    let index = SkillIndex::build(skills_dir);
    let skills = index.list();
    let mut ingested = 0;

    for skill in skills {
        let content = match std::fs::read_to_string(&skill.path) {
            Ok(c) => c,
            Err(e) => {
                warn!(path = %skill.path.display(), error = %e, "Failed to read skill file");
                continue;
            }
        };

        let aka_json = skill
            .aka
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());

        let requires_json = skill
            .requires
            .as_ref()
            .and_then(|r| serde_json::to_string(r).ok());

        let actions_json = skill
            .actions
            .as_ref()
            .and_then(|a| serde_json::to_string(a).ok());

        let context_hooks_json = skill
            .context_hooks
            .as_ref()
            .and_then(|h| serde_json::to_string(h).ok());

        let file_path_str = skill.path.to_string_lossy().to_string();

        // Upsert topic
        if let Err(e) = db.skill_topic_upsert_full(
            &skill.name,
            skill.description.as_deref(),
            aka_json.as_deref(),
            skill.allowed_tools.as_deref(),
            &file_path_str,
            requires_json.as_deref(),
            actions_json.as_deref(),
            context_hooks_json.as_deref(),
        ) {
            warn!(topic = %skill.name, error = %e, "Failed to upsert skill topic");
            continue;
        }

        // Delete existing blocks for this topic (full re-ingest)
        if let Err(e) = db.skill_blocks_delete_topic(&skill.name) {
            warn!(topic = %skill.name, error = %e, "Failed to clear old blocks");
            continue;
        }

        // Parse body into sections
        let body = strip_frontmatter(&content);
        let sections = parse_sections(&body);

        for section in &sections {
            if section.content.is_empty() && section.title.trim_start_matches('#').trim().is_empty()
            {
                continue;
            }
            if let Err(e) = db.skill_block_insert(
                &skill.name,
                "section",
                Some(&section.title),
                &section.content,
                section.sort_order,
            ) {
                warn!(
                    topic = %skill.name,
                    section = %section.title,
                    error = %e,
                    "Failed to insert skill block"
                );
            }
        }

        // Update line count
        let total_lines = content.lines().count() as i32;
        let checksum = format!("{:x}", md5_hash(content.as_bytes()));
        let _ = db.skill_topic_update_stats(&skill.name, total_lines, &checksum);

        debug!(
            topic = %skill.name,
            sections = sections.len(),
            lines = total_lines,
            "Ingested skill"
        );
        ingested += 1;
    }

    // Rebuild FTS after bulk ingest
    if let Err(e) = db.skill_rebuild_fts() {
        warn!(error = %e, "Failed to rebuild skill FTS after ingest");
    }

    tracing::info!(count = ingested, "Skill ingest complete");
    ingested
}

/// Simple hash for checksum (no crypto needed)
fn md5_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Materialize a single topic from DB to SKILL.md
pub fn materialize_topic(db: &crate::db::MissionDB, topic: &str) -> Result<String, String> {
    let topic_meta = db
        .skill_topic_get(topic)
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| format!("Topic not found: {}", topic))?;

    let blocks = db
        .skill_blocks_for_topic(topic)
        .map_err(|e| format!("DB error: {}", e))?;

    // Build frontmatter
    let mut output = String::from("---\n");
    output.push_str(&format!("name: {}\n", topic));
    if let Some(ref desc) = topic_meta.description {
        output.push_str(&format!("description: {}\n", desc));
    }
    if let Some(ref tools) = topic_meta.allowed_tools {
        output.push_str(&format!("allowed-tools: {}\n", tools));
    }
    if let Some(ref aka) = topic_meta.aka {
        // aka is stored as JSON array string
        if let Ok(aliases) = serde_json::from_str::<Vec<String>>(aka) {
            if !aliases.is_empty() {
                output.push_str(&format!(
                    "aka: [{}]\n",
                    aliases.join(", ")
                ));
            }
        }
    }
    output.push_str("---\n\n");

    // Separate sections and fragments
    let sections: Vec<_> = blocks.iter().filter(|b| b.block_type == "section").collect();
    let fragments: Vec<_> = blocks.iter().filter(|b| b.block_type == "fragment").collect();

    // Write sections
    for block in &sections {
        if let Some(ref title) = block.title {
            output.push_str(title);
            output.push('\n');
        }
        if !block.content.is_empty() {
            output.push_str(&block.content);
            output.push_str("\n\n");
        }
        output.push_str("---\n\n");
    }

    // Write fragments as appendix
    if !fragments.is_empty() {
        output.push_str("## 待整理碎片\n\n");
        for block in &fragments {
            output.push_str(&format!("- {}\n", block.content.replace('\n', " ")));
        }
    }

    // Snapshot current content before overwriting (Phase 4: rollback)
    // Also detect external modifications (split-brain prevention)
    let file_path = &topic_meta.file_path;
    if let Ok(existing) = std::fs::read_to_string(file_path) {
        if !existing.is_empty() {
            let old_checksum = format!("{:x}", md5_hash(existing.as_bytes()));

            // Split-brain check: if file was modified externally since last ingest,
            // refuse to overwrite and require explicit ingest first
            if let Some(ref db_checksum) = topic_meta.checksum {
                if *db_checksum != old_checksum {
                    return Err(format!(
                        "Conflict: '{}' was modified externally (file checksum {} != DB checksum {}). \
                         Run ingest first to sync, or use mission_skill_rollback to restore.",
                        topic, &old_checksum[..8.min(old_checksum.len())],
                        &db_checksum[..8.min(db_checksum.len())]
                    ));
                }
            }

            let _ = db.skill_version_save(topic, &existing, &old_checksum);
        }
    }

    // Write to file
    if let Some(parent) = std::path::Path::new(file_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(file_path, &output).map_err(|e| format!("Write error: {}", e))?;

    // Update stats
    let total_lines = output.lines().count() as i32;
    let checksum = format!("{:x}", md5_hash(output.as_bytes()));
    let _ = db.skill_topic_update_stats(topic, total_lines, &checksum);

    Ok(output)
}

/// Materialize all topics from DB to SKILL.md files
pub fn materialize_all(db: &crate::db::MissionDB) -> Result<usize, String> {
    let topics = db
        .skill_topic_list()
        .map_err(|e| format!("DB error: {}", e))?;

    let mut count = 0;
    for topic in &topics {
        match materialize_topic(db, &topic.topic) {
            Ok(_) => count += 1,
            Err(e) => warn!(topic = %topic.topic, error = %e, "Failed to materialize"),
        }
    }
    tracing::info!(count, "Materialized all skills");
    Ok(count)
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
