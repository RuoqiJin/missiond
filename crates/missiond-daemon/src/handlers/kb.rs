use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use tracing::info;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::embedding_worker::resolve_llm_credentials;
use crate::context_budget::apply_context_budget;
use crate::state::EmbeddingTask;
use crate::lenient;
use crate::context_budget::MAX_ROUTER_PAYLOAD_BYTES;
use crate::helpers::default_mission_home;

#[derive(Deserialize)]
struct KBRememberArgs {
    category: String,
    key: String,
    summary: String,
    #[serde(default)]
    detail: Option<Value>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    confidence: Option<f64>,
}

#[derive(Deserialize)]
struct KBKeyArgs {
    key: String,
}

#[derive(Deserialize)]
struct KBSearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct KBListArgs {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct KBImportArgs {
    format: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct KBDiscoverArgs {
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
struct KBGCArgs {
    action: String,
    #[serde(default, deserialize_with = "lenient::option_i64")]
    days: Option<i64>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Knowledge Base (Jarvis Memory) =====
        "mission_kb_remember" => {
            let args: KBRememberArgs = serde_json::from_value(args)?;
            let input = missiond_core::types::KBRememberInput {
                category: args.category,
                key: args.key,
                summary: args.summary,
                detail: args.detail,
                source: args.source,
                confidence: args.confidence,
            };
            let result = state.mission.db()
                .kb_remember(&input)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Trigger async embedding update via Worker (avoids block_in_place in MCP handler)
            let _ = state.embedding_tx.try_send(EmbeddingTask::ProcessKBEntry(result.entry.id.clone()));
            Ok(ToolResult::json_pretty(&result))
        }
        "mission_kb_forget" => {
            let KBKeyArgs { key } = serde_json::from_value(args)?;
            // Get entry ID before deletion for cache invalidation
            let entry_id = state.mission.db().kb_get_id_by_key(&key).ok().flatten();
            let deleted = state.mission.db()
                .kb_forget(&key)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Remove from embedding cache if deleted
            if deleted {
                if let Some(id) = entry_id {
                    let mut guard = state.embedding_cache.write().await;
                    guard.retain(|(eid, _)| eid != &id);
                }
            }
            Ok(ToolResult::json(&serde_json::json!({
                "deleted": deleted,
                "key": key,
            })))
        }
        "mission_kb_batch_forget" => {
            let keys_val = args.get("keys").cloned().unwrap_or(Value::Array(vec![]));
            let keys: Vec<String> = match serde_json::from_value::<Vec<String>>(keys_val.clone()) {
                Ok(v) => v,
                Err(_) => {
                    // Claude may pass JSON string "[\"a\",\"b\"]" or comma-separated "a,b,c"
                    if let Some(s) = keys_val.as_str() {
                        serde_json::from_str::<Vec<String>>(s).unwrap_or_else(|_| {
                            s.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()
                        })
                    } else {
                        return Ok(ToolResult::error("keys: expected array or JSON string"));
                    }
                }
            };
            if keys.is_empty() {
                return Ok(ToolResult::error("keys array is empty"));
            }
            let count = state.mission.db()
                .kb_batch_forget(&keys)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "deleted_count": count,
                "requested_keys": keys.len(),
            })))
        }
        "mission_kb_search" => {
            let KBSearchArgs { query, category } = serde_json::from_value(args)
                .unwrap_or(KBSearchArgs { query: None, category: None });
            let query = query.unwrap_or_default();
            if query.is_empty() && category.is_none() {
                let entries = state.mission.db()
                    .kb_list(None)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                return Ok(ToolResult::json_pretty(&entries));
            }

            let db = state.mission.db();
            let top_k = 20usize;

            // 1. FTS5 ranked IDs (fallback to LIKE for Chinese)
            let mut fts_ranked = db.kb_search_fts_ranked(&query, category.as_deref())
                .unwrap_or_default();
            if fts_ranked.is_empty() {
                fts_ranked = db.kb_search_like_ranked(&query, category.as_deref())
                    .unwrap_or_default();
            }

            // 2. Embedding cosine similarity against kb_search_cache
            let query_embedding = state.embedding_service.as_ref()
                .and_then(|svc| svc.embed(&query));
            let cache = state.kb_search_cache.read().await;
            let vec_ranked: Vec<(String, usize, f32)> = if let Some(ref qe) = query_embedding {
                let mut scores: Vec<(usize, f32)> = cache.iter()
                    .enumerate()
                    .map(|(i, (_, vec))| (i, missiond_core::embedding::cosine_similarity(qe, vec)))
                    .collect();
                scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                scores.iter()
                    .take(top_k * 3)
                    .enumerate()
                    .map(|(rank, (idx, sim))| (cache[*idx].0.clone(), rank, *sim))
                    .collect()
            } else {
                Vec::new()
            };
            drop(cache);

            // 3. RRF merge
            let rrf_k = 60;
            let mut merged: std::collections::HashMap<String, (Option<usize>, Option<usize>, Option<f32>)> =
                std::collections::HashMap::new();
            for (id, rank) in &fts_ranked {
                merged.entry(id.clone()).or_insert((None, None, None)).0 = Some(*rank);
            }
            for (id, rank, sim) in &vec_ranked {
                let entry = merged.entry(id.clone()).or_insert((None, None, None));
                entry.1 = Some(*rank);
                entry.2 = Some(*sim);
            }
            let mut ranked: Vec<(String, f64, Option<usize>, Option<usize>, Option<f32>)> = merged
                .into_iter()
                .map(|(id, (fts_r, vec_r, sim))| {
                    let score = missiond_core::embedding::rrf_score(fts_r, vec_r, rrf_k);
                    (id, score, fts_r, vec_r, sim)
                })
                .collect();
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            ranked.truncate(top_k);

            // 4. Fetch full KnowledgeEntry objects in RRF order
            let mut results = Vec::new();
            for (id, _rrf, _fts_r, _vec_r, _sim) in &ranked {
                if let Ok(Some(entry)) = db.kb_get_by_id(id) {
                    results.push(entry);
                }
            }

            // Update access stats
            if !results.is_empty() {
                let _ = db.kb_update_access_stats(&results);
            }

            Ok(ToolResult::json_pretty(&results))
        }
        "mission_kb_get" => {
            let KBKeyArgs { key } = serde_json::from_value(args)?;
            let entry = state.mission.db()
                .kb_get(&key)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            match entry {
                Some(e) => Ok(ToolResult::json_pretty(&e)),
                None => Ok(ToolResult::error(format!("Key not found: {}", key))),
            }
        }
        "mission_kb_list" => {
            let KBListArgs { category } =
                serde_json::from_value(args).unwrap_or(KBListArgs { category: None });
            let entries = state.mission.db()
                .kb_list(category.as_deref())
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&entries))
        }
        "mission_kb_import" => {
            let KBImportArgs { format, path } = serde_json::from_value(args)?;
            match format.as_str() {
                "servers_yaml" => {
                    let yaml_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| default_mission_home().join("servers.yaml"));
                    let infra = missiond_core::InfraConfig::load(&yaml_path);
                    let mut imported = 0;
                    for server in &infra.servers {
                        let detail = serde_json::to_value(server).ok();
                        let summary = format!(
                            "{} ({}) — {}",
                            server.name,
                            server.provider,
                            server.roles.join(", ")
                        );
                        let input = missiond_core::types::KBRememberInput {
                            category: "infra".to_string(),
                            key: server.id.clone(),
                            summary,
                            detail,
                            source: Some("import".to_string()),
                            confidence: Some(1.0),
                        };
                        state.mission.db()
                            .kb_remember(&input)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        imported += 1;
                    }
                    Ok(ToolResult::json(&serde_json::json!({
                        "imported": imported,
                        "source": yaml_path.display().to_string(),
                    })))
                }
                _ => Ok(ToolResult::error(format!("Unsupported import format: {}", format))),
            }
        }

        "mission_kb_discover" => {
            let KBDiscoverArgs { host, port, password } = serde_json::from_value(args)?;

            // Resolve host: if it looks like an infra key (no @ or .), try infra registry
            let (ssh_user, ssh_host, ssh_port, ssh_pass) = if !host.contains('@') && !host.contains('.') {
                // Try infra registry lookup
                let server = state.infra.get(&host);
                let ip = server.and_then(|s| s.host.as_deref()).unwrap_or(&host);
                // Look up credentials from KB
                let db = state.mission.db();
                let cred_pass = db.kb_search(&format!("{} password", host), Some("credential"))
                    .ok()
                    .and_then(|entries| entries.into_iter().next())
                    .and_then(|e| e.detail.as_ref().and_then(|d| d.get("password").and_then(|v| v.as_str().map(String::from))));
                ("root".to_string(), ip.to_string(), port.unwrap_or(22), password.or(cred_pass))
            } else if host.contains('@') {
                let parts: Vec<&str> = host.splitn(2, '@').collect();
                (parts[0].to_string(), parts[1].to_string(), port.unwrap_or(22), password)
            } else {
                ("root".to_string(), host.clone(), port.unwrap_or(22), password)
            };

            // Build probe script (piped to remote bash to avoid quoting issues)
            let probe_script = concat!(
                "echo \"HOSTNAME=$(hostname)\"\n",
                "echo \"UNAME=$(uname -a)\"\n",
                "echo \"OS=$(. /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\" || echo unknown)\"\n",
                "echo \"CPU=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo unknown)\"\n",
                "MEM=$(LANG=C free -h 2>/dev/null | awk '/Mem:/{print $2}'); echo \"MEM=${MEM:-unknown}\"\n",
                "DISK=$(LANG=C df -h / 2>/dev/null | awk 'NR==2{print $2}'); echo \"DISK=${DISK:-unknown}\"\n",
                "echo \"UPTIME=$(uptime -p 2>/dev/null || uptime || echo unknown)\"\n",
                "DOCKER=$(docker ps --format '{{.Names}}:{{.Image}}' 2>/dev/null | tr '\\n' ','); echo \"DOCKER=${DOCKER:-none}\"\n",
                "LISTEN=$(LANG=C ss -tlnp 2>/dev/null | awk 'NR>1{print $4}' | tr '\\n' ','); echo \"LISTEN=${LISTEN:-unknown}\"\n",
            );

            // Build SSH command args (pipe probe_script to stdin)
            let mut ssh_args: Vec<String> = Vec::new();
            if let Some(ref pass) = ssh_pass {
                ssh_args.extend(["sshpass".into(), "-p".into(), pass.clone(), "ssh".into()]);
            } else {
                ssh_args.push("ssh".into());
                ssh_args.extend(["-o".into(), "BatchMode=yes".into()]);
            }
            ssh_args.extend([
                "-o".into(), "StrictHostKeyChecking=no".into(),
                "-o".into(), "ConnectTimeout=10".into(),
                "-p".into(), ssh_port.to_string(),
                format!("{}@{}", ssh_user, ssh_host),
                "bash".into(),
            ]);

            let program = ssh_args.remove(0);
            let mut cmd = tokio::process::Command::new(&program);
            cmd.args(&ssh_args);
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let mut child = cmd.spawn()
                .map_err(|e| anyhow!("Failed to spawn SSH: {}", e))?;

            // Write probe script to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(probe_script.as_bytes()).await.ok();
                drop(stdin);
            }

            let output = child.wait_with_output().await
                .map_err(|e| anyhow!("SSH failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(ToolResult::error(format!("SSH probe failed: {}", stderr.trim())));
            }

            // Parse key=value output
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut detail = serde_json::Map::new();
            for line in stdout.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim().to_lowercase();
                    let val = v.trim().to_string();
                    if !val.is_empty() && val != "unknown" && val != "none" {
                        detail.insert(key, serde_json::Value::String(val));
                    }
                }
            }

            // Add connection info
            detail.insert("ssh_user".to_string(), serde_json::Value::String(ssh_user.clone()));
            detail.insert("ssh_host".to_string(), serde_json::Value::String(ssh_host.clone()));
            if ssh_port != 22 {
                detail.insert("ssh_port".to_string(), serde_json::Value::Number(ssh_port.into()));
            }

            // Build summary
            let hostname = detail.get("hostname").and_then(|v| v.as_str()).unwrap_or("unknown");
            let os = detail.get("os").and_then(|v| v.as_str()).unwrap_or("unknown");
            let cpu = detail.get("cpu").and_then(|v| v.as_str()).unwrap_or("?");
            let mem = detail.get("mem").and_then(|v| v.as_str()).unwrap_or("?");
            let summary = format!("{} — {} ({}C, {})", hostname, os, cpu, mem);

            // Derive a key from hostname or host
            let key = hostname.to_lowercase().replace(' ', "-");

            let input = missiond_core::types::KBRememberInput {
                category: "infra".to_string(),
                key: key.clone(),
                summary: summary.clone(),
                detail: Some(serde_json::Value::Object(detail.clone())),
                source: Some("discovery".to_string()),
                confidence: Some(1.0),
            };
            state.mission.db()
                .kb_remember(&input)
                .map_err(|e| anyhow!("DB error: {}", e))?;

            Ok(ToolResult::json(&serde_json::json!({
                "status": "discovered",
                "key": key,
                "summary": summary,
                "detail": detail,
            })))
        }

        "mission_kb_gc" => {
            let KBGCArgs { action, days } = serde_json::from_value(args)?;
            let db = state.mission.db();
            match action.as_str() {
                "stats" => {
                    let stats = db.kb_stats()
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&stats))
                }
                "stale" => {
                    let threshold = days.unwrap_or(30);
                    let stale = db.kb_find_stale(threshold)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "threshold_days": threshold,
                        "count": stale.len(),
                        "entries": stale.iter().map(|e| serde_json::json!({
                            "category": e.category,
                            "key": e.key,
                            "summary": e.summary,
                            "updatedAt": e.updated_at,
                        })).collect::<Vec<_>>(),
                    })))
                }
                "duplicates" => {
                    let dups = db.kb_find_duplicates()
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "count": dups.len(),
                        "pairs": dups.iter().map(|(a, b, sim)| serde_json::json!({
                            "similarity": format!("{:.2}", sim),
                            "a": {"category": a.category, "key": a.key, "summary": a.summary, "accessCount": a.access_count},
                            "b": {"category": b.category, "key": b.key, "summary": b.summary, "accessCount": b.access_count},
                        })).collect::<Vec<_>>(),
                    })))
                }
                "clean_stale" => {
                    let threshold = days.unwrap_or(30);
                    let stale = db.kb_find_stale(threshold)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let keys: Vec<String> = stale.iter().map(|e| e.key.clone()).collect();
                    let count = db.kb_batch_forget(&keys)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "action": "clean_stale",
                        "threshold_days": threshold,
                        "deleted": count,
                        "keys": keys,
                    })))
                }
                "clean_duplicates" => {
                    let dups = db.kb_find_duplicates()
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    // Keep entry with higher access_count (or newer updated_at), delete the other
                    let mut to_delete = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for (a, b, sim) in &dups {
                        // Skip if either already marked for deletion
                        if seen.contains(&a.key) || seen.contains(&b.key) { continue; }
                        let loser = if a.access_count > b.access_count {
                            &b.key
                        } else if b.access_count > a.access_count {
                            &a.key
                        } else if a.updated_at >= b.updated_at {
                            &b.key
                        } else {
                            &a.key
                        };
                        to_delete.push(serde_json::json!({
                            "deleted_key": loser,
                            "kept_key": if loser == &a.key { &b.key } else { &a.key },
                            "similarity": format!("{:.2}", sim),
                        }));
                        seen.insert(loser.clone());
                    }
                    let keys: Vec<String> = to_delete.iter()
                        .filter_map(|d| d["deleted_key"].as_str().map(String::from))
                        .collect();
                    let count = db.kb_batch_forget(&keys)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "action": "clean_duplicates",
                        "deleted": count,
                        "details": to_delete,
                    })))
                }
                _ => Ok(ToolResult::error(format!("Unknown gc action: {}. Use: stats, stale, duplicates, clean_stale, clean_duplicates", action))),
            }
        }

        // ===== KB Analysis (via external AI) =====
        "mission_kb_analyze" => {
            // Parse parameters
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let mode = args_val.get("mode").and_then(|v| v.as_str()).unwrap_or("overview");
            let target_category = args_val.get("target_category").and_then(|v| v.as_str());
            let limit = args_val.get("limit").and_then(|v| v.as_u64()).unwrap_or(500) as u32;
            let offset = args_val.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let custom_prompt = args_val.get("custom_prompt").and_then(|v| v.as_str());
            let include_board_context = args_val.get("include_board_context")
                .and_then(|v| v.as_bool()).unwrap_or(false);
            let model: String = args_val.get("model").and_then(|v| v.as_str())
                .unwrap_or("gemini-3.1-pro").to_string();
            let max_tokens: u32 = args_val.get("max_tokens").and_then(|v| v.as_u64())
                .unwrap_or(16384) as u32;

            // 1. Read KB entries with pagination
            let entries = state.mission.db()
                .kb_list_paginated(target_category, limit, offset)
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if entries.is_empty() {
                return Ok(ToolResult::error("No KB entries found for the given filter."));
            }

            // Also get total count for pagination info
            let total_count = state.mission.db()
                .kb_list(target_category.map(|s| s))
                .map(|v| v.len())
                .unwrap_or(0);

            // 2. Build JSONL with metadata (compact format for LLM)
            let now = chrono::Utc::now();
            let mut jsonl_lines = Vec::with_capacity(entries.len());
            let include_detail = mode == "consolidation_plan";

            for e in &entries {
                if e.category == "credential" { continue; }
                let sanitized_summary = missiond_core::db::MissionDB::redact_sensitive(&e.summary);

                // Calculate age in days
                let age_days = chrono::DateTime::parse_from_rfc3339(&e.updated_at)
                    .map(|dt| (now - dt.with_timezone(&chrono::Utc)).num_days())
                    .unwrap_or(0);

                let mut item = serde_json::json!({
                    "category": e.category,
                    "key": e.key,
                    "summary": sanitized_summary,
                    "access_count": e.access_count,
                    "age_days": age_days,
                    "confidence": e.confidence,
                });

                if include_detail {
                    if let Some(detail) = &e.detail {
                        let detail_str = match detail {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let sanitized_detail = missiond_core::db::MissionDB::redact_sensitive(&detail_str);
                        item["detail"] = serde_json::Value::String(sanitized_detail);
                    }
                }

                jsonl_lines.push(serde_json::to_string(&item).unwrap_or_default());
            }
            let kb_jsonl = jsonl_lines.join("\n");

            // 2b. Build Board context if requested
            let board_context = if include_board_context {
                let tasks = state.mission.db().list_board_tasks(None, false)
                    .unwrap_or_default();
                let mut open_lines = Vec::new();
                let mut done_lines = Vec::new();
                for t in &tasks {
                    let line = format!("{} {}", &t.id[..8], t.title);
                    match t.status {
                        missiond_core::types::BoardTaskStatus::Done => done_lines.push(line),
                        _ => open_lines.push(line),
                    }
                }
                Some(format!(
                    "[Board Tasks Context]\n<open>\n{}\n<done>\n{}",
                    open_lines.join("\n"),
                    done_lines.join("\n")
                ))
            } else {
                None
            };

            // 3. Build prompt and optional response_format based on mode
            let mut response_format: Option<serde_json::Value> = None;
            let analysis_prompt = match mode {
                "consolidation_plan" => {
                    response_format = Some(serde_json::json!({
                        "type": "json_schema",
                        "json_schema": {
                            "name": "kb_consolidation_actions",
                            "strict": false,
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "actions": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "action_type": { "type": "string", "enum": ["merge", "delete", "update", "distill"] },
                                                "target_keys": {
                                                    "type": "array",
                                                    "items": { "type": "string" },
                                                    "description": "Keys of entries to delete or merge"
                                                },
                                                "new_entry": {
                                                    "type": "object",
                                                    "properties": {
                                                        "category": { "type": "string" },
                                                        "key": { "type": "string" },
                                                        "summary": { "type": "string" },
                                                        "confidence": { "type": "number" }
                                                    },
                                                    "required": ["category", "key", "summary"]
                                                },
                                                "linked_task_id": {
                                                    "type": "string",
                                                    "description": "Matched Board Task ID (short, 8-char prefix)"
                                                },
                                                "reason": { "type": "string" }
                                            },
                                            "required": ["action_type", "target_keys", "reason"]
                                        }
                                    }
                                },
                                "required": ["actions"]
                            }
                        }
                    }));

                    let board_section = if let Some(ref ctx) = board_context {
                        format!("\n\n=== BOARD TASKS CONTEXT ===\n{}\n===========================\n\n\
                            === 任务感知整理规则 ===\n\
                            1. 关联 Open 任务的 KB → 保护，不删不合并，仅用 update 补 linked_task_id\n\
                            2. 关联 Done 任务的 KB → distill 蒸馏收敛（提取最终结论升维到 architecture/feature，删流水账）\n\
                            3. 无关联 Orphan → preference/ops/platform 保留；老旧 debug/bugfix 可删除\n\
                            4. 每个 action 的 reason 中说明关联了哪个任务（或标注 orphan）\n", ctx)
                    } else {
                        String::new()
                    };

                    format!(
                        "你是 MissionD 的知识库自治整理引擎。分析传入的 JSONL 条目，生成可执行的清理计划。\n\n\
                        规则：\n\
                        1. 重复/相似合并 (merge)：相同主题不同措辞 → 合并，保留最高 confidence，汇总 summary\n\
                        2. 碎片整合 (update)：松散相关条目 → 整合成连贯大条目\n\
                        3. 过时清理 (delete)：基于 age_days 和 access_count，旧策略已被覆盖的条目\n\
                        4. 类别修正 (update)：category 放错的条目移到正确分类\n\
                        5. 蒸馏收敛 (distill)：已完成项目的多条流水账 → 提取最终结论为一条精华\n\n\
                        保守原则：不确定就不动。每个 action 必须有 reason。\
                        {}\n\n共 {} 条（第 {} 到 {} 条）：\n{}",
                        board_section,
                        entries.len(), offset + 1, offset + entries.len() as u32, kb_jsonl
                    )
                }
                "custom" => {
                    format!(
                        "{}\n\n知识库数据（共 {} 条，JSONL格式）：\n{}",
                        custom_prompt.unwrap_or("请分析以下知识库数据。"),
                        entries.len(), kb_jsonl
                    )
                }
                _ => { // overview - 查重+升维版
                    format!(
                        "作为 MissionD 核心系统的知识管理专家，请深度审查以下知识库（KB）条目。\n\n
请务必重点完成以下两项任务，并给出具体的、可操作的建议：\n\n
1. 【冗余与合并分析】\n
- 识别同一问题的多阶段记录、重复的调试流水账（尤其是频繁触发的遗留 bug 记录）。\n
- 明确列出建议合并保留的主 key 和建议删除的冗余 key 列表。\n\n
2. 【类别纠偏与升维建议】\n
- 严格对照系统的生命周期与分类约束：\n
  * preference: 用户偏好/纠正/否定 (长期保留保护)\n
  * memory:decision / memory:architecture: 架构决策/核心技术事实 (长期保留保护)\n
  * project: 项目专属上下文 (长期保留保护)\n
  * memory:bugfix: 已修复 bug 的根因分析 (30天 GC)\n
  * memory:debug: 调试弯路/临时排查经验 (短周期 GC)\n
  * memory:ops: 运维基建/CI脚本/痛点信号\n
- 找出被埋没或错放的条目。例如：误记在 debug/bugfix 中但实际是高价值架构决策的条目（面临被误删风险，应升维至 decision）；或属于用户习惯却混入普通 memory 的条目（应移至 preference）。
- 列出需要修改 category 的条目 key，并给出建议的新 category 及简短理由。\n\n
附带的知识库数据如下（JSONL格式，共 {} 条）：\n{}",
                        entries.len(), kb_jsonl
                    )
                }
            };

            // 4. Resolve LLM credentials
            let (base_url, jwt) = resolve_llm_credentials().await?;

            // 5. Apply context budget
            let mut analysis_messages: Vec<Value> = vec![
                serde_json::json!({"role": "user", "content": analysis_prompt})
            ];
            let budget_result = apply_context_budget(&mut analysis_messages, MAX_ROUTER_PAYLOAD_BYTES);
            if budget_result.trimmed {
                info!("KB analyze: context budget applied — {}", budget_result.note.as_deref().unwrap_or("trimmed"));
            }

            // 6. Build request body with optional response_format
            let url = format!("{}/v1/chat/completions", base_url);
            let mut body = serde_json::json!({
                "model": model,
                "messages": analysis_messages,
                "max_tokens": max_tokens,
            });
            if let Some(fmt) = &response_format {
                body.as_object_mut().unwrap().insert("response_format".to_string(), fmt.clone());
            }

            info!("KB analyze [{}]: sending {} entries ({} chars) to {} via {}",
                mode, entries.len(), kb_jsonl.len(), model, url);

            // 7. Call router API (rate-limited with 429 retry)
            let result = state.gemini.send(&state.http_client, &url, &jwt, &body).await?;

            let content = result
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_str())
                .unwrap_or("(empty response)");
            let finish_reason = result
                .pointer("/choices/0/finish_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let usage = result.get("usage");
            let resp_model = result.get("model").and_then(|v| v.as_str()).unwrap_or(&model);

            // 8. Build response with pagination metadata
            let mut resp = serde_json::json!({
                "model": resp_model,
                "mode": mode,
                "entries_in_request": entries.len(),
                "total_entries": total_count,
                "offset": offset,
                "has_more": (offset as usize + entries.len()) < total_count,
                "usage": usage,
            });

            // For consolidation_plan, try to parse as JSON and auto-save to queue
            let save_plan = args_val.get("save_plan").and_then(|v| v.as_bool()).unwrap_or(true);
            if mode == "consolidation_plan" {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                    resp["plan"] = parsed.clone();
                    // Auto-save plan to operation queue
                    if save_plan {
                        if let Some(actions) = parsed.get("actions").and_then(|a| a.as_array()) {
                            let plan_id = uuid::Uuid::new_v4().to_string();
                            let task_id_param = args_val.get("task_id").and_then(|v| v.as_str());
                            let ops: Vec<missiond_core::types::KBOperation> = actions.iter().filter_map(|a| {
                                let operation = a.get("action_type").and_then(|v| v.as_str())
                                    .or_else(|| a.get("action").and_then(|v| v.as_str()))
                                    .or_else(|| a.get("operation").and_then(|v| v.as_str()))?;
                                let keys: Vec<String> = a.get("target_keys").and_then(|v| v.as_array())
                                    .or_else(|| a.get("keys").and_then(|v| v.as_array()))
                                    .map(|arr| arr.iter().filter_map(|k| k.as_str().map(|s| s.to_string())).collect())
                                    .or_else(|| a.get("key").and_then(|v| v.as_str()).map(|k| vec![k.to_string()]))?;
                                Some(missiond_core::types::KBOperation {
                                    operation: operation.to_string(),
                                    target_keys: keys,
                                    rationale: {
                                        let reason = a.get("reason").and_then(|v| v.as_str())
                                            .or_else(|| a.get("rationale").and_then(|v| v.as_str()));
                                        // For update ops, embed new_entry in rationale JSON
                                        if operation == "update" || operation == "recategorize" || operation == "category_fix" {
                                            let mut meta = serde_json::Map::new();
                                            if let Some(r) = reason {
                                                meta.insert("reason".into(), serde_json::json!(r));
                                            }
                                            if let Some(ne) = a.get("new_entry") {
                                                meta.insert("new_entry".into(), ne.clone());
                                            }
                                            Some(serde_json::to_string(&meta).unwrap_or_default())
                                        } else {
                                            reason.map(|s| s.to_string())
                                        }
                                    },
                                })
                            }).collect();
                            if !ops.is_empty() {
                                match state.mission.db().kb_ops_save_plan(&plan_id, task_id_param, &ops) {
                                    Ok(n) => {
                                        resp["plan_id"] = serde_json::json!(plan_id);
                                        resp["operations_saved"] = serde_json::json!(n);
                                        info!(plan_id = %plan_id, ops = n, "KB consolidation plan saved to queue");
                                    }
                                    Err(e) => {
                                        resp["save_error"] = serde_json::json!(format!("Failed to save plan: {}", e));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    resp["analysis"] = serde_json::Value::String(content.to_string());
                    resp["parse_warning"] = serde_json::json!("Response was not valid JSON. Returned as text.");
                }
            } else {
                resp["analysis"] = serde_json::Value::String(content.to_string());
            }

            if finish_reason == "length" || finish_reason == "max_tokens" {
                resp["warning"] = serde_json::json!("⚠️ 输出被截断：LLM 达到 max_tokens 限制。可增大 max_tokens 参数重试。");
                resp["finish_reason"] = serde_json::json!(finish_reason);
            }
            if let Some(note) = budget_result.note {
                resp["context_budget"] = serde_json::json!(note);
            }
            Ok(ToolResult::json_pretty(&resp))
        }

        // ===== KB Operation Queue =====
        "mission_kb_queue_status" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
            let status_filter = args_val.get("status").and_then(|v| v.as_str());

            let ops = state.mission.db().kb_ops_list(plan_id, status_filter)
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // If plan_id given, also get summary
            let summary = if let Some(pid) = plan_id {
                state.mission.db().kb_ops_plan_summary(pid).ok()
            } else {
                None
            };

            let mut resp = serde_json::json!({
                "operations": ops,
                "count": ops.len(),
            });
            if let Some(s) = summary {
                resp["summary"] = s;
            }
            Ok(ToolResult::json_pretty(&resp))
        }

        "mission_kb_execute_plan" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
            let limit = args_val.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

            // Expire stale pending ops (>24h)
            let expired = state.mission.db().kb_ops_expire_stale(86400).unwrap_or(0);
            if expired > 0 {
                info!(expired, "kb_execute_plan: expired stale pending ops");
            }

            let plan_id = plan_id.ok_or_else(|| anyhow!("plan_id is required"))?;

            // Get pending operations
            let ops = state.mission.db().kb_ops_list(Some(plan_id), Some("pending"))
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if ops.is_empty() {
                return Ok(ToolResult::text("No pending operations in queue."));
            }

            let batch: Vec<_> = ops.into_iter().take(limit).collect();
            let mut results = Vec::new();

            for op in &batch {
                // Mark as running
                let _ = state.mission.db().kb_ops_update_status(&op.id, "running", None, None);

                let target_keys: Vec<String> = serde_json::from_str(&op.target_keys).unwrap_or_default();
                let outcome = match op.operation.as_str() {
                    "delete" => {
                        let mut deleted = 0usize;
                        for key in &target_keys {
                            if state.mission.db().kb_forget(key).unwrap_or(false) {
                                deleted += 1;
                            }
                        }
                        Ok(format!("Deleted {}/{} keys", deleted, target_keys.len()))
                    }
                    "update" | "category_fix" | "recategorize" => {
                        // Update category/summary directly from rationale (contains new_entry JSON)
                        let meta: serde_json::Value = op.rationale.as_deref()
                            .and_then(|r| serde_json::from_str(r).ok())
                            .unwrap_or_default();
                        let new_entry = meta.get("new_entry");
                        let key = target_keys.first().map(|k| k.as_str())
                            .or_else(|| new_entry.and_then(|ne| ne.get("key").and_then(|v| v.as_str())));
                        let category = new_entry.and_then(|ne| ne.get("category").and_then(|v| v.as_str()));
                        let summary = new_entry.and_then(|ne| ne.get("summary").and_then(|v| v.as_str()));

                        match (key, category) {
                            (Some(key), Some(cat)) => {
                                let input = missiond_core::types::KBRememberInput {
                                    category: cat.to_string(),
                                    key: key.to_string(),
                                    summary: summary.unwrap_or("").to_string(),
                                    detail: new_entry.and_then(|ne| ne.get("detail").cloned()),
                                    source: Some("consolidation".to_string()),
                                    confidence: new_entry.and_then(|ne| ne.get("confidence").and_then(|v| v.as_f64())),
                                };
                                match state.mission.db().kb_remember(&input) {
                                    Ok(r) => Ok(format!("Updated key={} category={} action={}", key, cat, r.action)),
                                    Err(e) => Err(format!("Failed to update: {}", e)),
                                }
                            }
                            _ => Err("update operation requires new_entry with key and category in rationale".to_string()),
                        }
                    }
                    "merge" | "distill" => {
                        // Auto-dispatch: fetch entries, build prompt, submit to slot-memory-slow
                        let mut entries_text = String::new();
                        for key in &target_keys {
                            if let Ok(Some(entry)) = state.mission.db().kb_get(key) {
                                entries_text.push_str(&format!(
                                    "---\nKey: {}\nCategory: {}\nSummary: {}\nDetail: {}\n",
                                    entry.key, entry.category, entry.summary,
                                    entry.detail.as_ref().map(|d| d.to_string()).unwrap_or_default(),
                                ));
                            }
                        }
                        if entries_text.is_empty() {
                            Err(format!("No KB entries found for keys: {:?}", target_keys))
                        } else {
                            let rationale = op.rationale.as_deref().unwrap_or("");
                            let prompt = if op.operation == "merge" {
                                format!(
                                    "KB整理任务(merge):\n\n原因: {}\n\n以下KB条目内容重叠,请合并为一条。\
                                    保留最完整的key,用 mission_kb_remember 写入合并后的内容(category/summary/detail),\
                                    然后用 mission_kb_forget 删除多余的key。\n\n{}", rationale, entries_text
                                )
                            } else {
                                format!(
                                    "KB整理任务(distill):\n\n原因: {}\n\n以下KB条目需要精炼。\
                                    用 mission_kb_remember 更新每条的 summary(更简洁)和 detail(保留关键信息,删除冗余)。\n\n{}",
                                    rationale, entries_text
                                )
                            };
                            match state.mission.submit("memory", &prompt) {
                                Ok(task_id) => Ok(format!("dispatched:task_id={}", task_id)),
                                Err(e) => Err(format!("submit failed: {}", e)),
                            }
                        }
                    }
                    other => {
                        Err(format!("Unknown operation: {}", other))
                    }
                };

                match outcome {
                    Ok(msg) => {
                        let (status_str, result_json) = if msg.starts_with("dispatched:") {
                            // Extract task_id from "dispatched:task_id=xxx"
                            let task_id = msg.strip_prefix("dispatched:task_id=").unwrap_or(&msg);
                            ("dispatched", serde_json::json!({
                                "id": op.id,
                                "operation": op.operation,
                                "status": "dispatched",
                                "taskId": task_id,
                            }))
                        } else {
                            ("done", serde_json::json!({
                                "id": op.id,
                                "operation": op.operation,
                                "status": "done",
                                "result": msg,
                            }))
                        };
                        let _ = state.mission.db().kb_ops_update_status(&op.id, status_str, Some(&msg), None);
                        results.push(result_json);
                    }
                    Err(msg) => {
                        let _ = state.mission.db().kb_ops_update_status(&op.id, "failed", None, Some(&msg));
                        results.push(serde_json::json!({
                            "id": op.id,
                            "operation": op.operation,
                            "status": "failed",
                            "error": msg,
                        }));
                    }
                }
            }

            // Signal unified scheduler to dispatch any newly created submit tasks
            if results.iter().any(|r| r.get("status").and_then(|s| s.as_str()) == Some("dispatched")) {
                state.submit_notify.notify_one();
            }

            // Get remaining count
            let remaining = state.mission.db().kb_ops_list(Some(plan_id), Some("pending"))
                .map(|v| v.len()).unwrap_or(0);

            Ok(ToolResult::json_pretty(&serde_json::json!({
                "executed": results.len(),
                "results": results,
                "remaining": remaining,
            })))
        }

        _ => Err(anyhow!("Unknown kb tool: {name}")),
    }
}
