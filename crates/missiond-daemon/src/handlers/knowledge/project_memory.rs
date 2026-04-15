use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MemoryEntry {
    pub file: String,
    pub name: String,
    pub mem_type: String,
    pub description: String,
}

/// Compute the Claude Code memory directory for a given project path.
/// `/Users/jinchen/Projects/missiond` → `~/.claude/projects/-Users-jinchen-Projects-missiond/memory`
pub fn claude_memory_dir(project_path: &str) -> PathBuf {
    let encoded = project_path.replace('/', "-");
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude/projects")
        .join(&encoded)
        .join("memory")
}

/// List all memory entries by scanning *.md files and parsing YAML frontmatter.
/// Excludes MEMORY.md (the index file).
pub fn list_memories(project_path: &str) -> Vec<MemoryEntry> {
    let dir = claude_memory_dir(project_path);
    let entries = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut results: Vec<MemoryEntry> = Vec::new();

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip non-md files and the index file
        if !file_name.ends_with(".md") || file_name == "MEMORY.md" {
            continue;
        }

        // Read first ~30 lines to find frontmatter
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (name, mem_type, description) = parse_frontmatter(&content);

        results.push(MemoryEntry {
            file: file_name,
            name,
            mem_type,
            description,
        });
    }

    results.sort_by(|a, b| a.file.cmp(&b.file));
    results
}

/// Parse YAML frontmatter between `---` delimiters.
/// Extracts `name`, `type`, and `description` fields by line scanning.
fn parse_frontmatter(content: &str) -> (String, String, String) {
    let mut name = String::new();
    let mut mem_type = String::new();
    let mut description = String::new();

    let mut in_frontmatter = false;
    let mut found_open = false;

    for (i, line) in content.lines().enumerate() {
        if i > 30 {
            break;
        }

        let trimmed = line.trim();
        if trimmed == "---" {
            if !found_open {
                found_open = true;
                in_frontmatter = true;
                continue;
            } else {
                // Closing delimiter
                break;
            }
        }

        if !in_frontmatter {
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("name:") {
            name = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("type:") {
            mem_type = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("description:") {
            description = val.trim().to_string();
        }
    }

    (name, mem_type, description)
}

/// Read a single memory file's full content.
/// `file` is just the filename like "feedback_no_wait.md".
/// Returns Err if file doesn't exist or contains path traversal.
pub fn read_memory(project_path: &str, file: &str) -> Result<String, std::io::Error> {
    // Path traversal protection
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "filename must not contain path separators or '..'",
        ));
    }

    let path = claude_memory_dir(project_path).join(file);
    std::fs::read_to_string(path)
}

/// Read the MEMORY.md index file content.
pub fn read_memory_index(project_path: &str) -> Option<String> {
    let path = claude_memory_dir(project_path).join("MEMORY.md");
    std::fs::read_to_string(path).ok()
}
