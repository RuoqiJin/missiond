use anyhow::Result;
use missiond_core::types::{ContextGatherRunInput, EvidenceItemInput};
use missiond_mcp::tools::{ToolContent, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
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
            json!({
                "action": "skill_evidence",
                "skill": args.skill.clone(),
                "target_id": args.infra_target.clone(),
                "query": query,
                "project_id": effective_project_id.clone(),
                "limit": limit
            })
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
                    "query": query,
                    "project_id": effective_project_id.clone(),
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
    let source_summaries = build_source_summaries(&sources);
    let support_catalog = build_support_catalog(&sources);
    let evidence_item_inputs = build_evidence_items(
        &sources,
        &source_summaries,
        &support_catalog,
        profile,
        effective_project_id.as_deref(),
        args.task_id.as_deref(),
    );
    let evidence_items = serde_json::to_value(&evidence_item_inputs).unwrap_or_else(|_| json!([]));
    let response_sources =
        response_sources(&sources, &source_summaries, selection.include_raw_sources);
    let evidence_refs = if selection.include_raw_sources {
        collect_evidence_refs(&sources)
    } else {
        collect_evidence_refs_from_value(&source_summaries)
    };
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
        "sources": response_sources,
        "source_summaries": source_summaries,
        "raw_sources_omitted": !selection.include_raw_sources,
        "raw_sources_policy": raw_sources_policy(selection.include_raw_sources),
        "source_profile": profile.as_str(),
        "evidence_lanes": evidence_lanes,
        "evidence_items": evidence_items,
        "support_catalog": support_catalog,
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

        let artifact_hash = context_pack_file
            .as_ref()
            .map(|(hash, _)| hash.as_str())
            .or_else(|| artifact.get("hash").and_then(Value::as_str));
        let context_gather_run =
            build_context_gather_run_input(&payload, profile, selection, artifact_hash);
        let evidence_lane_persistence =
            persist_evidence_lane_projection(state, &context_gather_run, &evidence_item_inputs)
                .await;

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
            object.insert(
                "evidence_lane_persistence".to_string(),
                evidence_lane_persistence,
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
        "runtime_truth",
        "project_ssot",
        "reviewed_kb",
        "active_board",
        "support_refs",
        "skill_evidence",
        "conversation_audit",
        "cold_archive"
    ])
}

fn build_evidence_lanes(sources: &serde_json::Map<String, Value>) -> Value {
    json!({
        "schema": "missiond.context-gather-evidence-lanes.v1",
        "lanes": {
            "runtime_truth": evidence_lane(
                sources,
                "runtime_truth",
                "runtime-env-and-monitor",
                "deployed runtime paths, compiled runtime locations, release/health/smoke status, and monitor endpoints",
                &["runtime_environment"],
                &["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                "compact_only",
                "operational",
                "current_rule",
                "hot_runtime",
                true,
            ),
            "project_ssot": evidence_lane(
                sources,
                "project_ssot",
                "file-first-lisp-and-compiled-project-universe",
                "project identity plus active Lisp/compiled project facts",
                &["project_resolution", "project_registry", "ssot"],
                &["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                "compact_only",
                "internal",
                "current_rule",
                "compiled_runtime_bound",
                true,
            ),
            "reviewed_kb": evidence_lane(
                sources,
                "reviewed_kb",
                "knowledge_review_state",
                "curated active KB retrieval after review overlay",
                &["kb"],
                &["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                "compact_only",
                "internal",
                "active_fact",
                "ttl_or_review_bound",
                true,
            ),
            "active_board": evidence_lane(
                sources,
                "active_board",
                "board_projection",
                "active Board task coordination records",
                &["board_tasks"],
                &["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                "compact_only",
                "internal",
                "current_state",
                "active_task_bound",
                true,
            ),
            "skill_evidence": evidence_lane(
                sources,
                "skill_evidence",
                "evidence-only",
                "skill and infra operational hints; not runtime truth",
                &["skill_context", "infra"],
                &["deploy_ops", "full_debug"],
                "compact_only",
                "internal",
                "evidence_only",
                "version_bound_or_historical",
                false,
            ),
            "conversation_audit": evidence_lane(
                sources,
                "conversation_audit",
                "provider_durable_conversation_read_model",
                "bounded query-scoped conversation episode/fact evidence; raw messages remain opt-in",
                &["conversation_logs"],
                &["conversation_audit", "full_debug"],
                "raw_opt_in_only",
                "audit",
                "derived_from_conversation",
                "time_range_bound",
                false,
            ),
            "cold_archive": evidence_lane(
                sources,
                "cold_archive",
                "forensics-only-cold-archive",
                "archived sessions, transcript dumps, raw provider logs, and research dumps",
                &[],
                &["full_debug"],
                "explicit_path_or_full_debug_only",
                "audit",
                "historical_evidence",
                "cold_archive",
                false,
            ),
            "support_refs": evidence_lane(
                sources,
                "support_refs",
                "redacted-support-catalog",
                "deploy center, service manifest, endpoint, database/migration, agent, and secret-ref provenance",
                &["project_resolution", "project_registry", "credential_refs"],
                &["intent_default", "deploy_ops", "conversation_audit", "full_debug"],
                "secret_refs_only",
                "reference",
                "current_reference",
                "runtime_or_catalog_bound",
                true,
            ),
        }
    })
}

fn evidence_lane(
    sources: &serde_json::Map<String, Value>,
    lane_id: &str,
    authority_class: &str,
    role: &str,
    keys: &[&str],
    default_profiles: &[&str],
    raw_policy: &str,
    privacy_class: &str,
    validity: &str,
    freshness: &str,
    injectable_by_default: bool,
) -> Value {
    let mut source_keys = Vec::new();
    let mut item_count = 0usize;
    for key in keys {
        let Some(value) = sources.get(*key) else {
            continue;
        };
        let count = evidence_source_item_count(key, value);
        if count > 0 {
            source_keys.push((*key).to_string());
            item_count += count;
        }
    }
    json!({
        "lane_id": lane_id,
        "authority_class": authority_class,
        "role": role,
        "default_profiles": default_profiles,
        "raw_policy": raw_policy,
        "privacy_class": privacy_class,
        "validity": validity,
        "freshness": freshness,
        "injectable_by_default": injectable_by_default,
        "source_count": source_keys.len(),
        "item_count": item_count,
        "source_keys": source_keys,
    })
}

fn evidence_source_item_count(key: &str, value: &Value) -> usize {
    match key {
        "kb" => array_len(value.get("items"))
            .max(array_len(value.get("results")))
            .max(array_len(value.get("data")))
            .max(value.as_array().map(Vec::len).unwrap_or(0)),
        "board_tasks" => array_len(value.get("data"))
            .max(array_len(value.get("items")))
            .max(array_len(value.get("results"))),
        "conversation_logs" => array_len(value.get("results"))
            .max(array_len(value.get("items")))
            .max(array_len(value.get("data")))
            .max(value.as_array().map(Vec::len).unwrap_or(0)),
        "skill_context" => {
            array_len(value.get("skills"))
                + array_len(value.get("project_skill_links"))
                + array_len(value.get("operational_facts"))
        }
        "infra" => array_len(value.get("items")).max(value.as_array().map(Vec::len).unwrap_or(0)),
        "credential_refs" => array_len(value.get("credentialRefs"))
            .max(array_len(value.get("credential_refs")))
            .max(value.as_array().map(Vec::len).unwrap_or(0)),
        _ if value.is_null() => 0,
        _ if value.as_object().is_some_and(|object| object.is_empty()) => 0,
        _ => 1,
    }
}

fn build_support_catalog(sources: &serde_json::Map<String, Value>) -> Value {
    let project = first_project_payload(sources);
    let service = first_service_runtime_payload(sources);
    let service_catalog = service.and_then(|value| {
        value
            .get("supportCatalog")
            .or_else(|| value.get("support_catalog"))
    });
    let credential_refs = redacted_credential_refs(sources);
    let credential_ref_count = credential_refs.len();

    json!({
        "schema": "missiond.support-catalog.v1",
        "authority": "compiled-project-service-runtime-plus-redacted-support-refs",
        "project_id": text_from_sources(&[project], &["id", "project_id", "projectId"])
            .or_else(|| text_from_sources(&[service], &["project"])),
        "service_id": text_from_sources(&[service, service_catalog], &["id", "service_id", "serviceId"]),
        "resolver_source": text_from_sources(&[project], &["source"])
            .or_else(|| text_from_sources(&[service_catalog], &["resolver_source", "resolverSource"])),
        "deploy_center_slug": text_from_sources(
            &[service_catalog, service],
            &["deploy_center_slug", "deployCenterSlug", "deployCenter", "deploy_center"],
        ),
        "runtime_target": {
            "environment": text_from_sources(&[service_catalog, service], &["environment"]),
            "target": text_from_sources(&[service_catalog, service], &["runtime_target", "runtimeTarget", "surface"]),
            "ops_capability": text_from_sources(&[service_catalog, service], &["ops_capability", "opsCapability"]),
        },
        "urls": {
            "public_base_url": text_from_sources(&[service_catalog, service], &["public_base_url", "publicBaseUrl"]),
            "frontend_url": text_from_sources(&[service_catalog, service], &["frontend_url", "frontendUrl"]),
            "api_base_url": text_from_sources(&[service_catalog, service], &["api_base_url", "apiBaseUrl"]),
        },
        "domains": string_list_from_sources(&[service_catalog, service], &["domains"]),
        "manifest_refs": {
            "root": text_from_sources(&[service_catalog, service], &["root"]),
            "intent": text_from_sources(&[service_catalog, service], &["intent"]),
            "backend": text_from_sources(&[service_catalog, service], &["backend"]),
            "frontend": text_from_sources(&[service_catalog, service], &["frontend"]),
            "operations": text_from_sources(&[service_catalog, service], &["operations"]),
            "service_manifest_refs": string_list_from_sources(
                &[service_catalog, service],
                &["service_manifest_refs", "serviceManifestRefs", "source_evidence", "sourceEvidence"],
            ),
        },
        "endpoints": {
            "health": string_list_from_sources(&[service_catalog, service], &["health", "health_endpoints", "healthEndpoints"]),
            "smoke": string_list_from_sources(&[service_catalog, service], &["smoke", "smoke_endpoints", "smokeEndpoints"]),
        },
        "dependencies": string_list_from_sources(&[service_catalog, service], &["dependencies"]),
        "database": {
            "migration_namespace": text_from_sources(
                &[service_catalog, service],
                &["db_migration_namespace", "dbMigrationNamespace", "migration_namespace", "migrationNamespace"],
            ),
            "database_namespace": text_from_sources(
                &[service_catalog, service],
                &["db_namespace", "dbNamespace", "database_namespace", "databaseNamespace"],
            ),
        },
        "agent_refs": string_list_from_sources(&[service_catalog, service], &["agent_refs", "agentRefs", "vm_refs", "vmRefs"]),
        "credential_refs": credential_refs,
        "credential_ref_count": credential_ref_count,
        "secret_policy": "Only secret namespace/key references and availability state are exposed. Secret values are not indexed or injected."
    })
}

fn first_project_payload<'a>(sources: &'a serde_json::Map<String, Value>) -> Option<&'a Value> {
    sources
        .get("project_resolution")
        .and_then(|value| {
            value
                .get("matched_project")
                .or_else(|| value.get("matchedProject"))
        })
        .or_else(|| sources.get("project_registry"))
}

fn first_service_runtime_payload<'a>(
    sources: &'a serde_json::Map<String, Value>,
) -> Option<&'a Value> {
    first_project_payload(sources)
        .and_then(|value| {
            value
                .get("serviceRuntime")
                .or_else(|| value.get("service_runtime"))
        })
        .or_else(|| {
            sources.get("project_resolution").and_then(|value| {
                value
                    .get("matched_project")
                    .or_else(|| value.get("matchedProject"))
                    .and_then(|project| {
                        project
                            .get("serviceRuntime")
                            .or_else(|| project.get("service_runtime"))
                    })
            })
        })
}

fn text_from_sources(sources: &[Option<&Value>], keys: &[&str]) -> Option<String> {
    sources.iter().find_map(|source| {
        let source = source.as_ref()?;
        keys.iter().find_map(|key| text_field(source, key))
    })
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn string_list_from_sources(sources: &[Option<&Value>], keys: &[&str]) -> Vec<String> {
    for source in sources.iter().flatten() {
        for key in keys {
            let values = string_list_field(source, key);
            if !values.is_empty() {
                return values;
            }
        }
    }
    Vec::new()
}

fn string_list_field(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect(),
        Some(Value::String(text)) if !text.trim().is_empty() => vec![text.trim().to_string()],
        _ => Vec::new(),
    }
}

fn redacted_credential_refs(sources: &serde_json::Map<String, Value>) -> Vec<Value> {
    let Some(source) = sources.get("credential_refs") else {
        return Vec::new();
    };
    credential_ref_items(source)
        .into_iter()
        .take(20)
        .map(redacted_credential_ref)
        .collect()
}

fn credential_ref_items(value: &Value) -> Vec<&Value> {
    value
        .get("credentialRefs")
        .or_else(|| value.get("credential_refs"))
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .or_else(|| value.as_array().map(|items| items.iter().collect()))
        .unwrap_or_default()
}

fn redacted_credential_ref(item: &Value) -> Value {
    json!({
        "namespace": text_field(item, "namespace"),
        "key_name": text_field(item, "keyName").or_else(|| text_field(item, "key_name")),
        "target_id": text_field(item, "targetId").or_else(|| text_field(item, "target_id")),
        "required_capability": text_field(item, "requiredCapability")
            .or_else(|| text_field(item, "required_capability")),
        "availability": text_field(item, "availability"),
        "purpose": text_field(item, "purpose").map(|value| compact_text(&value, 220)),
        "provenance": text_field(item, "provenance")
            .or_else(|| text_field(item, "source"))
            .or_else(|| text_field(item, "sourcePath"))
            .or_else(|| text_field(item, "source_path")),
    })
}

fn build_evidence_items(
    sources: &serde_json::Map<String, Value>,
    source_summaries: &Value,
    support_catalog: &Value,
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
) -> Vec<EvidenceItemInput> {
    let mut items = Vec::new();

    add_source_summary_item(
        &mut items,
        source_summaries,
        "runtime_environment",
        "runtime_truth",
        "runtime_environment",
        "Runtime truth",
        "Current runtime environment, compiled runtime locations, and monitor endpoints.",
        profile,
        project_id,
        task_id,
    );
    for source_key in ["project_resolution", "project_registry", "ssot"] {
        add_source_summary_item(
            &mut items,
            source_summaries,
            source_key,
            "project_ssot",
            source_key,
            "Project SSOT",
            "Project resolver, registry, and Lisp/compiled project universe facts.",
            profile,
            project_id,
            task_id,
        );
    }
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "kb",
        "items",
        "reviewed_kb",
        "knowledge",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "board_tasks",
        "items",
        "active_board",
        "board_task",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "skill_context",
        "skills",
        "skill_evidence",
        "skill_metadata",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "skill_context",
        "project_skill_links",
        "skill_evidence",
        "skill_project_link",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "infra",
        "items",
        "skill_evidence",
        "skill_operational_fact",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "conversation_logs",
        "items",
        "conversation_audit",
        "conversation_fact_extract",
        profile,
        project_id,
        task_id,
        10,
    );

    if support_catalog_has_content(support_catalog) {
        let source_id = text_field(support_catalog, "service_id")
            .or_else(|| text_field(support_catalog, "project_id"));
        push_evidence_item(
            &mut items,
            "support_refs",
            "support_catalog",
            source_id.as_deref(),
            None,
            project_id,
            task_id,
            "Support catalog",
            "Domain, service, deploy, endpoint, DB/migration, agent, and redacted secret-reference support catalog.",
            support_catalog,
            profile,
            None,
        );
    }

    if let Some(source) = sources.get("credential_refs") {
        for credential in credential_ref_items(source).into_iter().take(20) {
            let redacted = redacted_credential_ref(credential);
            let source_id = credential_ref_source_id(&redacted);
            push_evidence_item(
                &mut items,
                "support_refs",
                "skill_credential_ref",
                source_id.as_deref(),
                source_id.as_deref(),
                project_id,
                task_id,
                "Credential reference",
                "Redacted credential reference. Secret value is intentionally unavailable to retrieval and worker context.",
                &redacted,
                profile,
                None,
            );
        }
    }

    items
}

#[allow(clippy::too_many_arguments)]
fn add_source_summary_item(
    items: &mut Vec<EvidenceItemInput>,
    source_summaries: &Value,
    source_key: &str,
    lane_id: &str,
    source_type: &str,
    title: &str,
    fallback_summary: &str,
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
) {
    let Some(summary) = source_summaries.get(source_key) else {
        return;
    };
    if summary_is_empty(summary) {
        return;
    }
    let summary_text =
        compact_json_text(summary, 900).unwrap_or_else(|| fallback_summary.to_string());
    let source_id = text_from_sources(
        &[Some(summary)],
        &["id", "matched_project_id", "matchedProjectId"],
    );
    let source_ref = source_ref_from_value(summary);
    push_evidence_item(
        items,
        lane_id,
        source_type,
        source_id.as_deref(),
        source_ref.as_deref(),
        project_id,
        task_id,
        title,
        &summary_text,
        summary,
        profile,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_summary_collection_items(
    items: &mut Vec<EvidenceItemInput>,
    source_summaries: &Value,
    source_key: &str,
    collection_key: &str,
    lane_id: &str,
    source_type: &str,
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
    limit: usize,
) {
    let Some(collection) = source_summaries
        .get(source_key)
        .and_then(|summary| summary.get(collection_key))
        .and_then(Value::as_array)
    else {
        return;
    };
    for item in collection.iter().take(limit) {
        let title = evidence_title(item, source_type);
        let summary = evidence_summary(item);
        let source_id = source_id_from_value(item);
        let source_ref = source_ref_from_value(item);
        let score = numeric_field(item, &["score", "confidence"]);
        push_evidence_item(
            items,
            lane_id,
            source_type,
            source_id.as_deref(),
            source_ref.as_deref(),
            project_id,
            task_id,
            &title,
            &summary,
            item,
            profile,
            score,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_evidence_item(
    items: &mut Vec<EvidenceItemInput>,
    lane_id: &str,
    source_type: &str,
    source_id: Option<&str>,
    source_ref: Option<&str>,
    project_id: Option<&str>,
    task_id: Option<&str>,
    title: &str,
    summary: &str,
    evidence_value: &Value,
    profile: SourceProfile,
    score: Option<f64>,
) {
    let title = compact_text(title, 180);
    let summary = compact_text(summary, 1200);
    let id = evidence_item_id(
        lane_id,
        source_type,
        source_id,
        source_ref,
        &title,
        &summary,
    );
    let mut refs = collect_evidence_refs_from_value(evidence_value);
    refs.truncate(12);
    let (authority_class, validity, privacy_class, freshness, raw_policy) =
        evidence_item_policy(lane_id);
    items.push(EvidenceItemInput {
        id,
        lane_id: lane_id.to_string(),
        source_type: source_type.to_string(),
        source_id: source_id.map(ToOwned::to_owned),
        source_ref: source_ref.map(ToOwned::to_owned),
        project_id: project_id.map(ToOwned::to_owned),
        task_id: task_id.map(ToOwned::to_owned),
        title,
        summary,
        authority_class: authority_class.to_string(),
        validity: validity.to_string(),
        privacy_class: privacy_class.to_string(),
        freshness: freshness.to_string(),
        score,
        raw_policy: raw_policy.to_string(),
        evidence_refs: Value::Array(refs),
        metadata: json!({
            "source_profile": profile.as_str(),
            "projection": "mission_context_gather.compact_evidence",
            "derived_from_raw_source": lane_id == "conversation_audit" || lane_id == "skill_evidence",
        }),
    });
}

fn evidence_item_policy(
    lane_id: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    match lane_id {
        "runtime_truth" => (
            "runtime-env-and-monitor",
            "current_rule",
            "operational",
            "hot_runtime",
            "compact_only",
        ),
        "project_ssot" => (
            "file-first-lisp-and-compiled-project-universe",
            "current_rule",
            "internal",
            "compiled_runtime_bound",
            "compact_only",
        ),
        "reviewed_kb" => (
            "knowledge_review_state",
            "active_fact",
            "internal",
            "ttl_or_review_bound",
            "compact_only",
        ),
        "active_board" => (
            "board_projection",
            "current_state",
            "internal",
            "active_task_bound",
            "compact_only",
        ),
        "skill_evidence" => (
            "evidence-only",
            "evidence_only",
            "internal",
            "version_bound_or_historical",
            "compact_only",
        ),
        "conversation_audit" => (
            "provider_durable_conversation_read_model",
            "derived_from_conversation",
            "audit",
            "time_range_bound",
            "raw_opt_in_only",
        ),
        "support_refs" => (
            "redacted-support-catalog",
            "current_reference",
            "reference",
            "runtime_or_catalog_bound",
            "secret_refs_only",
        ),
        _ => (
            "forensics-only-cold-archive",
            "historical_evidence",
            "audit",
            "cold_archive",
            "explicit_path_or_full_debug_only",
        ),
    }
}

fn evidence_item_id(
    lane_id: &str,
    source_type: &str,
    source_id: Option<&str>,
    source_ref: Option<&str>,
    title: &str,
    summary: &str,
) -> String {
    let input = format!(
        "{lane_id}|{source_type}|{}|{}|{title}|{summary}",
        source_id.unwrap_or(""),
        source_ref.unwrap_or("")
    );
    format!("evi-{}", short_sha256(&input, 16))
}

fn short_sha256(input: &str, hex_chars: usize) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(hex_chars)
        .collect()
}

fn support_catalog_has_content(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.iter().any(|(key, value)| {
            !matches!(key.as_str(), "schema" | "authority" | "secret_policy")
                && !json_value_is_empty(value)
        })
    })
}

fn summary_is_empty(value: &Value) -> bool {
    json_value_is_empty(value)
        || value
            .as_object()
            .is_some_and(|object| object.keys().all(|key| key == "schema" || key == "kind"))
}

fn json_value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(object) => object.is_empty() || object.values().all(json_value_is_empty),
        _ => false,
    }
}

fn compact_json_text(value: &Value, max_chars: usize) -> Option<String> {
    serde_json::to_string(value)
        .ok()
        .map(|text| compact_text(&text, max_chars))
        .filter(|text| !text.is_empty())
}

fn evidence_title(value: &Value, fallback: &str) -> String {
    text_from_sources(
        &[Some(value)],
        &[
            "title",
            "key",
            "name",
            "id",
            "conversation_id",
            "conversationId",
            "session_id",
            "sessionId",
            "sourceSkill",
        ],
    )
    .unwrap_or_else(|| fallback.to_string())
}

fn evidence_summary(value: &Value) -> String {
    text_from_sources(
        &[Some(value)],
        &[
            "summary",
            "description",
            "snippet",
            "content",
            "text",
            "excerpt",
            "purpose",
        ],
    )
    .map(|text| compact_text(&text, 900))
    .or_else(|| compact_json_text(value, 900))
    .unwrap_or_else(|| "Compact evidence summary".to_string())
}

fn source_id_from_value(value: &Value) -> Option<String> {
    text_from_sources(
        &[Some(value)],
        &[
            "id",
            "key",
            "conversation_id",
            "conversationId",
            "session_id",
            "sessionId",
            "name",
            "sourceSkill",
            "targetId",
            "target_id",
        ],
    )
}

fn source_ref_from_value(value: &Value) -> Option<String> {
    text_from_sources(
        &[Some(value)],
        &[
            "source_ref",
            "sourceRef",
            "source_path",
            "sourcePath",
            "path",
            "intent_path",
            "intentPath",
            "file_path",
            "source_file",
        ],
    )
}

fn numeric_field(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

fn credential_ref_source_id(value: &Value) -> Option<String> {
    let namespace = text_field(value, "namespace")?;
    let key_name = text_field(value, "key_name").or_else(|| text_field(value, "keyName"))?;
    Some(format!("{namespace}:{key_name}"))
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
        "raw_sources_in_response": selection.include_raw_sources,
        "raw_sources_omitted": !selection.include_raw_sources,
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
                        json!({
                            "source_count": value
                                .get("source_count")
                                .cloned()
                                .unwrap_or_else(|| json!(0)),
                            "item_count": value
                                .get("item_count")
                                .cloned()
                                .unwrap_or_else(|| json!(0)),
                        }),
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
        "raw_sources_in_response": selection.include_raw_sources,
        "raw_sources_omitted": !selection.include_raw_sources,
        "filtered_semantic_conversation_hits": filtered_semantic_conversation_hits(sources),
        "conversation_filtering": "conversation search owns project/time/type filter metrics; context-gather records whether the lane was enabled."
    })
}

fn filtered_semantic_conversation_hits(sources: &serde_json::Map<String, Value>) -> Value {
    sources
        .get("conversation_logs")
        .and_then(|value| {
            value
                .get("filteredSemanticHits")
                .or_else(|| value.get("filtered_semantic_hits"))
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn response_sources(
    sources: &serde_json::Map<String, Value>,
    source_summaries: &Value,
    include_raw_sources: bool,
) -> Value {
    if include_raw_sources {
        Value::Object(sources.clone())
    } else {
        source_summaries.clone()
    }
}

fn raw_sources_policy(include_raw_sources: bool) -> &'static str {
    if include_raw_sources {
        "Raw legacy sources are included because include_raw_sources=true or source_profile=full_debug."
    } else {
        "Raw legacy sources are omitted from the tool response and worker context pack; use source_summaries/evidence_lanes or rerun with include_raw_sources=true/full_debug for diagnostics."
    }
}

fn build_source_summaries(sources: &serde_json::Map<String, Value>) -> Value {
    let mut summaries = serde_json::Map::new();
    for (key, value) in sources {
        summaries.insert(key.clone(), summarize_source(key, value));
    }
    Value::Object(summaries)
}

fn summarize_source(key: &str, value: &Value) -> Value {
    match key {
        "runtime_environment" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "authority");
            insert_field(&mut map, value, "runtime_dir");
            insert_field(&mut map, value, "compiled_runtime_dir");
            insert_field(&mut map, value, "repo_runtime_authority");
            insert_field(&mut map, value, "monitor_endpoints");
            Value::Object(map)
        }
        "project_resolution" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "status");
            insert_field(&mut map, value, "matched_project_id");
            insert_field(&mut map, value, "matchedProjectId");
            insert_field(&mut map, value, "candidate_count");
            if let Some(project) = value.get("matched_project") {
                map.insert("matched_project".to_string(), summarize_project(project));
            }
            Value::Object(map)
        }
        "project_registry" => {
            let mut map = summary_base(key);
            for (name, summarized) in summarize_project(value).as_object().into_iter().flatten() {
                map.insert(name.clone(), summarized.clone());
            }
            Value::Object(map)
        }
        "ssot" => {
            let mut map = summary_base(key);
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                map.insert(
                    "text_preview".to_string(),
                    Value::String(compact_text(text, 720)),
                );
                map.insert("text_chars".to_string(), json!(text.chars().count()));
            }
            insert_field(&mut map, value, "path");
            insert_field(&mut map, value, "source");
            Value::Object(map)
        }
        "kb" => summarize_array_source(key, value, 5),
        "board_tasks" => {
            let mut map = summary_base(key);
            if let Some(meta) = value.get("meta") {
                map.insert("meta".to_string(), meta.clone());
            }
            map.insert(
                "item_count".to_string(),
                json!(array_len(value.get("data"))),
            );
            map.insert(
                "items".to_string(),
                summarize_items(value.get("data"), 5, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "id");
                    insert_field(&mut item_map, item, "title");
                    insert_field(&mut item_map, item, "status");
                    Value::Object(item_map)
                }),
            );
            Value::Object(map)
        }
        "skill_context" => {
            let mut map = summary_base(key);
            map.insert(
                "skills".to_string(),
                summarize_items(value.get("skills"), 10, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "name");
                    insert_field(&mut item_map, item, "matched_by");
                    insert_field(&mut item_map, item, "path");
                    insert_compact_field(&mut item_map, item, "description", 180);
                    Value::Object(item_map)
                }),
            );
            map.insert(
                "project_skill_links".to_string(),
                summarize_items(value.get("project_skill_links"), 6, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "skill");
                    insert_field(&mut item_map, item, "confidence");
                    insert_field(&mut item_map, item, "matchedBy");
                    insert_field(&mut item_map, item, "path");
                    Value::Object(item_map)
                }),
            );
            map.insert(
                "operational_fact_count".to_string(),
                json!(array_len(value.get("operational_facts"))),
            );
            map.insert("kb_count".to_string(), json!(array_len(value.get("kb"))));
            map.insert(
                "board_count".to_string(),
                json!(array_len(value.get("board"))),
            );
            Value::Object(map)
        }
        "infra" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "authority");
            insert_field(&mut map, value, "redaction");
            map.insert(
                "item_count".to_string(),
                json!(array_len(value.get("items"))),
            );
            map.insert(
                "items".to_string(),
                summarize_items(value.get("items"), 5, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "sourceSkill");
                    insert_field(&mut item_map, item, "sourcePath");
                    insert_field(&mut item_map, item, "sourceLine");
                    insert_field(&mut item_map, item, "confidence");
                    insert_field(&mut item_map, item, "promoteTo");
                    insert_field(&mut item_map, item, "credentialInlineRisk");
                    insert_compact_field(&mut item_map, item, "excerpt", 360);
                    Value::Object(item_map)
                }),
            );
            Value::Object(map)
        }
        "credential_refs" => {
            let mut map = summary_base(key);
            map.insert(
                "credential_ref_count".to_string(),
                json!(array_len(value.get("credentialRefs"))),
            );
            map.insert(
                "credentialRefs".to_string(),
                summarize_items(value.get("credentialRefs"), 8, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "namespace");
                    insert_field(&mut item_map, item, "keyName");
                    insert_field(&mut item_map, item, "targetId");
                    insert_field(&mut item_map, item, "requiredCapability");
                    insert_field(&mut item_map, item, "availability");
                    insert_compact_field(&mut item_map, item, "purpose", 220);
                    Value::Object(item_map)
                }),
            );
            Value::Object(map)
        }
        "conversation_logs" => summarize_conversation_source(key, value),
        _ => summarize_array_source(key, value, 5),
    }
}

fn summary_base(kind: &str) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "schema".to_string(),
        Value::String("missiond.source-summary.v1".to_string()),
    );
    map.insert("kind".to_string(), Value::String(kind.to_string()));
    map
}

fn summarize_project(value: &Value) -> Value {
    let mut map = serde_json::Map::new();
    for key in [
        "id",
        "path",
        "intent_path",
        "intentPath",
        "kind",
        "active",
        "source",
        "db_status",
        "dbStatus",
        "github_url",
        "parent_id",
    ] {
        insert_field(&mut map, value, key);
    }
    Value::Object(map)
}

fn summarize_array_source(key: &str, value: &Value, limit: usize) -> Value {
    let mut map = summary_base(key);
    if let Some(items) = value.as_array() {
        map.insert("item_count".to_string(), json!(items.len()));
        map.insert(
            "items".to_string(),
            Value::Array(
                items
                    .iter()
                    .take(limit)
                    .map(summarize_generic_item)
                    .collect(),
            ),
        );
    } else {
        map.insert(
            "shape".to_string(),
            Value::String(value_shape(value).to_string()),
        );
    }
    Value::Object(map)
}

fn summarize_conversation_source(key: &str, value: &Value) -> Value {
    let mut map = summary_base(key);
    for item_key in ["results", "items", "data"] {
        if value.get(item_key).and_then(Value::as_array).is_some() {
            map.insert(
                "item_count".to_string(),
                json!(array_len(value.get(item_key))),
            );
            map.insert(
                "items".to_string(),
                summarize_items(value.get(item_key), 5, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "conversation_id");
                    insert_field(&mut item_map, item, "conversationId");
                    insert_field(&mut item_map, item, "session_id");
                    insert_field(&mut item_map, item, "sessionId");
                    insert_field(&mut item_map, item, "project");
                    insert_field(&mut item_map, item, "conversation_type");
                    insert_field(&mut item_map, item, "timestamp");
                    insert_compact_field(&mut item_map, item, "snippet", 260);
                    insert_compact_field(&mut item_map, item, "content", 260);
                    Value::Object(item_map)
                }),
            );
            return Value::Object(map);
        }
    }
    summarize_array_source(key, value, 5)
}

fn summarize_generic_item(item: &Value) -> Value {
    if let Some(object) = item.as_object() {
        let mut map = serde_json::Map::new();
        for key in [
            "id",
            "key",
            "title",
            "category",
            "source_path",
            "sourcePath",
            "path",
            "status",
        ] {
            insert_field(&mut map, item, key);
        }
        for key in ["summary", "description", "text", "content", "snippet"] {
            insert_compact_field(&mut map, item, key, 260);
        }
        if map.is_empty() {
            map.insert("field_count".to_string(), json!(object.len()));
        }
        Value::Object(map)
    } else if let Some(text) = item.as_str() {
        Value::String(compact_text(text, 260))
    } else {
        item.clone()
    }
}

fn summarize_items<F>(value: Option<&Value>, limit: usize, mapper: F) -> Value
where
    F: Fn(&Value) -> Value,
{
    let Some(items) = value.and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    Value::Array(items.iter().take(limit).map(mapper).collect())
}

fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map(Vec::len).unwrap_or(0)
}

fn insert_field(map: &mut serde_json::Map<String, Value>, value: &Value, key: &str) {
    if let Some(field) = value.get(key) {
        map.insert(key.to_string(), field.clone());
    }
}

fn insert_compact_field(
    map: &mut serde_json::Map<String, Value>,
    value: &Value,
    key: &str,
    max_chars: usize,
) {
    if let Some(text) = value.get(key).and_then(Value::as_str) {
        map.insert(
            key.to_string(),
            Value::String(compact_text(text, max_chars)),
        );
    }
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut truncated = collapsed.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn value_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
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

fn build_context_gather_run_input(
    payload: &Value,
    profile: SourceProfile,
    selection: SourceSelection,
    artifact_hash: Option<&str>,
) -> ContextGatherRunInput {
    let query = payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let project_id = payload
        .get("project_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let task_id = payload
        .get("task_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let lane_counts = payload
        .get("context_noise_metrics")
        .and_then(|value| value.get("lane_counts"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut metrics = payload
        .get("context_noise_metrics")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(object) = metrics.as_object_mut() {
        object.insert(
            "raw_source_injected".to_string(),
            Value::Bool(selection.include_raw_sources),
        );
        object.insert(
            "credential_opt_in".to_string(),
            Value::Bool(selection.include_credentials),
        );
        object.insert(
            "conversation_opt_in".to_string(),
            Value::Bool(selection.include_conversations),
        );
        object.insert(
            "support_ref_count".to_string(),
            payload
                .get("support_catalog")
                .and_then(|value| value.get("credential_ref_count"))
                .cloned()
                .unwrap_or_else(|| json!(0)),
        );
        object.insert(
            "resolver_source".to_string(),
            resolver_source_from_payload(payload)
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "runtime_root_consistent".to_string(),
            runtime_root_consistent_from_payload(payload)
                .map(Value::Bool)
                .unwrap_or(Value::Null),
        );
    }
    ContextGatherRunInput {
        id: stable_context_gather_run_id(
            &query,
            project_id.as_deref(),
            task_id.as_deref(),
            profile,
            artifact_hash,
        ),
        query,
        project_id,
        task_id,
        source_profile: profile.as_str().to_string(),
        lane_counts,
        metrics,
        raw_sources_included: selection.include_raw_sources,
        credential_opt_in: selection.include_credentials,
        conversation_opt_in: selection.include_conversations,
        resolver_source: resolver_source_from_payload(payload),
        runtime_root_consistent: runtime_root_consistent_from_payload(payload),
        artifact_hash: artifact_hash.map(ToOwned::to_owned),
        diagnostics: payload
            .get("diagnostics")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    }
}

fn stable_context_gather_run_id(
    query: &str,
    project_id: Option<&str>,
    task_id: Option<&str>,
    profile: SourceProfile,
    artifact_hash: Option<&str>,
) -> String {
    let input = format!(
        "{}|{}|{}|{}|{}",
        query,
        project_id.unwrap_or(""),
        task_id.unwrap_or(""),
        profile.as_str(),
        artifact_hash.unwrap_or("")
    );
    format!("context-gather-{}", short_sha256(&input, 16))
}

fn resolver_source_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("source_summaries")
        .and_then(|summaries| summaries.get("project_resolution"))
        .and_then(|summary| summary.get("matched_project"))
        .and_then(|project| project.get("source"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("source_summaries")
                .and_then(|summaries| summaries.get("project_registry"))
                .and_then(|summary| summary.get("source"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("support_catalog")
                .and_then(|catalog| catalog.get("resolver_source"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn runtime_root_consistent_from_payload(payload: &Value) -> Option<bool> {
    let runtime = payload.get("runtime_environment")?;
    let project_root = runtime.get("project_root").and_then(Value::as_str)?;
    let runtime_dir = runtime.get("runtime_dir").and_then(Value::as_str)?;
    let compiled_runtime_dir = runtime
        .get("compiled_runtime_dir")
        .and_then(Value::as_str)?;
    let env_runtime = runtime
        .get("env_presence")
        .and_then(|env| env.get("MISSIOND_RUNTIME_DIR"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let env_compiled = runtime
        .get("env_presence")
        .and_then(|env| env.get("MISSIOND_COMPILED_RUNTIME_DIR"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if env_runtime || env_compiled {
        return Some(!runtime_dir.trim().is_empty() && !compiled_runtime_dir.trim().is_empty());
    }
    Some(runtime_dir.starts_with(project_root) && compiled_runtime_dir.starts_with(runtime_dir))
}

async fn persist_evidence_lane_projection(
    state: &AppState,
    run: &ContextGatherRunInput,
    items: &[EvidenceItemInput],
) -> Value {
    let mut errors = Vec::new();
    let mut evidence_items_written = 0usize;
    if let Err(err) = state.store.record_context_gather_run(run).await {
        tracing::warn!(run_id = run.id.as_str(), error = %err, "failed to persist context_gather_runs row");
        errors.push(json!({"target": "context_gather_runs", "error": err.to_string()}));
    }
    if !items.is_empty() {
        match state.store.upsert_evidence_items(items).await {
            Ok(count) => evidence_items_written = count,
            Err(err) => {
                tracing::warn!(run_id = run.id.as_str(), error = %err, "failed to persist evidence_items projection");
                errors.push(json!({"target": "evidence_items", "error": err.to_string()}));
            }
        }
    }
    json!({
        "schema": "missiond.evidence-lane-persistence.v1",
        "ok": errors.is_empty(),
        "context_gather_run_id": run.id.as_str(),
        "evidence_item_count": items.len(),
        "evidence_items_written": evidence_items_written,
        "errors": errors,
    })
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
    collect_evidence_refs_from_value(&Value::Object(sources.clone()))
}

fn collect_evidence_refs_from_value(value: &Value) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_evidence_refs_inner(value, "$", &mut refs);
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
    use serde_json::{json, Value};

    use super::{
        build_evidence_items, build_evidence_lanes, build_source_summaries, build_support_catalog,
        collect_evidence_refs_from_value, context_noise_metrics, context_pack_artifact_payload,
        response_sources, source_selection, ContextGatherArgs, SourceProfile,
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

    #[test]
    fn compact_response_sources_omit_raw_skill_operational_facts() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "skill_context".to_string(),
            json!({
                "skills": [{"name": "deploy-ops", "path": "/skills/deploy-ops/SKILL.md", "matched_by": "query"}],
                "operational_facts": [{
                    "skill": "deploy-ops",
                    "source_path": "/skills/deploy-ops/SKILL.md",
                    "source_line": 174,
                    "key": "xjp-router docker-compose.yml volumes",
                    "value": "full raw operational fact"
                }]
            }),
        );
        let summaries = build_source_summaries(&sources);
        let compact = response_sources(&sources, &summaries, false);
        let skill_summary = compact
            .get("skill_context")
            .expect("skill context summary in compact response");
        assert!(skill_summary.get("operational_facts").is_none());
        assert_eq!(
            skill_summary
                .get("operational_fact_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        let refs = collect_evidence_refs_from_value(&compact);
        assert!(refs.iter().any(|item| {
            item.get("value").and_then(|value| value.as_str())
                == Some("/skills/deploy-ops/SKILL.md")
        }));
        assert!(!refs.iter().any(|item| {
            item.get("value").and_then(|value| value.as_str())
                == Some("xjp-router docker-compose.yml volumes")
        }));

        let raw = response_sources(&sources, &summaries, true);
        assert!(raw
            .get("skill_context")
            .and_then(|value| value.get("operational_facts"))
            .and_then(|value| value.as_array())
            .is_some_and(|items| items.len() == 1));
    }

    #[test]
    fn evidence_lanes_count_only_non_empty_sources() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "conversation_logs".to_string(),
            json!({"results": [], "filteredSemanticHits": 3}),
        );
        sources.insert(
            "runtime_environment".to_string(),
            json!({
                "schema": "missiond.runtime-environment-context.v1",
                "authority": "runtime-env-and-monitor",
                "runtime_dir": "/runtime/missiond"
            }),
        );
        let lanes = build_evidence_lanes(&sources);
        let conversation_lane = lanes
            .get("lanes")
            .and_then(|value| value.get("conversation_audit"))
            .expect("conversation lane");
        assert_eq!(
            conversation_lane
                .get("source_count")
                .and_then(|value| value.as_u64()),
            Some(0)
        );
        assert_eq!(
            conversation_lane
                .get("item_count")
                .and_then(|value| value.as_u64()),
            Some(0)
        );

        let runtime_lane = lanes
            .get("lanes")
            .and_then(|value| value.get("runtime_truth"))
            .expect("runtime lane");
        assert_eq!(
            runtime_lane
                .get("source_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn context_noise_metrics_reports_filtered_conversation_hits() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "conversation_logs".to_string(),
            json!({"results": [], "filteredSemanticHits": 4}),
        );
        let lanes = build_evidence_lanes(&sources);
        let args = args(json!({"query": "audit", "source_profile": "conversation_audit"}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        let metrics = context_noise_metrics(profile, selection, &sources, &lanes);
        assert_eq!(
            metrics
                .get("filtered_semantic_conversation_hits")
                .and_then(|value| value.as_u64()),
            Some(4)
        );
    }

    #[test]
    fn support_catalog_redacts_credential_refs() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "project_registry".to_string(),
            json!({
                "id": "payments",
                "source": "compiled-service-runtime",
                "serviceRuntime": {
                    "id": "payments-api",
                    "project": "payments",
                    "domains": ["pay.example.com"],
                    "health": ["/health"],
                    "public_base_url": "https://pay.example.com"
                }
            }),
        );
        sources.insert(
            "credential_refs".to_string(),
            json!({
                "credentialRefs": [{
                    "namespace": "secret-store",
                    "keyName": "PAYMENTS_DB_URL",
                    "value": "postgres://should-not-appear",
                    "availability": "available"
                }]
            }),
        );

        let catalog = build_support_catalog(&sources);
        assert_eq!(
            catalog.get("service_id").and_then(Value::as_str),
            Some("payments-api")
        );
        let rendered = serde_json::to_string(&catalog).expect("support catalog json");
        assert!(rendered.contains("PAYMENTS_DB_URL"));
        assert!(!rendered.contains("postgres://should-not-appear"));
    }

    #[test]
    fn evidence_items_use_typed_lanes_and_compact_sources() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "runtime_environment".to_string(),
            json!({
                "schema": "missiond.runtime-environment-context.v1",
                "authority": "runtime-env-and-monitor",
                "runtime_dir": "/runtime/missiond",
                "compiled_runtime_dir": "/runtime/missiond/compiled"
            }),
        );
        sources.insert(
            "conversation_logs".to_string(),
            json!({"results": [{"conversation_id": "c1", "content": "bounded audit summary"}]}),
        );
        let summaries = build_source_summaries(&sources);
        let catalog = build_support_catalog(&sources);
        let items = build_evidence_items(
            &sources,
            &summaries,
            &catalog,
            SourceProfile::ConversationAudit,
            Some("payments"),
            None,
        );
        assert!(items.iter().any(|item| item.lane_id == "runtime_truth"));
        assert!(items.iter().any(
            |item| item.lane_id == "conversation_audit" && item.raw_policy == "raw_opt_in_only"
        ));
    }
}
