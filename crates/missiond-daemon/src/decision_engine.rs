
use std::sync::atomic::Ordering;

use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::state::AppState;
use crate::event_bus::{DaemonEvent, TraceContext};
use crate::llm_gateway::call_gemini_for_flow;
use crate::memory_scheduler::ensure_memory_slot_by_id;

pub(crate) struct TierResult {
    hit: bool,
    detail: serde_json::Value,
}

/// Save routing trace JSON to the question record.
/// Also increments DaemonStats tier counters based on resolved_tier.
pub(crate) fn save_routing_trace(state: &AppState, question_id: &str, resolved_tier: &str, path: &[serde_json::Value], latency_ms: u128) {
    // Increment tier-specific DaemonStats counters
    match resolved_tier {
        "T1" => { state.stats.decisions_tier1_hit.fetch_add(1, Ordering::Relaxed); }
        "T2" => { state.stats.decisions_tier2_hit.fetch_add(1, Ordering::Relaxed); }
        "T3" => { state.stats.decisions_tier3_dispatched.fetch_add(1, Ordering::Relaxed); }
        _ => {} // "downgraded", "error", "T2-escalated" — no dedicated counter
    }

    let trace = serde_json::json!({
        "resolved_tier": resolved_tier,
        "path": path,
        "latency_ms": latency_ms,
    });
    let _ = state.mission.db().set_question_routing_trace(question_id, &trace.to_string());
}

/// Decision Engine: process pending questions targeted at master.
// @beacon: decision
/// Routes through Tier 1 (KB) → Tier 2 (Gemini) → Tier 3 (slot-decision) responsibility chain.
/// Records routing_trace for each decision for observability.
pub(crate) async fn process_pending_master_questions(state: &AppState) {
    // Query pending questions with target=master (spawn_blocking: batch scan)
    let questions = match state.db_exec.run(|db| db.list_agent_questions(Some("pending"), Some("master"), None)).await {
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
            state.event_bus.publish_traced(
                DaemonEvent::QuestionResolved { question_id: question.id.clone(), resolution: "downgraded".to_string() },
                TraceContext { trace_id: question.task_id.clone(), summary: Some("Anti-loop: retry limit reached".to_string()), ..Default::default() },
            );
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
                            state.event_bus.publish_traced(
                                DaemonEvent::QuestionResolved { question_id: question.id.clone(), resolution: "downgraded".to_string() },
                                TraceContext { trace_id: question.task_id.clone(), summary: Some("Preference miss → downgrade".to_string()), ..Default::default() },
                            );
                            save_routing_trace(state, &question.id, "downgraded", &path, start.elapsed().as_millis());
                        }
                    }
                    Err(_) => {
                        path.push(serde_json::json!({"tier": "T1", "status": "error"}));
                        let _ = state.mission.db().downgrade_question_to_user(&question.id);
                        state.event_bus.publish_traced(
                            DaemonEvent::QuestionResolved { question_id: question.id.clone(), resolution: "downgraded".to_string() },
                            TraceContext { trace_id: question.task_id.clone(), summary: Some("T1 error → downgrade".to_string()), ..Default::default() },
                        );
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
    let tier_start = std::time::Instant::now();
    // Path A: FTS5 search with ranked results (spawn_blocking: FTS5 heavy)
    let q_text = question.question.clone();
    let fts_results = state.db_exec.run(move |db| {
        db.kb_search_ranked(&q_text, Some("policy:decision"), 20)
    }).await.unwrap_or_default();

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
            // Scheduling signal (wake submit consumer)
            state.event_bus.publish(DaemonEvent::TaskCreated { task_id: String::new() });
            // Semantic trace: decision resolved
            state.event_bus.publish_traced(
                DaemonEvent::DecisionResolved {
                    question_id: question.id.clone(),
                    tier: "T1".to_string(),
                    duration_ms: tier_start.elapsed().as_millis() as u64,
                },
                TraceContext {
                    trace_id: question.task_id.clone(),
                    summary: Some(format!("T1 hybrid hit: {}", entry.key)),
                    ..Default::default()
                },
            );

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
    state.event_bus.publish(DaemonEvent::TaskCreated { task_id: String::new() });
    state.event_bus.publish_traced(
        DaemonEvent::DecisionResolved {
            question_id: question.id.clone(),
            tier: "T1".to_string(),
            duration_ms: tier_start.elapsed().as_millis() as u64,
        },
        TraceContext {
            trace_id: question.task_id.clone(),
            summary: Some(format!("T1 FTS hit: {}", top.key)),
            ..Default::default()
        },
    );
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
    let tier_start = std::time::Instant::now();
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
    let full_prompt = format!("{}\n\n---\n\n{}", state.prompts.tier2_system(), user_prompt);

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
                    state.event_bus.publish(DaemonEvent::TaskCreated { task_id: String::new() });
                    state.event_bus.publish_traced(
                        DaemonEvent::DecisionResolved {
                            question_id: question.id.clone(),
                            tier: "T2".to_string(),
                            duration_ms: tier_start.elapsed().as_millis() as u64,
                        },
                        TraceContext {
                            trace_id: question.task_id.clone(),
                            summary: Some(format!("T2 Gemini: {}", decision.chars().take(60).collect::<String>())),
                            ..Default::default()
                        },
                    );
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
        state.prompts.tier3_header(),
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
    prompt_parts.push(state.prompts.tier3_footer().replace("{0}", &question.id));

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
            state.event_bus.publish_traced(
                DaemonEvent::DecisionResolved {
                    question_id: question.id.clone(),
                    tier: "T3".to_string(),
                    duration_ms: 0, // dispatch is async, real duration unknown
                },
                TraceContext {
                    trace_id: question.task_id.clone(),
                    summary: Some("T3 dispatched to slot-decision".to_string()),
                    ..Default::default()
                },
            );
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
    // spawn_blocking: time-based scan
    let stale_questions = match state.db_exec.run(|db| db.find_stale_master_questions(900)).await {
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
        state.event_bus.publish_traced(
            DaemonEvent::QuestionResolved { question_id: question.id.clone(), resolution: "downgraded".to_string() },
            TraceContext { trace_id: question.task_id.clone(), summary: Some("Stale decision: 15min timeout → downgrade".to_string()), ..Default::default() },
        );

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
