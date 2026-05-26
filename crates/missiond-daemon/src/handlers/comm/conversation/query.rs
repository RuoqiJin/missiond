use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::context::v3_blueprint_runtime::ConversationIngestionRuntimeConfig;
use crate::lenient;
use crate::state::AppState;

fn compact_preview(content: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in content.chars().take(max_chars) {
        out.push(ch);
    }
    if content.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn load_conversation_config() -> Result<ConversationIngestionRuntimeConfig> {
    ConversationIngestionRuntimeConfig::load_for_current_dir()
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

pub(super) async fn handle_query(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    let config = load_conversation_config()?;
    match name {
        "mission_token_stats" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                session_id: Option<String>,
                slot_id: Option<String>,
                since: Option<String>,
                group_by: Option<String>,
            }
            let Args {
                session_id,
                slot_id,
                since,
                group_by,
            } = serde_json::from_value(args).unwrap_or(Args {
                session_id: None,
                slot_id: None,
                since: None,
                group_by: None,
            });
            let rows = state
                .store
                .token_stats(
                    session_id.as_deref(),
                    slot_id.as_deref(),
                    since.as_deref(),
                    group_by.as_deref(),
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&rows))
        }

        // ===== Conversation Logs =====
        "mission_conversation_list" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                status: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
                #[serde(alias = "conversation_type")]
                conversation_type: Option<String>,
                #[serde(alias = "task_id")]
                task_id: Option<String>,
                since: Option<String>,
                until: Option<String>,
                source: Option<String>,
                project: Option<String>,
            }
            let Args {
                status,
                limit,
                conversation_type,
                task_id,
                since,
                until,
                source,
                project,
            } = serde_json::from_value(args).unwrap_or(Args {
                status: None,
                limit: None,
                conversation_type: None,
                task_id: None,
                since: None,
                until: None,
                source: None,
                project: None,
            });

            let mut query = missiond_core::db::conversation_query::ConversationQuery::new();
            if let Some(s) = status {
                query = query.status(s);
            }
            if let Some(l) = limit {
                query = query.limit(l);
            }
            if let Some(ct) = conversation_type {
                query = query.conv_type(
                    missiond_core::db::conversation_query::ConversationTypeFilter::from_str(&ct),
                );
            }
            if let Some(tid) = task_id {
                query = query.task_id(tid);
            }
            if let Some(s) = since {
                query = query.since(s);
            }
            if let Some(u) = until {
                query = query.until(u);
            }
            if let Some(src) = source {
                query = query.source(src);
            }
            if let Some(proj) = project {
                query = query.project(proj);
            }

            let manager = missiond_core::services::ConversationManager::new(std::sync::Arc::clone(
                &state.store,
            ));
            let convs = manager
                .list(query)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // Batch-fetch conversation labels (star, etc.) and embed in response
            let conv_ids: Vec<&str> = convs.iter().map(|c| c.id.as_str()).collect();
            let labels_map = state
                .store
                .conversation_label_get_batch(&conv_ids)
                .await
                .unwrap_or_default();

            if labels_map.is_empty() {
                Ok(ToolResult::json_pretty(&convs))
            } else {
                // Serialize convs with labels embedded
                let mut arr: Vec<serde_json::Value> = serde_json::to_value(&convs)
                    .unwrap_or_default()
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for item in &mut arr {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        if let Some(labels) = labels_map.get(id) {
                            item.as_object_mut().map(|obj| {
                                obj.insert("labels".to_string(), serde_json::json!(labels));
                            });
                        }
                    }
                }
                Ok(ToolResult::json_pretty(&arr))
            }
        }

        "mission_conversation_get" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: String,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                tail: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64", alias = "since_id")]
                since_id: Option<i64>,
                #[serde(
                    default,
                    deserialize_with = "lenient::option_bool",
                    alias = "include_raw"
                )]
                include_raw: Option<bool>,
                #[serde(
                    default,
                    deserialize_with = "lenient::option_bool",
                    alias = "include_labels"
                )]
                include_labels: Option<bool>,
                #[serde(
                    default,
                    deserialize_with = "lenient::option_bool",
                    alias = "include_user_index"
                )]
                include_user_index: Option<bool>,
                #[serde(
                    default,
                    deserialize_with = "lenient::option_bool",
                    alias = "include_turns"
                )]
                include_turns: Option<bool>,
            }
            let Args {
                session_id,
                tail,
                since_id,
                include_raw,
                include_labels,
                include_user_index,
                include_turns,
            } = serde_json::from_value(args)?;
            let conv = state
                .store
                .get_conversation(&session_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let slot_id_for_display = conv.as_ref().and_then(|c| c.slot_id.clone());
            let is_jarvis = conv.as_ref().map(|c| c.conversation_type.as_str()) == Some("jarvis");

            let msgs = state
                .store
                .get_conversation_messages(
                    &session_id,
                    since_id,
                    tail.unwrap_or(config.conversation_get_tail_default),
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let messages: Vec<serde_json::Value> = if include_raw.unwrap_or(false) {
                // Full messages for frontend (includes rawContent/model/metadata for image rendering)
                msgs.iter()
                    .map(|m| {
                        let mut role_display = m.role_display.clone();
                        if (m.role == "system"
                            || m.role == "worker_user"
                            || m.role == "agent_user"
                            || (m.role == "user" && !is_jarvis))
                            && slot_id_for_display.is_some()
                        {
                            role_display = slot_id_for_display.clone();
                        }
                        serde_json::json!({
                            "id": m.id,
                            "seq": m.seq,
                            "sessionId": m.session_id,
                            "role": m.role,
                            "rawRole": m.raw_role,
                            "roleDisplay": role_display,
                            "content": m.content,
                            "rawContent": m.raw_content,
                            "messageUuid": m.message_uuid,
                            "parentUuid": m.parent_uuid,
                            "model": m.model,
                            "timestamp": m.timestamp,
                            "metadata": m.metadata,
                            "toolName": m.tool_name,
                        })
                    })
                    .collect()
            } else {
                // Lite messages for LLM consumption (strip base64 images to protect context)
                msgs.iter()
                    .map(|m| {
                        let mut role_display = m.role_display.clone();
                        if (m.role == "system"
                            || m.role == "worker_user"
                            || m.role == "agent_user"
                            || (m.role == "user" && !is_jarvis))
                            && slot_id_for_display.is_some()
                        {
                            role_display = slot_id_for_display.clone();
                        }
                        serde_json::json!({
                            "id": m.id,
                            "seq": m.seq,
                            "role": m.role,
                            "rawRole": m.raw_role,
                            "roleDisplay": role_display,
                            "content": m.content,
                            "timestamp": m.timestamp,
                            "messageUuid": m.message_uuid,
                            "parentUuid": m.parent_uuid,
                        })
                    })
                    .collect()
            };
            // Fetch labels if requested
            let labels_map = if include_labels.unwrap_or(false) && !msgs.is_empty() {
                let msg_ids: Vec<i64> = msgs.iter().map(|m| m.id).collect();
                state
                    .store
                    .label_get_batch(&msg_ids)
                    .await
                    .unwrap_or_default()
            } else {
                Default::default()
            };
            // Include child (subagent) conversations summary
            let children = state
                .store
                .get_child_conversations(&session_id)
                .await
                .unwrap_or_default();
            let mut result = serde_json::json!({
                "conversation": conv,
                "messages": messages,
                "count": messages.len(),
            });
            if !labels_map.is_empty() {
                // Convert HashMap<i64, Vec<(String, String)>> → JSON { "123": [["label", "value"], ...] }
                let labels_json: serde_json::Map<String, Value> = labels_map
                    .into_iter()
                    .map(|(id, pairs)| {
                        let arr: Vec<Value> = pairs
                            .into_iter()
                            .map(|(l, v)| serde_json::json!([l, v]))
                            .collect();
                        (id.to_string(), Value::Array(arr))
                    })
                    .collect();
                result["labels"] = Value::Object(labels_json);
            }
            if !children.is_empty() {
                let child_summaries: Vec<serde_json::Value> = children
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "id": c.id,
                            "messageCount": c.message_count,
                            "status": c.status,
                            "startedAt": c.started_at,
                        })
                    })
                    .collect();
                result["subagents"] = serde_json::json!(child_summaries);
            }
            // Embed lightweight user message index for minimap sidebar
            if include_user_index.unwrap_or(false) {
                let user_msgs = state
                    .store
                    .get_user_message_index(&session_id)
                    .await
                    .unwrap_or_default();
                let items: Vec<Value> = user_msgs.iter()
                    .map(|(id, ts, preview)| serde_json::json!({ "id": id, "time": ts, "preview": preview }))
                    .collect();
                result["userIndex"] = serde_json::json!(items);
            }
            // Embed conversation turns for turn splitter visualization
            if include_turns.unwrap_or(false) {
                let turns = state
                    .store
                    .get_turns_after(&session_id, -1)
                    .await
                    .unwrap_or_default();
                let items: Vec<Value> = turns
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "turnIdx": t.turn_idx,
                            "startMessageId": t.start_message_id,
                            "endMessageId": t.end_message_id,
                            "userContent": t.user_content,
                            "toolNames": t.tool_names,
                            "toolCallCount": t.tool_call_count,
                            "messageCount": t.message_count,
                            "hasCodeChange": t.has_code_change,
                            "hasMcpCall": t.has_mcp_call,
                            "startedAt": t.started_at,
                            "endedAt": t.ended_at,
                            "topic": t.topic,
                        })
                    })
                    .collect();
                result["turns"] = serde_json::json!(items);
            }
            Ok(ToolResult::json(&result))
        }

        "mission_conversation_analysis_context" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: String,
                #[serde(default, deserialize_with = "lenient::option_i64", alias = "max_turns")]
                max_turns: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64", alias = "max_chars")]
                max_chars: Option<i64>,
            }
            let Args {
                session_id,
                max_turns,
                max_chars,
            } = serde_json::from_value(args)?;
            let max_turns = max_turns.unwrap_or(30).clamp(1, 100);
            let max_chars = max_chars.unwrap_or(240).clamp(80, 1000) as usize;
            let conv = state
                .store
                .get_conversation(&session_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let msgs = state
                .store
                .get_conversation_messages(&session_id, None, (max_turns * 8).max(50))
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let turns = state
                .store
                .get_turns_after(&session_id, -1)
                .await
                .unwrap_or_default();
            let conversation_type = conv
                .as_ref()
                .map(|c| c.conversation_type.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let is_human_read_model = matches!(
                conversation_type.as_str(),
                "user" | "jarvis" | "codex_chat" | "gemini_chat"
            );

            let mut role_counts = serde_json::Map::new();
            let mut tool_counts = serde_json::Map::new();
            let mut user_utterances = Vec::new();
            let mut assistant_samples = Vec::new();
            let mut worker_or_meta_messages = 0usize;

            for msg in &msgs {
                let role_count = role_counts
                    .entry(msg.role.clone())
                    .or_insert_with(|| serde_json::json!(0));
                *role_count = serde_json::json!(role_count.as_i64().unwrap_or(0) + 1);

                if let Some(tool_name) = msg.tool_name.as_deref() {
                    for tool in tool_name
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        let entry = tool_counts
                            .entry(tool.to_string())
                            .or_insert_with(|| serde_json::json!(0));
                        *entry = serde_json::json!(entry.as_i64().unwrap_or(0) + 1);
                    }
                }

                if msg.role == "user" && is_human_read_model {
                    user_utterances.push(serde_json::json!({
                        "id": msg.id,
                        "timestamp": msg.timestamp,
                        "content": compact_preview(&msg.content, max_chars),
                    }));
                } else if msg.role == "assistant" && assistant_samples.len() < 12 {
                    assistant_samples.push(serde_json::json!({
                        "id": msg.id,
                        "timestamp": msg.timestamp,
                        "content": compact_preview(&msg.content, max_chars),
                    }));
                } else if msg.role == "worker_user"
                    || msg.role == "agent_user"
                    || msg.role == "system"
                    || !is_human_read_model
                {
                    worker_or_meta_messages += 1;
                }
            }

            let mut turn_items = Vec::new();
            for turn in turns.iter().rev().take(max_turns as usize).rev() {
                turn_items.push(serde_json::json!({
                    "turnIdx": turn.turn_idx,
                    "startMessageId": turn.start_message_id,
                    "endMessageId": turn.end_message_id,
                    "userContent": turn.user_content.as_deref().map(|s| compact_preview(s, max_chars)),
                    "toolNames": turn.tool_names,
                    "toolCallCount": turn.tool_call_count,
                    "messageCount": turn.message_count,
                    "topic": turn.topic,
                    "startedAt": turn.started_at,
                    "endedAt": turn.ended_at,
                }));
            }

            Ok(ToolResult::json(&serde_json::json!({
                "schema": "missiond.conversation.analysis_context.v1",
                "sessionId": session_id,
                "conversation": conv,
                "conversationType": conversation_type,
                "isHumanReadModel": is_human_read_model,
                "roleCounts": role_counts,
                "toolCounts": tool_counts,
                "userUtterances": user_utterances,
                "assistantSamples": assistant_samples,
                "workerOrMetaMessageCount": worker_or_meta_messages,
                "turns": turn_items,
                "limits": { "maxTurns": max_turns, "maxChars": max_chars },
                "policy": "bounded read model; worker/provider chatter is counted but not used as user intent",
            })))
        }

        "mission_conversation_search" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                query: String,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                offset: Option<i64>,
                #[serde(alias = "session_id")]
                session_id: Option<String>,
                #[serde(alias = "exclude_session_id")]
                exclude_session_id: Option<String>,
                /// hybrid (default), fts, semantic
                #[serde(alias = "query_mode")]
                query_mode: Option<String>,
                /// last_24h, last_7d, last_30d
                #[serde(alias = "time_range")]
                time_range: Option<String>,
                project: Option<String>,
                /// Filter by conversation_type (e.g. "gemini_chat")
                #[serde(alias = "conversation_type")]
                conversation_type: Option<String>,
            }
            let Args {
                query,
                limit,
                offset,
                session_id,
                exclude_session_id,
                query_mode,
                time_range,
                project,
                conversation_type,
            } = serde_json::from_value(args)?;
            let top_k = limit.unwrap_or(config.conversation_search_default_limit) as usize;
            let skip = offset.unwrap_or(0) as usize;
            let mode = query_mode.as_deref().unwrap_or("hybrid");

            // Map conversationType shorthand to actual DB values (comma-separated for IN clause)
            let conversation_type = conversation_type.map(|ct| match ct.as_str() {
                "gemini" => "gemini_chat,router_chat".to_string(),
                "system" => "meta,worker".to_string(),
                "user" | "jarvis" | "all" => ct,
                _ => ct,
            });

            // Resolve timeRange to ISO timestamp
            let time_after: Option<String> = time_range.as_deref().and_then(|tr| {
                let hours = match tr {
                    "last_24h" => 24,
                    "last_7d" => 24 * 7,
                    "last_30d" => 24 * 30,
                    _ => return None,
                };
                Some(
                    chrono::Utc::now()
                        .checked_sub_signed(chrono::Duration::hours(hours))?
                        .to_rfc3339(),
                )
            });

            // ── Path 0: ID/metadata fast path ──
            // If query looks like a UUID prefix or short hex ID, search conversation metadata directly
            let is_id_query = query.len() >= 4
                && query.len() <= 36
                && query.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
            if is_id_query {
                let meta_results = state
                    .store
                    .search_conversations_by_metadata(
                        &query,
                        top_k as i64,
                        conversation_type.as_deref(),
                    )
                    .await
                    .unwrap_or_default();
                if !meta_results.is_empty() {
                    let mut results = Vec::new();
                    for (sid, _score) in &meta_results {
                        let conv = state.store.get_conversation(sid).await.ok().flatten();
                        results.push(serde_json::json!({
                            "sessionId": sid,
                            "project": conv.as_ref().and_then(|c| c.project.as_deref()),
                            "status": conv.as_ref().map(|c| c.status.as_str()),
                            "slotId": conv.as_ref().and_then(|c| c.slot_id.as_deref()),
                            "summary": conv.as_ref().and_then(|c| c.llm_summary.as_deref()),
                            "messageCount": conv.as_ref().map(|c| c.message_count),
                            "startedAt": conv.as_ref().map(|c| &c.started_at),
                            "matchReason": format!("[ID匹配] {}", sid),
                            "source": conv.as_ref().map(|c| c.source.as_str()),
                        }));
                    }
                    return Ok(ToolResult::json(&serde_json::json!({
                        "results": results,
                        "count": results.len(),
                        "totalHits": results.len(),
                        "query": query,
                        "mode": "metadata",
                    })));
                }
            }

            // ── Path A: single-session search — semantic + FTS hybrid ──
            if let Some(ref sid) = session_id {
                let lim = top_k as i64;
                // Try pgvector semantic search within session first
                let query_embedding = state
                    .embedding_service
                    .as_ref()
                    .and_then(|svc| svc.embed(&query));
                let semantic_results = if let Some(ref qe) = query_embedding {
                    state
                        .store
                        .session_semantic_search(qe, sid, lim)
                        .await
                        .unwrap_or_default()
                } else {
                    vec![]
                };

                if !semantic_results.is_empty() {
                    // Return semantic results (exact cosine, no HNSW loss)
                    let msgs_lite: Vec<serde_json::Value> = semantic_results
                        .iter()
                        .map(|(id, role, content, ts, sim)| {
                            serde_json::json!({
                                "id": id, "sessionId": sid,
                                "role": role, "content": content, "timestamp": ts,
                                "similarity": sim,
                            })
                        })
                        .collect();
                    return Ok(ToolResult::json(&serde_json::json!({
                        "results": msgs_lite, "count": msgs_lite.len(), "query": query,
                        "mode": "single_session_semantic",
                    })));
                }

                // Fallback: FTS-only
                let msgs = state
                    .store
                    .search_messages_filtered(
                        &query,
                        Some(sid),
                        None,
                        None,
                        time_after.as_deref(),
                        lim,
                    )
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let msgs_lite: Vec<serde_json::Value> = msgs
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "id": m.id, "sessionId": m.session_id,
                            "role": m.role, "content": m.content, "timestamp": m.timestamp,
                            "toolName": m.tool_name,
                        })
                    })
                    .collect();
                return Ok(ToolResult::json(&serde_json::json!({
                    "results": msgs_lite, "count": msgs_lite.len(), "query": query,
                    "mode": "single_session_fts",
                })));
            }

            // ── Path B: session-level search ──

            // 1. FTS path (for hybrid + fts modes)
            let fts_ranked: Vec<(String, usize)> = if mode != "semantic" {
                let pool_limit = ((top_k + skip) * 3) as i64;
                let fts_sessions = state
                    .store
                    .search_conversation_sessions_fts_filtered(
                        &query,
                        pool_limit,
                        time_after.as_deref(),
                        project.as_deref(),
                        conversation_type.as_deref(),
                    )
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                fts_sessions
                    .into_iter()
                    .enumerate()
                    .map(|(rank, (sid, _score))| (sid, rank))
                    .collect()
            } else {
                Vec::new()
            };

            // 2. Vector path: pgvector HNSW on topic vectors (replaces in-memory TopicCache)
            let vec_ranked: Vec<(String, usize, f32)> = if mode != "fts" {
                let query_embedding = state
                    .embedding_service
                    .as_ref()
                    .and_then(|svc| svc.embed(&query));
                if let Some(ref qe) = query_embedding {
                    let db_results = state
                        .store
                        .semantic_conversation_search(qe, ((top_k + skip) * 3) as i64)
                        .await
                        .unwrap_or_default();
                    db_results
                        .into_iter()
                        .enumerate()
                        .map(|(rank, (sid, sim))| (sid, rank, sim as f32))
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // 3. Merge / rank
            let rrf_k = 60;
            let mut session_scores: std::collections::HashMap<
                String,
                (Option<usize>, Option<usize>, Option<f32>),
            > = std::collections::HashMap::new();
            for (sid, rank) in &fts_ranked {
                session_scores
                    .entry(sid.clone())
                    .or_insert((None, None, None))
                    .0 = Some(*rank);
            }
            for (sid, rank, sim) in &vec_ranked {
                let entry = session_scores
                    .entry(sid.clone())
                    .or_insert((None, None, None));
                entry.1 = Some(*rank);
                entry.2 = Some(*sim);
            }

            let mut ranked: Vec<(String, f64, Option<usize>, Option<usize>, Option<f32>)> =
                session_scores
                    .into_iter()
                    .map(|(sid, (fts_r, vec_r, sim))| {
                        let score = match mode {
                            "fts" => fts_r.map(|r| 1.0 / (rrf_k + r + 1) as f64).unwrap_or(0.0),
                            "semantic" => {
                                vec_r.map(|r| 1.0 / (rrf_k + r + 1) as f64).unwrap_or(0.0)
                            }
                            _ => missiond_core::embedding::rrf_score(fts_r, vec_r, rrf_k),
                        };
                        (sid, score, fts_r, vec_r, sim)
                    })
                    .collect();
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some(ref ex_sid) = exclude_session_id {
                ranked.retain(|(sid, _, _, _, _)| sid != ex_sid);
            }

            // Apply offset + limit
            let total_hits = ranked.len();
            if skip > 0 {
                ranked = ranked.into_iter().skip(skip).collect();
            }
            ranked.truncate(top_k);

            // 4. Enrich with snippets (Postgres FTS native) or llmSummary fallback
            let mut results = Vec::new();
            for (sid, _rrf, fts_r, _vec_r, sim) in &ranked {
                let conv = state.store.get_conversation(sid).await.ok().flatten();

                // Build matchReason: FTS snippet if keyword-matched, llmSummary if vector-only
                let match_reason = if fts_r.is_some() {
                    // FTS hit: get native snippet text from the store.
                    let snippets = state
                        .store
                        .get_session_fts_snippets(sid, &query, 3)
                        .await
                        .unwrap_or_default();
                    if snippets.is_empty() {
                        conv.as_ref()
                            .and_then(|c| c.llm_summary.as_deref())
                            .unwrap_or("(无摘要)")
                            .to_string()
                    } else {
                        snippets
                            .iter()
                            .map(|(role, snip)| format!("[{}] {}", role, snip))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                } else {
                    // Vector-only hit — use llmSummary as context
                    format!(
                        "[语义匹配] {}",
                        conv.as_ref()
                            .and_then(|c| c.llm_summary.as_deref())
                            .unwrap_or("(无摘要)")
                    )
                };

                results.push(serde_json::json!({
                    "sessionId": sid,
                    "project": conv.as_ref().and_then(|c| c.project.as_deref()),
                    "status": conv.as_ref().map(|c| c.status.as_str()),
                    "slotId": conv.as_ref().and_then(|c| c.slot_id.as_deref()),
                    "summary": conv.as_ref().and_then(|c| c.llm_summary.as_deref()),
                    "messageCount": conv.as_ref().map(|c| c.message_count),
                    "startedAt": conv.as_ref().map(|c| &c.started_at),
                    "matchReason": match_reason,
                    "cosineSim": sim,
                }));
            }

            Ok(ToolResult::json(&serde_json::json!({
                "results": results,
                "count": results.len(),
                "totalHits": total_hits,
                "query": query,
                "mode": mode,
                "ftsHits": fts_ranked.len(),
                "vecHits": vec_ranked.len(),
            })))
        }

        "mission_message_search" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                query: String,
                #[serde(alias = "session_id")]
                session_id: Option<String>,
                role: Option<String>,
                #[serde(alias = "tool_name")]
                tool_name: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
                #[serde(alias = "time_range")]
                time_range: Option<String>,
            }
            let Args {
                query,
                session_id,
                role,
                tool_name,
                limit,
                time_range,
            } = serde_json::from_value(args)?;
            let lim = limit.unwrap_or(config.message_search_default_limit);

            let time_after: Option<String> = time_range.as_deref().and_then(|tr| {
                let hours = match tr {
                    "last_24h" => 24,
                    "last_7d" => 24 * 7,
                    "last_30d" => 24 * 30,
                    _ => return None,
                };
                Some(
                    chrono::Utc::now()
                        .checked_sub_signed(chrono::Duration::hours(hours))?
                        .to_rfc3339(),
                )
            });

            // Try hybrid search (pgvector + FTS) when embeddings available AND no extra filters.
            // Hybrid CTE doesn't support role/tool_name/time_after filters; fall through to FTS if set.
            let has_extra_filters = role.is_some() || tool_name.is_some() || time_after.is_some();
            let query_embedding = if !has_extra_filters {
                state
                    .embedding_service
                    .as_ref()
                    .and_then(|svc| svc.embed(&query))
            } else {
                None
            };

            if let Some(ref qe) = query_embedding {
                let hybrid = state
                    .store
                    .hybrid_message_search(qe, &query, session_id.as_deref(), lim)
                    .await
                    .unwrap_or_default();

                if !hybrid.is_empty() {
                    let results: Vec<serde_json::Value> = hybrid
                        .iter()
                        .map(|(id, sid, r, content, ts, score)| {
                            serde_json::json!({
                                "id": id, "sessionId": sid, "role": r,
                                "content": content, "timestamp": ts, "rrfScore": score,
                            })
                        })
                        .collect();
                    return Ok(ToolResult::json(&serde_json::json!({
                        "results": results, "count": results.len(), "query": query,
                        "mode": "hybrid",
                        "filters": { "sessionId": session_id },
                    })));
                }
            }

            // Fallback: FTS-only
            let msgs = state
                .store
                .search_messages_filtered(
                    &query,
                    session_id.as_deref(),
                    role.as_deref(),
                    tool_name.as_deref(),
                    time_after.as_deref(),
                    lim,
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let results: Vec<serde_json::Value> = msgs
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "sessionId": m.session_id,
                        "role": m.role,
                        "content": m.content,
                        "timestamp": m.timestamp,
                        "toolName": m.tool_name,
                    })
                })
                .collect();

            Ok(ToolResult::json(&serde_json::json!({
                "results": results,
                "count": results.len(),
                "query": query,
                "mode": "fts",
                "filters": {
                    "sessionId": session_id,
                    "role": role,
                    "toolName": tool_name,
                    "timeRange": time_range,
                },
            })))
        }

        "mission_user_message_index" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: String,
            }
            let Args { session_id } = serde_json::from_value(args)?;
            let rows = state
                .store
                .get_user_message_index(&session_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|(id, ts, preview)| {
                    serde_json::json!({ "id": id, "time": ts, "preview": preview })
                })
                .collect();
            Ok(ToolResult::json(&serde_json::json!({
                "items": items,
                "count": items.len(),
            })))
        }

        "mission_conversation_set_label" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: String,
                label: String,
                #[serde(default)]
                value: Option<String>,
                #[serde(default)]
                source: Option<String>,
            }
            let Args {
                session_id,
                label,
                value,
                source,
            } = serde_json::from_value(args)?;
            state
                .store
                .conversation_label_set(
                    &session_id,
                    &label,
                    value.as_deref().unwrap_or("1"),
                    source.as_deref().unwrap_or("user"),
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(
                &serde_json::json!({ "ok": true, "sessionId": session_id, "label": label }),
            ))
        }

        "mission_conversation_delete_label" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: String,
                label: String,
            }
            let Args { session_id, label } = serde_json::from_value(args)?;
            state
                .store
                .conversation_label_delete(&session_id, &label)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(
                &serde_json::json!({ "ok": true, "sessionId": session_id, "label": label }),
            ))
        }

        "mission_context_around" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(
                    default,
                    deserialize_with = "lenient::option_i64",
                    alias = "message_id"
                )]
                message_id: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                before: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                after: Option<i64>,
            }
            let Args {
                message_id,
                before,
                after,
            } = serde_json::from_value(args)?;
            let message_id = message_id.ok_or_else(|| anyhow!("messageId is required"))?;

            // Defensive limits
            let before = before.unwrap_or(config.context_before_default).min(50);
            let after = after.unwrap_or(config.context_after_default).min(50);

            let result = state
                .store
                .get_messages_around(message_id, before, after)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            match result {
                None => Ok(ToolResult::json(&serde_json::json!({
                    "error": "message not found",
                    "messageId": message_id,
                }))),
                Some((session_id, msgs)) => {
                    let anchor_index = msgs.iter().position(|m| m.id == message_id);
                    let total: Option<i64> = state
                        .store
                        .get_conversation(&session_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|c| c.message_count);

                    let messages: Vec<serde_json::Value> = msgs
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "id": m.id,
                                "role": m.role,
                                "content": m.content,
                                "timestamp": m.timestamp,
                                "toolName": m.tool_name,
                            })
                        })
                        .collect();

                    Ok(ToolResult::json(&serde_json::json!({
                        "anchor": { "id": message_id, "index": anchor_index },
                        "sessionId": session_id,
                        "messages": messages,
                        "count": messages.len(),
                        "totalInSession": total,
                    })))
                }
            }
        }
        _ => Err(anyhow!("Unknown conversation query tool: {name}")),
    }
}
