use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::lenient;
use crate::state::EmbeddingTask;


pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Unified entry point: mission_conversation_query =====
        // Routes action param → legacy individual handlers
        "mission_conversation_query" => {
            let action = args.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("list");
            let mapped_name = match action {
                "list" => "mission_conversation_list",
                "get" => "mission_conversation_get",
                "search" => "mission_conversation_search",
                "message_search" => "mission_message_search",
                "context" => "mission_context_around",
                "events" => "mission_conversation_events",
                other => return Err(anyhow!("Unknown conversation action: {other}")),
            };
            // Forward with same args (action field is harmlessly ignored by sub-handlers)
            Box::pin(handle(state, mapped_name, args)).await
        }

        // ===== Token Stats =====
        "mission_token_stats" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                session_id: Option<String>,
                slot_id: Option<String>,
                since: Option<String>,
                group_by: Option<String>,
            }
            let Args { session_id, slot_id, since, group_by } =
                serde_json::from_value(args).unwrap_or(Args {
                    session_id: None, slot_id: None, since: None, group_by: None,
                });
            let rows = state.store.token_stats(
                session_id.as_deref(),
                slot_id.as_deref(),
                since.as_deref(),
                group_by.as_deref(),
            ).await.map_err(|e| anyhow!("DB error: {}", e))?;
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
            }
            let Args { status, limit, conversation_type, task_id, since, until, source } =
                serde_json::from_value(args).unwrap_or(Args { status: None, limit: None, conversation_type: None, task_id: None, since: None, until: None, source: None });
            let convs = state.store
                .list_conversations(status.as_deref(), limit.unwrap_or(20), conversation_type.as_deref(), task_id.as_deref(), since.as_deref(), until.as_deref(), source.as_deref()).await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&convs))
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
                #[serde(default, deserialize_with = "lenient::option_bool", alias = "include_raw")]
                include_raw: Option<bool>,
            }
            let Args { session_id, tail, since_id, include_raw } = serde_json::from_value(args)?;
            let conv = state.store.get_conversation(&session_id).await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let msgs = state.store.get_conversation_messages(&session_id, since_id, tail.unwrap_or(50)).await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let messages: Vec<serde_json::Value> = if include_raw.unwrap_or(false) {
                // Full messages for frontend (includes rawContent/model/metadata for image rendering)
                msgs.iter().map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "seq": m.seq,
                        "sessionId": m.session_id,
                        "role": m.role,
                        "roleDisplay": m.role_display,
                        "content": m.content,
                        "rawContent": m.raw_content,
                        "messageUuid": m.message_uuid,
                        "parentUuid": m.parent_uuid,
                        "model": m.model,
                        "timestamp": m.timestamp,
                        "metadata": m.metadata,
                    })
                }).collect()
            } else {
                // Lite messages for LLM consumption (strip base64 images to protect context)
                msgs.iter().map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "seq": m.seq,
                        "role": m.role,
                        "roleDisplay": m.role_display,
                        "content": m.content,
                        "timestamp": m.timestamp,
                        "messageUuid": m.message_uuid,
                        "parentUuid": m.parent_uuid,
                    })
                }).collect()
            };
            // Include child (subagent) conversations summary
            let children = state.store.get_child_conversations(&session_id).await
                .unwrap_or_default();
            let mut result = serde_json::json!({
                "conversation": conv,
                "messages": messages,
                "count": messages.len(),
            });
            if !children.is_empty() {
                let child_summaries: Vec<serde_json::Value> = children.iter().map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "messageCount": c.message_count,
                        "status": c.status,
                        "startedAt": c.started_at,
                    })
                }).collect();
                result["subagents"] = serde_json::json!(child_summaries);
            }
            Ok(ToolResult::json(&result))
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
            }
            let Args { query, limit, offset, session_id, exclude_session_id,
                        query_mode, time_range, project } = serde_json::from_value(args)?;
            let top_k = limit.unwrap_or(10) as usize;
            let skip = offset.unwrap_or(0) as usize;
            let mode = query_mode.as_deref().unwrap_or("hybrid");

            // Resolve timeRange to ISO timestamp
            let time_after: Option<String> = time_range.as_deref().and_then(|tr| {
                let hours = match tr {
                    "last_24h" => 24,
                    "last_7d" => 24 * 7,
                    "last_30d" => 24 * 30,
                    _ => return None,
                };
                Some(chrono::Utc::now()
                    .checked_sub_signed(chrono::Duration::hours(hours))?
                    .to_rfc3339())
            });

            // ── Path A: single-session message search (SQL-pushed filter) ──
            if let Some(ref sid) = session_id {
                let lim = top_k as i64;
                let msgs = state.store.search_messages_filtered(&query, Some(sid), None, None, time_after.as_deref(), lim).await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let msgs_lite: Vec<serde_json::Value> = msgs.iter().map(|m| {
                    serde_json::json!({
                        "id": m.id, "sessionId": m.session_id,
                        "role": m.role, "content": m.content, "timestamp": m.timestamp,
                        "toolName": m.tool_name,
                    })
                }).collect();
                return Ok(ToolResult::json(&serde_json::json!({
                    "results": msgs_lite, "count": msgs_lite.len(), "query": query,
                    "mode": "single_session",
                })));
            }

            // ── Path B: session-level search ──

            // 1. FTS5 path (for hybrid + fts modes)
            let fts_ranked: Vec<(String, usize)> = if mode != "semantic" {
                let pool_limit = ((top_k + skip) * 3) as i64;
                let fts_sessions = state.store.search_conversation_sessions_fts_filtered(
                    &query, pool_limit, time_after.as_deref(), project.as_deref(),
                ).await.map_err(|e| anyhow!("DB error: {}", e))?;
                fts_sessions.into_iter()
                    .enumerate()
                    .map(|(rank, (sid, _score))| (sid, rank))
                    .collect()
            } else {
                Vec::new()
            };

            // 2. Vector path: MaxSim over multi-topic cache (for hybrid + semantic modes)
            let vec_ranked: Vec<(String, usize, f32)> = if mode != "fts" {
                let query_embedding = state.embedding_service.as_ref()
                    .and_then(|svc| svc.embed(&query));
                let cache = state.conversation_topic_cache.read().await;
                let result = if let Some(ref qe) = query_embedding {
                    missiond_core::embedding::maxsim_search(qe, &cache, (top_k + skip) * 3)
                } else {
                    Vec::new()
                };
                drop(cache);
                result
            } else {
                Vec::new()
            };

            // 3. Merge / rank
            let rrf_k = 60;
            let mut session_scores: std::collections::HashMap<String, (Option<usize>, Option<usize>, Option<f32>)> =
                std::collections::HashMap::new();
            for (sid, rank) in &fts_ranked {
                session_scores.entry(sid.clone()).or_insert((None, None, None)).0 = Some(*rank);
            }
            for (sid, rank, sim) in &vec_ranked {
                let entry = session_scores.entry(sid.clone()).or_insert((None, None, None));
                entry.1 = Some(*rank);
                entry.2 = Some(*sim);
            }

            let mut ranked: Vec<(String, f64, Option<usize>, Option<usize>, Option<f32>)> = session_scores
                .into_iter()
                .map(|(sid, (fts_r, vec_r, sim))| {
                    let score = match mode {
                        "fts" => fts_r.map(|r| 1.0 / (rrf_k + r + 1) as f64).unwrap_or(0.0),
                        "semantic" => vec_r.map(|r| 1.0 / (rrf_k + r + 1) as f64).unwrap_or(0.0),
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

            // 4. Enrich with snippets (FTS5 native) or llmSummary fallback
            let mut results = Vec::new();
            for (sid, _rrf, fts_r, _vec_r, sim) in &ranked {
                let conv = state.store.get_conversation(sid).await.ok().flatten();

                // Build matchReason: FTS snippet if keyword-matched, llmSummary if vector-only
                let match_reason = if fts_r.is_some() {
                    // FTS hit — get native snippet() from SQLite
                    let snippets = state.store.get_session_fts_snippets(sid, &query, 3).await
                        .unwrap_or_default();
                    if snippets.is_empty() {
                        conv.as_ref()
                            .and_then(|c| c.llm_summary.as_deref())
                            .unwrap_or("(无摘要)")
                            .to_string()
                    } else {
                        snippets.iter()
                            .map(|(role, snip)| format!("[{}] {}", role, snip))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                } else {
                    // Vector-only hit — use llmSummary as context
                    format!("[语义匹配] {}",
                        conv.as_ref()
                            .and_then(|c| c.llm_summary.as_deref())
                            .unwrap_or("(无摘要)"))
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
            let Args { query, session_id, role, tool_name, limit, time_range } =
                serde_json::from_value(args)?;
            let lim = limit.unwrap_or(20);

            let time_after: Option<String> = time_range.as_deref().and_then(|tr| {
                let hours = match tr {
                    "last_24h" => 24,
                    "last_7d" => 24 * 7,
                    "last_30d" => 24 * 30,
                    _ => return None,
                };
                Some(chrono::Utc::now()
                    .checked_sub_signed(chrono::Duration::hours(hours))?
                    .to_rfc3339())
            });

            let msgs = state.store.search_messages_filtered(
                &query,
                session_id.as_deref(),
                role.as_deref(),
                tool_name.as_deref(),
                time_after.as_deref(),
                lim,
            ).await.map_err(|e| anyhow!("DB error: {}", e))?;

            let results: Vec<serde_json::Value> = msgs.iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "sessionId": m.session_id,
                    "role": m.role,
                    "content": m.content,
                    "timestamp": m.timestamp,
                    "toolName": m.tool_name,
                })
            }).collect();

            Ok(ToolResult::json(&serde_json::json!({
                "results": results,
                "count": results.len(),
                "query": query,
                "filters": {
                    "sessionId": session_id,
                    "role": role,
                    "toolName": tool_name,
                    "timeRange": time_range,
                },
            })))
        }

        "mission_context_around" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(default, deserialize_with = "lenient::option_i64", alias = "message_id")]
                message_id: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                before: Option<i64>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                after: Option<i64>,
            }
            let Args { message_id, before, after } = serde_json::from_value(args)?;
            let message_id = message_id.ok_or_else(|| anyhow!("messageId is required"))?;

            // Defensive limits
            let before = before.unwrap_or(3).min(50);
            let after = after.unwrap_or(5).min(50);

            let result = state.store.get_messages_around(message_id, before, after).await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            match result {
                None => Ok(ToolResult::json(&serde_json::json!({
                    "error": "message not found",
                    "messageId": message_id,
                }))),
                Some((session_id, msgs)) => {
                    let anchor_index = msgs.iter().position(|m| m.id == message_id);
                    let total: Option<i64> = state.store.get_conversation(&session_id).await
                        .ok().flatten().map(|c| c.message_count);

                    let messages: Vec<serde_json::Value> = msgs.iter().map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "role": m.role,
                            "content": m.content,
                            "timestamp": m.timestamp,
                            "toolName": m.tool_name,
                        })
                    }).collect();

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

        "mission_trigger_backfill" => {
            let provider = state.embedding_service.as_ref()
                .map(|svc| svc.provider_id().to_string())
                .unwrap_or_else(|| "none".to_string());

            // Gather stats for all three systems
            let conv_missing = state.store.conversations_missing_summary(9999).await.map(|v| v.len()).unwrap_or(0);
            let conv_stale = if let Some(svc) = state.embedding_service.as_ref() {
                state.store.conversations_stale_embedding(svc.provider_id(), 9999).await.map(|v| v.len()).unwrap_or(0)
            } else { 0 };
            let kb_missing = state.store.kb_entries_missing_embedding(None).await.map(|v| v.len()).unwrap_or(0);
            let kb_stale = state.store.kb_entries_stale_embedding(&provider, 9999).await.map(|v| v.len()).unwrap_or(0);
            let skill_missing = state.store.skill_topics_missing_embedding(9999).await.map(|v| v.len()).unwrap_or(0);
            let skill_stale = state.store.skill_topics_stale_embedding(&provider, 9999).await.map(|v| v.len()).unwrap_or(0);

            let total = conv_missing + conv_stale + kb_missing + kb_stale + skill_missing + skill_stale;
            if total == 0 {
                return Ok(ToolResult::json(&serde_json::json!({
                    "status": "nothing_to_do",
                    "currentProvider": provider,
                })));
            }

            let _ = state.embedding_tx.try_send(EmbeddingTask::BackfillAll);
            Ok(ToolResult::json(&serde_json::json!({
                "status": "triggered",
                "currentProvider": provider,
                "kb": { "stale": kb_stale, "missing": kb_missing },
                "skill": { "stale": skill_stale, "missing": skill_missing },
                "conversation": { "stale": conv_stale, "missing": conv_missing },
            })))
        }

        "mission_habit_scan" => {
            let unscanned = state.store.count_unscanned_conversations().await.unwrap_or(0);

            let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("status");
            match action {
                "trigger" => {
                    if unscanned == 0 {
                        return Ok(ToolResult::json(&serde_json::json!({
                            "status": "nothing_to_do",
                            "unscanned": 0,
                        })));
                    }
                    // Reset cadence to allow immediate run
                    let _ = state.store.daemon_state_set("last_habit_scan_at", 0).await;
                    Ok(ToolResult::json(&serde_json::json!({
                        "status": "triggered",
                        "unscanned": unscanned,
                        "message": "Habit scan will run on next learning tick (within 60s)",
                    })))
                }
                _ => {
                    // Status: show scan progress
                    let total = state.store.count_scannable_conversations().await.unwrap_or(0);
                    let scanned = total - unscanned;
                    Ok(ToolResult::json(&serde_json::json!({
                        "total": total,
                        "scanned": scanned,
                        "unscanned": unscanned,
                        "progress": if total > 0 { format!("{:.1}%", scanned as f64 / total as f64 * 100.0) } else { "N/A".to_string() },
                    })))
                }
            }
        }

        "mission_embedding_stats" => {
            let mut stats = state.store.embedding_stats().await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Add cache sizes
            let kb_cache_size = state.kb_search_cache.read().await.len();
            let policy_cache_size = state.embedding_cache.read().await.len();
            let skill_cache_size = state.skill_embedding_cache.read().await.len();
            let conv_cache_size = state.conversation_topic_cache.read().await.len();
            let ast_cache_size = state.ast_embedding_cache.read().await.len();
            let provider = state.embedding_service.as_ref()
                .map(|svc| svc.provider_id())
                .unwrap_or("none");
            stats["cache"] = serde_json::json!({
                "kbSearch": kb_cache_size,
                "policyDecision": policy_cache_size,
                "skill": skill_cache_size,
                "conversation": conv_cache_size,
                "ast": ast_cache_size,
            });
            stats["currentProvider"] = serde_json::json!(provider);
            // AST embedding stats (code intelligence health)
            if let Ok(ast) = state.store.ast_stats().await {
                let coverage = if ast.total_nodes > 0 {
                    format!("{:.1}%", ast.embedded_nodes as f64 / ast.total_nodes as f64 * 100.0)
                } else {
                    "N/A".to_string()
                };
                stats["ast"] = serde_json::json!({
                    "totalNodes": ast.total_nodes,
                    "embeddedNodes": ast.embedded_nodes,
                    "coverage": coverage,
                    "totalFiles": ast.total_files,
                    "totalRepos": ast.total_repos,
                });
            }
            Ok(ToolResult::json_pretty(&stats))
        }

        // ===== Conversation Events & Agent Trajectory =====
        "mission_conversation_events" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: Option<String>,
                #[serde(alias = "event_type")]
                event_type: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
            }
            let Args { session_id, event_type, limit } =
                serde_json::from_value(args).unwrap_or(Args { session_id: None, event_type: None, limit: None });
            if let Some(sid) = &session_id {
                let events = state.store.get_conversation_events(sid, event_type.as_deref(), limit.unwrap_or(100)).await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "sessionId": sid,
                    "events": events,
                    "count": events.len(),
                })))
            } else {
                // No sessionId → return event type summary
                let summary = state.store.get_event_type_summary(None).await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let summary_obj: Vec<serde_json::Value> = summary.iter().map(|(t, c)| {
                    serde_json::json!({ "eventType": t, "count": c })
                }).collect();
                Ok(ToolResult::json(&serde_json::json!({
                    "summary": summary_obj,
                    "totalTypes": summary.len(),
                })))
            }
        }
        "mission_agent_trajectory" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "tool_use_id")]
                tool_use_id: String,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
            }
            let Args { tool_use_id, limit } = serde_json::from_value(args)?;
            let msgs = state.store.get_agent_trajectory(&tool_use_id, limit.unwrap_or(200)).await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Extract agentId from first message metadata
            let agent_id = msgs.first()
                .and_then(|m| m.metadata.as_ref())
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get("agentId").and_then(|a| a.as_str()).map(|s| s.to_string()));
            // Strip raw_content for LLM consumption
            let msgs_lite: Vec<serde_json::Value> = msgs.iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "role": m.role,
                    "content": m.content,
                    "timestamp": m.timestamp,
                })
            }).collect();
            Ok(ToolResult::json(&serde_json::json!({
                "toolUseId": tool_use_id,
                "agentId": agent_id,
                "messages": msgs_lite,
                "count": msgs_lite.len(),
            })))
        }

        "mission_conversation_message" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let message_id = args_val.get("message_id").and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow!("missing message_id"))?;
            // Include translation if available
            let translation = state.store.get_translation(message_id).await
                .ok()
                .flatten()
                .map(|(t, _)| t);
            match state.store.get_conversation_message_by_id(message_id).await.map_err(|e| anyhow!("DB error: {}", e))? {
                Some(msg) => Ok(ToolResult::json_pretty(&serde_json::json!({
                    "id": msg.id,
                    "session_id": msg.session_id,
                    "role": msg.role,
                    "content": msg.content,
                    "raw_content": msg.raw_content,
                    "model": msg.model,
                    "timestamp": msg.timestamp,
                    "translation": translation,
                }))),
                None => Ok(ToolResult::error("Message not found")),
            }
        }

        "mission_session_narrations" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let session_id = args_val.get("session_id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing session_id"))?;
            let narrations = state.store.get_narrations_for_session(session_id).await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let items: Vec<serde_json::Value> = narrations.iter()
                .map(|(msg_id, title, intent, result)| serde_json::json!({
                    "message_id": msg_id,
                    "step_title": title,
                    "step_intent": intent,
                    "step_result": result,
                }))
                .collect();
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "session_id": session_id,
                "narrations": items,
                "count": items.len(),
            })))
        }

        // ===== Activity Report =====
        "mission_activity_report" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                since: String,
                until: Option<String>,
            }
            let Args { since, until } = serde_json::from_value(args)?;
            let until = until.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
            // 1. Conversations
            let convs = state.store.list_conversations(None, 500, Some("all"), None, Some(&since), Some(&until), None).await
                .unwrap_or_default();
            let mut by_source: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
            let mut by_type: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
            for c in &convs {
                *by_source.entry(c.source.clone()).or_default() += 1;
                *by_type.entry(c.conversation_type.clone()).or_default() += 1;
            }

            // 2. Board tasks — time-based filtering via static parse helpers
            let since_iso = missiond_core::db::shared::parse_since(&since).replace(' ', "T");
            let until_iso = missiond_core::db::shared::parse_until(&until).replace(' ', "T");
            let board_created = state.store.query_board_tasks_in_range("created_at", &since_iso, &until_iso).await
                .unwrap_or_default();
            let board_completed = state.store.query_board_tasks_in_range_with_status("done", &since_iso, &until_iso).await
                .unwrap_or_default();

            // 3. Timeline stats
            let timeline_stats = state.store.query_timeline_stats(Some(&since), Some(&until)).await.ok();

            // 4. Git commits (graceful degradation)
            let git_commits = {
                let git_since = missiond_core::db::shared::parse_since(&since);
                let git_until = missiond_core::db::shared::parse_until(&until);
                match std::process::Command::new("git")
                    .args(["log", "--oneline", &format!("--after={}", git_since), &format!("--before={}", git_until), "--all"])
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let commits: Vec<serde_json::Value> = stdout.lines()
                            .filter(|l| !l.is_empty())
                            .map(|l| {
                                let (hash, msg) = l.split_once(' ').unwrap_or((l, ""));
                                serde_json::json!({"hash": hash, "message": msg})
                            })
                            .collect();
                        serde_json::json!({"total": commits.len(), "commits": commits})
                    }
                    _ => serde_json::json!({"error": "git not available or not a repository", "total": 0}),
                }
            };

            Ok(ToolResult::json_pretty(&serde_json::json!({
                "since": since,
                "until": until,
                "conversations": {
                    "total": convs.len(),
                    "by_source": by_source,
                    "by_type": by_type,
                },
                "board": {
                    "created": board_created,
                    "completed": board_completed,
                },
                "timeline": timeline_stats,
                "git": git_commits,
            })))
        }

        _ => Err(anyhow!("Unknown conversation tool: {name}")),
    }
}
