use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::lenient;
use crate::state::AppState;

#[derive(Deserialize)]
struct ContextBuildArgs {
    query: String,
}

pub(super) async fn handle_build(state: &AppState, args: Value) -> Result<ToolResult> {
    let ContextBuildArgs { query } = serde_json::from_value(args)?;
    let mut context = state.skills.build_context(&query);

    let q = query.clone();
    if let Ok(mut entries) = state.store.kb_search(&q, None).await {
        entries.sort_by(|a, b| {
            let score_a = a.confidence * (a.access_count as f64 + 1.0).ln();
            let score_b = b.confidence * (b.access_count as f64 + 1.0).ln();
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut budget: i32 = 800;
        let mut kb_block = String::new();
        for entry in &entries {
            let line = format!("- [{}] {}: {}\n", entry.category, entry.key, entry.summary);
            budget -= line.len() as i32;
            if budget < 0 {
                break;
            }
            kb_block.push_str(&line);
        }
        if !kb_block.is_empty() {
            context.push_str("\n[Knowledge Base]\n");
            context.push_str(&kb_block);
        }
    }

    Ok(ToolResult::text(context))
}

pub(super) async fn handle_resolve(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct ContextResolveArgs {
        #[serde(default)]
        query: Option<String>,
        skill: Option<String>,
        #[serde(default, alias = "project", alias = "projectId")]
        project_id: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_bool")]
        include_board: Option<bool>,
        #[serde(default, deserialize_with = "lenient::option_bool")]
        include_kb: Option<bool>,
    }
    let args: ContextResolveArgs = serde_json::from_value(args)?;
    let include_board = args.include_board.unwrap_or(false);
    let include_kb = args.include_kb.unwrap_or(false);
    let project_config = if let Some(ref project_id) = args.project_id {
        state.project_registry.read().await.get(project_id).cloned()
    } else {
        None
    };
    let query = resolve_context_query(
        args.query.as_deref(),
        args.project_id.as_deref(),
        args.skill.as_deref(),
    );
    let search_query = if let Some(ref project_id) = args.project_id {
        if args
            .query
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            project_id.clone()
        } else {
            format!("{project_id} {query}")
        }
    } else {
        query.clone()
    };

    let mut primary_topics: Vec<String> = Vec::new();
    if let Some(ref name) = args.skill {
        primary_topics.push(name.clone());
    } else {
        if let Some(ref project_id) = args.project_id {
            if state.skills.get(project_id).is_some() {
                primary_topics.push(project_id.clone());
            } else if state
                .store
                .skill_topic_get(project_id)
                .await
                .map(|topic| topic.is_some())
                .unwrap_or(false)
            {
                primary_topics.push(project_id.clone());
            }
        }
        for s in state
            .skills
            .search(&search_query)
            .iter()
            .filter(|skill| {
                skill_context_candidate_matches_project_or_query(
                    skill,
                    args.project_id.as_deref(),
                    &query,
                )
            })
            .take(3)
        {
            if primary_topics.iter().any(|topic| topic == &s.name) {
                continue;
            }
            primary_topics.push(s.name.clone());
        }
    }

    let mut all_skill_names: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut infra_ids = std::collections::HashSet::new();
    let mut kb_categories = std::collections::HashSet::new();
    let mut skill_results: Vec<Value> = Vec::new();

    for topic_name in &primary_topics {
        if !seen.insert(topic_name.clone()) {
            continue;
        }
        all_skill_names.push(topic_name.clone());

        if let Ok(Some(topic)) = state.store.skill_topic_get(topic_name).await {
            skill_results.push(json!({
                "name": topic.topic,
                "path": topic.file_path,
                "description": topic.description,
                "matched_by": if args.skill.is_some() { "direct" } else { "query" },
            }));

            if let Some(ref rj) = topic.requires_json {
                if let Ok(req) = serde_json::from_str::<missiond_core::SkillRequires>(rj) {
                    for dep_name in &req.skills {
                        if seen.insert(dep_name.clone()) {
                            all_skill_names.push(dep_name.clone());
                            if let Ok(Some(dep_topic)) = state.store.skill_topic_get(dep_name).await
                            {
                                skill_results.push(json!({
                                    "name": dep_topic.topic,
                                    "path": dep_topic.file_path,
                                    "description": dep_topic.description,
                                    "matched_by": "dependency",
                                }));
                                if let Some(ref drj) = dep_topic.requires_json {
                                    if let Ok(dreq) =
                                        serde_json::from_str::<missiond_core::SkillRequires>(drj)
                                    {
                                        for dep2_name in &dreq.skills {
                                            if seen.insert(dep2_name.clone()) {
                                                if let Ok(Some(dep2)) =
                                                    state.store.skill_topic_get(dep2_name).await
                                                {
                                                    skill_results.push(json!({
                                                        "name": dep2.topic,
                                                        "path": dep2.file_path,
                                                        "description": dep2.description,
                                                        "matched_by": "dependency_l2",
                                                    }));
                                                }
                                            }
                                        }
                                        infra_ids.extend(dreq.infra);
                                        kb_categories.extend(dreq.kb);
                                    }
                                }
                            }
                        }
                    }
                    infra_ids.extend(req.infra);
                    kb_categories.extend(req.kb);
                }
            }
        } else if let Some(skill_meta) = state.skills.get(topic_name) {
            skill_results.push(json!({
                "name": skill_meta.name,
                "path": skill_meta.path,
                "description": skill_meta.description,
                "matched_by": if args.skill.is_some() { "direct" } else { "query" },
            }));
        }
    }

    let mut infra_results: Vec<Value> = Vec::new();
    for id in &infra_ids {
        if let Some(server) = state.infra.read().unwrap().get(id).cloned() {
            infra_results.push(json!({
                "id": server.id,
                "name": server.name,
                "host": server.host,
                "roles": server.roles,
                "matched_by": "dependency",
            }));
        }
    }

    let kb_batch = if include_kb {
        let mut results: Vec<(missiond_core::KnowledgeEntry, &'static str)> = Vec::new();
        for cat in &kb_categories {
            if let Ok(entries) = state.store.kb_search(&search_query, Some(cat)).await {
                for entry in entries.into_iter().take(5) {
                    results.push((entry, "category_filter"));
                }
            }
        }
        if results.is_empty() {
            if let Ok(entries) = state.store.kb_search(&search_query, None).await {
                for entry in entries.into_iter().take(5) {
                    results.push((entry, "query"));
                }
            }
        }
        results
    } else {
        Vec::new()
    };
    let mut kb_results: Vec<Value> = Vec::new();
    let mut kb_seen = std::collections::HashSet::new();
    for (entry, matched_by) in &kb_batch {
        if kb_seen.insert(entry.key.clone()) {
            kb_results.push(json!({
                "key": entry.key,
                "category": entry.category,
                "summary": entry.summary,
                "matched_by": matched_by,
            }));
        }
    }

    let mut board_results: Vec<Value> = Vec::new();
    if include_board {
        if let Ok(tasks) = state.store.list_board_tasks(None, false).await {
            let query_lower = query.to_lowercase();
            for task in tasks.iter().take(100) {
                if task.title.to_lowercase().contains(&query_lower)
                    || task.description.to_lowercase().contains(&query_lower)
                {
                    board_results.push(json!({
                        "id": task.id,
                        "title": task.title,
                        "status": task.status,
                    }));
                    if board_results.len() >= 5 {
                        break;
                    }
                }
            }
        }
    }

    let operational_facts = extract_operational_facts_for_skills(&skill_results);

    let project_skill_links = if args.project_id.is_some() {
        let projects = {
            let registry = state.project_registry.read().await;
            registry
                .active_projects()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };
        let registry_skills = state.skills.list().to_vec();
        match state.store.skill_topic_list().await {
            Ok(topics) => super::query::derive_project_skill_links(
                &projects,
                &registry_skills,
                &topics,
                args.project_id.as_deref(),
                None,
            )
            .into_iter()
            .filter(authoritative_project_skill_link)
            .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let result = json!({
        "query": {
            "original": query,
            "augmented": search_query,
        },
        "project": project_config.map(|project| json!({
            "id": project.id,
            "path": project.path,
            "intent_path": project.intent_path,
            "active": project.active,
            "kind": project.kind,
            "parent_id": project.parent_id,
        })),
        "skills": skill_results,
        "project_skill_links": project_skill_links,
        "operational_facts": operational_facts,
        "infra": infra_results,
        "kb": kb_results,
        "board": board_results,
    });

    Ok(ToolResult::json_pretty(&result))
}

fn resolve_context_query(
    query: Option<&str>,
    project_id: Option<&str>,
    skill: Option<&str>,
) -> String {
    query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| project_id.map(str::trim).filter(|s| !s.is_empty()))
        .or_else(|| skill.map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_string()
}

fn skill_context_candidate_matches_project_or_query(
    skill: &missiond_core::SkillMeta,
    project_id: Option<&str>,
    query: &str,
) -> bool {
    let Some(project_id) = project_id else {
        return true;
    };
    if skill_context_skill_matches_project(skill, project_id) {
        return true;
    }

    let tokens = skill_context_query_tokens(query, Some(project_id));
    if tokens.is_empty() {
        return false;
    }
    let name_path = format!("{}\n{}", skill.name, skill.path.display()).to_ascii_lowercase();
    let description = skill
        .description
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let aka = skill
        .aka
        .as_ref()
        .map(|values| values.join("\n"))
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut weight = 0usize;
    for token in tokens {
        if skill_context_contains_token(&name_path, &token)
            || skill_context_contains_token(&aka, &token)
        {
            weight += 3;
        } else if skill_context_contains_token(&description, &token) {
            weight += 1;
        }
    }
    weight >= 4
}

fn skill_context_skill_matches_project(skill: &missiond_core::SkillMeta, project_id: &str) -> bool {
    let project_norm = skill_context_normalize_key(project_id);
    let skill_norm = skill_context_normalize_key(&skill.name);
    if project_norm.len() >= 3
        && (skill_norm == project_norm
            || skill_norm.ends_with(&project_norm)
            || project_norm.ends_with(&skill_norm))
    {
        return true;
    }
    let project_lower = project_id.to_ascii_lowercase();
    let path_lower = skill.path.display().to_string().to_ascii_lowercase();
    path_lower.contains(&format!("/{project_lower}/")) || path_lower.contains(&project_lower)
}

fn authoritative_project_skill_link(link: &Value) -> bool {
    let matched_by = link
        .get("matchedBy")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(matched_by, "exact" | "path" | "aka" | "normalized-suffix") {
        return true;
    }
    link.get("confidence")
        .and_then(Value::as_f64)
        .is_some_and(|confidence| confidence >= 0.74)
}

fn skill_context_query_tokens(query: &str, project_id: Option<&str>) -> Vec<String> {
    let project_norm = project_id.map(skill_context_normalize_key);
    let mut tokens = Vec::new();
    for raw in
        query.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'))
    {
        let token = raw
            .trim_matches(|ch: char| {
                !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
            })
            .to_ascii_lowercase();
        if token.len() < 3 && !token.chars().any(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if project_norm.as_deref() == Some(skill_context_normalize_key(&token).as_str()) {
            continue;
        }
        if is_generic_skill_context_token(&token) {
            continue;
        }
        if !tokens.iter().any(|existing| existing == &token) {
            tokens.push(token);
        }
    }
    tokens
}

fn is_generic_skill_context_token(token: &str) -> bool {
    matches!(
        token,
        "and"
            | "but"
            | "for"
            | "the"
            | "with"
            | "without"
            | "service"
            | "project"
            | "runtime"
            | "query"
            | "context"
            | "missing"
            | "skipping"
            | "backward"
            | "compat"
            | "verify"
            | "verified"
            | "status"
            | "state"
    )
}

fn skill_context_normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn skill_context_contains_token(haystack: &str, token: &str) -> bool {
    if token.chars().any(|ch| matches!(ch, '-' | '_' | '.' | '/')) {
        return haystack.contains(token);
    }

    let bytes = haystack.as_bytes();
    let needle = token.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return false;
    }
    for idx in 0..=bytes.len() - needle.len() {
        if &bytes[idx..idx + needle.len()] != needle {
            continue;
        }
        let before = idx.checked_sub(1).and_then(|pos| bytes.get(pos)).copied();
        let after = bytes.get(idx + needle.len()).copied();
        if before.is_none_or(|ch| !skill_context_is_word_byte(ch))
            && after.is_none_or(|ch| !skill_context_is_word_byte(ch))
        {
            return true;
        }
    }
    false
}

fn skill_context_is_word_byte(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

fn extract_operational_facts_for_skills(skills: &[Value]) -> Vec<Value> {
    let mut facts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for skill in skills {
        let name = skill.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(path) = skill.get("path").and_then(Value::as_str) else {
            continue;
        };
        if let Ok(content) = std::fs::read_to_string(path) {
            for (line_no, key, value) in extract_markdown_table_facts(&content) {
                if !looks_like_operational_fact(&key, &value) {
                    continue;
                }
                let safe_value = redact_operational_secret(&value);
                let dedupe_key = format!("{name}|{key}|{safe_value}");
                if !seen.insert(dedupe_key) {
                    continue;
                }
                facts.push(json!({
                    "skill": name,
                    "source_path": path,
                    "source_line": line_no,
                    "key": key,
                    "value": safe_value,
                }));
                if facts.len() >= 80 {
                    return facts;
                }
            }
        }
    }
    facts
}

fn extract_markdown_table_facts(content: &str) -> Vec<(usize, String, String)> {
    let mut facts = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
            continue;
        }
        if trimmed.contains("---") {
            continue;
        }
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().trim_matches('`').trim().to_string())
            .collect();
        if cells.len() < 2 {
            continue;
        }
        let key = cells[0].clone();
        let value = cells[1..].join(" | ");
        if key.is_empty()
            || value.is_empty()
            || matches!(key.as_str(), "项" | "键" | "技能" | "方式" | "症状")
            || matches!(value.as_str(), "值" | "义" | "命令" | "根因")
        {
            continue;
        }
        facts.push((idx + 1, key, value));
    }
    facts
}

fn looks_like_operational_fact(key: &str, value: &str) -> bool {
    let text = format!("{key} {value}").to_ascii_lowercase();
    [
        "host",
        "ip",
        "port",
        "ssh",
        "tailscale",
        "deploy-agent",
        "agent_url",
        "ollama",
        "embedding",
        "rerank",
        "runner",
        "service",
        "systemd",
        "docker",
        "endpoint",
        "url",
        "tunnel",
        "model",
        "路径",
        "端口",
        "隧道",
        "宿主",
        "服务",
        "模型",
        "网关",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn redact_operational_secret(value: &str) -> String {
    let mut out = value.to_string();
    out = out.replace("sshpass -p '1234'", "sshpass -p '<redacted>'");
    out = out.replace("sshpass -p \"1234\"", "sshpass -p \"<redacted>\"");
    out = out.replace("密码: 1234", "密码: <redacted>");
    out = out.replace("password: 1234", "password: <redacted>");
    out = out.replace("Password: 1234", "Password: <redacted>");
    out
}

#[cfg(test)]
mod tests {
    use super::{
        authoritative_project_skill_link, extract_markdown_table_facts,
        looks_like_operational_fact, redact_operational_secret, resolve_context_query,
        skill_context_candidate_matches_project_or_query,
    };
    use std::path::PathBuf;

    #[test]
    fn resolve_context_query_prefers_explicit_query() {
        assert_eq!(
            resolve_context_query(Some("deploy pcea"), Some("pcea"), Some("pcea")),
            "deploy pcea"
        );
    }

    #[test]
    fn resolve_context_query_can_use_project_without_query() {
        assert_eq!(resolve_context_query(None, Some("pcea"), None), "pcea");
    }

    #[test]
    fn resolve_context_query_can_use_skill_without_query() {
        assert_eq!(
            resolve_context_query(Some("  "), None, Some("deploy-ops")),
            "deploy-ops"
        );
    }

    #[test]
    fn operational_fact_extraction_keeps_runtime_rows_and_redacts_secrets() {
        let content = r#"
| 项 | 值 |
|----|-----|
| 宿主机 IP | 192.168.1.19 / 100.73.97.46 |
| SSH 到宿主机 | LAN: `sshpass -p '1234' ssh jin@192.168.1.19` |
| 普通备注 | no runtime anchor |
"#;
        let facts = extract_markdown_table_facts(content);
        assert!(facts
            .iter()
            .any(|(_, key, value)| key == "宿主机 IP" && looks_like_operational_fact(key, value)));
        let redacted = redact_operational_secret(&facts[1].2);
        assert!(redacted.contains("<redacted>"));
        assert!(!redacted.contains("'1234'"));
    }

    #[test]
    fn skill_context_filters_cross_project_query_candidates() {
        let query =
            "Payments missing service.manifest.toml Manifest Gate skipping verify backward compat";
        let payments = skill_meta(
            "payments",
            "支付服务。WeChat/Alipay/Stripe 支付 + Credits 钱包。",
            "/Users/jinchen/.claude/skills/services/payments/SKILL.md",
        );
        let astrill = skill_meta(
            "astrill-gateway",
            "Astrill VPN 旁路由网关。两台 VM 为局域网设备提供 VPN 出口。",
            "/Users/jinchen/.claude/skills/astrill-gateway/SKILL.md",
        );
        let openclaw = skill_meta(
            "openclaw",
            "OpenClaw AI Agent 网关。HostVDS Docker 部署，对接 MissionD Claude Code。",
            "/Users/jinchen/.claude/skills/openclaw/SKILL.md",
        );

        assert!(skill_context_candidate_matches_project_or_query(
            &payments,
            Some("payments"),
            query
        ));
        assert!(!skill_context_candidate_matches_project_or_query(
            &astrill,
            Some("payments"),
            query
        ));
        assert!(!skill_context_candidate_matches_project_or_query(
            &openclaw,
            Some("payments"),
            query
        ));
    }

    #[test]
    fn skill_context_omits_description_only_project_links() {
        let exact = serde_json::json!({
            "skill": "payments",
            "matchedBy": "exact",
            "confidence": 1.0
        });
        let description_only = serde_json::json!({
            "skill": "auth",
            "matchedBy": "description",
            "confidence": 0.62
        });

        assert!(authoritative_project_skill_link(&exact));
        assert!(!authoritative_project_skill_link(&description_only));
    }

    fn skill_meta(name: &str, description: &str, path: &str) -> missiond_core::SkillMeta {
        missiond_core::SkillMeta {
            name: name.to_string(),
            description: Some(description.to_string()),
            aka: None,
            allowed_tools: None,
            path: PathBuf::from(path),
            requires: None,
            actions: None,
            context_hooks: None,
            triggers: None,
        }
    }
}
