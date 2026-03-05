
use anyhow::{anyhow, Result};
use serde_json::json;
use tracing::{debug, error, info, warn};

use crate::state::AppState;
use crate::llm_gateway::call_gemini_for_flow;
use crate::memory_scheduler::ensure_memory_slot_by_id;

pub(crate) struct TierResult {
    hit: bool,
    detail: serde_json::Value,
}

/// Save routing trace JSON to the question record
pub(crate) fn save_routing_trace(state: &AppState, question_id: &str, resolved_tier: &str, path: &[serde_json::Value], latency_ms: u128) {
    let trace = serde_json::json!({
        "resolved_tier": resolved_tier,
        "path": path,
        "latency_ms": latency_ms,
    });
    let _ = state.mission.db().set_question_routing_trace(question_id, &trace.to_string());
}

// ============ AIOps CronSensor ============

/// Phase 2: Concurrent health scan of all servers with healthEndpoint configured.
/// Uses JoinSet for parallel HTTP GET with 5s timeout per server.
pub(crate) async fn health_scan(state: &AppState) {
    let servers = &state.infra.servers;
    let mut set = tokio::task::JoinSet::new();

    for server in servers {
        let endpoint = match &server.health_endpoint {
            Some(e) => e.clone(),
            None => continue,
        };
        let client = state.http_client.clone();
        let server_id = server.id.clone();
        let server_name = server.name.clone();

        set.spawn(async move {
            let healthy = match client
                .get(&endpoint)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            };
            (server_id, server_name, endpoint, healthy)
        });
    }

    while let Some(result) = set.join_next().await {
        let (server_id, server_name, endpoint, healthy) = match result {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "Health check task panicked");
                continue;
            }
        };

        if !healthy {
            let incident = missiond_core::types::MissionIncident {
                id: format!("inc-{}", uuid::Uuid::new_v4()),
                severity: missiond_core::types::IncidentSeverity::High,
                source: missiond_core::types::IncidentSource::HealthCheck,
                title: format!("{} 健康检查失败", server_name),
                description: format!(
                    "服务器 {} ({}) 的健康端点 {} 无响应或返回非 200。\n\n\
                     建议操作：\n\
                     1. mission_reachability(target=\"{}\") 确认网络连通性\n\
                     2. mission_os_diagnose(target=\"{}\") 检查系统状态\n\
                     3. 检查 Docker 容器状态",
                    server_name, server_id, endpoint, server_id, server_id
                ),
                server_id: Some(server_id.clone()),
                raw_payload: json!({
                    "endpoint": endpoint,
                    "server_id": server_id,
                    "server_name": server_name,
                }),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            if let Err(e) = state.incident_tx.try_send(incident) {
                warn!(server_id = %server_id, "Incident channel full, dropping health check: {}", e);
            }
        }
    }
}

// ============ AIOps Reactor ============

/// Process a single incident from the event bus:
/// 1. Dedupe check (5-minute window on dedupe_key)
/// 2. Insert into DB
/// 3. Create a high-priority Board task for slot-ops
pub(crate) async fn process_incident(state: &AppState, incident: missiond_core::types::MissionIncident) {
    let db = state.mission.db();
    let dedupe_key = format!("{}:{}", incident.source, incident.title);

    // PtySlot incidents: dispatch remediation to a Claude Code (Opus) slot
    if matches!(incident.source, missiond_core::types::IncidentSource::PtySlot) {
        if let Some(slot_id) = incident.raw_payload.get("slot_id").and_then(|v| v.as_str()) {
            create_pty_remediation_task(state, slot_id, &incident.title, &incident.description);
            // Still insert incident record below, but skip default Board task creation
            if let Err(e) = db.insert_incident(
                &incident.id,
                &incident.severity.to_string(),
                &incident.source.to_string(),
                &incident.title,
                &incident.description,
                incident.server_id.as_deref(),
                Some(&incident.raw_payload.to_string()),
                None,
                &dedupe_key,
            ) {
                warn!(error = %e, "AIOps: failed to insert incident record");
            }
            return;
        }
    }

    // 5-minute dedupe window — drop duplicate incidents
    match db.has_recent_incident(&dedupe_key, 300) {
        Ok(true) => {
            debug!(
                dedupe_key = %dedupe_key,
                "Incident dedupe: dropping duplicate within 5min window"
            );
            return;
        }
        Ok(false) => {}
        Err(e) => {
            warn!(error = %e, "Incident dedupe check failed, proceeding anyway");
        }
    }

    // Truncate raw_payload for prompt safety (max 2000 chars)
    let raw_str = incident.raw_payload.to_string();
    let truncated_payload = if raw_str.len() > 2000 {
        format!("{}... (truncated, {} total)", &raw_str[..2000], raw_str.len())
    } else {
        raw_str
    };

    // Build Board task description with structured context
    let description = format!(
        "## AIOps 自动检测到异常\n\n\
         **严重等级**: {severity}\n\
         **来源**: {source}\n\
         **服务器**: {server}\n\
         **时间**: {time}\n\n\
         ### 描述\n{desc}\n\n\
         ### 原始数据\n```json\n{payload}\n```",
        severity = incident.severity,
        source = incident.source,
        server = incident.server_id.as_deref().unwrap_or("N/A"),
        time = incident.created_at,
        desc = incident.description,
        payload = truncated_payload,
    );

    // Determine priority and auto-execute from severity
    // Warning → Board backlog only (autoExecute=false), High/Critical → auto-dispatch to slot-ops
    let (priority, auto_execute) = match incident.severity {
        missiond_core::types::IncidentSeverity::Critical => ("urgent", true),
        missiond_core::types::IncidentSeverity::High => ("high", true),
        missiond_core::types::IncidentSeverity::Warning => ("medium", false),
    };

    // Create board task
    let task_input = missiond_core::types::CreateBoardTaskInput {
        title: format!("[AIOps] {}", incident.title),
        description: Some(description),
        priority: Some(priority.to_string()),
        category: Some("ops".to_string()),
        assignee: Some("slot-ops".to_string()),
        auto_execute: Some(auto_execute),
        server: incident.server_id.clone(),
        project: None,
        due_date: None,
        parent_id: None,
        prompt_template: None,
        hidden: None,
        flow_template: None,
        depends_on: None,
    };

    let board_task_id = match db.create_board_task(&task_input) {
        Ok(task) => {
            info!(
                incident_id = %incident.id,
                board_task_id = %task.id,
                severity = %incident.severity,
                title = %incident.title,
                "AIOps: incident → board task created"
            );
            Some(task.id)
        }
        Err(e) => {
            error!(error = %e, "AIOps: failed to create board task for incident");
            None
        }
    };

    // Fire submit_notify so Autopilot picks up the new task
    state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });

    // Insert incident record into DB
    if let Err(e) = db.insert_incident(
        &incident.id,
        &incident.severity.to_string(),
        &incident.source.to_string(),
        &incident.title,
        &incident.description,
        incident.server_id.as_deref(),
        Some(&incident.raw_payload.to_string()),
        board_task_id.as_deref(),
        &dedupe_key,
    ) {
        warn!(error = %e, "AIOps: failed to insert incident record");
    }
}

// ============ PTY Auto-Remediation ============

/// Dispatch a remediation Board task to a Claude Code (Opus) slot.
/// The slot will use MCP tools (pty_screen, pty_send, pty_kill) to observe and fix.
pub(crate) fn create_pty_remediation_task(
    state: &AppState,
    target_slot_id: &str,
    incident_title: &str,
    incident_description: &str,
) {
    let description = format!(
        "## PTY 工位自愈任务\n\n\
         **目标工位**: `{target_slot}`\n\
         **问题**: {title}\n\n\
         ### 问题详情\n{desc}\n\n\
         ### 操作指南\n\
         1. `mission_pty_screen(slotId=\"{target_slot}\")` 查看目标工位当前屏幕\n\
         2. `mission_pty_status(slotId=\"{target_slot}\")` 检查工位状态\n\
         3. 根据屏幕内容判断修复方式：\n\
            - MCP 不可用 → `mission_pty_kill(slotId=\"{target_slot}\")` 重启（auto_restart 会自动恢复）\n\
            - 卡在确认提示 → `mission_pty_send(slotId=\"{target_slot}\", message=\"...\")` 发送合适回复\n\
            - 其他情况 → 先观察，再决定\n\
         4. 操作后再次 `mission_pty_screen` 确认修复效果\n\
         5. 重复 observe-act 直到目标工位恢复正常\n\n\
         **注意**: 如果无法修复，在任务中说明原因即可。",
        target_slot = target_slot_id,
        title = incident_title,
        desc = incident_description,
    );

    let task_input = missiond_core::types::CreateBoardTaskInput {
        title: format!("[自愈] {}", incident_title),
        description: Some(description),
        priority: Some("high".to_string()),
        category: Some("ops".to_string()),
        assignee: Some("slot-coder-1".to_string()),
        auto_execute: Some(true),
        server: None,
        project: None,
        due_date: None,
        parent_id: None,
        prompt_template: None,
        hidden: None,
        flow_template: None,
        depends_on: None,
    };

    match state.mission.db().create_board_task(&task_input) {
        Ok(task) => {
            info!(
                task_id = %task.id,
                target_slot = %target_slot_id,
                "PTY remediation: Board task created for Opus slot"
            );
            // Notify autopilot to pick up immediately
            state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });
        }
        Err(e) => {
            error!(error = %e, "PTY remediation: failed to create Board task");
        }
    }
}

/// Decision Engine: process pending questions targeted at master.
/// Routes through Tier 1 (KB) → Tier 2 (Gemini) → Tier 3 (slot-decision) responsibility chain.
/// Records routing_trace for each decision for observability.
pub(crate) async fn process_pending_master_questions(state: &AppState) {
    // Query pending questions with target=master
    let questions = match state.mission.db().list_agent_questions(Some("pending"), Some("master"), None) {
        Ok(qs) => qs,
        Err(e) => {
            warn!(error = %e, "Decision Engine: failed to list pending master questions");
            return;
        }
    };

    if questions.is_empty() {
        return;
    }

    info!(
        count = questions.len(),
        "Decision Engine: {} pending master question(s) to route",
        questions.len()
    );

    for question in questions {
        let start = std::time::Instant::now();
        let mut path: Vec<serde_json::Value> = Vec::new();

        // Anti-loop: retry_count >= 3 → downgrade to user
        if question.retry_count >= 3 {
            warn!(question_id = %question.id, retries = question.retry_count,
                "Decision Engine: retry limit reached, downgrading to user");
            path.push(serde_json::json!({"tier": "anti-loop", "status": "downgraded", "reason": format!("retry_count={}", question.retry_count)}));
            let _ = state.mission.db().downgrade_question_to_user(&question.id);
            save_routing_trace(state, &question.id, "downgraded", &path, start.elapsed().as_millis());
            continue;
        }

        // Route by decision_type (forced fast-lane)
        let result = match question.decision_type.as_str() {
            "debug" | "investigation" => {
                // Direct to Tier 3 — text LLM can't solve these
                info!(question_id = %question.id, dt = %question.decision_type, "Decision Engine: → Tier 3 (fast-lane)");
                path.push(serde_json::json!({"tier": "T3", "status": "fast-lane", "reason": format!("decision_type={}", question.decision_type)}));
                let r = decision_tier3_dispatch(state, &question).await;
                if let Ok(ref tr) = r {
                    path.push(tr.detail.clone());
                    save_routing_trace(state, &question.id, "T3", &path, start.elapsed().as_millis());
                }
                r.map(|tr| tr.hit)
            }
            "architecture" | "risk" => {
                // Direct to Tier 2 — skip muscle memory
                info!(question_id = %question.id, dt = %question.decision_type, "Decision Engine: → Tier 2 (fast-lane)");
                path.push(serde_json::json!({"tier": "T2", "status": "fast-lane", "reason": format!("decision_type={}", question.decision_type)}));
                let r = decision_tier2_gemini(state, &question).await;
                if let Ok(ref tr) = r {
                    path.push(tr.detail.clone());
                    if tr.hit {
                        save_routing_trace(state, &question.id, "T2", &path, start.elapsed().as_millis());
                    } else {
                        // T2 escalated but no T3 for this fast-lane — downgrade
                        path.push(serde_json::json!({"tier": "T2", "status": "escalated_no_t3"}));
                        save_routing_trace(state, &question.id, "T2-escalated", &path, start.elapsed().as_millis());
                    }
                }
                r.map(|tr| tr.hit)
            }
            "preference" => {
                // Tier 1 only — if miss, downgrade to user (don't let Gemini guess)
                info!(question_id = %question.id, "Decision Engine: → Tier 1 (preference exclusive)");
                let r = decision_tier1_kb(state, &question).await;
                match r {
                    Ok(ref tr) => {
                        path.push(tr.detail.clone());
                        if tr.hit {
                            save_routing_trace(state, &question.id, "T1", &path, start.elapsed().as_millis());
                        } else {
                            info!(question_id = %question.id, "Tier 1 miss for preference → downgrade to user");
                            path.push(serde_json::json!({"tier": "preference", "status": "downgraded", "reason": "t1_miss_preference_exclusive"}));
                            let _ = state.mission.db().downgrade_question_to_user(&question.id);
                            save_routing_trace(state, &question.id, "downgraded", &path, start.elapsed().as_millis());
                        }
                    }
                    Err(_) => {
                        path.push(serde_json::json!({"tier": "T1", "status": "error"}));
                        let _ = state.mission.db().downgrade_question_to_user(&question.id);
                        save_routing_trace(state, &question.id, "downgraded", &path, start.elapsed().as_millis());
                    }
                }
                r.map(|tr| tr.hit)
            }
            _ => {
                // "implementation" and others: full pass-through Tier 1 → 2 → 3
                info!(question_id = %question.id, "Decision Engine: → full pass-through (implementation)");

                // Tier 1
                let t1 = decision_tier1_kb(state, &question).await;
                match t1 {
                    Ok(tr) if tr.hit => {
                        path.push(tr.detail);
                        save_routing_trace(state, &question.id, "T1", &path, start.elapsed().as_millis());
                        Ok(true)
                    }
                    Ok(tr) => {
                        path.push(tr.detail);
                        // Tier 2
                        let t2 = decision_tier2_gemini(state, &question).await;
                        match t2 {
                            Ok(tr2) if tr2.hit => {
                                path.push(tr2.detail);
                                save_routing_trace(state, &question.id, "T2", &path, start.elapsed().as_millis());
                                Ok(true)
                            }
                            Ok(tr2) => {
                                path.push(tr2.detail);
                                // Tier 3
                                let t3 = decision_tier3_dispatch(state, &question).await;
                                match t3 {
                                    Ok(ref tr3) => {
                                        path.push(tr3.detail.clone());
                                        save_routing_trace(state, &question.id, "T3", &path, start.elapsed().as_millis());
                                    }
                                    Err(ref e) => {
                                        path.push(serde_json::json!({"tier": "T3", "status": "error", "reason": e.to_string()}));
                                        save_routing_trace(state, &question.id, "error", &path, start.elapsed().as_millis());
                                    }
                                }
                                t3.map(|tr3| tr3.hit)
                            }
                            Err(e) => {
                                path.push(serde_json::json!({"tier": "T2", "status": "error", "reason": e.to_string()}));
                                // Still try Tier 3
                                let t3 = decision_tier3_dispatch(state, &question).await;
                                match t3 {
                                    Ok(ref tr3) => {
                                        path.push(tr3.detail.clone());
                                        save_routing_trace(state, &question.id, "T3", &path, start.elapsed().as_millis());
                                    }
                                    Err(ref e2) => {
                                        path.push(serde_json::json!({"tier": "T3", "status": "error", "reason": e2.to_string()}));
                                        save_routing_trace(state, &question.id, "error", &path, start.elapsed().as_millis());
                                    }
                                }
                                t3.map(|tr3| tr3.hit)
                            }
                        }
                    }
                    Err(e) => {
                        path.push(serde_json::json!({"tier": "T1", "status": "error", "reason": e.to_string()}));
                        // Still try Tier 2 → 3
                        let t2 = decision_tier2_gemini(state, &question).await;
                        match t2 {
                            Ok(tr2) if tr2.hit => {
                                path.push(tr2.detail);
                                save_routing_trace(state, &question.id, "T2", &path, start.elapsed().as_millis());
                                Ok(true)
                            }
                            _ => {
                                if let Ok(ref tr2) = t2 { path.push(tr2.detail.clone()); }
                                let t3 = decision_tier3_dispatch(state, &question).await;
                                if let Ok(ref tr3) = t3 { path.push(tr3.detail.clone()); }
                                save_routing_trace(state, &question.id, if t3.is_ok() { "T3" } else { "error" }, &path, start.elapsed().as_millis());
                                t3.map(|tr3| tr3.hit)
                            }
                        }
                    }
                }
            }
        };

        if let Err(e) = result {
            warn!(question_id = %question.id, error = %e, "Decision Engine: routing failed");
            let _ = state.mission.db().increment_question_retry(&question.id);
        }
    }
}

/// Tier 1: Hybrid KB search (FTS5 + semantic vectors via RRF fusion)
///
/// When embedding service is available:
///   Path A: FTS5 → Top 20 (with rank)
///   Path B: Cosine similarity on in-memory vectors → Top 20
///   Merge via RRF (k=60), hit if cosine > 0.60 AND rrf above threshold
///
/// Fallback (embedding unavailable): FTS5 + keyword overlap ≥ 30%
pub(crate) async fn decision_tier1_kb(state: &AppState, question: &missiond_core::types::AgentQuestion) -> Result<TierResult> {
    // Path A: FTS5 search with ranked results
    let fts_results = state.mission.db()
        .kb_search_ranked(&question.question, Some("policy:decision"), 20)
        .unwrap_or_default();

    let fts_ids: Vec<(String, usize)> = fts_results.iter()
        .map(|(e, rank)| (e.id.clone(), *rank))
        .collect();

    // Path B: Semantic vector search (if embedding service available)
    let query_embedding = state.embedding_service.as_ref()
        .and_then(|svc| svc.embed(&question.question));

    let cache_guard = state.embedding_cache.read().await;
    let has_vectors = query_embedding.is_some() && !cache_guard.is_empty();

    if fts_ids.is_empty() && !has_vectors {
        debug!(question_id = %question.id, "Tier 1: no policy:decision entries and no vectors");
        return Ok(TierResult {
            hit: false,
            detail: serde_json::json!({"tier": "T1", "status": "missed", "reason": "no_entries"}),
        });
    }

    // Hybrid merge if we have both FTS and vector signals
    if let Some(ref q_emb) = query_embedding {
        if !cache_guard.is_empty() {
            let hybrid = missiond_core::embedding::hybrid_search(
                q_emb,
                &cache_guard,
                &fts_ids,
                20,   // top_k
                60,   // rrf_k
            );
            drop(cache_guard); // Release read lock

            if hybrid.is_empty() {
                return Ok(TierResult {
                    hit: false,
                    detail: serde_json::json!({"tier": "T1", "status": "missed", "reason": "hybrid_empty"}),
                });
            }

            let top = &hybrid[0];
            let cosine = top.cosine_sim.unwrap_or(0.0);
            let cosine_str = format!("{:.3}", cosine);
            let rrf_str = format!("{:.4}", top.rrf_score);

            // Hit criteria: cosine > 0.60 AND rrf_score above minimum threshold
            let cosine_ok = cosine > 0.60;
            let rrf_threshold = 1.0 / 61.0; // Must appear in at least one top-1 position
            let rrf_ok = top.rrf_score >= rrf_threshold;

            if !cosine_ok || !rrf_ok {
                debug!(
                    question_id = %question.id,
                    cosine = %cosine_str, rrf = %rrf_str,
                    "Tier 1: hybrid match below threshold"
                );
                return Ok(TierResult {
                    hit: false,
                    detail: serde_json::json!({
                        "tier": "T1",
                        "status": "missed",
                        "method": "hybrid",
                        "reason": "below_threshold",
                        "semantic_similarity": cosine_str,
                        "rrf_score": rrf_str,
                        "fts_rank": top.fts_rank,
                        "vec_rank": top.vec_rank,
                    }),
                });
            }

            // Fetch the actual KB entry
            let entry = fts_results.iter()
                .find(|(e, _)| e.id == top.id)
                .map(|(e, _)| e.clone());
            let entry = match entry {
                Some(e) => e,
                None => {
                    // Entry found by vector but not by FTS — fetch from DB
                    state.mission.db().kb_get_by_id(&top.id)
                        .unwrap_or(None)
                        .ok_or_else(|| anyhow!("KB entry {} not found", top.id))?
                }
            };

            // Hit — answer
            let answer = format!("[主控裁决·肌肉记忆] {}", entry.summary);
            info!(
                question_id = %question.id,
                cosine = %cosine_str, rrf = %rrf_str,
                rule_key = %entry.key,
                "Tier 1: hybrid hit, answering"
            );

            state.mission.db().answer_agent_question(&question.id, &answer)
                .map_err(|e| anyhow!("Failed to answer: {}", e))?;
            state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });

            return Ok(TierResult {
                hit: true,
                detail: serde_json::json!({
                    "tier": "T1",
                    "status": "decided",
                    "method": "hybrid",
                    "rule_key": entry.key,
                    "semantic_similarity": cosine_str,
                    "fts_rank": top.fts_rank,
                    "vec_rank": top.vec_rank,
                    "rrf_score": rrf_str,
                }),
            });
        }
    }
    drop(cache_guard);

    // Fallback: FTS-only path (when embeddings are disabled or cache empty)
    if fts_results.is_empty() {
        return Ok(TierResult {
            hit: false,
            detail: serde_json::json!({"tier": "T1", "status": "missed", "reason": "fts_empty"}),
        });
    }

    let top = &fts_results[0].0;

    // Keyword overlap check (original logic)
    let q_tokens: std::collections::HashSet<&str> = question.question
        .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|w| w.len() > 1)
        .collect();
    let rule_tokens: std::collections::HashSet<&str> = top.summary
        .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|w| w.len() > 1)
        .collect();

    if q_tokens.is_empty() {
        return Ok(TierResult {
            hit: false,
            detail: serde_json::json!({"tier": "T1", "status": "missed", "method": "fts_only", "reason": "empty_tokens"}),
        });
    }

    let overlap = q_tokens.intersection(&rule_tokens).count();
    let coverage = overlap as f64 / q_tokens.len() as f64;
    let coverage_str = format!("{:.0}%", coverage * 100.0);

    if coverage < 0.3 {
        debug!(question_id = %question.id, coverage = %coverage_str,
            "Tier 1: FTS keyword overlap below 30%, passing to next tier");
        return Ok(TierResult {
            hit: false,
            detail: serde_json::json!({
                "tier": "T1", "status": "missed", "method": "fts_only",
                "coverage": coverage_str, "rule_key": top.key,
            }),
        });
    }

    let answer = format!("[主控裁决·肌肉记忆] {}", top.summary);
    info!(question_id = %question.id, coverage = %coverage_str,
        rule_key = %top.key, "Tier 1: FTS-only hit, answering");

    state.mission.db().answer_agent_question(&question.id, &answer)
        .map_err(|e| anyhow!("Failed to answer: {}", e))?;
    state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });
    Ok(TierResult {
        hit: true,
        detail: serde_json::json!({
            "tier": "T1", "status": "decided", "method": "fts_only",
            "coverage": coverage_str, "rule_key": top.key,
        }),
    })
}

/// Tier 2: Call Gemini with structured JSON contract
pub(crate) async fn decision_tier2_gemini(state: &AppState, question: &missiond_core::types::AgentQuestion) -> Result<TierResult> {
    // Build context: task description + question + phase + recent notes
    let mut context_parts = Vec::new();

    // 1. Task description (if linked)
    if let Some(ref task_id) = question.task_id {
        if let Ok(Some(task)) = state.mission.db().get_board_task(task_id) {
            context_parts.push(format!("## 任务目标\n**{}**\n{}", task.title, task.description));
            if let Some(ref fp) = task.flow_phase {
                context_parts.push(format!("## 当前阶段\n{}", fp));
            }
            // Last 3 board notes
            if let Ok(notes) = state.mission.db().get_board_task_notes(task_id) {
                let recent: Vec<_> = notes.iter().rev().take(3).collect();
                if !recent.is_empty() {
                    let notes_text = recent.iter()
                        .map(|n| format!("[{}] {}", n.created_at, n.content))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    context_parts.push(format!("## 最近进展\n{}", notes_text));
                }
            }
        }
    }

    // 2. Question + options
    context_parts.push(format!("## 待决策问题\n{}", question.question));
    if !question.context.is_empty() {
        context_parts.push(format!("## 补充上下文\n{}", question.context));
    }
    if let Some(ref opts) = question.options {
        context_parts.push(format!("## 候选选项\n{}", opts));
    }

    let user_prompt = context_parts.join("\n\n");
    let system_prompt = "你是一个高级架构决策大脑。请基于提供的上下文，对当前遭遇的困境做出决断。\
        你必须且只能输出严格的 JSON 格式。\
        如果你可以直接做出决定，请输出：{\"action\": \"DECIDE\", \"decision\": \"你的具体决策\", \"reasoning\": \"简要理由\"}。\
        如果该问题需要动态运行代码、查看报错日志或全局搜索未知的代码库才能决定，你必须弃权并输出：{\"action\": \"ESCALATE_TIER_3\", \"decision\": \"\", \"reasoning\": \"需要阅读源码\"}。";

    let full_prompt = format!("{}\n\n---\n\n{}", system_prompt, user_prompt);

    // Reuse call_gemini_for_flow with a decision-specific conversation key
    let conv_key = format!("decision-{}", question.id);
    let response = call_gemini_for_flow(state, &conv_key, &full_prompt).await;

    match response {
        Ok(text) => {
            // Parse JSON response
            // Try to extract JSON from the response (Gemini may wrap in markdown)
            let json_text = if let Some(start) = text.find('{') {
                if let Some(end) = text.rfind('}') {
                    &text[start..=end]
                } else {
                    &text
                }
            } else {
                &text
            };

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_text) {
                let action = parsed.get("action").and_then(|a| a.as_str()).unwrap_or("");
                let decision = parsed.get("decision").and_then(|d| d.as_str()).unwrap_or("");
                let reasoning = parsed.get("reasoning").and_then(|r| r.as_str()).unwrap_or("");

                if action == "DECIDE" && !decision.is_empty() {
                    let answer = format!("[主控裁决·Gemini] {}\n\n理由：{}", decision, reasoning);
                    info!(question_id = %question.id, "Tier 2: Gemini decided");
                    state.mission.db().answer_agent_question(&question.id, &answer)
                        .map_err(|e| anyhow!("Failed to answer: {}", e))?;
                    state.event_bus.publish(crate::event_bus::DaemonEvent::TaskCreated { task_id: String::new() });
                    return Ok(TierResult {
                        hit: true,
                        detail: serde_json::json!({"tier": "T2", "status": "decided", "action": "DECIDE", "reasoning": reasoning}),
                    });
                } else if action == "ESCALATE_TIER_3" {
                    info!(question_id = %question.id, reason = %reasoning, "Tier 2: Gemini escalated to Tier 3");
                    return Ok(TierResult {
                        hit: false,
                        detail: serde_json::json!({"tier": "T2", "status": "escalated", "action": "ESCALATE_TIER_3", "reasoning": reasoning}),
                    });
                }
            }

            // Parsing failed — treat as escalation
            warn!(question_id = %question.id, "Tier 2: Gemini response not valid JSON, escalating");
            Ok(TierResult {
                hit: false,
                detail: serde_json::json!({"tier": "T2", "status": "escalated", "reason": "parse_failed"}),
            })
        }
        Err(e) => {
            warn!(question_id = %question.id, error = %e, "Tier 2: Gemini call failed");
            Err(e)
        }
    }
}

/// Tier 3: Dispatch to slot-decision workstation via slot_tasks
pub(crate) async fn decision_tier3_dispatch(state: &AppState, question: &missiond_core::types::AgentQuestion) -> Result<TierResult> {
    let slot_id = "slot-decision";

    // Build investigation prompt
    let mut prompt_parts = vec![
        "【最高优先任务】主干线工位在执行任务时遇到了阻塞。".to_string(),
        format!("阻塞问题：{}", question.question),
    ];
    if !question.context.is_empty() {
        prompt_parts.push(format!("补充上下文：{}", question.context));
    }
    if let Some(ref opts) = question.options {
        prompt_parts.push(format!("候选选项：{}", opts));
    }
    if let Some(ref task_id) = question.task_id {
        if let Ok(Some(task)) = state.mission.db().get_board_task(task_id) {
            prompt_parts.push(format!("关联任务：{} ({})", task.title, task_id));
            // Flow phase — lets Tier 3 judge decision invasiveness by stage
            if let Some(ref fp) = task.flow_phase {
                prompt_parts.push(format!("当前 Flow 阶段：{}", fp));
            }
            // Last 5 Board notes — prevents Tier 3 from repeating failed approaches
            if let Ok(notes) = state.mission.db().get_board_task_notes(task_id) {
                let recent: Vec<_> = notes.iter().rev().take(5).collect();
                if !recent.is_empty() {
                    let notes_text = recent.iter()
                        .map(|n| format!("[{}] {}", &n.created_at[..19.min(n.created_at.len())], n.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    prompt_parts.push(format!("最近进展（Board Notes）：\n{}", notes_text));
                }
            }
        }
    }
    prompt_parts.push(format!(
        "\n你的唯一目标是：通过阅读代码、运行测试等方式，查明真相并给出明确的决策。\n\
         **警告**：你是只读/诊断性质的工位，禁止进行任何实质性的业务代码修改。\n\
         当你得出结论后，请立刻调用 `mission_question_answer(id=\"{}\", answer=\"你的结论\")` 提交结果。",
        question.id
    ));

    let prompt = prompt_parts.join("\n");
    let prompt_summary = if prompt.len() > 200 {
        let end = prompt.char_indices().nth(200).map(|(i,_)| i).unwrap_or(prompt.len());
        format!("{}...", &prompt[..end])
    } else {
        prompt.clone()
    };

    // Ensure slot-decision PTY is running (reuse memory slot spawn pattern)
    if !ensure_memory_slot_by_id(state, slot_id).await {
        return Err(anyhow!("Failed to ensure slot-decision PTY"));
    }

    // Create slot_task for tracking
    let now = chrono::Utc::now().to_rfc3339();
    let slot_task = missiond_core::types::SlotTask {
        id: uuid::Uuid::new_v4().to_string(),
        slot_id: slot_id.to_string(),
        task_type: "decision".to_string(),
        status: "running".to_string(),
        prompt_summary: Some(prompt_summary),
        source_sessions: None,
        output_count: 0,
        created_at: now.clone(),
        started_at: Some(now),
        completed_at: None,
        duration_ms: None,
        error: None,
        conversation_id: None,
    };
    let _ = state.mission.db().insert_slot_task(&slot_task);

    // Send prompt to slot-decision PTY
    let timeout_ms = 300_000; // 5 min
    match state.pty.send(slot_id, &prompt, timeout_ms).await {
        Ok(_) => {
            info!(question_id = %question.id, slot_task_id = %slot_task.id,
                "Tier 3: dispatched to slot-decision");
            Ok(TierResult {
                hit: true,
                detail: serde_json::json!({"tier": "T3", "status": "dispatched", "slot_task_id": slot_task.id}),
            })
        }
        Err(e) => {
            warn!(question_id = %question.id, error = %e, "Tier 3: failed to send to slot-decision");
            Err(anyhow!("Tier 3 dispatch failed: {}", e))
        }
    }
}

/// Decision Engine: reap stale decision tasks (15min timeout)
/// Three recovery actions: downgrade question to user, kill PTY, board warning note
pub(crate) async fn reap_stale_decision_tasks(state: &AppState) {
    let stale_questions = match state.mission.db().find_stale_master_questions(900) {
        Ok(qs) => qs,
        Err(e) => {
            warn!(error = %e, "Decision reaper: failed to query stale master questions");
            return;
        }
    };
    if stale_questions.is_empty() {
        return;
    }

    // Kill slot-decision PTY once (shared across all stale questions)
    let slot_id = "slot-decision";
    if let Some(status) = state.pty.get_status(slot_id).await {
        if status.state != missiond_core::pty::SessionState::Exited {
            info!(slot_id = %slot_id, "Decision reaper: killing slot-decision PTY");
            let _ = state.pty.kill(slot_id).await;
        }
    }

    // Force-fail all stale decision slot_tasks
    if let Ok(stale_tasks) = state.mission.db().find_stale_decision_tasks(900) {
        for task in &stale_tasks {
            let _ = state.mission.db().slot_task_set_failed(
                &task.id,
                "Decision reaper: 15min timeout, forced termination",
            );
        }
    }

    for question in &stale_questions {
        warn!(question_id = %question.id, question = %question.question,
            "Decision reaper: master question pending > 15min, forcing recovery");

        // 1. Downgrade question target from master → user
        if let Err(e) = state.mission.db().downgrade_question_to_user(&question.id) {
            warn!(question_id = %question.id, error = %e, "Decision reaper: downgrade failed");
        }

        // 2. Write warning note to associated board task
        if let Some(ref task_id) = question.task_id {
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task_id.clone(),
                    content: "🚨 [决策引擎告警] 影子决策工位调查超时（15min），进程已强制终止。问题已转交人工，请 Master 尽快介入排查。".to_string(),
                    note_type: Some("warning".to_string()),
                    author: Some("decision-engine".to_string()),
                },
            );
        }
    }
}

/// Decision Engine Phase 4: Harvest successful decisions into policy:decision KB entries.
/// Triggered when a Board task is marked done — scans answered master questions,
/// calls Gemini to generalize into reusable rules, writes to KB.
pub(crate) async fn harvest_decisions_for_task(state: &AppState, task_id: &str, task_title: &str) {
    // 1. Scan answered master questions for this task
    let questions = match state.mission.db().list_questions_for_task(task_id) {
        Ok(qs) => qs,
        Err(e) => {
            warn!(task_id, error = %e, "Decision harvester: failed to query questions");
            return;
        }
    };

    // Filter to only master-targeted questions (decision engine handled)
    let master_questions: Vec<_> = questions.iter()
        .filter(|q| q.target == "master" && q.answer.is_some())
        .collect();

    if master_questions.is_empty() {
        debug!(task_id, "Decision harvester: no master decisions to harvest");
        return;
    }

    info!(task_id, count = master_questions.len(), "Decision harvester: harvesting decisions");

    // 2. Build Q&A summary for Gemini
    let qa_text: String = master_questions.iter()
        .map(|q| {
            let dt = &q.decision_type;
            let answer = q.answer.as_deref().unwrap_or("");
            format!("- [{}] Q: {}\n  A: {}", dt, q.question, answer)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 3. Call Gemini to generalize (with Few-Shot examples for quality)
    let prompt = format!(
        "以下是刚成功完成的任务「{}」中的具体技术决策（均已通过代码落地验证）。\n\
        请将每条决策提炼为一条普适的架构/决策规则。\n\n\
        **要求：**\n\
        1. 剥离具体变量名、版本号、文件路径，保留问题特征和决策原则\n\
        2. 每条规则输出为 JSON 对象，包含 key、summary、detail\n\
        3. key：动宾结构连字符短语（如 resolve-dependency-conflict-legacy）\n\
        4. summary：`[触发条件词簇] → [核心原则] → [动作]`，必须包含触发该问题的所有可能同义词、报错关键字和核心名词\n\
        5. detail：JSON 对象 {{\"scenario\": \"...\", \"decision\": \"...\", \"reasoning\": \"...\"}}\n\
        6. 如果多条决策本质相同，合并为一条\n\
        7. 只输出 JSON 数组，不要包含任何其他文本\n\n\
        **Few-Shot 示例（必须严格遵循此格式）：**\n\
        错误示范 summary：'遇到依赖冲突时选择兼容方案。'\n\
        正确示范 summary：'遇到 第三方 API 接口 废弃 deprecated 过期 报错 不兼容 时，优先采用 降级 兼容 强行覆盖 旧版本 原则，切勿 大规模重构 业务逻辑'\n\
        正确示范完整条目：\n\
        {{\"key\":\"handle-third-party-api-deprecation\",\"summary\":\"遇到 第三方 API 接口 废弃 deprecated 过期 报错 不兼容 时，优先采用 降级 兼容 强行覆盖 旧版本 原则，切勿 大规模重构 业务逻辑\",\"detail\":{{\"scenario\":\"调用外部依赖或第三方 SDK 时发现其 API 已废弃导致编译或运行报错\",\"decision\":\"寻找最小侵入性的兼容方案（如编写 adapter、忽略警告或锁死旧版本）\",\"reasoning\":\"保持主线业务稳定性，避免因外部环境变化引发不可控的级联重构\"}}}}\n\n\
        决策记录：\n{}", task_title, qa_text
    );

    let gemini_response = match call_gemini_for_flow(state, task_id, &prompt).await {
        Ok(r) => r,
        Err(e) => {
            warn!(task_id, error = %e, "Decision harvester: Gemini call failed");
            return;
        }
    };

    // 4. Parse Gemini response — extract JSON array robustly
    // Gemini may wrap JSON in markdown fences or natural language text
    let json_text = if let Some(start) = gemini_response.find('[') {
        if let Some(end) = gemini_response.rfind(']') {
            &gemini_response[start..=end]
        } else {
            gemini_response.trim()
        }
    } else {
        gemini_response.trim()
    };

    let rules: Vec<serde_json::Value> = match serde_json::from_str(json_text) {
        Ok(r) => r,
        Err(e) => {
            warn!(task_id, error = %e, resp = %json_text, "Decision harvester: failed to parse Gemini JSON");
            return;
        }
    };

    let mut written = 0;
    for rule in &rules {
        let key = rule.get("key").and_then(|v| v.as_str()).unwrap_or_default();
        let summary = rule.get("summary").and_then(|v| v.as_str()).unwrap_or_default();
        let detail = rule.get("detail");

        if key.is_empty() || summary.is_empty() {
            continue;
        }

        let input = missiond_core::types::KBRememberInput {
            category: "policy:decision".to_string(),
            key: key.to_string(),
            summary: summary.to_string(),
            detail: detail.cloned(),
            source: Some("decision-harvester".to_string()),
            confidence: Some(0.8),
        };

        match state.mission.db().kb_remember(&input) {
            Ok(result) => {
                info!(key, action = %result.action, "Decision harvester: wrote policy:decision");
                written += 1;
            }
            Err(e) => {
                warn!(key, error = %e, "Decision harvester: kb_remember failed");
            }
        }
    }

    if written > 0 {
        // Write a board note about the harvest
        let _ = state.mission.db().add_board_task_note(
            &missiond_core::types::AddBoardTaskNoteInput {
                task_id: task_id.to_string(),
                content: format!("🧠 [决策引擎] 从 {} 条主控决策中提炼了 {} 条 policy:decision 规则，已写入肌肉记忆。",
                    master_questions.len(), written),
                note_type: Some("progress".to_string()),
                author: Some("decision-harvester".to_string()),
            },
        );
    }
}
