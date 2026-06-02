use anyhow::{Result, anyhow};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::context::v3_blueprint_runtime::{
    CompiledProjectUniverseEntry, CompiledServiceRuntimeEntry, load_compiled_project_universe,
};
use crate::state::AppState;

/// Common intent.lisp locations to scan when intent_path is not set in DB.
const INTENT_CANDIDATES: &[&str] = &[
    ".missiond/intent.lisp",
    ".jarvis/intent.lisp",
    "intent.lisp",
];

/// Sections always included in full for the summary action.
const SUMMARY_SECTIONS: &[&str] = &["design-constraints"];

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("summary");

    match action {
        "list" => handle_list(state).await,
        "read" | "section" | "summary" => {
            let project_id = args.get("project").and_then(|v| v.as_str());
            let section_name = args.get("section").and_then(|v| v.as_str());
            handle_read(state, action, project_id, section_name).await
        }
        "paths" => {
            let project_id = args.get("project").and_then(|v| v.as_str());
            handle_paths(state, project_id).await
        }
        _ => Ok(ToolResult::error(format!(
            "Unknown action: {}. Use: read, section, summary, list, paths",
            action
        ))),
    }
}

async fn handle_list(state: &AppState) -> Result<ToolResult> {
    let projects = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let items: Vec<serde_json::Value> = projects
        .iter()
        .map(|p| {
            let intent_file = resolve_intent_path(&p.path, p.intent_path.as_deref());
            let (exists, lines) = match &intent_file {
                Some(f) => {
                    let content = std::fs::read_to_string(f).unwrap_or_default();
                    (true, content.lines().count())
                }
                None => (false, 0),
            };
            serde_json::json!({
                "id": p.id,
                "path": p.path,
                "intentPath": intent_file.as_ref().map(|f| f.display().to_string()),
                "exists": exists,
                "lines": lines,
                "active": p.active,
            })
        })
        .collect();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "projects": items,
        "hint": "Use mission_intent(project=\"<id>\") to read a project's intent.lisp"
    })))
}

async fn handle_read(
    state: &AppState,
    action: &str,
    project_id: Option<&str>,
    section_name: Option<&str>,
) -> Result<ToolResult> {
    // 1. Resolve project
    let (resolved_id, project_path, intent_path) = resolve_project(state, project_id).await?;

    // 2. Read file
    let content = std::fs::read_to_string(&intent_path).map_err(|e| {
        anyhow!(
            "Failed to read {}: {}. Run mission_project(action=\"init\", path=\"{}\") to register.",
            intent_path.display(),
            e,
            project_path
        )
    })?;

    let total_lines = content.lines().count();

    match action {
        "read" => {
            let base = std::path::Path::new(&project_path);
            let has_split_files = has_intent_pillar_files(base);

            if has_split_files || content.len() > 30_000 {
                // Multi-file intent or single large file — return paths for parallel Read
                return handle_paths(state, Some(&resolved_id)).await;
            }
            Ok(ToolResult::text(format!(
                "# intent.lisp — {} ({} lines)\n# path: {}\n\n{}",
                resolved_id,
                total_lines,
                intent_path.display(),
                content
            )))
        }
        "section" => {
            let name = section_name
                .ok_or_else(|| anyhow!("section parameter is required for action=section"))?;
            match extract_section(&content, name) {
                Some(block) => Ok(ToolResult::text(format!(
                    "# intent.lisp — {} :: ({})\n\n{}",
                    resolved_id, name, block
                ))),
                None => {
                    let available = list_top_level_sections(&content);
                    Ok(ToolResult::error(format!(
                        "Section '{}' not found. Available sections: {}",
                        name,
                        available.join(", ")
                    )))
                }
            }
        }
        "summary" => {
            let mut parts = Vec::new();
            // Always include the header (first ~15 lines before first section)
            let header = extract_header(&content);
            if !header.is_empty() {
                parts.push(header);
            }
            // Extract core sections in full
            for sec in SUMMARY_SECTIONS {
                if let Some(block) = extract_section(&content, sec) {
                    parts.push(block);
                }
            }
            // Extract pillar index: just the pillar names + first-line descriptions
            let pillar_index = extract_pillar_index(&content);
            if !pillar_index.is_empty() {
                parts.push(format!(
                    ";; PILLAR INDEX ({} pillars)\n{}",
                    pillar_index
                        .lines()
                        .filter(|l| l.starts_with("  (pillar"))
                        .count(),
                    pillar_index
                ));
            }
            let available = list_top_level_sections(&content);
            let summary = parts.join("\n\n");
            let summary_lines = summary.lines().count();
            Ok(ToolResult::text(format!(
                "# intent.lisp — {} (summary: {} lines / total: {} lines)\n\
                 # path: {}\n\
                 # all sections: {}\n\
                 # Use action=\"read\" for full content or action=\"section\" for specific blocks\n\n{}",
                resolved_id,
                summary_lines,
                total_lines,
                intent_path.display(),
                available.join(", "),
                summary
            )))
        }
        _ => unreachable!(),
    }
}

/// Return all intent*.lisp file paths for a project, for parallel loading by Claude Code.
async fn handle_paths(state: &AppState, project_id: Option<&str>) -> Result<ToolResult> {
    let (resolved_id, project_path, _main_intent) = resolve_project(state, project_id).await?;

    let base = Path::new(&project_path);

    // Scan .missiond/ directory for all intent*.lisp files
    let mut files: Vec<serde_json::Value> = Vec::new();

    // Check .missiond/ directory
    let missiond_dir = base.join(".missiond");
    if missiond_dir.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&missiond_dir) {
            for entry in rd.filter_map(|e| e.ok()) {
                let p = entry.path();
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if name.starts_with("intent") && name.ends_with(".lisp") {
                    let lines = std::fs::read_to_string(&p)
                        .map(|c| c.lines().count())
                        .unwrap_or(0);
                    files.push(serde_json::json!({
                        "path": p.display().to_string(),
                        "name": name,
                        "lines": lines,
                    }));
                }
            }
        }
    }

    // Also check .jarvis/ directory
    let jarvis_dir = base.join(".jarvis");
    if jarvis_dir.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&jarvis_dir) {
            for entry in rd.filter_map(|e| e.ok()) {
                let p = entry.path();
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if name.starts_with("intent") && name.ends_with(".lisp") {
                    let lines = std::fs::read_to_string(&p)
                        .map(|c| c.lines().count())
                        .unwrap_or(0);
                    files.push(serde_json::json!({
                        "path": p.display().to_string(),
                        "name": name,
                        "lines": lines,
                    }));
                }
            }
        }
    }

    // Check root intent.lisp
    let root_intent = base.join("intent.lisp");
    if root_intent.exists() {
        let lines = std::fs::read_to_string(&root_intent)
            .map(|c| c.lines().count())
            .unwrap_or(0);
        files.push(serde_json::json!({
            "path": root_intent.display().to_string(),
            "name": "intent.lisp",
            "lines": lines,
        }));
    }

    // Sort by name for deterministic output
    files.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });

    let total_lines: usize = files
        .iter()
        .map(|f| f["lines"].as_u64().unwrap_or(0) as usize)
        .sum();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "project": resolved_id,
        "base_path": project_path,
        "files": files,
        "total_files": files.len(),
        "total_lines": total_lines,
        "hint": "Use Read tool to load each file path in parallel for full context injection"
    })))
}

/// Resolve project ID → (id, project_path, intent_file_path)
async fn resolve_project(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<(String, String, PathBuf)> {
    let id = match project_id {
        Some(id) => id.to_string(),
        None => {
            // Infer from CWD
            let cwd = std::env::current_dir().map_err(|e| anyhow!("Cannot get CWD: {}", e))?;
            let cwd_str = cwd.to_str().unwrap_or("");
            let reg = state.project_registry.read().await;
            match reg.resolve(cwd_str) {
                Some(id) => id.to_string(),
                None => {
                    // Fallback: derive from CWD directory name
                    cwd.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase()
                }
            }
        }
    };

    // Look up in DB — try exact match first, then fuzzy fallback
    let project = match state
        .store
        .get_project(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => (id.clone(), p),
        None => {
            if let Some(compiled) = resolve_compiled_project(&id) {
                return compiled;
            }
            fuzzy_resolve_project(state, &id).await?
        }
    };

    let (resolved_id, p) = project;
    let intent_file = resolve_intent_path(&p.path, p.intent_path.as_deref()).ok_or_else(|| {
        anyhow!(
            "No intent.lisp found for project '{}' at {}. Checked: {}",
            resolved_id,
            p.path,
            INTENT_CANDIDATES.join(", ")
        )
    })?;
    Ok((resolved_id, p.path, intent_file))
}

/// Fuzzy fallback: find project whose ID contains the query (case-insensitive).
/// Returns Ok if exactly one match, Err with candidate list otherwise.
async fn fuzzy_resolve_project(
    state: &AppState,
    query: &str,
) -> Result<(String, missiond_core::types::ProjectConfig)> {
    let all = state
        .store
        .list_projects()
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    let q = query.to_lowercase();
    let matches: Vec<_> = all
        .into_iter()
        .filter(|p| p.id.to_lowercase().contains(&q))
        .collect();

    match matches.len() {
        0 => Err(anyhow!(
            "Project '{}' not found in DB. Run mission_intent(action=\"list\") to see available projects, \
             or mission_project(action=\"init\", path=\"/path/to/project\") to register.",
            query
        )),
        1 => {
            let p = matches.into_iter().next().unwrap();
            Ok((p.id.clone(), p))
        }
        _ => {
            let ids: Vec<String> = matches.iter().map(|p| p.id.clone()).collect();
            Err(anyhow!(
                "Project '{}' is ambiguous — matches {} projects: [{}]. Specify exact id.",
                query,
                ids.len(),
                ids.join(", ")
            ))
        }
    }
}

fn resolve_compiled_project(query: &str) -> Option<Result<(String, String, PathBuf)>> {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let loaded = load_compiled_project_universe(&project_root, None);
    let payload = loaded.payload?;
    let normalized = normalize_lookup_key(query);

    for project in &payload.projects {
        if compiled_project_matches(project, &normalized) {
            return Some(resolve_compiled_project_entry(project));
        }
    }
    for service in &payload.services {
        if compiled_service_matches(service, &normalized) {
            return Some(resolve_compiled_service_entry(service));
        }
    }
    None
}

fn resolve_compiled_project_entry(
    project: &CompiledProjectUniverseEntry,
) -> Result<(String, String, PathBuf)> {
    let resolved_id = project
        .id
        .clone()
        .ok_or_else(|| anyhow!("Compiled project has no id"))?;
    let project_path = project
        .root
        .as_deref()
        .or(project.path.as_deref())
        .ok_or_else(|| anyhow!("Compiled project '{}' has no root/path", resolved_id))?;
    let intent_file =
        resolve_intent_path(project_path, project.intent.as_deref()).ok_or_else(|| {
            anyhow!(
                "No intent.lisp found for compiled project '{}' at {}. Checked: {}",
                resolved_id,
                project_path,
                INTENT_CANDIDATES.join(", ")
            )
        })?;
    Ok((resolved_id, project_path.to_string(), intent_file))
}

fn resolve_compiled_service_entry(
    service: &CompiledServiceRuntimeEntry,
) -> Result<(String, String, PathBuf)> {
    let resolved_id = service
        .project
        .clone()
        .or_else(|| service.id.clone())
        .ok_or_else(|| anyhow!("Compiled service has no id/project"))?;
    let project_path = service
        .root
        .as_deref()
        .ok_or_else(|| anyhow!("Compiled service '{}' has no root", resolved_id))?;
    let intent_file =
        resolve_intent_path(project_path, service.intent.as_deref()).ok_or_else(|| {
            anyhow!(
                "No intent.lisp found for compiled service '{}' at {}. Checked: {}",
                resolved_id,
                project_path,
                INTENT_CANDIDATES.join(", ")
            )
        })?;
    Ok((resolved_id, project_path.to_string(), intent_file))
}

fn compiled_project_matches(project: &CompiledProjectUniverseEntry, normalized: &str) -> bool {
    project.id.as_deref().map(normalize_lookup_key).as_deref() == Some(normalized)
        || project
            .aliases
            .iter()
            .any(|alias| normalize_lookup_key(alias) == normalized)
        || project
            .service_ids
            .iter()
            .any(|service_id| normalize_lookup_key(service_id) == normalized)
}

fn compiled_service_matches(service: &CompiledServiceRuntimeEntry, normalized: &str) -> bool {
    service.id.as_deref().map(normalize_lookup_key).as_deref() == Some(normalized)
        || service
            .project
            .as_deref()
            .map(normalize_lookup_key)
            .as_deref()
            == Some(normalized)
}

fn normalize_lookup_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

/// Resolve the intent.lisp file path for a project.
fn resolve_intent_path(project_path: &str, db_intent_path: Option<&str>) -> Option<PathBuf> {
    let base = Path::new(project_path);

    // 1. Try DB-stored intent_path first
    if let Some(rel) = db_intent_path {
        let full = base.join(rel);
        if full.exists() {
            return Some(full);
        }
    }

    // 2. Scan common locations
    for candidate in INTENT_CANDIDATES {
        let full = base.join(candidate);
        if full.exists() {
            return Some(full);
        }
    }

    None
}

/// Extract the header portion (before first top-level section, typically the intent form opening).
fn extract_header(content: &str) -> String {
    let mut lines = Vec::new();
    for line in content.lines() {
        // Stop at the first section comment block (;; ==== pattern)
        // but include the opening (intent ...) and survey metadata
        if lines.len() > 2 && line.trim_start().starts_with(";; ==") {
            break;
        }
        lines.push(line);
        // Cap header at 20 lines
        if lines.len() >= 20 {
            break;
        }
    }
    lines.join("\n")
}

/// Extract a named top-level S-EXP section using paren-counting.
/// Looks for `  (section-name` at indentation level 2+ (inside the top-level intent form).
fn extract_section(content: &str, section_name: &str) -> Option<String> {
    let pattern = format!("({}", section_name);
    let lines: Vec<&str> = content.lines().collect();
    let mut start = None;

    // Find the start line
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(&pattern)
            && (trimmed.len() == pattern.len()
                || trimmed
                    .as_bytes()
                    .get(pattern.len())
                    .map(|b| b.is_ascii_whitespace() || *b == b'\n' || *b == b')')
                    == Some(true))
        {
            start = Some(i);
            break;
        }
    }

    let start = start?;

    // Count parens to find the matching close
    let mut depth: i32 = 0;
    let mut end = start;

    for (i, line) in lines[start..].iter().enumerate() {
        for ch in line.chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        end = start + i;
        if depth <= 0 {
            break;
        }
    }

    Some(lines[start..=end].join("\n"))
}

/// Extract a compact pillar index: "(pillar NAME" + the module-catalog/data-model child names.
/// Returns one line per pillar with its first comment or child name list.
fn extract_pillar_index(content: &str) -> String {
    let mut index = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("(pillar ") {
            // Extract pillar name
            let pillar_line = trimmed;
            // Collect the module-catalog entries or first few child names
            let mut children = Vec::new();
            let mut depth: i32 = 0;
            for ch in trimmed.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            let mut j = i + 1;
            while j < lines.len() && depth > 0 {
                let line = lines[j].trim();
                for ch in line.chars() {
                    match ch {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
                // Capture module-catalog entries or top-level child form names
                if line.starts_with("(module-catalog") || line.starts_with("(data-model") {
                    // Collect names inside
                    let mut k = j + 1;
                    let mut inner_depth: i32 = 0;
                    for ch in line.chars() {
                        match ch {
                            '(' => inner_depth += 1,
                            ')' => inner_depth -= 1,
                            _ => {}
                        }
                    }
                    while k < lines.len() && inner_depth > 0 {
                        let inner = lines[k].trim();
                        for ch in inner.chars() {
                            match ch {
                                '(' => inner_depth += 1,
                                ')' => inner_depth -= 1,
                                _ => {}
                            }
                        }
                        // Extract module/table names
                        if inner.starts_with("(mod ")
                            || inner.starts_with("(table ")
                            || inner.starts_with("(view ")
                        {
                            if let Some(end) = inner[1..].find(|c: char| c.is_whitespace()) {
                                let after_keyword = &inner[1 + end..].trim_start();
                                if let Some(name_end) = after_keyword
                                    .find(|c: char| c.is_whitespace() || c == ')' || c == '"')
                                {
                                    children.push(after_keyword[..name_end].to_string());
                                }
                            }
                        }
                        k += 1;
                    }
                }
                j += 1;
            }
            if children.is_empty() {
                index.push(format!("  {}", pillar_line));
            } else {
                index.push(format!("  {} → [{}]", pillar_line, children.join(", ")));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    index.join("\n")
}

/// Check if a project has split intent-pillar-*.lisp files.
fn has_intent_pillar_files(project_root: &std::path::Path) -> bool {
    for dir_name in &[".missiond", ".jarvis"] {
        let dir = project_root.join(dir_name);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("intent-pillar-") && name.ends_with(".lisp") {
                    return true;
                }
            }
        }
    }
    false
}

/// List all top-level section names (forms at indent level 2+ inside the intent form).
fn list_top_level_sections(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Look for forms that start with ( at indentation >= 2 spaces
        if line.starts_with("  (") || line.starts_with("\t(") {
            // Extract the form name
            if let Some(name_end) =
                trimmed[1..].find(|c: char| c.is_whitespace() || c == ')' || c == '\n')
            {
                let name = &trimmed[1..1 + name_end];
                // Filter out comment-only lines and very short names
                if name.len() > 1 && !name.starts_with(';') && !sections.contains(&name.to_string())
                {
                    sections.push(name.to_string());
                }
            }
        }
    }
    sections
}
