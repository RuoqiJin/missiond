use anyhow::Result;
use missiond_mcp::tools::{ToolContent, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::handlers::comm::conversation;
use crate::handlers::sysinfra::infra;
use crate::state::AppState;

use super::{board, intent, kb, project, skill};

#[derive(Debug, Deserialize)]
struct ContextGatherArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default, alias = "sourceProfile")]
    source_profile: Option<String>,
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default, alias = "infraTarget")]
    infra_target: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    unknowns: Vec<String>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_kb: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_ssot: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_project: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_skill: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_infra: Option<bool>,
    #[serde(
        default,
        alias = "includeBoard",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_board: Option<bool>,
    #[serde(
        default,
        alias = "includeConversations",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_conversations: Option<bool>,
    #[serde(
        default,
        alias = "includeCredentials",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_credentials: Option<bool>,
    #[serde(
        default,
        alias = "includeRawSources",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_raw_sources: Option<bool>,
    #[serde(default, alias = "conversationTimeRange")]
    conversation_time_range: Option<String>,
    #[serde(default, alias = "taskId")]
    task_id: Option<String>,
    #[serde(default, alias = "sourceId")]
    source_id: Option<String>,
    #[serde(default, alias = "conversationId", alias = "conversation_id")]
    conversation_id: Option<String>,
    #[serde(default, alias = "userId", alias = "user_id")]
    user_id: Option<String>,
    #[serde(default, alias = "tenantId", alias = "tenant_id")]
    tenant_id: Option<String>,
    #[serde(default, alias = "applicationId", alias = "application_id")]
    application_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default, alias = "topicId", alias = "topic_id")]
    topic_id: Option<String>,
    #[serde(default, alias = "topicLabel", alias = "topic_label")]
    topic_label: Option<String>,
    #[serde(default, alias = "permissionContext", alias = "permission_context")]
    permission_context: Option<Value>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    persist: Option<bool>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Default, Deserialize)]
struct ContextBootArgs {
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default, alias = "taskId")]
    task_id: Option<String>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_capsule: Option<bool>,
}

const CODEX_BOOT_CONTEXT_REL: &str = ".missiond/v3/evidence/codex-boot-context.lisp";
const CODEX_BOOT_CONTEXT_FALLBACK: &str =
    include_str!("../../../../../.missiond/v3/evidence/codex-boot-context.lisp");
const CONTEXT_GATHER_RUNTIME_REL: &str = ".missiond/v3/runtime/context-gather";
const CONTEXT_GATHER_WORKER_VISIBLE_REL: &str = ".missiond/v3/runtime/context-gather-worker";

fn default_limit() -> usize {
    8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceProfile {
    IntentDefault,
    DeployOps,
    ConversationAudit,
    FullDebug,
}

impl SourceProfile {
    fn from_arg(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("intent_default")
        {
            "deploy_ops" | "deploy-ops" => Self::DeployOps,
            "conversation_audit" | "conversation-audit" => Self::ConversationAudit,
            "full_debug" | "full-debug" => Self::FullDebug,
            _ => Self::IntentDefault,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::IntentDefault => "intent_default",
            Self::DeployOps => "deploy_ops",
            Self::ConversationAudit => "conversation_audit",
            Self::FullDebug => "full_debug",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SourceSelection {
    include_project: bool,
    include_ssot: bool,
    include_kb: bool,
    include_skill: bool,
    include_infra: bool,
    include_board: bool,
    include_conversations: bool,
    include_credentials: bool,
    include_raw_sources: bool,
}

fn flag(default: bool, override_value: Option<bool>) -> bool {
    override_value.unwrap_or(default)
}

fn source_selection(args: &ContextGatherArgs, profile: SourceProfile) -> SourceSelection {
    let explicit_skill = normalized_scope_value(args.skill.as_deref()).is_some();
    let explicit_infra = normalized_scope_value(args.infra_target.as_deref()).is_some();
    let deploy_ops = profile == SourceProfile::DeployOps;
    let conversation_audit = profile == SourceProfile::ConversationAudit;
    let full_debug = profile == SourceProfile::FullDebug;

    SourceSelection {
        include_project: flag(true, args.include_project),
        include_ssot: flag(true, args.include_ssot),
        include_kb: flag(true, args.include_kb),
        include_skill: flag(
            full_debug || deploy_ops || explicit_skill,
            args.include_skill,
        ),
        include_infra: flag(
            full_debug || deploy_ops || explicit_infra || explicit_skill,
            args.include_infra,
        ),
        include_board: flag(true, args.include_board),
        include_conversations: flag(full_debug || conversation_audit, args.include_conversations),
        include_credentials: flag(full_debug || deploy_ops, args.include_credentials),
        include_raw_sources: flag(full_debug, args.include_raw_sources),
    }
}

fn normalized_scope_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !matches!(*value, "unknown" | "null" | "undefined"))
        .map(ToOwned::to_owned)
}

fn compact_topic_label(text: &str) -> Option<String> {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if collapsed.is_empty() {
        return None;
    }
    let mut label = collapsed.chars().take(96).collect::<String>();
    if collapsed.chars().count() > 96 {
        label.push_str("...");
    }
    Some(label)
}

fn stable_topic_id(
    user_id: Option<&str>,
    tenant_id: Option<&str>,
    application_id: Option<&str>,
    channel: &str,
    topic_label: Option<&str>,
) -> Option<String> {
    let topic_label = topic_label
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let input = format!(
        "{}|{}|{}|{}|{}",
        tenant_id.unwrap_or(""),
        user_id.unwrap_or(""),
        application_id.unwrap_or("missiond"),
        channel,
        topic_label.to_ascii_lowercase()
    );
    let digest = Sha256::digest(input.as_bytes());
    let short = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("topic-{short}"))
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_context_boot" {
        return handle_context_boot(args);
    }

    let args: ContextGatherArgs = serde_json::from_value(args)?;
    let profile = SourceProfile::from_arg(args.source_profile.as_deref());
    let selection = source_selection(&args, profile);
    let query = normalized_query(&args);
    if query.is_empty()
        && args.project_id.is_none()
        && args.skill.is_none()
        && args.infra_target.is_none()
    {
        return Ok(ToolResult::error(
            "mission_context_gather requires query, project/project_id, skill, infra_target, or unknowns",
        ));
    }

    let limit = args.limit.clamp(1, 25);
    let mut sources = serde_json::Map::new();
    let mut diagnostics = Vec::new();
    let mut project_resolution_payload: Option<Value> = None;
    sources.insert(
        "runtime_environment".to_string(),
        runtime_environment_payload(),
    );

    let mut effective_project_id = args.project_id.clone();
    if effective_project_id.is_none() && !query.is_empty() && selection.include_project {
        match project::handle(
            state,
            "mission_project",
            json!({
                "action": "resolve",
                "query": query,
                "limit": 8,
                "include_unregistered_candidates": true
            }),
        )
        .await
        {
            Ok(result) => {
                let is_error = result.is_error.unwrap_or(false);
                let value = tool_result_to_value(result);
                if is_error {
                    diagnostics.push(json!({"source": "project_resolution", "error": value}));
                } else {
                    if value.get("status").and_then(Value::as_str) == Some("resolved") {
                        effective_project_id = value
                            .get("matched_project_id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                    project_resolution_payload = Some(value.clone());
                    sources.insert("project_resolution".to_string(), value);
                }
            }
            Err(err) => diagnostics.push(json!({
                "source": "project_resolution",
                "error": err.to_string()
            })),
        }
    }

    if selection.include_project {
        if let Some(project_payload) = project_resolution_payload
            .as_ref()
            .and_then(|value| value.get("matched_project"))
            .filter(|value| value.is_object())
            .cloned()
        {
            sources.insert("project_registry".to_string(), project_payload);
        } else {
            let payload = if let Some(project_id) = effective_project_id.as_deref() {
                json!({"action": "get", "id": project_id})
            } else {
                json!({"action": "list"})
            };
            insert_subcall(
                &mut sources,
                &mut diagnostics,
                "project_registry",
                project::handle(state, "mission_project", payload).await,
            );
        }
    }

    if selection.include_ssot {
        let payload = if let Some(project_id) = effective_project_id.as_deref() {
            json!({"action": "summary", "project": project_id})
        } else {
            json!({"action": "list"})
        };
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "ssot",
            intent::handle(state, "mission_intent", payload).await,
        );
    }

    if selection.include_kb {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "kb",
            kb::handle(
                state,
                "mission_kb_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": effective_project_id.clone(),
                    "limit": limit,
                    "include_archived": false
                }),
            )
            .await,
        );
    }

    if selection.include_skill {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "skill_context",
            skill::handle(
                state,
                "mission_skill_context",
                json!({
                    "action": "resolve",
                    "query": query,
                    "project_id": effective_project_id.clone(),
                    "skill": args.skill.clone(),
                    "include_kb": false,
                    "include_board": false
                }),
            )
            .await,
        );
    }

    if selection.include_infra {
        let infra_payload = if let Some(target_id) = args.infra_target.as_deref() {
            json!({"action": "get", "id": target_id})
        } else {
            json!({"action": "skill_evidence", "skill": args.skill.clone(), "target_id": args.infra_target.clone(), "limit": limit})
        };
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "infra",
            infra::handle(state, "mission_infra_query", infra_payload).await,
        );
    }

    if selection.include_credentials {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "credential_refs",
            infra::handle(
                state,
                "mission_infra_query",
                json!({
                    "action": "credential_refs",
                    "target_id": args.infra_target.clone(),
                    "skill": args.skill.clone(),
                    "limit": limit
                }),
            )
            .await,
        );
    }

    if selection.include_board {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "board_tasks",
            board::handle(
                state,
                "mission_board_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": effective_project_id.clone(),
                    "scope": "active",
                    "limit": limit
                }),
            )
            .await,
        );
    }

    if selection.include_conversations {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "conversation_logs",
            conversation::handle(
                state,
                "mission_conversation_query",
                json!({
                    "action": "search",
                    "query": query,
                        "project": effective_project_id.clone(),
                        "limit": limit,
                        "user_id": args.user_id.clone(),
                        "tenant_id": args.tenant_id.clone(),
                        "application_id": args.application_id.clone(),
                        "channel": args.channel.clone(),
                        "time_range": args
                            .conversation_time_range
                        .as_deref()
                        .unwrap_or("last_30d"),
                    "query_mode": "hybrid"
                }),
            )
            .await,
        );
    }

    let evidence_lanes = build_evidence_lanes(&sources);
    let authority_order = authority_order();
    let noise_diagnostics = noise_diagnostics(profile, selection, &sources);
    let context_noise_metrics =
        context_noise_metrics(profile, selection, &sources, &evidence_lanes);
    let evidence_refs = collect_evidence_refs(&sources);
    let unresolved = args
        .unknowns
        .iter()
        .filter(|unknown| !unknown.trim().is_empty())
        .map(|unknown| {
            json!({
                "unknown": unknown,
                "status": "needs_synthesis",
                "hint": "Review sources and promote to resolved/evidence_gap in the context-pack."
            })
        })
        .collect::<Vec<_>>();

    let sources_used = sources.keys().cloned().collect::<Vec<_>>();
    let runtime_environment = sources
        .get("runtime_environment")
        .cloned()
        .unwrap_or(Value::Null);
    let user_id = normalized_scope_value(args.user_id.as_deref());
    let tenant_id = normalized_scope_value(args.tenant_id.as_deref());
    let application_id = normalized_scope_value(args.application_id.as_deref())
        .or_else(|| normalized_scope_value(effective_project_id.as_deref()))
        .or_else(|| Some("missiond".to_string()));
    let channel =
        normalized_scope_value(args.channel.as_deref()).unwrap_or_else(|| "cli".to_string());
    let topic_label =
        normalized_scope_value(args.topic_label.as_deref()).or_else(|| compact_topic_label(&query));
    let topic_id = normalized_scope_value(args.topic_id.as_deref()).or_else(|| {
        stable_topic_id(
            user_id.as_deref(),
            tenant_id.as_deref(),
            application_id.as_deref(),
            &channel,
            topic_label.as_deref(),
        )
    });
    let mut payload = json!({
            "ok": diagnostics.is_empty(),
            "schema": "missiond.context-gather.v1",
            "query": query,
            "project_id": effective_project_id.clone(),
            "requested_project_id": args.project_id.clone(),
            "skill": args.skill.clone(),
            "infra_target": args.infra_target.clone(),
            "task_id": args.task_id.clone(),
            "source_id": args.source_id.clone(),
            "conversation_id": args.conversation_id.clone(),
            "isolation_scope": {
                "user_id": user_id.clone(),
                "tenant_id": tenant_id.clone(),
                "application_id": application_id.clone(),
                "channel": channel.clone(),
            },
            "topic_id": topic_id.clone(),
            "topic_label": topic_label.clone(),
            "permission_context": args.permission_context.clone(),
            "unknowns": args.unknowns.clone(),
        "sources_used": sources_used,
        "runtime_environment": runtime_environment,
        "sources": Value::Object(sources),
        "source_profile": profile.as_str(),
        "evidence_lanes": evidence_lanes,
        "authority_order": authority_order,
        "noise_diagnostics": noise_diagnostics,
        "context_noise_metrics": context_noise_metrics,
        "evidence_refs": evidence_refs,
        "unresolved": unresolved,
        "diagnostics": diagnostics,
        "next_action": "Synthesize grounded intent. If intent is confirmed, assign a plan-authoring worker to compile plan.lisp from the confirmed intent plus tool/resource inventory."
    });

    if args.persist.unwrap_or(false) {
        let artifact_payload =
            context_pack_artifact_payload(&payload, selection.include_raw_sources);
        let metadata = json!({
            "schema": "missiond.context-gather-artifact.v1",
            "query": payload.get("query").cloned().unwrap_or(Value::Null),
            "project_id": payload.get("project_id").cloned().unwrap_or(Value::Null),
            "task_id": payload.get("task_id").cloned().unwrap_or(Value::Null),
            "source_id": payload.get("source_id").cloned().unwrap_or(Value::Null),
            "source_profile": payload.get("source_profile").cloned().unwrap_or(Value::Null),
            "raw_sources_in_artifact": selection.include_raw_sources,
            "context_noise_metrics": payload
                .get("context_noise_metrics")
                .cloned()
                .unwrap_or(Value::Null),
            "unknown_count": payload
                .get("unknowns")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0),
            "sources_used": payload.get("sources_used").cloned().unwrap_or(Value::Null),
        });
        let artifact = state
            .shared_memory
            .put_json_artifact(
                "context-gather",
                payload.get("project_id").and_then(Value::as_str),
                payload.get("task_id").and_then(Value::as_str),
                &artifact_payload,
                metadata,
            )
            .await?;
        let context_pack_file = if let Some(hash) = artifact.get("hash").and_then(Value::as_str) {
            Some((
                hash.to_string(),
                materialize_context_pack_file(hash, &artifact_payload)?,
            ))
        } else {
            None
        };
        let isolation = super::context_capsule::CapsuleIsolation {
            user_id: user_id.as_deref(),
            tenant_id: tenant_id.as_deref(),
            application_id: application_id.as_deref(),
            channel: &channel,
        };
        let (capsule_lisp, capsule_hash) = super::context_capsule::generate_lisp_capsule(
            &isolation,
            &artifact_payload,
            topic_id.as_deref(),
            topic_label.as_deref(),
            args.task_id.as_deref(),
            None,
            None,
        );
        let capsule_path = materialize_context_capsule(&capsule_hash, &capsule_lisp);
        if let Some(conversation_id) = args.conversation_id.as_deref() {
            if let Err(err) = state
                .store
                .bind_context_capsule(
                    conversation_id,
                    &capsule_hash,
                    topic_id.as_deref(),
                    topic_label.as_deref(),
                )
                .await
            {
                tracing::warn!(conversation_id, error = %err, "failed to bind context capsule to conversation");
            }
            if let (Some(embedding_service), Some(topic)) =
                (state.embedding_service.as_ref(), topic_label.as_deref())
            {
                let topic_text = format!(
                    "{}\n{}",
                    topic,
                    args.unknowns
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                if let Some(vector) = embedding_service.embed(&topic_text) {
                    if let Err(err) = state
                        .store
                        .set_conversation_topic_vectors(
                            conversation_id,
                            &[(topic.to_string(), vector)],
                            embedding_service.provider_id(),
                        )
                        .await
                    {
                        tracing::warn!(conversation_id, error = %err, "failed to write conversation topic vector");
                    }
                }
            }
        }
        if let Some(task_id) = args.task_id.as_deref() {
            if let Ok(Some(task)) = state.store.get_board_task(task_id).await {
                let mut metadata = task.runtime_metadata.clone();
                if !metadata.is_object() {
                    metadata = json!({});
                }
                if let Some(object) = metadata.as_object_mut() {
                    object.insert(
                        "context_capsule_hash".to_string(),
                        Value::String(capsule_hash.clone()),
                    );
                    object.insert("topic_id".to_string(), json!(topic_id));
                    object.insert("topic_label".to_string(), json!(topic_label));
                    if let Some((hash, _)) = context_pack_file.as_ref() {
                        object.insert(
                            "grounding_context_id".to_string(),
                            Value::String(format!("context-gather:{hash}")),
                        );
                    }
                }
                let update = missiond_core::types::UpdateBoardTaskInput {
                    runtime_metadata: Some(metadata),
                    ..Default::default()
                };
                if let Err(err) = state
                    .store
                    .update_board_task(task.id.as_str(), &update)
                    .await
                {
                    tracing::warn!(task_id = task.id.as_str(), error = %err, "failed to bind context capsule to BoardTask runtime_metadata");
                }
            }
        }

        if let Some(object) = payload.as_object_mut() {
            if let Some((hash, context_pack_file)) = context_pack_file.as_ref() {
                object.insert(
                    "grounding_context_id".to_string(),
                    Value::String(format!("context-gather:{}", hash)),
                );
                object.insert(
                    "context_pack_path".to_string(),
                    Value::String(format!("shared-artifact://{}", hash)),
                );
                object.insert(
                    "context_pack_file".to_string(),
                    Value::String(context_pack_file.worker_path.display().to_string()),
                );
                object.insert(
                    "canonical_context_pack_file".to_string(),
                    Value::String(context_pack_file.canonical_path.display().to_string()),
                );
                object.insert("artifact_hash".to_string(), Value::String(hash.clone()));
            }
            object.insert("artifact".to_string(), artifact);
            object.insert(
                "context_capsule_hash".to_string(),
                Value::String(capsule_hash),
            );
            if let Ok(path) = capsule_path {
                object.insert(
                    "context_capsule_file".to_string(),
                    Value::String(path.display().to_string()),
                );
            }
        }
    }

    Ok(ToolResult::json_pretty(&payload))
}

fn authority_order() -> Value {
    json!([
        "runtime_environment",
        "ssot_project_facts",
        "reviewed_kb_memory",
        "active_board_projection",
        "conversation_read_model",
        "skill_infra_evidence"
    ])
}

fn build_evidence_lanes(sources: &serde_json::Map<String, Value>) -> Value {
    json!({
        "schema": "missiond.context-gather-evidence-lanes.v1",
        "lanes": {
            "runtime_environment": evidence_lane(
                sources,
                "runtime-env-and-monitor",
                "deployed runtime paths, compiled runtime locations, and monitor endpoints",
                &["runtime_environment"],
            ),
            "ssot_project_facts": evidence_lane(
                sources,
                "file-first-lisp-and-compiled-project-universe",
                "project identity plus active Lisp/compiled project facts",
                &["project_resolution", "project_registry", "ssot"],
            ),
            "reviewed_kb_memory": evidence_lane(
                sources,
                "knowledge_review_state",
                "curated active KB retrieval after review overlay",
                &["kb"],
            ),
            "active_board_projection": evidence_lane(
                sources,
                "board_projection",
                "active Board task coordination records",
                &["board_tasks"],
            ),
            "conversation_read_model": evidence_lane(
                sources,
                "provider_durable_conversation_read_model",
                "bounded query-scoped conversation audit evidence",
                &["conversation_logs"],
            ),
            "skill_infra_evidence": evidence_lane(
                sources,
                "evidence-only",
                "skill and infra operational hints; not runtime truth",
                &["skill_context", "infra", "credential_refs"],
            ),
        }
    })
}

fn evidence_lane(
    sources: &serde_json::Map<String, Value>,
    authority: &str,
    role: &str,
    keys: &[&str],
) -> Value {
    let mut source_keys = Vec::new();
    for key in keys {
        if sources.contains_key(*key) {
            source_keys.push((*key).to_string());
        }
    }
    json!({
        "authority": authority,
        "role": role,
        "source_count": source_keys.len(),
        "source_keys": source_keys,
    })
}

fn noise_diagnostics(
    profile: SourceProfile,
    selection: SourceSelection,
    sources: &serde_json::Map<String, Value>,
) -> Value {
    let mut excluded = Vec::new();
    for (name, included) in [
        ("project_registry", selection.include_project),
        ("ssot", selection.include_ssot),
        ("kb", selection.include_kb),
        ("skill_context", selection.include_skill),
        ("infra", selection.include_infra),
        ("credential_refs", selection.include_credentials),
        ("board_tasks", selection.include_board),
        ("conversation_logs", selection.include_conversations),
    ] {
        if !included {
            excluded.push(name);
        }
    }
    json!({
        "schema": "missiond.context-noise-diagnostics.v1",
        "source_profile": profile.as_str(),
        "included_sources": sources.keys().cloned().collect::<Vec<_>>(),
        "excluded_by_profile": excluded,
        "credential_lane_opt_in": selection.include_credentials,
        "conversation_lane_opt_in": selection.include_conversations,
        "raw_sources_in_artifact": selection.include_raw_sources,
        "rule": "Default grounding is authority-aware and does not preload conversations, infra skill evidence, or credential refs unless the source profile or explicit include flag opts in."
    })
}

fn context_noise_metrics(
    profile: SourceProfile,
    selection: SourceSelection,
    sources: &serde_json::Map<String, Value>,
    evidence_lanes: &Value,
) -> Value {
    let lane_counts = evidence_lanes
        .get("lanes")
        .and_then(Value::as_object)
        .map(|lanes| {
            lanes
                .iter()
                .map(|(lane, value)| {
                    (
                        lane.clone(),
                        value
                            .get("source_count")
                            .cloned()
                            .unwrap_or_else(|| json!(0)),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    json!({
        "schema": "missiond.context-noise-metrics.v1",
        "source_profile": profile.as_str(),
        "raw_source_count": sources.len(),
        "lane_counts": lane_counts,
        "conversation_lane_enabled": selection.include_conversations,
        "credential_lane_enabled": selection.include_credentials,
        "raw_sources_in_artifact": selection.include_raw_sources,
        "filtered_semantic_conversation_hits": Value::Null,
        "conversation_filtering": "conversation search owns project/time/type filter metrics; context-gather records whether the lane was enabled."
    })
}

fn context_pack_artifact_payload(payload: &Value, include_raw_sources: bool) -> Value {
    if include_raw_sources {
        return payload.clone();
    }
    let mut compact = payload.clone();
    if let Some(object) = compact.as_object_mut() {
        object.remove("sources");
        object.insert("raw_sources_omitted".to_string(), Value::Bool(true));
        object.insert(
            "raw_sources_policy".to_string(),
            Value::String(
                "Raw legacy sources are omitted from the worker context pack; use evidence_lanes or rerun with include_raw_sources=true/full_debug for diagnostics."
                    .to_string(),
            ),
        );
    }
    compact
}

fn handle_context_boot(args: Value) -> Result<ToolResult> {
    let args: ContextBootArgs = serde_json::from_value(args).unwrap_or_default();
    let (capsule, source_path) = read_codex_boot_context();
    let include_capsule = args.include_capsule.unwrap_or(true);
    let capsule_len = capsule.chars().count();
    Ok(ToolResult::json_pretty(&json!({
        "ok": true,
        "schema": "missiond.codex-boot-context.v1",
        "project_id": args.project_id,
        "task_id": args.task_id,
        "source_path": source_path,
        "capsule_chars": capsule_len,
        "layers": ["L0-always-on", "L1-current-task", "L2-grounded-facts", "L3-cold-evidence"],
        "capsule": if include_capsule { Value::String(capsule) } else { Value::Null },
        "next_action": "Use this boot capsule as the collaboration protocol. For missing task/project facts, call mission_context_gather with explicit unknowns instead of preloading broad KB or logs."
    })))
}

fn read_codex_boot_context() -> (String, String) {
    for candidate in codex_boot_context_candidates() {
        if let Ok(text) = fs::read_to_string(&candidate) {
            return (text, candidate.display().to_string());
        }
    }
    (
        CODEX_BOOT_CONTEXT_FALLBACK.to_string(),
        "compiled-fallback:.missiond/v3/evidence/codex-boot-context.lisp".to_string(),
    )
}

fn codex_boot_context_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("MISSIOND_CODEX_BOOT_CONTEXT") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(root) = env::var("MISSIOND_PROJECT_ROOT") {
        candidates.push(PathBuf::from(root).join(CODEX_BOOT_CONTEXT_REL));
    }
    if let Ok(root) = env::var("MISSIOND_ORCHESTRATOR_ROOT") {
        candidates.push(PathBuf::from(root).join(CODEX_BOOT_CONTEXT_REL));
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join(CODEX_BOOT_CONTEXT_REL));
    }
    candidates
}

const CONTEXT_CAPSULE_RUNTIME_REL: &str = ".missiond/v3/runtime/context-capsules";

#[derive(Debug)]
struct MaterializedContextPack {
    canonical_path: PathBuf,
    worker_path: PathBuf,
}

fn materialize_context_capsule(hash: &str, lisp_content: &str) -> Result<PathBuf> {
    let dir = context_capsule_runtime_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{hash}.lisp"));
    fs::write(&path, lisp_content)?;
    Ok(path)
}

fn materialize_context_pack_file(hash: &str, payload: &Value) -> Result<MaterializedContextPack> {
    let bytes = serde_json::to_vec_pretty(payload)?;
    let canonical_dir = context_gather_runtime_dir();
    fs::create_dir_all(&canonical_dir)?;
    let canonical_path = canonical_dir.join(format!("{hash}.json"));
    fs::write(&canonical_path, &bytes)?;

    let worker_path = if context_gather_uses_external_runtime() {
        let worker_dir = context_gather_worker_visible_dir();
        fs::create_dir_all(&worker_dir)?;
        let worker_path = worker_dir.join(format!("{hash}.json"));
        fs::write(&worker_path, &bytes)?;
        worker_path
    } else {
        canonical_path.clone()
    };

    Ok(MaterializedContextPack {
        canonical_path,
        worker_path,
    })
}

fn runtime_environment_payload() -> Value {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    let compiled_runtime_dir = missiond_compiled_runtime_dir(&runtime_dir);
    let repo_runtime_dir = project_root.join(".missiond/v3/runtime");
    let compiled_runtime_config = compiled_runtime_dir.join("compiled-runtime-config.json");

    json!({
        "schema": "missiond.runtime-environment-context.v1",
        "authority": "runtime-env-and-monitor",
        "rule": "For deployed MissionD, runtime artifacts are authoritative under MISSIOND_RUNTIME_DIR and MISSIOND_COMPILED_RUNTIME_DIR. Repo .missiond/v3/runtime/** is dev/cold evidence only and must not be used to declare deployed compiled projections missing. A bounded worker-readable mirror under .missiond/v3/runtime/context-gather-worker/** may be written for provider CLIs that cannot read outside their workspace; it is an ignored projection, not the canonical artifact.",
        "project_root": project_root.display().to_string(),
        "orchestrator_root": env::var("MISSIOND_ORCHESTRATOR_ROOT").ok(),
        "runtime_dir": runtime_dir.display().to_string(),
        "compiled_runtime_dir": compiled_runtime_dir.display().to_string(),
        "repo_runtime_dir": repo_runtime_dir.display().to_string(),
        "repo_runtime_authority": "dev-cold-evidence-only",
        "context_pack_worker_visible_dir": context_gather_worker_visible_dir().display().to_string(),
        "monitor_endpoints": {
            "canonical_local_http": "http://127.0.0.1:9120/api/monitor/jarvis",
            "canonical_public_https": "https://jarvis.xiaojinpro.top/api/monitor/jarvis",
            "public_path": "/api/monitor/jarvis",
            "daemon_path": "/api/monitor/jarvis",
            "rule": "Use these endpoints for Jarvis chain readiness. Do not guess ports or probe unix-socket paths unless a dedicated diagnostic asks for it."
        },
        "compiled_runtime_config": {
            "path": compiled_runtime_config.display().to_string(),
            "exists": compiled_runtime_config.exists()
        },
        "env_presence": {
            "MISSIOND_PROJECT_ROOT": env::var("MISSIOND_PROJECT_ROOT").is_ok(),
            "MISSIOND_ORCHESTRATOR_ROOT": env::var("MISSIOND_ORCHESTRATOR_ROOT").is_ok(),
            "MISSIOND_RUNTIME_DIR": env::var("MISSIOND_RUNTIME_DIR").is_ok(),
            "MISSIOND_COMPILED_RUNTIME_DIR": env::var("MISSIOND_COMPILED_RUNTIME_DIR").is_ok()
        },
        "diagnostic": "If runtime files appear missing in the repository, check this runtime_environment source and monitor_endpoints.canonical_local_http or monitor_endpoints.canonical_public_https before reporting a deployed runtime failure."
    })
}

fn context_gather_runtime_dir() -> PathBuf {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    if env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return runtime_dir.join("context-gather");
    }
    project_root.join(CONTEXT_GATHER_RUNTIME_REL)
}

fn context_gather_worker_visible_dir() -> PathBuf {
    missiond_project_root().join(CONTEXT_GATHER_WORKER_VISIBLE_REL)
}

fn context_gather_uses_external_runtime() -> bool {
    env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn context_capsule_runtime_dir() -> PathBuf {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    if env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return runtime_dir.join("context-capsules");
    }
    project_root.join(CONTEXT_CAPSULE_RUNTIME_REL)
}

fn missiond_runtime_dir(project_root: &Path) -> PathBuf {
    if let Ok(value) = env::var("MISSIOND_RUNTIME_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    project_root.join(".missiond/v3/runtime")
}

fn missiond_compiled_runtime_dir(runtime_dir: &Path) -> PathBuf {
    if let Ok(value) = env::var("MISSIOND_COMPILED_RUNTIME_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    runtime_dir.join("compiled")
}

fn missiond_project_root() -> PathBuf {
    for key in [
        "MISSIOND_PROJECT_ROOT",
        "MISSIOND_REPO_ROOT",
        "MISSIOND_WORKSPACE_ROOT",
        "MISSIOND_ORCHESTRATOR_ROOT",
    ] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn normalized_query(args: &ContextGatherArgs) -> String {
    args.query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let joined = args
                .unknowns
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        })
        .or_else(|| args.project_id.clone())
        .or_else(|| args.skill.clone())
        .or_else(|| args.infra_target.clone())
        .unwrap_or_default()
}

fn insert_subcall(
    sources: &mut serde_json::Map<String, Value>,
    diagnostics: &mut Vec<Value>,
    key: &str,
    result: Result<ToolResult>,
) {
    match result {
        Ok(result) => {
            let is_error = result.is_error.unwrap_or(false);
            let value = tool_result_to_value(result);
            if is_error {
                diagnostics.push(json!({"source": key, "error": value}));
            } else {
                sources.insert(key.to_string(), value);
            }
        }
        Err(err) => diagnostics.push(json!({"source": key, "error": err.to_string()})),
    }
}

fn tool_result_to_value(result: ToolResult) -> Value {
    let Some(ToolContent::Text { text }) = result.content.into_iter().next() else {
        return Value::Null;
    };
    serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "text": text }))
}

fn collect_evidence_refs(sources: &serde_json::Map<String, Value>) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_evidence_refs_inner(&Value::Object(sources.clone()), "$", &mut refs);
    refs.truncate(60);
    refs
}

fn collect_evidence_refs_inner(value: &Value, path: &str, refs: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            for key in [
                "path",
                "intentPath",
                "intent_path",
                "file_path",
                "source_path",
                "sourcePath",
                "source_file",
                "id",
                "key",
            ] {
                if let Some(v) = map.get(key).and_then(Value::as_str) {
                    if !v.is_empty() {
                        refs.push(json!({"source_path": path, "field": key, "value": v}));
                    }
                }
            }
            for (k, v) in map {
                collect_evidence_refs_inner(v, &format!("{path}.{k}"), refs);
                if refs.len() >= 60 {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                collect_evidence_refs_inner(item, &format!("{path}[{idx}]"), refs);
                if refs.len() >= 60 {
                    return;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ContextGatherArgs, SourceProfile, context_pack_artifact_payload, source_selection,
    };

    fn args(value: serde_json::Value) -> ContextGatherArgs {
        serde_json::from_value(value).expect("context gather args")
    }

    #[test]
    fn intent_default_profile_excludes_noisy_sources() {
        let args = args(json!({"query": "MissionD noise"}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        assert_eq!(profile.as_str(), "intent_default");
        assert!(selection.include_project);
        assert!(selection.include_ssot);
        assert!(selection.include_kb);
        assert!(selection.include_board);
        assert!(!selection.include_skill);
        assert!(!selection.include_infra);
        assert!(!selection.include_conversations);
        assert!(!selection.include_credentials);
        assert!(!selection.include_raw_sources);
    }

    #[test]
    fn deploy_ops_profile_enables_skill_infra_and_credentials() {
        let args = args(json!({"query": "deploy payments", "source_profile": "deploy_ops"}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        assert!(selection.include_skill);
        assert!(selection.include_infra);
        assert!(selection.include_credentials);
        assert!(!selection.include_conversations);
    }

    #[test]
    fn explicit_conversation_opt_in_overrides_default_profile() {
        let args = args(json!({"query": "audit prior answer", "include_conversations": true}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        assert!(selection.include_conversations);
        assert!(!selection.include_credentials);
    }

    #[test]
    fn compact_artifact_payload_omits_raw_sources() {
        let payload = json!({
            "schema": "missiond.context-gather.v1",
            "sources": {"conversation_logs": [{"sessionId": "abc"}]},
            "evidence_lanes": {"lanes": {}}
        });
        let compact = context_pack_artifact_payload(&payload, false);
        assert!(compact.get("sources").is_none());
        assert_eq!(
            compact.get("raw_sources_omitted").and_then(|v| v.as_bool()),
            Some(true)
        );
        let raw = context_pack_artifact_payload(&payload, true);
        assert!(raw.get("sources").is_some());
    }
}
