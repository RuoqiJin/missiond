use anyhow::{anyhow, Result};
use missiond_core::types::{EvidenceSearchInput, KBRememberInput, KnowledgeReviewInput};
use missiond_mcp::tools::{ToolContent, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tracing::info;

use crate::context::v3_blueprint_runtime::MemoryKbRuntimeConfig;
use crate::events_sync;
use crate::helpers::default_mission_home;
use crate::lenient;
use crate::state::AppState;
use crate::state::{CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES};
use crate::state::{MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};

const MAX_PENDING_BATCH_REPLAYS: u32 = 3;
const MEMORY_PROVIDER_URL_ENV: &str = "MISSIOND_MEMORY_PROVIDER_URL";
const MEMORY_PROVIDER_TOKEN_ENV: &str = "MISSIOND_MEMORY_PROVIDER_TOKEN";
const MEMORY_PROVIDER_MODE_ENV: &str = "MISSIOND_MEMORY_PROVIDER_MODE";

fn classify_memory_input_noise(role: &str, content: &str) -> Option<&'static str> {
    // User utterances are the source of truth for memory extraction. Keep them
    // even when they mention deployment, workers, or diagnostics.
    if role == "user" {
        return None;
    }

    let lower = content.to_ascii_lowercase();
    const DEPLOYMENT_MONITOR_NEEDLES: &[&str] = &[
        "deploy monitor",
        "deployment-monitor",
        "deployment-event-response",
        "deploy-center provenance",
        "xjp_build_wait",
        "xjp_deploy_watch",
        "xjp_deploy_status",
        "deploy_created",
        "build_started",
        "build_succeeded",
        "build_failed",
        "deploy_started",
        "deploy_succeeded",
        "deploy_failed",
        "smoke_succeeded",
        "smoke_failed",
        "rollback_started",
        "rollback_succeeded",
        "rollback_failed",
        "agent_heartbeat",
        "agent_update_started",
        "agent_update_succeeded",
        "agent_update_failed",
        "provenance_changed",
        "provenance_partial",
        "digest_resolution_failed",
        "reported_digest_missing",
        "runner_queued",
        "build_cache_unavailable",
    ];
    if DEPLOYMENT_MONITOR_NEEDLES
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return Some("deployment-monitor");
    }

    if lower.contains("lisp-code-sync")
        && (lower.contains("report")
            || lower.contains("watcher")
            || lower.contains("runtime/lisp-code-sync"))
    {
        return Some("runtime-report");
    }

    if lower.contains("matched skills")
        || lower.contains("board task id")
        || lower.contains("任务完成时")
        || lower.contains("completion protocol")
        || lower.contains("mission_board_update")
        || lower.contains("mission_board_note_add")
    {
        return Some("worker-instruction");
    }

    if lower.contains("## 预加载上下文") || lower.contains("preloaded context") {
        return Some("provider-preamble");
    }

    None
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_memory
    if name == "mission_memory" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        return match action {
            "provider_status" | "status" => {
                handle_inner(state, "mission_memory_provider_status", args).await
            }
            "query" => handle_inner(state, "mission_memory_query", args).await,
            "remember" => handle_inner(state, "mission_memory_remember", args).await,
            "review" => handle_inner(state, "mission_memory_review", args).await,
            "evidence_search" | "evidence-search" => {
                handle_inner(state, "mission_memory_evidence_search", args).await
            }
            "evidence_promote" | "evidence-promote" => {
                handle_inner(state, "mission_memory_evidence_promote", args).await
            }
            "evidence_backfill" | "evidence-backfill" => {
                handle_inner(state, "mission_memory_evidence_backfill", args).await
            }
            "pending" => handle_inner(state, "mission_memory_pending", args).await,
            "pause" => handle_inner(state, "mission_memory_pause", args).await,
            "token_stats" => {
                // Delegate to conversation handler which has mission_token_stats
                crate::handlers::conversation::handle(state, "mission_token_stats", args).await
            }
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemoryProviderSelection {
    XjpMemory {
        base_url: String,
        token: Option<String>,
    },
    LocalPostgresCompatibility,
    NullMemory,
}

impl MemoryProviderSelection {
    fn from_env() -> Self {
        if let Ok(url) = std::env::var(MEMORY_PROVIDER_URL_ENV) {
            let base_url = url.trim().trim_end_matches('/').to_string();
            if !base_url.is_empty() {
                let token = std::env::var(MEMORY_PROVIDER_TOKEN_ENV)
                    .ok()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                return Self::XjpMemory { base_url, token };
            }
        }

        match std::env::var(MEMORY_PROVIDER_MODE_ENV)
            .unwrap_or_else(|_| "null-memory".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "local" | "local-postgres" | "local_postgres" | "compat" | "compatibility" => {
                Self::LocalPostgresCompatibility
            }
            _ => Self::NullMemory,
        }
    }

    fn status_payload(&self) -> Value {
        match self {
            Self::XjpMemory { base_url, token } => json!({
                "provider": "xjp-memory",
                "configured": true,
                "baseUrl": base_url,
                "auth": if token.is_some() { "bearer-token-configured" } else { "none" },
                "mode": "remote-provider",
            }),
            Self::LocalPostgresCompatibility => json!({
                "provider": "local-postgres-memory",
                "configured": true,
                "mode": "compatibility-provider",
                "note": "mission_memory query/remember/review is routed to local mission_kb_* compatibility tools.",
            }),
            Self::NullMemory => json!({
                "provider": "null-memory",
                "configured": false,
                "mode": "disabled",
                "requiredEnv": [MEMORY_PROVIDER_URL_ENV],
                "optionalEnv": [MEMORY_PROVIDER_TOKEN_ENV, MEMORY_PROVIDER_MODE_ENV],
            }),
        }
    }
}

fn get_string_any<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn get_bool_any(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|v| v.as_bool()))
}

fn get_usize_any(value: &Value, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|v| v.as_u64())
            .and_then(|n| usize::try_from(n).ok())
    })
}

fn get_i64_any(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
        })
    })
}

fn get_string_list_any(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|field| match field {
            Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>(),
            ),
            Value::String(text) => Some(
                text.split(',')
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn tool_result_to_value(result: &ToolResult) -> Value {
    result
        .content
        .first()
        .and_then(|content| match content {
            ToolContent::Text { text } => serde_json::from_str(text).ok(),
        })
        .unwrap_or_else(|| json!({"content": result.content}))
}

fn provider_scope_from_args(args: &Value) -> Value {
    let explicit_scope = args
        .get("scope")
        .and_then(|scope| scope.as_object())
        .cloned()
        .unwrap_or_default();
    let mut scope = serde_json::Map::new();
    for (key, value) in explicit_scope {
        scope.insert(key, value);
    }

    let fields = [
        ("tenant_id", ["tenant_id", "tenantId"].as_slice()),
        ("universe_id", ["universe_id", "universeId"].as_slice()),
        (
            "project_id",
            ["project_id", "projectId", "project"].as_slice(),
        ),
        ("user_id", ["user_id", "userId"].as_slice()),
        ("source_type", ["source_type", "sourceType"].as_slice()),
        ("source_id", ["source_id", "sourceId"].as_slice()),
        ("authority", ["authority"].as_slice()),
        (
            "privacy_class",
            ["privacy_class", "privacyClass"].as_slice(),
        ),
    ];
    for (target, aliases) in fields {
        if !scope.contains_key(target) {
            if let Some(value) = get_string_any(args, aliases) {
                scope.insert(target.to_string(), json!(value));
            }
        }
    }
    Value::Object(scope)
}

fn provider_query_payload(args: &Value) -> Value {
    json!({
        "scope": provider_scope_from_args(args),
        "query": get_string_any(args, &["query"]).unwrap_or_default(),
        "include_archived": get_bool_any(args, &["include_archived", "includeArchived"]).unwrap_or(false),
        "limit": get_usize_any(args, &["limit"]).unwrap_or(20).clamp(1, 100),
    })
}

fn provider_remember_payload(args: &Value) -> Result<Value> {
    let text = get_string_any(args, &["text", "summary", "content"])
        .ok_or_else(|| anyhow!("mission_memory remember requires text"))?;
    let tags = args.get("tags").cloned().unwrap_or_else(|| json!([]));
    Ok(json!({
        "scope": provider_scope_from_args(args),
        "text": text,
        "tags": tags,
    }))
}

fn provider_review_payload(args: &Value) -> Result<Value> {
    let memory_id = get_string_any(
        args,
        &["memory_id", "memoryId", "knowledge_id", "knowledgeId"],
    )
    .ok_or_else(|| anyhow!("mission_memory review requires memoryId"))?;
    let state = get_string_any(args, &["state"])
        .ok_or_else(|| anyhow!("mission_memory review requires state"))?;
    let rationale = get_string_any(args, &["rationale"])
        .ok_or_else(|| anyhow!("mission_memory review requires rationale"))?;
    Ok(json!({
        "memory_id": memory_id,
        "state": state,
        "rationale": rationale,
        "reviewer": get_string_any(args, &["reviewer"]).unwrap_or("missiond"),
    }))
}

async fn call_xjp_memory(
    state: &AppState,
    base_url: &str,
    token: Option<&str>,
    method: reqwest::Method,
    path: &str,
    payload: Option<Value>,
) -> Result<ToolResult> {
    let url = format!("{base_url}{path}");
    let mut request = state.http_client.request(method, &url);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(payload) = payload {
        request = request.json(&payload);
    }
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({ "raw": body }));
    if !status.is_success() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_HTTP_ERROR",
                format!("xjp-memory returned HTTP {status} for {path}"),
            )
            .with_suggestion(format!(
                "Check {MEMORY_PROVIDER_URL_ENV}, provider health, and secret-store token configuration."
            )),
        ));
    }
    Ok(ToolResult::json_pretty(&json!({
        "provider": "xjp-memory",
        "path": path,
        "response": parsed,
    })))
}

async fn handle_provider_status(state: &AppState) -> Result<ToolResult> {
    let selection = MemoryProviderSelection::from_env();
    match selection {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            let remote = call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::GET,
                "/v1/memory/provider_status",
                None,
            )
            .await?;
            Ok(remote)
        }
        other => Ok(ToolResult::json_pretty(&other.status_payload())),
    }
}

async fn handle_provider_query(state: &AppState, args: Value) -> Result<ToolResult> {
    match MemoryProviderSelection::from_env() {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::POST,
                "/v1/memory/query",
                Some(provider_query_payload(&args)),
            )
            .await
        }
        MemoryProviderSelection::LocalPostgresCompatibility => {
            let local_args = json!({
                "action": "search",
                "query": get_string_any(&args, &["query"]).unwrap_or_default(),
                "project": get_string_any(&args, &["project", "projectId", "project_id"]),
                "include_archived": get_bool_any(&args, &["include_archived", "includeArchived"]).unwrap_or(false),
                "limit": get_usize_any(&args, &["limit"]).unwrap_or(20),
            });
            crate::handlers::knowledge::kb::handle(state, "mission_kb_query", local_args).await
        }
        MemoryProviderSelection::NullMemory => Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_DISABLED",
                "mission_memory query requires a configured memory provider.",
            )
            .with_suggestion(format!(
                "Set {MEMORY_PROVIDER_URL_ENV}=https://.../xjp-memory or {MEMORY_PROVIDER_MODE_ENV}=local-postgres for compatibility."
            )),
        )),
    }
}

async fn handle_provider_remember(state: &AppState, args: Value) -> Result<ToolResult> {
    match MemoryProviderSelection::from_env() {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::POST,
                "/v1/memory/remember",
                Some(provider_remember_payload(&args)?),
            )
            .await
        }
        MemoryProviderSelection::LocalPostgresCompatibility => {
            let local_args = json!({
                "category": get_string_any(&args, &["category"]).unwrap_or("memory:decision"),
                "key": get_string_any(&args, &["key"]).unwrap_or("mission-memory-provider-write"),
                "summary": get_string_any(&args, &["summary", "text", "content"]).unwrap_or_default(),
                "detail": args.get("detail").cloned().unwrap_or_else(|| json!({
                    "source": "mission_memory.local-postgres-compatibility",
                    "scope": provider_scope_from_args(&args),
                    "tags": args.get("tags").cloned().unwrap_or_else(|| json!([])),
                })),
                "source": get_string_any(&args, &["source"]).unwrap_or("mission_memory"),
                "confidence": args.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.8),
                "project": get_string_any(&args, &["project", "projectId", "project_id"]),
            });
            crate::handlers::knowledge::kb::handle(state, "mission_kb_remember", local_args).await
        }
        MemoryProviderSelection::NullMemory => Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_DISABLED",
                "mission_memory remember requires a configured memory provider.",
            )
            .with_suggestion(format!(
                "Set {MEMORY_PROVIDER_URL_ENV}=https://.../xjp-memory or {MEMORY_PROVIDER_MODE_ENV}=local-postgres for compatibility."
            )),
        )),
    }
}

async fn handle_provider_review(state: &AppState, args: Value) -> Result<ToolResult> {
    match MemoryProviderSelection::from_env() {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::POST,
                "/v1/memory/review",
                Some(provider_review_payload(&args)?),
            )
            .await
        }
        MemoryProviderSelection::LocalPostgresCompatibility => {
            let local_args = json!({
                "action": "upsert",
                "knowledge_id": get_string_any(&args, &["knowledge_id", "knowledgeId", "memory_id", "memoryId"]),
                "key": get_string_any(&args, &["key"]),
                "state": get_string_any(&args, &["state"]),
                "rationale": get_string_any(&args, &["rationale"]),
                "reviewer": get_string_any(&args, &["reviewer"]).unwrap_or("mission_memory"),
                "confidence": args.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.8),
                "evidence_refs": args.get("evidence_refs").cloned().unwrap_or_else(|| json!({
                    "source": "mission_memory.local-postgres-compatibility"
                })),
            });
            crate::handlers::knowledge::kb::handle(state, "mission_kb_review", local_args).await
        }
        MemoryProviderSelection::NullMemory => Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_DISABLED",
                "mission_memory review requires a configured memory provider.",
            )
            .with_suggestion(format!(
                "Set {MEMORY_PROVIDER_URL_ENV}=https://.../xjp-memory or {MEMORY_PROVIDER_MODE_ENV}=local-postgres for compatibility."
            )),
        )),
    }
}

fn provider_evidence_search_payload(args: &Value) -> Value {
    json!({
        "scope": provider_scope_from_args(args),
        "query": get_string_any(args, &["query"]).unwrap_or_default(),
        "allowed_lanes": get_string_list_any(args, &["allowed_lanes", "allowedLanes", "lanes"]),
        "project_id": get_string_any(args, &["project", "projectId", "project_id"]),
        "task_id": get_string_any(args, &["taskId", "task_id"]),
        "include_global": get_bool_any(args, &["include_global", "includeGlobal"]).unwrap_or(true),
        "limit": get_usize_any(args, &["limit"]).unwrap_or(20).clamp(1, 100),
    })
}

async fn handle_provider_evidence_search(state: &AppState, args: Value) -> Result<ToolResult> {
    match MemoryProviderSelection::from_env() {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::POST,
                "/v1/memory/evidence/search",
                Some(provider_evidence_search_payload(&args)),
            )
            .await
        }
        MemoryProviderSelection::LocalPostgresCompatibility => {
            let input = EvidenceSearchInput {
                query: get_string_any(&args, &["query"]).unwrap_or_default().to_string(),
                allowed_lanes: get_string_list_any(&args, &["allowed_lanes", "allowedLanes", "lanes"]),
                project_id: get_string_any(&args, &["project", "projectId", "project_id"])
                    .map(ToOwned::to_owned),
                task_id: get_string_any(&args, &["taskId", "task_id"]).map(ToOwned::to_owned),
                include_global: get_bool_any(&args, &["include_global", "includeGlobal"])
                    .unwrap_or(true),
                limit: get_i64_any(&args, &["limit"]).unwrap_or(20).clamp(1, 100),
            };
            let items = state
                .store
                .search_evidence_items(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&json!({
                "ok": true,
                "schema": "missiond.memory-evidence-search.v1",
                "provider": "local-postgres-memory",
                "filter_before_vector": true,
                "query": input.query,
                "allowed_lanes": input.allowed_lanes,
                "project_id": input.project_id,
                "task_id": input.task_id,
                "include_global": input.include_global,
                "count": items.len(),
                "items": items,
            })))
        }
        MemoryProviderSelection::NullMemory => Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_DISABLED",
                "mission_memory evidence_search requires a configured memory provider.",
            )
            .with_suggestion(format!(
                "Set {MEMORY_PROVIDER_URL_ENV}=https://.../xjp-memory or {MEMORY_PROVIDER_MODE_ENV}=local-postgres for compatibility."
            )),
        )),
    }
}

fn promotion_bound_present(args: &Value) -> bool {
    get_string_any(
        args,
        &[
            "ttl",
            "ttlDays",
            "valid_until",
            "validUntil",
            "version_bound",
            "versionBound",
            "release_id",
            "releaseId",
            "commit",
            "commit_sha",
            "commitSha",
        ],
    )
    .is_some()
        || args.get("ttl_days").and_then(Value::as_i64).is_some()
}

fn evidence_promotion_requires_bound(summary: &str, category: &str) -> bool {
    let text = format!("{summary} {category}").to_ascii_lowercase();
    [
        "deploy",
        "deployment",
        "release",
        "runtime",
        "config",
        "dependency",
        "migration",
        "database",
        "image",
        "compose",
        "workflow",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn default_promotion_category(lane_id: &str) -> &'static str {
    match lane_id {
        "conversation_audit" => "memory:conversation-evidence",
        "skill_evidence" => "memory:skill-evidence",
        "support_refs" => "memory:support-reference",
        _ => "memory:evidence",
    }
}

async fn handle_provider_evidence_promote(state: &AppState, args: Value) -> Result<ToolResult> {
    match MemoryProviderSelection::from_env() {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::POST,
                "/v1/memory/evidence/promote",
                Some(args),
            )
            .await
        }
        MemoryProviderSelection::LocalPostgresCompatibility => {
            let evidence_id = get_string_any(&args, &["evidence_id", "evidenceId", "id"])
                .ok_or_else(|| anyhow!("evidence_promote requires evidence_id"))?;
            let evidence = state
                .store
                .get_evidence_item(evidence_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("evidence item not found: {evidence_id}"))?;

            if matches!(evidence.lane_id.as_str(), "runtime_truth" | "project_ssot") {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        "EVIDENCE_PROMOTION_NOT_ALLOWED",
                        "runtime_truth and project_ssot are already authoritative lanes and must not be promoted into KB truth.",
                    )
                    .with_suggestion(
                        "Reference the evidence item directly, or promote only reviewed KB/conversation/skill/support evidence.",
                    ),
                ));
            }

            let category = get_string_any(&args, &["category"])
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| default_promotion_category(&evidence.lane_id).to_string());
            let key = get_string_any(&args, &["key"])
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("promoted:{}:{}", evidence.lane_id, evidence.id));
            let summary = get_string_any(&args, &["summary", "text", "content"])
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| evidence.summary.clone());

            if evidence_promotion_requires_bound(&summary, &category)
                && !promotion_bound_present(&args)
            {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        "EVIDENCE_PROMOTION_BOUND_REQUIRED",
                        "Deployment/config/dependency evidence must include ttl, valid_until, version_bound, release_id, or commit before promotion.",
                    )
                    .with_suggestion(
                        "Retry with ttlDays, validUntil, versionBound, releaseId, or commitSha.",
                    ),
                ));
            }

            let detail = json!({
                "schema": "missiond.promoted-evidence-detail.v1",
                "source": "mission_memory.evidence_promote",
                "promotion": {
                    "ttl": get_string_any(&args, &["ttl"]),
                    "ttl_days": args.get("ttlDays").or_else(|| args.get("ttl_days")).cloned(),
                    "valid_until": get_string_any(&args, &["validUntil", "valid_until"]),
                    "version_bound": get_string_any(&args, &["versionBound", "version_bound"]),
                    "release_id": get_string_any(&args, &["releaseId", "release_id"]),
                    "commit": get_string_any(&args, &["commit", "commitSha", "commit_sha"]),
                },
                "evidence_item": evidence,
                "extra_detail": args.get("detail").cloned().unwrap_or(Value::Null),
            });
            let remember = state
                .store
                .kb_remember(&KBRememberInput {
                    category,
                    key,
                    summary,
                    detail: Some(detail),
                    source: Some("evidence_lane_promotion".to_string()),
                    confidence: Some(args.get("confidence").and_then(Value::as_f64).unwrap_or(0.8)),
                    project_id: get_string_any(&args, &["project", "projectId", "project_id"])
                        .map(ToOwned::to_owned)
                        .or_else(|| evidence.project_id.clone()),
                })
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            let review = state
                .store
                .kb_review_upsert(&KnowledgeReviewInput {
                    knowledge_id: remember.entry.id.clone(),
                    state: "active".to_string(),
                    batch_id: get_string_any(&args, &["batchId", "batch_id"])
                        .unwrap_or("evidence-promotion")
                        .to_string(),
                    reviewer: get_string_any(&args, &["reviewer"])
                        .unwrap_or("mission_memory.evidence_promote")
                        .to_string(),
                    rationale: get_string_any(&args, &["rationale"])
                        .unwrap_or("Promoted reviewed compact evidence item into active KB with provenance.")
                        .to_string(),
                    evidence_refs: json!({
                        "evidence_item_id": evidence_id,
                        "lane_id": remember.entry.detail.as_ref()
                            .and_then(|detail| detail.get("evidence_item"))
                            .and_then(|item| item.get("laneId").or_else(|| item.get("lane_id")))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "source": "mission_memory.evidence_promote",
                    }),
                    superseded_by: None,
                    confidence: args.get("reviewConfidence")
                        .or_else(|| args.get("review_confidence"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.8),
                    applied_at: None,
                })
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            Ok(ToolResult::json_pretty(&json!({
                "ok": true,
                "schema": "missiond.memory-evidence-promotion.v1",
                "provider": "local-postgres-memory",
                "promotion": {
                    "evidence_id": evidence_id,
                    "knowledge_id": remember.entry.id,
                    "action": remember.action,
                    "review_state": review.state,
                    "non_destructive": true,
                },
                "knowledge": remember.entry,
                "review": review,
            })))
        }
        MemoryProviderSelection::NullMemory => Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_DISABLED",
                "mission_memory evidence_promote requires a configured memory provider.",
            )
            .with_suggestion(format!(
                "Set {MEMORY_PROVIDER_URL_ENV}=https://.../xjp-memory or {MEMORY_PROVIDER_MODE_ENV}=local-postgres for compatibility."
            )),
        )),
    }
}

async fn handle_provider_evidence_backfill(state: &AppState, args: Value) -> Result<ToolResult> {
    match MemoryProviderSelection::from_env() {
        MemoryProviderSelection::XjpMemory { base_url, token } => {
            call_xjp_memory(
                state,
                &base_url,
                token.as_deref(),
                reqwest::Method::POST,
                "/v1/memory/evidence/backfill",
                Some(args),
            )
            .await
        }
        MemoryProviderSelection::LocalPostgresCompatibility => {
            let source = get_string_any(&args, &["source", "sourceType", "source_type"])
                .unwrap_or("conversation");
            let limit = get_i64_any(&args, &["limit"]).unwrap_or(100).clamp(1, 500);
            let mut results = serde_json::Map::new();
            if matches!(source, "conversation" | "conversations" | "all") {
                let count = state
                    .store
                    .backfill_conversation_evidence_items(limit)
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                results.insert(
                    "conversation".to_string(),
                    json!({
                        "evidence_items_written": count,
                        "raw_deleted": false,
                        "raw_layer": "conversation_messages",
                    }),
                );
            }
            if matches!(source, "skill" | "skills" | "all") {
                let query = get_string_any(&args, &["query"])
                    .unwrap_or("deploy rollback migration smoke health credential database service");
                let context_result = super::context_gather::handle(
                    state,
                    "mission_context_gather",
                    json!({
                        "query": query,
                        "project": get_string_any(&args, &["project", "projectId", "project_id"]),
                        "source_profile": "deploy_ops",
                        "include_skill": true,
                        "include_infra": true,
                        "include_credentials": false,
                        "include_raw_sources": false,
                        "persist": true,
                        "limit": limit.min(25),
                    }),
                )
                .await?;
                let context_payload = tool_result_to_value(&context_result);
                results.insert(
                    "skill".to_string(),
                    json!({
                        "context_gather_persisted": true,
                        "evidence_lane_persistence": context_payload.get("evidence_lane_persistence").cloned().unwrap_or(Value::Null),
                        "raw_deleted": false,
                        "raw_layer": "skill files and infra support refs",
                    }),
                );
            }
            Ok(ToolResult::json_pretty(&json!({
                "ok": true,
                "schema": "missiond.memory-evidence-backfill.v1",
                "provider": "local-postgres-memory",
                "source": source,
                "limit": limit,
                "results": results,
                "non_destructive": true,
            })))
        }
        MemoryProviderSelection::NullMemory => Ok(ToolResult::structured_error(
            ToolError::new(
                "MEMORY_PROVIDER_DISABLED",
                "mission_memory evidence_backfill requires a configured memory provider.",
            )
            .with_suggestion(format!(
                "Set {MEMORY_PROVIDER_URL_ENV}=https://.../xjp-memory or {MEMORY_PROVIDER_MODE_ENV}=local-postgres for compatibility."
            )),
        )),
    }
}

fn load_memory_kb_config() -> Result<MemoryKbRuntimeConfig> {
    MemoryKbRuntimeConfig::load_for_current_dir()
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Pluggable Memory Provider Facade =====
        "mission_memory_provider_status" => handle_provider_status(state).await,
        "mission_memory_query" => handle_provider_query(state, args).await,
        "mission_memory_remember" => handle_provider_remember(state, args).await,
        "mission_memory_review" => handle_provider_review(state, args).await,
        "mission_memory_evidence_search" => handle_provider_evidence_search(state, args).await,
        "mission_memory_evidence_promote" => handle_provider_evidence_promote(state, args).await,
        "mission_memory_evidence_backfill" => handle_provider_evidence_backfill(state, args).await,

        // ===== Memory Extraction =====
        // Message-level pipeline tracking: returns pending messages with IDs.
        // State auto-committed by Daemon on extraction completion — no manual done() needed.
        "mission_memory_pending" | "mission_memory_pending_user" => {
            // De-bounce guard: if realtime extraction is in-flight and we already served
            // pending messages in this cycle, replay the cached batch a few times. This
            // keeps provider context compaction recoverable while still preventing a
            // tight polling loop from being mistaken for new work.
            {
                let mut es = state.extraction_state.write().await;
                if es.pending_served
                    && matches!(
                        es.phase,
                        crate::state::ExtractionPhase::Sending
                            | crate::state::ExtractionPhase::WaitingForSlotIdle
                    )
                {
                    if let Some(payload) = es.pending_payload.clone() {
                        if es.pending_replay_count < MAX_PENDING_BATCH_REPLAYS {
                            es.pending_replay_count += 1;
                            let batch_id = es
                                .pending_batch_id
                                .clone()
                                .unwrap_or_else(|| "unknown-batch".to_string());
                            let replay_count = es.pending_replay_count;
                            return Ok(ToolResult::text(&format!(
                                "[realtime-extract replay] batch={} replay={}/{}\n\
                                 这是一份已返回批次的缓存重放，用于恢复 provider context compaction 后丢失的上下文；请基于本批内容输出总结，不要继续轮询。\n\n{}",
                                batch_id, replay_count, MAX_PENDING_BATCH_REPLAYS, payload
                            )));
                        }
                    }
                    return Ok(ToolResult::structured_error(
                        ToolError::new(
                            "MEMORY_PENDING_ALREADY_SERVED",
                            "当前 realtime extraction 批次已经由 mission_memory_pending 返回过，且可重放缓存缺失或已达到重放上限。",
                        )
                        .with_suggestion(
                            "请基于上一轮已经返回或已重放的消息直接输出总结；水位线由系统在本轮完成后推进，下一批会自动调度。",
                        ),
                    ));
                }
            }

            let config = load_memory_kb_config()?;
            let pending_msg_limit = config.pending_message_limit;
            let pending = state
                .store
                .get_pending_realtime_messages_with_limit(pending_msg_limit)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if pending.is_empty() {
                return Ok(ToolResult::text("没有待分析的新对话内容。"));
            }

            let mut output = String::new();
            let mut all_msg_ids: Vec<i64> = Vec::new();
            let mut skip_counts: BTreeMap<&'static str, u32> = BTreeMap::new();
            let mut user_count = 0usize;
            for (session_id, project, msgs) in &pending {
                let mut session_output = String::new();
                for msg in msgs {
                    if let Some(reason) = classify_memory_input_noise(&msg.role, &msg.content) {
                        *skip_counts.entry(reason).or_insert(0) += 1;
                        continue;
                    }
                    all_msg_ids.push(msg.id);
                    if msg.role == "user" {
                        user_count += 1;
                        session_output.push_str(&format!(
                            "[#{}][{}] ★ user: {}\n\n",
                            msg.id, msg.timestamp, msg.content
                        ));
                    } else if msg.role == "tool_result" {
                        let max_chars = config.tool_result_preview_chars;
                        let content = if msg.content.len() > max_chars {
                            let end = events_sync::floor_char_boundary(&msg.content, max_chars);
                            format!("{}…({}字符)", &msg.content[..end], msg.content.len())
                        } else {
                            msg.content.clone()
                        };
                        session_output.push_str(&format!(
                            "[#{}][{}] tool_result: {}\n\n",
                            msg.id, msg.timestamp, content
                        ));
                    } else {
                        let max_chars = config.assistant_preview_chars;
                        let content = if msg.content.len() > max_chars {
                            let end = events_sync::floor_char_boundary(&msg.content, max_chars);
                            format!("{}…", &msg.content[..end])
                        } else {
                            msg.content.clone()
                        };
                        session_output.push_str(&format!(
                            "[#{}][{}] assistant: {}\n\n",
                            msg.id, msg.timestamp, content
                        ));
                    }
                }
                if !session_output.is_empty() {
                    output.push_str(&format!(
                        "## session: {} (project: {})\n\n",
                        session_id, project
                    ));
                    output.push_str(&session_output);
                }
            }
            if !skip_counts.is_empty() {
                let mut es = state.extraction_state.write().await;
                for (reason, count) in &skip_counts {
                    es.record_input_skip(reason, *count);
                }
            }

            let session_count = pending.len();
            let msg_count = all_msg_ids.len();
            let truncated_note = if msg_count >= pending_msg_limit {
                format!(
                    " ⚠️ 已达上限 {}，可能还有更多未显示的消息。处理完当前批次后系统将自动推送下一批。",
                    pending_msg_limit
                )
            } else {
                String::new()
            };
            let batch_id = format!("batch-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
            let skip_note = if skip_counts.is_empty() {
                String::new()
            } else {
                let parts = skip_counts
                    .iter()
                    .map(|(reason, count)| format!("{reason}={count}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "输入过滤诊断: 已跳过 {} 条噪声消息 ({parts})。\n",
                    skip_counts.values().sum::<u32>()
                )
            };
            let header = format!(
                "[realtime-extract] [{}] {} 个会话, {} 条消息 (其中 {} 条用户消息){}\n\
                 {}\
                 水位线由系统自动管理，处理完毕后直接输出总结即可，无需调用 done 工具。\n\n\
                 ★ = 用户原话，优先级最高。每句用户消息都是刻意的。\n\
                 assistant 消息仅提供上下文，不需逐条分析。\n\
                 tool_result 消息包含工具输出（文件内容、命令结果），提供操作上下文。\n\n\
                 提取规则:\n\
                 - 用户偏好/纠正/否定 → category: preference (最高优先)\n\
                 - 架构决策/技术事实 → category: memory 或子分类\n\
                 - 「好」「行」= 用户认可 AI 方案，记录为决策\n\
                 - 「别...」「不要...」= 高价值偏好\n\
                 - 运维痛点/调试弯路 → category: memory:ops / memory:debug\n\
                 - 不存: 纯任务指令、当天工作日志、代码提交记录\n\
                 - 存入前用 mission_kb_search 检查去重\n\n",
                batch_id, session_count, msg_count, user_count, truncated_note, skip_note,
            );
            let rendered_payload = format!("{}{}", header, output);

            // Set latch: mark pending as served for this extraction cycle
            {
                let mut es = state.extraction_state.write().await;
                if matches!(
                    es.phase,
                    crate::state::ExtractionPhase::Sending
                        | crate::state::ExtractionPhase::WaitingForSlotIdle
                ) {
                    es.mark_pending_batch_served(batch_id, rendered_payload.clone());
                }
            }

            Ok(ToolResult::text(&rendered_payload))
        }

        "mission_memory_pause" => {
            #[derive(Deserialize)]
            struct Args {
                #[serde(default, deserialize_with = "lenient::option_bool")]
                paused: Option<bool>,
            }
            let args: Args = serde_json::from_value(args).unwrap_or(Args { paused: None });
            let current = state
                .control_manager
                .current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
            // Route through ControlTree (single source of truth)
            let new_val = args.paused.unwrap_or(!current); // toggle if not specified
            state
                .control_manager
                .set_domain(crate::control_tree::CtlDomain::Memory, new_val);
            if new_val {
                info!("Memory extraction PAUSED by user (via ControlTree domain)");
            } else {
                // Clean up legacy flag file if it exists
                let flag = default_mission_home().join("memory_paused");
                let _ = std::fs::remove_file(&flag);
                info!("Memory extraction RESUMED by user (via ControlTree domain)");
            }
            Ok(ToolResult::text(if new_val {
                "记忆任务已暂停（2 小时后自动恢复）。调用 mission_memory_pause(paused: false) 手动恢复。"
            } else {
                "记忆任务已恢复。"
            }))
        }

        "mission_memory_status" => {
            let paused = state
                .control_manager
                .current()
                .is_domain_paused(crate::control_tree::CtlDomain::Memory);
            let now = chrono::Utc::now().timestamp();

            // Fast lane state
            let fast_es = state.extraction_state.read().await;
            let fast_busy = state
                .memory_slot_busy_since
                .load(std::sync::atomic::Ordering::Relaxed);
            let fast_lane = serde_json::json!({
                "slotId": MEMORY_SLOT_ID,
                "phase": format!("{:?}", fast_es.phase),
                "activeType": fast_es.active_type,
                "phaseAge": if fast_es.phase_started_at > 0 { now - fast_es.phase_started_at } else { 0 },
                "busySince": fast_busy,
                "busyDuration": if fast_busy > 0 { now - fast_busy } else { 0 },
                "currentTargets": fast_es.watermark_targets.iter()
                    .map(|(sid, _)| sid.clone()).collect::<Vec<_>>(),
                "currentTaskId": fast_es.current_task_id,
                "pendingServed": fast_es.pending_served,
                "pendingBatchId": fast_es.pending_batch_id,
                "pendingReplayCount": fast_es.pending_replay_count,
                "inputSkipDiagnostics": fast_es.input_skip_diagnostics(),
            });
            drop(fast_es);

            // Slow lane state
            let slow_es = state.slow_extraction_state.read().await;
            let slow_busy = state
                .slow_slot_busy_since
                .load(std::sync::atomic::Ordering::Relaxed);
            let slow_lane = serde_json::json!({
                "slotId": MEMORY_SLOW_SLOT_ID,
                "phase": format!("{:?}", slow_es.phase),
                "activeType": slow_es.active_type,
                "phaseAge": if slow_es.phase_started_at > 0 { now - slow_es.phase_started_at } else { 0 },
                "busySince": slow_busy,
                "busyDuration": if slow_busy > 0 { now - slow_busy } else { 0 },
                "currentConvId": slow_es.current_deep_conv_id,
                "currentTaskId": slow_es.current_task_id,
                "currentOutputCount": slow_es.current_output_count,
                "zeroOutputCount": slow_es.deep_analysis_zero_output_count,
                "zeroOutputFuseUntil": slow_es.deep_analysis_fuse_until,
                "zeroOutputFuseActive": slow_es.deep_analysis_fuse_active(now),
                "inputSkipDiagnostics": slow_es.input_skip_diagnostics(),
            });
            drop(slow_es);

            // Pending counts
            let pending_realtime = state.store.count_pending_realtime().await.unwrap_or(0);
            let pending_deep = state
                .store
                .count_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
                .await
                .unwrap_or(0);

            // Timestamps
            let last_consolidation = state
                .store
                .last_completed_slot_task_at("kb_consolidation")
                .await
                .unwrap_or(None)
                .unwrap_or(0);
            let last_gc = state
                .store
                .daemon_state_get("last_auto_gc_at")
                .await
                .unwrap_or(None)
                .unwrap_or(0);

            // KB stats (full — includes mostAccessed, oldest, subcategories)
            let kb_stats = state
                .store
                .kb_stats()
                .await
                .map(|s| {
                    serde_json::json!({
                        "total": s["total"],
                        "categories": s.get("categoryRollup").unwrap_or(&s["categories"]),
                        "subcategories": s["categories"],
                        "neverAccessed": s["neverAccessed"],
                        "mostAccessed": s["mostAccessed"],
                        "oldest": s["oldest"],
                    })
                })
                .unwrap_or(serde_json::json!(null));

            // Recent memory slot tasks (last 15 across both slots)
            let mut recent: Vec<serde_json::Value> = Vec::new();
            for sid in &[MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID] {
                if let Ok(tasks) = state.store.list_slot_tasks(Some(sid), None, None, 10).await {
                    for t in tasks {
                        recent.push(serde_json::json!({
                            "id": t.id,
                            "slotId": t.slot_id,
                            "taskType": t.task_type,
                            "status": t.status,
                            "durationMs": t.duration_ms,
                            "createdAt": t.created_at,
                            "error": t.error,
                            "outputCount": t.output_count,
                            "sourceSessions": t.source_sessions,
                            "conversationId": t.conversation_id,
                        }));
                    }
                }
            }
            recent.sort_by(|a, b| {
                let ta = a["createdAt"].as_str().unwrap_or("");
                let tb = b["createdAt"].as_str().unwrap_or("");
                tb.cmp(ta)
            });
            recent.truncate(15);

            // Queue detail (per-session / per-conversation)
            let realtime_detail: Vec<serde_json::Value> = state.store.pending_realtime_detail().await
                .unwrap_or_default()
                .into_iter()
                .map(|(sid, cnt, oldest)| serde_json::json!({"sessionId": sid, "msgCount": cnt, "oldest": oldest}))
                .collect();
            let deep_detail: Vec<serde_json::Value> = state.store.pending_deep_detail(
                CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES
            ).await.unwrap_or_default()
                .into_iter()
                .map(|(id, ended, retries)| serde_json::json!({"conversationId": id, "endedAt": ended, "retries": retries}))
                .collect();

            Ok(ToolResult::json(&serde_json::json!({
                "paused": paused,
                "fastLane": fast_lane,
                "slowLane": slow_lane,
                "pendingRealtime": pending_realtime,
                "pendingDeep": pending_deep,
                "inputFilter": {
                    "slotExclusions": ["slot-memory*", "slot-diagnosis*", "agent-*"],
                    "textNoiseReasons": [
                        "deployment-monitor",
                        "runtime-report",
                        "worker-instruction",
                        "provider-preamble"
                    ],
                },
                "realtimeDetail": realtime_detail,
                "deepDetail": deep_detail,
                "lastKbConsolidation": if last_consolidation > 0 {
                    chrono::DateTime::from_timestamp(last_consolidation, 0)
                        .map(|d| d.to_rfc3339()).unwrap_or_default()
                } else { String::new() },
                "lastAutoGc": if last_gc > 0 {
                    chrono::DateTime::from_timestamp(last_gc, 0)
                        .map(|d| d.to_rfc3339()).unwrap_or_default()
                } else { String::new() },
                "kbStats": kb_stats,
                "recentTasks": recent,
            })))
        }

        _ => Err(anyhow!("Unknown memory tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_memory_input_noise, evidence_promotion_requires_bound, get_string_list_any,
        promotion_bound_present, provider_evidence_search_payload, provider_query_payload,
        provider_remember_payload,
    };
    use serde_json::json;

    #[test]
    fn memory_input_filter_preserves_user_utterances() {
        assert_eq!(
            classify_memory_input_noise("user", "deploy_succeeded 这个事件要记入 EventBus"),
            None
        );
    }

    #[test]
    fn memory_input_filter_classifies_deployment_monitor_noise() {
        assert_eq!(
            classify_memory_input_noise("assistant", "deploy monitor: deploy_succeeded"),
            Some("deployment-monitor")
        );
        assert_eq!(
            classify_memory_input_noise("tool_result", "agent_heartbeat from deploy-agent"),
            Some("deployment-monitor")
        );
        assert_eq!(
            classify_memory_input_noise(
                "assistant",
                "deployment-event-response observed build_started then reported_digest_missing; use xjp_deploy_watch",
            ),
            Some("deployment-monitor")
        );
        assert_eq!(
            classify_memory_input_noise(
                "tool_result",
                "deploy-center provenance_partial with agent_update_failed diagnostic",
            ),
            Some("deployment-monitor")
        );
    }

    #[test]
    fn memory_input_filter_classifies_runtime_and_worker_noise() {
        assert_eq!(
            classify_memory_input_noise("assistant", "lisp-code-sync watcher report path"),
            Some("runtime-report")
        );
        assert_eq!(
            classify_memory_input_noise("assistant", "Board Task ID: abc; mission_board_update"),
            Some("worker-instruction")
        );
    }

    #[test]
    fn provider_query_payload_normalizes_scope_aliases() {
        let payload = provider_query_payload(&json!({
            "query": "12900kf",
            "projectId": "missiond",
            "tenantId": "xjp",
            "includeArchived": true,
            "limit": 500
        }));
        assert_eq!(payload["query"], "12900kf");
        assert_eq!(payload["include_archived"], true);
        assert_eq!(payload["limit"], 100);
        assert_eq!(payload["scope"]["project_id"], "missiond");
        assert_eq!(payload["scope"]["tenant_id"], "xjp");
    }

    #[test]
    fn provider_remember_payload_requires_and_maps_text() {
        let payload = provider_remember_payload(&json!({
            "text": "MissionD memory provider uses xjp-memory.",
            "tags": ["memory", "provider"],
            "scope": {"universe_id": "xjp"}
        }))
        .expect("payload");
        assert_eq!(payload["text"], "MissionD memory provider uses xjp-memory.");
        assert_eq!(payload["tags"][0], "memory");
        assert_eq!(payload["scope"]["universe_id"], "xjp");

        assert!(provider_remember_payload(&json!({"tags": []})).is_err());
    }

    #[test]
    fn evidence_search_payload_normalizes_lane_allowlist() {
        let payload = provider_evidence_search_payload(&json!({
            "query": "payments migration",
            "allowedLanes": ["project_ssot", "reviewed_kb"],
            "projectId": "payments",
            "limit": 200
        }));
        assert_eq!(payload["query"], "payments migration");
        assert_eq!(payload["allowed_lanes"][0], "project_ssot");
        assert_eq!(payload["project_id"], "payments");
        assert_eq!(payload["limit"], 100);

        let lanes =
            get_string_list_any(&json!({"lanes": "runtime_truth, support_refs"}), &["lanes"]);
        assert_eq!(lanes, vec!["runtime_truth", "support_refs"]);
    }

    #[test]
    fn evidence_promotion_requires_bounds_for_deploy_config_dependency_facts() {
        assert!(evidence_promotion_requires_bound(
            "Payments deploy image changed",
            "memory:skill-evidence"
        ));
        assert!(evidence_promotion_requires_bound(
            "DB migration namespace points at billing",
            "memory:support-reference"
        ));
        assert!(!evidence_promotion_requires_bound(
            "User prefers concise final answers",
            "memory:conversation-evidence"
        ));
        assert!(!promotion_bound_present(&json!({"rationale": "reviewed"})));
        assert!(promotion_bound_present(
            &json!({"versionBound": "release-2026-06-02"})
        ));
        assert!(promotion_bound_present(&json!({"ttl_days": 14})));
    }
}
