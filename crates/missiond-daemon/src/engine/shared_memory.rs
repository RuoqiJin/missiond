use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use missiond_core::db::traits::MissionStore;
use missiond_core::event::events::{BoardEvent, SlotEvent, SystemEvent, TaskEvent};
use missiond_core::types::{AddBoardTaskNoteInput, UpdateBoardTaskInput};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::bus::BusServices;

const DEFAULT_QUERY_LIMIT: i64 = 50;
const MAX_QUERY_LIMIT: i64 = 500;
const DEFAULT_LEASE_SECS: i64 = 1800;
const MAX_LEASE_SECS: i64 = 7200;
const EVIDENCE_REQUIRED_CODE: &str = "EVIDENCE_REQUIRED";
const CLAIM_CONFLICT_CODE: &str = "CLAIM_CONFLICT";
const COMPLETION_ARTIFACT_INVALID_CODE: &str = "COMPLETION_ARTIFACT_INVALID";
const COMPLETION_ARTIFACT_WRITE_FAILED_CODE: &str = "COMPLETION_ARTIFACT_WRITE_FAILED";
const CAPABILITY_DENIED_CODE: &str = "CAPABILITY_DENIED";
const RUNTIME_METADATA_REQUIRED_CODE: &str = "RUNTIME_METADATA_REQUIRED";
const TASK_CONTRACT_REQUIRED_CODE: &str = "TASK_CONTRACT_REQUIRED";
const WRITE_SCOPE_VIOLATION_CODE: &str = "WRITE_SCOPE_VIOLATION";
#[allow(dead_code)]
const FEATURE_DISABLED_CODE: &str = "FEATURE_DISABLED";

#[derive(Debug, Clone)]
pub(crate) struct StructuredControlError {
    pub code: &'static str,
    pub message: String,
    pub details: Value,
    pub suggestion: Option<String>,
}

impl StructuredControlError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: json!({}),
            suggestion: None,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl std::fmt::Display for StructuredControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StructuredControlError {}

fn control_error(code: &'static str, message: impl Into<String>) -> anyhow::Error {
    StructuredControlError::new(code, message).into()
}

fn control_error_details(
    code: &'static str,
    message: impl Into<String>,
    details: Value,
) -> anyhow::Error {
    StructuredControlError::new(code, message)
        .with_details(details)
        .into()
}

fn kernel_routed_action_error(action: &str) -> anyhow::Error {
    StructuredControlError::new(
        CAPABILITY_DENIED_CODE,
        format!("shared memory action `{action}` must enter through ControlPlaneKernel"),
    )
    .with_details(json!({
        "action": action,
        "required": "ControlPlaneKernel",
        "adapter": "mission_shared_memory"
    }))
    .with_suggestion("call the mission_shared_memory adapter so it can route this control action through ControlPlaneKernel")
    .into()
}

fn ensure_optional_feature_enabled_for_shared_action(
    action: &str,
    feature: &str,
    env_key: &str,
    reason: &str,
) -> Result<()> {
    if crate::feature_gates::optional_feature_enabled(env_key) {
        return Ok(());
    }

    Err(StructuredControlError::new(
        FEATURE_DISABLED_CODE,
        format!(
            "mission_shared_memory action `{action}` belongs to optional MissionD {feature} layer and is disabled in kernel-core mode"
        ),
    )
    .with_details(json!({
        "schema": "missiond.feature-disabled.v1",
        "tool": "mission_shared_memory",
        "action": action,
        "feature": feature,
        "layer": "full-os",
        "enable_env": env_key,
        "enable_all_env": crate::feature_gates::FULL_OS_ENV,
        "reason": reason
    }))
    .with_suggestion(format!(
        "enable {}=true or {}=true for this full-os action; kernel-core keeps only delegate/lease/capability/attempt/artifact/settle/projection paths",
        env_key,
        crate::feature_gates::FULL_OS_ENV
    ))
    .into())
}

fn ensure_router_experiments_enabled_for_shared_action(action: &str) -> Result<()> {
    ensure_optional_feature_enabled_for_shared_action(
        action,
        "router-experiments",
        crate::feature_gates::ROUTER_EXPERIMENTS_ENV,
        "model_route_outcomes and route learning are non-core projections",
    )
}

fn ensure_workflow_enabled_for_shared_action(action: &str) -> Result<()> {
    ensure_optional_feature_enabled_for_shared_action(
        action,
        "workflow",
        crate::feature_gates::WORKFLOW_ENV,
        "workflow runs, checkpoints, plan DAG, review gate, and swarm orchestration are full-os optional layers",
    )
}

#[derive(Clone)]
pub(crate) struct SharedMemoryService {
    pool: PgPool,
    store: Arc<dyn MissionStore>,
    bus: Arc<BusServices>,
    missiond_root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimRequest {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub owner_id: String,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub scope_kind: String,
    pub scope_key: String,
    pub lease_secs: i64,
    pub metadata: Value,
    pub allow_system_bypass: bool,
    pub bypass_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReleaseLeaseRequest {
    pub claim_id: String,
    pub owner_id: Option<String>,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub details: Value,
    pub allow_system_bypass: bool,
    pub bypass_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct HeartbeatLeaseRequest {
    pub claim_id: String,
    pub owner_id: Option<String>,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub lease_secs: i64,
    pub details: Value,
    pub allow_system_bypass: bool,
    pub bypass_reason: Option<String>,
}

pub(crate) struct CapabilityGrantInput<'a> {
    pub(crate) subject_kind: &'a str,
    pub(crate) subject_id: &'a str,
    pub(crate) operation: &'a str,
    pub(crate) scope_kind: &'a str,
    pub(crate) scope_key: &'a str,
    pub(crate) project_id: Option<&'a str>,
    pub(crate) task_id: Option<&'a str>,
    pub(crate) issuer: &'a str,
    pub(crate) evidence_requirement: Option<&'a str>,
    pub(crate) details: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct CapabilityCheckRequest {
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub operation: String,
    pub scope_kind: String,
    pub scope_key: String,
    pub task_id: Option<String>,
    pub allow_system_bypass: bool,
    pub bypass_reason: Option<String>,
    pub details: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct JobEventRequest {
    pub task_id: String,
    pub project_id: Option<String>,
    pub agent_id: String,
    pub event_kind: String,
    pub attempt_id: Option<String>,
    pub worker_id: Option<String>,
    pub conversation_id: Option<String>,
    pub runtime_metadata: Value,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct AppendSharedEventRequest {
    pub stream_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub event_kind: String,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    pub parent_event_ids: Value,
    pub trace_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkerSettleRequest {
    pub task_id: String,
    pub project_id: Option<String>,
    pub slot_id: Option<String>,
    pub conversation_id: Option<String>,
    pub artifact_hash: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub attempt_id: Option<String>,
    pub allow_system_bypass: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskResultPutRequest {
    pub task_id: String,
    pub project_id: String,
    pub slot_id: Option<String>,
    pub conversation_id: Option<String>,
    pub provider: String,
    pub result_status: String,
    pub summary: String,
    pub content: Value,
    pub details: Value,
    pub accepted_shard_id: Option<String>,
    pub attempt_id: Option<String>,
    pub grant_id: Option<String>,
    pub subject_kind: String,
    pub subject_id: String,
    pub producer: Value,
    pub raw_evidence: Option<Value>,
    pub evidence_refs: Vec<Value>,
    pub has_explicit_evidence: bool,
    pub created_at: String,
    pub allow_system_bypass: bool,
    pub(crate) raw_args: Value,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TaskRuntimeContract {
    pub(crate) project_id: Option<String>,
    pub(crate) project_root: Option<String>,
    pub(crate) task_contract_id: Option<String>,
    pub(crate) parent_board_task_id: Option<String>,
    pub(crate) source_board_task_id: Option<String>,
    pub(crate) accepted_shard_id: Option<String>,
    pub(crate) context_pack_path: Option<String>,
    pub(crate) task_class: Option<String>,
    pub(crate) engine_hint: Option<String>,
    pub(crate) pool_hint: Option<String>,
    pub(crate) output_contract: Option<String>,
    pub(crate) conversation_id: Option<String>,
    pub(crate) grounding_context_id: Option<String>,
    pub(crate) read_scope: Vec<String>,
    pub(crate) write_scope: Vec<String>,
    pub(crate) must_not_touch: Vec<String>,
    pub(crate) capability_grant_ids: Vec<String>,
    pub(crate) sandbox_profile: Option<String>,
    pub(crate) completion_materialization_policy: Option<String>,
}

#[derive(Debug, Clone)]
struct WorktreeManifestSnapshot {
    head: Option<String>,
    changed_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct TaskContractMetadataProjection {
    id: String,
    task_contract_id: String,
    project_id: Option<String>,
    dispatch_metadata: Value,
    read_scope: Vec<String>,
    write_scope: Vec<String>,
    must_not_touch: Vec<String>,
    capability_grant_ids: Vec<String>,
    sandbox_profile: Option<String>,
    completion_materialization_policy: Option<String>,
    grounding_refs: Value,
    context_refs: Value,
}

impl SharedMemoryService {
    pub(crate) fn new(
        pool: PgPool,
        store: Arc<dyn MissionStore>,
        bus: Arc<BusServices>,
        missiond_root: PathBuf,
    ) -> Self {
        Self {
            pool,
            store,
            bus,
            missiond_root,
        }
    }

    pub(crate) async fn handle_action(&self, args: &Value) -> Result<Value> {
        let action = string_arg(args, "action").unwrap_or("query");
        match action {
            "append" => self.append_event(args).await,
            "query" => self.query(args).await,
            "artifact_put" | "put_artifact" => self.artifact_put(args).await,
            "artifact_get" | "get_artifact" => self.artifact_get(args).await,
            "task_result_put" | "put_task_result" => Err(kernel_routed_action_error(action)),
            "task_result_get" | "get_task_result" => self.task_result_get(args).await,
            "task_evidence_summary" | "evidence_summary" => self.task_evidence_summary(args).await,
            "workflow_start" | "start_workflow" => {
                ensure_workflow_enabled_for_shared_action(action)?;
                self.workflow_start(args).await
            }
            "workflow_checkpoint" | "checkpoint_workflow" => {
                ensure_workflow_enabled_for_shared_action(action)?;
                self.workflow_checkpoint(args).await
            }
            "workflow_status" | "get_workflow_status" => {
                ensure_workflow_enabled_for_shared_action(action)?;
                self.workflow_status(args).await
            }
            "workflow_summary" | "workflow_runs_summary" => {
                ensure_workflow_enabled_for_shared_action(action)?;
                Ok(self.workflow_runs_summary(bounded_limit(args)).await)
            }
            "runtime_artifact_index" | "index_runtime_artifact" => {
                self.runtime_artifact_index(args).await
            }
            "runtime_artifact_list" | "list_runtime_artifacts" => {
                self.runtime_artifact_list(args).await
            }
            "runtime_artifact_prune" | "prune_runtime_artifacts" => {
                self.runtime_artifact_prune(args).await
            }
            "evidence_view" | "evidence_governance_view" | "get_evidence_view" => {
                self.evidence_view(args).await
            }
            "worker_settle" | "completion_settle" | "settle_worker" | "capability_grant"
            | "grant_capability" | "capability_check" | "check_capability" | "job_event"
            | "record_job_event" | "claim" | "release" | "heartbeat" => {
                Err(kernel_routed_action_error(action))
            }
            "model_route_outcome_put" | "record_model_route_outcome" => {
                ensure_router_experiments_enabled_for_shared_action(action)?;
                self.model_route_outcome_put(args).await
            }
            "cursor" => self.cursor(args).await,
            other => Err(anyhow!("unknown shared memory action: {other}")),
        }
    }

    pub(crate) async fn job_event_command(&self, req: JobEventRequest) -> Result<Value> {
        self.job_event_request(req).await
    }

    pub(crate) async fn append_shared_event_command(
        &self,
        req: AppendSharedEventRequest,
    ) -> Result<Value> {
        self.append_event(&json!({
            "stream_id": req.stream_id,
            "project_id": req.project_id,
            "task_id": req.task_id,
            "agent_id": req.agent_id,
            "event_kind": req.event_kind,
            "idempotency_key": req.idempotency_key,
            "correlation_id": req.correlation_id,
            "parent_event_ids": req.parent_event_ids,
            "trace_id": req.trace_id,
            "payload": req.payload
        }))
        .await
    }

    pub(crate) async fn settle_worker_command(&self, req: WorkerSettleRequest) -> Result<Value> {
        self.worker_settle(req).await
    }

    pub(crate) async fn task_result_put_command(&self, req: TaskResultPutRequest) -> Result<Value> {
        self.task_result_put(req).await
    }

    pub(crate) async fn task_result_get_typed(&self, args: &Value) -> Result<Value> {
        self.task_result_get(args).await
    }

    pub(crate) async fn workflow_start_typed(&self, args: &Value) -> Result<Value> {
        ensure_workflow_enabled_for_shared_action("workflow_start")?;
        self.workflow_start(args).await
    }

    pub(crate) async fn workflow_checkpoint_typed(&self, args: &Value) -> Result<Value> {
        ensure_workflow_enabled_for_shared_action("workflow_checkpoint")?;
        self.workflow_checkpoint(args).await
    }

    pub(crate) async fn model_route_outcome_put_typed(&self, args: &Value) -> Result<Value> {
        ensure_router_experiments_enabled_for_shared_action("model_route_outcome_put")?;
        self.model_route_outcome_put(args).await
    }

    pub(crate) async fn release_lease_typed(&self, req: ReleaseLeaseRequest) -> Result<Value> {
        self.release(req).await
    }

    pub(crate) async fn heartbeat_lease_typed(&self, req: HeartbeatLeaseRequest) -> Result<Value> {
        self.heartbeat(req).await
    }

    pub(crate) async fn claim_lease_typed(&self, req: ClaimRequest) -> Result<Value> {
        self.require_capability(CapabilityCheckRequest {
            grant_id: req.grant_id.clone(),
            subject_kind: req.subject_kind.clone(),
            subject_id: req.subject_id.clone(),
            operation: "claim".to_string(),
            scope_kind: req.scope_kind.clone(),
            scope_key: req.scope_key.clone(),
            task_id: req.task_id.clone(),
            allow_system_bypass: req.allow_system_bypass,
            bypass_reason: req.bypass_reason.clone(),
            details: req.metadata.clone(),
        })
        .await?;
        self.claim(req).await
    }

    pub(crate) async fn grant_task_capabilities(
        &self,
        project_id: Option<&str>,
        task_id: &str,
        subject_kind: &str,
        subject_id: &str,
        read_scope: &[String],
        write_scope: &[String],
        must_not_touch: &[String],
        issuer: &str,
    ) -> Result<Vec<String>> {
        let mut grant_ids = Vec::new();
        for scope in read_scope {
            grant_ids.push(
                self.insert_capability_grant(CapabilityGrantInput {
                    subject_kind,
                    subject_id,
                    operation: "read",
                    scope_kind: "path",
                    scope_key: scope,
                    project_id,
                    task_id: Some(task_id),
                    issuer,
                    evidence_requirement: None,
                    details: json!({"source": "mission_task_delegate"}),
                })
                .await?,
            );
        }
        for scope in write_scope {
            grant_ids.push(
                self.insert_capability_grant(CapabilityGrantInput {
                    subject_kind,
                    subject_id,
                    operation: "write",
                    scope_kind: "path",
                    scope_key: scope,
                    project_id,
                    task_id: Some(task_id),
                    issuer,
                    evidence_requirement: Some("verification_and_changed_paths"),
                    details: json!({
                        "source": "mission_task_delegate",
                        "must_not_touch": must_not_touch
                    }),
                })
                .await?,
            );
        }
        grant_ids.push(
            self.insert_capability_grant(CapabilityGrantInput {
                subject_kind,
                subject_id,
                operation: "write",
                scope_kind: "task",
                scope_key: task_id,
                project_id,
                task_id: Some(task_id),
                issuer,
                evidence_requirement: Some("canonical_task_result_artifact"),
                details: json!({"source": "mission_task_delegate"}),
            })
            .await?,
        );
        grant_ids.push(
            self.insert_capability_grant(CapabilityGrantInput {
                subject_kind,
                subject_id,
                operation: "settle",
                scope_kind: "task",
                scope_key: task_id,
                project_id,
                task_id: Some(task_id),
                issuer,
                evidence_requirement: Some("canonical_task_result_artifact"),
                details: json!({"source": "mission_task_delegate"}),
            })
            .await?,
        );
        grant_ids.push(
            self.insert_capability_grant(CapabilityGrantInput {
                subject_kind,
                subject_id,
                operation: "claim",
                scope_kind: "task",
                scope_key: task_id,
                project_id,
                task_id: Some(task_id),
                issuer,
                evidence_requirement: None,
                details: json!({"source": "mission_task_delegate"}),
            })
            .await?,
        );
        grant_ids.push(
            self.insert_capability_grant(CapabilityGrantInput {
                subject_kind,
                subject_id,
                operation: "spawn",
                scope_kind: "task",
                scope_key: task_id,
                project_id,
                task_id: Some(task_id),
                issuer,
                evidence_requirement: Some("runtime_metadata_sandbox_profile"),
                details: json!({"source": "mission_task_delegate"}),
            })
            .await?,
        );
        Ok(grant_ids)
    }

    pub(crate) async fn upsert_task_contract_from_metadata(
        &self,
        task_id: &str,
        project_id: Option<&str>,
        runtime_metadata: &Value,
    ) -> Result<String> {
        let projection =
            task_contract_projection_from_metadata(task_id, project_id, runtime_metadata);
        sqlx::query(
            r#"
            INSERT INTO task_contracts
              (id, task_id, project_id, task_contract_id, dispatch_metadata,
               read_scope, write_scope, must_not_touch, capability_grant_ids,
               sandbox_profile, completion_materialization_policy, grounding_refs,
               context_refs)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            ON CONFLICT (task_id)
            DO UPDATE SET project_id = EXCLUDED.project_id,
                          task_contract_id = EXCLUDED.task_contract_id,
                          dispatch_metadata = EXCLUDED.dispatch_metadata,
                          read_scope = EXCLUDED.read_scope,
                          write_scope = EXCLUDED.write_scope,
                          must_not_touch = EXCLUDED.must_not_touch,
                          capability_grant_ids = EXCLUDED.capability_grant_ids,
                          sandbox_profile = EXCLUDED.sandbox_profile,
                          completion_materialization_policy = EXCLUDED.completion_materialization_policy,
                          grounding_refs = EXCLUDED.grounding_refs,
                          context_refs = EXCLUDED.context_refs,
                          updated_at = now()
            "#,
        )
        .bind(&projection.id)
        .bind(task_id)
        .bind(projection.project_id.as_deref())
        .bind(&projection.task_contract_id)
        .bind(projection.dispatch_metadata)
        .bind(json!(projection.read_scope))
        .bind(json!(projection.write_scope))
        .bind(json!(projection.must_not_touch))
        .bind(json!(projection.capability_grant_ids))
        .bind(projection.sandbox_profile.as_deref())
        .bind(projection.completion_materialization_policy.as_deref())
        .bind(projection.grounding_refs)
        .bind(projection.context_refs)
        .execute(&self.pool)
        .await?;
        Ok(projection.task_contract_id)
    }

    pub(crate) async fn ensure_task_contract_from_metadata(
        &self,
        task_id: &str,
        project_id: Option<&str>,
        runtime_metadata: &Value,
    ) -> Result<String> {
        if let Some(existing) = sqlx::query_scalar::<_, String>(
            "SELECT task_contract_id FROM task_contracts WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?
        {
            return Ok(existing);
        }

        let projection =
            task_contract_projection_from_metadata(task_id, project_id, runtime_metadata);
        let task_contract_id = projection.task_contract_id.clone();
        let inserted = sqlx::query_scalar::<_, String>(
            r#"
            INSERT INTO task_contracts
              (id, task_id, project_id, task_contract_id, dispatch_metadata,
               read_scope, write_scope, must_not_touch, capability_grant_ids,
               sandbox_profile, completion_materialization_policy, grounding_refs,
               context_refs)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            ON CONFLICT (task_id) DO NOTHING
            RETURNING task_contract_id
            "#,
        )
        .bind(&projection.id)
        .bind(task_id)
        .bind(projection.project_id.as_deref())
        .bind(&projection.task_contract_id)
        .bind(projection.dispatch_metadata)
        .bind(json!(projection.read_scope))
        .bind(json!(projection.write_scope))
        .bind(json!(projection.must_not_touch))
        .bind(json!(projection.capability_grant_ids))
        .bind(projection.sandbox_profile.as_deref())
        .bind(projection.completion_materialization_policy.as_deref())
        .bind(projection.grounding_refs)
        .bind(projection.context_refs)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(inserted) = inserted {
            return Ok(inserted);
        }

        sqlx::query_scalar::<_, String>(
            "SELECT task_contract_id FROM task_contracts WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "task_contracts insert raced but no row exists for task {task_id} ({task_contract_id})"
            )
        })
    }

    pub(crate) async fn update_task_contract_capability_grants(
        &self,
        task_id: &str,
        capability_grant_ids: &[String],
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE task_contracts
            SET capability_grant_ids = $2,
                updated_at = now()
            WHERE task_id = $1
            "#,
        )
        .bind(task_id)
        .bind(Value::Array(
            capability_grant_ids
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn task_completion_materialization_policy(
        &self,
        task_id: &str,
    ) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT completion_materialization_policy FROM task_contracts WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row
            .and_then(|row| {
                row.try_get::<Option<String>, _>("completion_materialization_policy")
                    .ok()
            })
            .flatten())
    }

    pub(crate) async fn insert_capability_grant(
        &self,
        input: CapabilityGrantInput<'_>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO capability_grants
              (id, subject_kind, subject_id, operation, scope_kind, scope_key,
               project_id, task_id, issuer, evidence_requirement, details)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            "#,
        )
        .bind(&id)
        .bind(input.subject_kind)
        .bind(input.subject_id)
        .bind(input.operation)
        .bind(input.scope_kind)
        .bind(input.scope_key)
        .bind(input.project_id)
        .bind(input.task_id)
        .bind(input.issuer)
        .bind(input.evidence_requirement)
        .bind(input.details)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub(crate) async fn audit_capability_bypass(
        &self,
        subject_kind: &str,
        subject_id: &str,
        operation: &str,
        scope_kind: &str,
        scope_key: &str,
        reason: &str,
        details: Value,
    ) -> Result<()> {
        self.audit_capability(
            None,
            Some(subject_kind),
            Some(subject_id),
            operation,
            scope_kind,
            scope_key,
            "allowed",
            None,
            json!({
                "reason": reason,
                "bypass": true,
                "details": details
            }),
        )
        .await
    }

    async fn job_event_request(&self, req: JobEventRequest) -> Result<Value> {
        let task_id = req.task_id;
        let project_id = req.project_id;
        let event_kind = req.event_kind;
        let agent_id = req.agent_id;
        let runtime_metadata = req.runtime_metadata;
        let payload = req.payload;
        let state = job_state_for_event(event_kind.as_str()).unwrap_or("running");
        let job_id = self
            .ensure_job_for_task(
                project_id.as_deref(),
                task_id.as_str(),
                state,
                runtime_metadata,
            )
            .await?;
        let mut attempt_id_result: Option<String> = None;
        if event_kind == "attempt.started" {
            let attempt_id = req
                .attempt_id
                .unwrap_or_else(|| format!("attempt:{task_id}:{}", Utc::now().timestamp_millis()));
            let worker_id = req.worker_id.as_deref().or(Some(agent_id.as_str()));
            let conversation_id = req.conversation_id.as_deref();
            sqlx::query(
                r#"
                INSERT INTO job_attempts
                  (id, job_id, worker_id, conversation_id, state, started_at, details)
                VALUES ($1,$2,$3,$4,'started',now(),$5)
                ON CONFLICT (id)
                DO UPDATE SET state = 'started',
                              started_at = COALESCE(job_attempts.started_at, now()),
                              details = EXCLUDED.details
                "#,
            )
            .bind(&attempt_id)
            .bind(&job_id)
            .bind(worker_id)
            .bind(conversation_id)
            .bind(payload.clone())
            .execute(&self.pool)
            .await?;
            sqlx::query(
                "UPDATE jobs SET current_attempt_id = $2, state = 'running', updated_at = now() WHERE id = $1",
            )
            .bind(&job_id)
            .bind(&attempt_id)
            .execute(&self.pool)
            .await?;
            let contract = self.task_runtime_contract(task_id.as_str()).await?;
            if let Some(project_root) = contract
                .project_root
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let changed_paths = git_status_changed_paths(project_root).unwrap_or_default();
                self.record_worktree_manifest(
                    task_id.as_str(),
                    contract.project_id.as_deref().or(project_id.as_deref()),
                    Some(job_id.as_str()),
                    Some(attempt_id.as_str()),
                    project_root,
                    "pre",
                    &changed_paths,
                    json!({
                        "source": "attempt-baseline",
                        "phase_role": "pre_attempt_baseline"
                    }),
                )
                .await?;
            }
            attempt_id_result = Some(attempt_id);
        }
        let event = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": event_kind,
                "project_id": project_id,
                "task_id": task_id,
                "agent_id": agent_id,
                "idempotency_key": format!("job-event:{task_id}:{event_kind}:{}", Utc::now().timestamp_millis()),
                "payload": payload
            }))
            .await?;
        Ok(json!({
            "schema": "missiond.job-event.v1",
            "ok": true,
            "job_id": job_id,
            "attempt_id": attempt_id_result,
            "state": state,
            "event": event
        }))
    }

    async fn model_route_outcome_put(&self, args: &Value) -> Result<Value> {
        let id = format!("route-outcome:{}", Uuid::new_v4());
        let request_id = string_arg(args, "request_id").or_else(|| string_arg(args, "requestId"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let provider = string_arg(args, "provider").unwrap_or("router_chat");
        let model = string_arg(args, "model").unwrap_or("unknown");
        let task_class = string_arg(args, "task_class")
            .or_else(|| string_arg(args, "taskClass"))
            .unwrap_or("router_chat");
        let outcome_value = args
            .get("outcome")
            .cloned()
            .unwrap_or_else(|| json!({"finish_reason": "completed"}));
        let outcome_text = outcome_value
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                outcome_value
                    .get("finish_reason")
                    .or_else(|| outcome_value.get("status"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "recorded".to_string());
        let outcome = if outcome_value.is_string() {
            json!({ "finish_reason": outcome_text })
        } else {
            outcome_value
        };
        let status = route_outcome_status(&outcome_text);
        let artifact_hash =
            string_arg(args, "artifact_hash").or_else(|| string_arg(args, "artifactHash"));
        let route = string_arg(args, "route");
        let decision = args.get("decision").cloned().unwrap_or_else(|| {
            args.get("metadata")
                .and_then(|value| value.get("route_decision"))
                .cloned()
                .unwrap_or_else(|| json!({}))
        });
        let job_state = string_arg(args, "job_state").or_else(|| string_arg(args, "jobState"));
        let latency_ms = args
            .get("latency_ms")
            .or_else(|| args.get("latencyMs"))
            .and_then(Value::as_i64);
        let prompt_tokens = args
            .get("prompt_tokens")
            .or_else(|| args.get("promptTokens"))
            .or_else(|| args.get("input_tokens"))
            .or_else(|| args.get("inputTokens"))
            .and_then(Value::as_i64);
        let completion_tokens = args
            .get("completion_tokens")
            .or_else(|| args.get("completionTokens"))
            .or_else(|| args.get("output_tokens"))
            .or_else(|| args.get("outputTokens"))
            .and_then(Value::as_i64);
        let total_tokens = args
            .get("total_tokens")
            .or_else(|| args.get("totalTokens"))
            .and_then(Value::as_i64);
        let cost_usd = args
            .get("cost_usd")
            .or_else(|| args.get("costUsd"))
            .and_then(Value::as_f64)
            .or_else(|| {
                args.get("cost_micros")
                    .or_else(|| args.get("costMicros"))
                    .and_then(Value::as_i64)
                    .map(|micros| micros as f64 / 1_000_000.0)
            });
        sqlx::query(
            r#"
            INSERT INTO model_route_outcomes
              (id, request_id, project_id, task_id, provider, model, task_class,
               route, decision, outcome, latency_ms, prompt_tokens,
               completion_tokens, total_tokens, cost_usd, artifact_hash, job_state,
               status)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
            "#,
        )
        .bind(&id)
        .bind(request_id)
        .bind(project_id)
        .bind(task_id)
        .bind(provider)
        .bind(model)
        .bind(task_class)
        .bind(route)
        .bind(decision)
        .bind(outcome)
        .bind(latency_ms)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(total_tokens)
        .bind(cost_usd)
        .bind(artifact_hash)
        .bind(job_state)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.model-route-outcome.v1",
            "ok": true,
            "id": id
        }))
    }

    pub(crate) async fn recommended_model_for_task_class(
        &self,
        task_class: &str,
    ) -> Result<Option<Value>> {
        if !crate::feature_gates::optional_feature_enabled(
            crate::feature_gates::ROUTER_EXPERIMENTS_ENV,
        ) || !runtime_feature_enabled("MISSIOND_ROUTE_LEARNING_ENABLE")
        {
            return Ok(None);
        }
        let row = sqlx::query(
            r#"
            WITH scored AS (
              SELECT provider,
                     model,
                     latency_ms,
                     total_tokens,
                     cost_usd,
                     CASE
                       WHEN status = 'succeeded'
                         OR lower(COALESCE(outcome->>'status', '')) IN ('completed', 'complete', 'success', 'succeeded', 'accepted')
                         OR lower(COALESCE(outcome->>'result', '')) IN ('completed', 'complete', 'success', 'succeeded', 'accepted')
                         OR lower(COALESCE(outcome->>'finish_reason', '')) IN ('completed', 'complete', 'success', 'succeeded', 'accepted', 'stop')
                       THEN 1.0 ELSE 0.0
                     END AS route_success
              FROM model_route_outcomes
              WHERE task_class = $1
                AND model IS NOT NULL
                AND model <> 'unknown'
                AND status IN ('recorded', 'succeeded', 'failed', 'blocked')
                AND created_at > now() - interval '14 days'
            )
            SELECT provider,
                   model,
                   COUNT(*)::bigint AS samples,
                   AVG(route_success) AS success_rate,
                   AVG(NULLIF(latency_ms, 0)) AS avg_latency_ms,
                   AVG(NULLIF(total_tokens, 0)) AS avg_total_tokens,
                   AVG(NULLIF(cost_usd, 0)) AS avg_cost_usd
            FROM scored
            GROUP BY provider, model
            HAVING COUNT(*) >= 5
            ORDER BY
              AVG(route_success) DESC,
              COALESCE(AVG(NULLIF(latency_ms, 0)), 999999999) ASC,
              COALESCE(AVG(NULLIF(total_tokens, 0)), 999999999) ASC,
              COALESCE(AVG(NULLIF(cost_usd, 0)), 999999999) ASC
            LIMIT 1
            "#,
        )
        .bind(task_class)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(json!({
            "schema": "missiond.model-route-recommendation.v1",
            "provider": row.try_get::<String, _>("provider")?,
            "model": row.try_get::<String, _>("model")?,
            "samples": row.try_get::<i64, _>("samples")?,
            "success_rate": row.try_get::<Option<f64>, _>("success_rate")?,
            "avg_latency_ms": row.try_get::<Option<f64>, _>("avg_latency_ms")?,
            "avg_total_tokens": row.try_get::<Option<f64>, _>("avg_total_tokens")?,
            "avg_cost_usd": row.try_get::<Option<f64>, _>("avg_cost_usd")?
        })))
    }

    async fn ensure_job_for_task(
        &self,
        project_id: Option<&str>,
        task_id: &str,
        state: &str,
        runtime_metadata: Value,
    ) -> Result<String> {
        let job_id = format!("job:{task_id}");
        sqlx::query(
            r#"
            INSERT INTO jobs
              (id, project_id, task_id, state, source_kind, source_id, runtime_metadata)
            VALUES ($1,$2,$3,$4,'board_task',$3,$5)
            ON CONFLICT (task_id)
            DO UPDATE SET state = EXCLUDED.state,
                          artifact_hash = COALESCE(EXCLUDED.artifact_hash, jobs.artifact_hash),
                          runtime_metadata = CASE
                            WHEN EXCLUDED.runtime_metadata = '{}'::jsonb THEN jobs.runtime_metadata
                            ELSE EXCLUDED.runtime_metadata
                          END,
                          updated_at = now()
            "#,
        )
        .bind(&job_id)
        .bind(project_id)
        .bind(task_id)
        .bind(state)
        .bind(runtime_metadata)
        .execute(&self.pool)
        .await?;
        Ok(job_id)
    }

    async fn record_control_plane_event(
        &self,
        project_id: Option<&str>,
        task_id: &str,
        event_kind: &str,
        agent_id: &str,
        payload: Value,
        state_override: Option<&str>,
        artifact_hash: Option<&str>,
    ) -> Result<String> {
        let state = state_override
            .or_else(|| job_state_for_event(event_kind))
            .unwrap_or("running");
        let job_id = self
            .ensure_job_for_task(project_id, task_id, state, json!({}))
            .await?;
        if let Some(hash) = artifact_hash {
            sqlx::query(
                r#"
                UPDATE jobs
                SET artifact_hash = $2,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(&job_id)
            .bind(hash)
            .execute(&self.pool)
            .await?;
        }
        let _ = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": event_kind,
                "project_id": project_id,
                "task_id": task_id,
                "agent_id": agent_id,
                "idempotency_key": format!("job-event:{task_id}:{event_kind}:{}", Utc::now().timestamp_millis()),
                "payload": payload
            }))
            .await?;
        Ok(job_id)
    }

    async fn project_board_task_view(
        &self,
        task_id: &str,
        job_id: Option<&str>,
        projected_status: &str,
        artifact_hash: Option<&str>,
        projection: Value,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO board_task_views
              (task_id, job_id, projected_status, artifact_hash, projection, projected_at)
            VALUES ($1,$2,$3,$4,$5,now())
            ON CONFLICT (task_id)
            DO UPDATE SET job_id = EXCLUDED.job_id,
                          projected_status = EXCLUDED.projected_status,
                          artifact_hash = EXCLUDED.artifact_hash,
                          projection = EXCLUDED.projection,
                          projected_at = now()
            "#,
        )
        .bind(task_id)
        .bind(job_id)
        .bind(projected_status)
        .bind(artifact_hash)
        .bind(projection)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn require_capability(&self, req: CapabilityCheckRequest) -> Result<String> {
        if let Some(grant_id) = req.grant_id.as_deref() {
            let grant = sqlx::query(
                r#"
                SELECT id
                FROM capability_grants
                WHERE id = $1
                  AND subject_kind = $2
                  AND subject_id = $3
                  AND operation = $4
                  AND scope_kind = $5
                  AND scope_key = $6
                  AND task_id IS NOT DISTINCT FROM $7
                  AND status = 'active'
                  AND consumed_at IS NULL
                  AND (expires_at IS NULL OR expires_at > now())
                LIMIT 1
                "#,
            )
            .bind(grant_id)
            .bind(req.subject_kind.as_str())
            .bind(req.subject_id.as_str())
            .bind(req.operation.as_str())
            .bind(req.scope_kind.as_str())
            .bind(req.scope_key.as_str())
            .bind(req.task_id.as_deref())
            .fetch_optional(&self.pool)
            .await?;
            if let Some(row) = grant {
                let grant_id: String = row.try_get("id")?;
                self.audit_capability(
                    Some(&grant_id),
                    Some(req.subject_kind.as_str()),
                    Some(req.subject_id.as_str()),
                    req.operation.as_str(),
                    req.scope_kind.as_str(),
                    req.scope_key.as_str(),
                    "allowed",
                    None,
                    json!({
                        "task_id": req.task_id,
                        "exact_grant": true,
                        "details": req.details
                    }),
                )
                .await?;
                return Ok(grant_id);
            }
        } else if req.allow_system_bypass
            && matches!(req.subject_kind.as_str(), "system" | "operator" | "daemon")
        {
            let synthetic = format!("bypass:{}:{}", req.subject_kind, req.operation);
            self.audit_capability(
                None,
                Some(req.subject_kind.as_str()),
                Some(req.subject_id.as_str()),
                req.operation.as_str(),
                req.scope_kind.as_str(),
                req.scope_key.as_str(),
                "allowed",
                None,
                json!({
                    "task_id": req.task_id,
                    "bypass": true,
                    "reason": req
                        .bypass_reason
                        .as_deref()
                        .unwrap_or("confirmed control-plane system/operator bypass"),
                    "details": req.details.clone()
                }),
            )
            .await?;
            return Ok(synthetic);
        }

        self.audit_capability(
            req.grant_id.as_deref(),
            Some(req.subject_kind.as_str()),
            Some(req.subject_id.as_str()),
            req.operation.as_str(),
            req.scope_kind.as_str(),
            req.scope_key.as_str(),
            "denied",
            Some(CAPABILITY_DENIED_CODE),
            json!({
                "reason": "no exact active subject-bound capability grant",
                "task_id": req.task_id.clone(),
                "grant_id": req.grant_id.clone(),
                "subject_kind": req.subject_kind,
                "subject_id": req.subject_id,
                "details": req.details.clone()
            }),
        )
        .await?;
        Err(control_error_details(
            CAPABILITY_DENIED_CODE,
            format!(
                "subject {}:{} lacks exact active grant for {} on {}:{}",
                req.subject_kind, req.subject_id, req.operation, req.scope_kind, req.scope_key
            ),
            json!({
                "task_id": req.task_id.clone(),
                "grant_id": req.grant_id.clone(),
                "subject_kind": req.subject_kind,
                "subject_id": req.subject_id,
                "operation": req.operation,
                "scope_kind": req.scope_kind,
                "scope_key": req.scope_key,
                "required": "grant_id + subject_kind + subject_id + operation + scope_kind + scope_key + task_id"
            }),
        ))
    }

    async fn consume_capability_grant(
        &self,
        grant_id: &str,
        operation: &str,
        task_id: &str,
        subject_kind: &str,
        subject_id: &str,
    ) -> Result<()> {
        if grant_id.starts_with("bypass:") {
            return Ok(());
        }

        let result = sqlx::query(
            r#"
            UPDATE capability_grants
            SET status = 'consumed', consumed_at = now()
            WHERE id = $1
              AND operation = $2
              AND task_id IS NOT DISTINCT FROM $3
              AND subject_kind = $4
              AND subject_id = $5
              AND status = 'active'
              AND consumed_at IS NULL
              AND (expires_at IS NULL OR expires_at > now())
            "#,
        )
        .bind(grant_id)
        .bind(operation)
        .bind(task_id)
        .bind(subject_kind)
        .bind(subject_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() != 1 {
            self.audit_capability(
                Some(grant_id),
                Some(subject_kind),
                Some(subject_id),
                operation,
                "task",
                task_id,
                "denied",
                Some(CAPABILITY_DENIED_CODE),
                json!({
                    "reason": "exact capability grant was not consumable",
                    "task_id": task_id,
                    "grant_id": grant_id,
                    "subject_kind": subject_kind,
                    "subject_id": subject_id
                }),
            )
            .await?;
            return Err(control_error_details(
                CAPABILITY_DENIED_CODE,
                format!("exact {operation} grant {grant_id} for task {task_id} was not consumable"),
                json!({
                    "task_id": task_id,
                    "grant_id": grant_id,
                    "subject_kind": subject_kind,
                    "subject_id": subject_id,
                    "required": "active exact unconsumed grant"
                }),
            ));
        }

        self.audit_capability(
            Some(grant_id),
            Some(subject_kind),
            Some(subject_id),
            operation,
            "task",
            task_id,
            "allowed",
            None,
            json!({
                "task_id": task_id,
                "grant_id": grant_id,
                "consumed": true,
                "exact_grant": true
            }),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn active_capability_grant_id(
        &self,
        task_id: &str,
        subject_kind: &str,
        subject_id: &str,
        operation: &str,
        scope_kind: &str,
        scope_key: &str,
    ) -> Result<Option<String>> {
        let grant_id = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM capability_grants
            WHERE task_id IS NOT DISTINCT FROM $1
              AND subject_kind = $2
              AND subject_id = $3
              AND operation = $4
              AND scope_kind = $5
              AND scope_key = $6
              AND status = 'active'
              AND consumed_at IS NULL
              AND (expires_at IS NULL OR expires_at > now())
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(task_id)
        .bind(subject_kind)
        .bind(subject_id)
        .bind(operation)
        .bind(scope_kind)
        .bind(scope_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(grant_id)
    }

    pub(crate) async fn task_runtime_contract(&self, task_id: &str) -> Result<TaskRuntimeContract> {
        let row = sqlx::query(
            r#"
            SELECT project_id, task_contract_id, dispatch_metadata, read_scope,
                   write_scope, must_not_touch, capability_grant_ids,
                   sandbox_profile, completion_materialization_policy
            FROM task_contracts
            WHERE task_id = $1
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Err(control_error_details(
                TASK_CONTRACT_REQUIRED_CODE,
                format!(
                    "task {task_id} has no canonical task_contracts row; legacy BoardTask.description/runtime_metadata fallback is disabled"
                ),
                json!({
                    "task_id": task_id,
                    "required": "task_contracts",
                    "legacy_fallback": false
                }),
            ));
        };
        let dispatch: Value = row
            .try_get("dispatch_metadata")
            .unwrap_or_else(|_| json!({}));
        let project_id = row
            .try_get::<Option<String>, _>("project_id")?
            .or_else(|| metadata_string_value_any(dispatch.get("project_id")));
        Ok(TaskRuntimeContract {
            project_id,
            project_root: metadata_string_value_any(dispatch.get("project_root")),
            task_contract_id: row.try_get::<Option<String>, _>("task_contract_id")?,
            parent_board_task_id: metadata_string_value_any(dispatch.get("parent_board_task_id"))
                .or_else(|| metadata_string_value_any(dispatch.get("parentBoardTaskId"))),
            source_board_task_id: metadata_string_value_any(dispatch.get("source_board_task_id"))
                .or_else(|| metadata_string_value_any(dispatch.get("sourceBoardTaskId")))
                .or_else(|| metadata_string_value_any(dispatch.get("source_id")))
                .or_else(|| metadata_string_value_any(dispatch.get("sourceId"))),
            accepted_shard_id: metadata_string_value_any(dispatch.get("accepted_shard_id"))
                .or_else(|| metadata_string_value_any(dispatch.get("acceptedShardId"))),
            context_pack_path: metadata_string_value_any(dispatch.get("context_pack_path"))
                .or_else(|| metadata_string_value_any(dispatch.get("contextPackPath"))),
            task_class: metadata_string_value_any(dispatch.get("task_class"))
                .or_else(|| metadata_string_value_any(dispatch.get("taskClass"))),
            engine_hint: metadata_string_value_any(dispatch.get("engine_hint"))
                .or_else(|| metadata_string_value_any(dispatch.get("engineHint"))),
            pool_hint: metadata_string_value_any(dispatch.get("pool_hint"))
                .or_else(|| metadata_string_value_any(dispatch.get("poolHint"))),
            output_contract: metadata_string_value_any(dispatch.get("output_contract"))
                .or_else(|| metadata_string_value_any(dispatch.get("outputContract"))),
            conversation_id: metadata_string_value_any(dispatch.get("conversation_id"))
                .or_else(|| metadata_string_value_any(dispatch.get("conversationId"))),
            grounding_context_id: metadata_string_value_any(dispatch.get("grounding_context_id"))
                .or_else(|| metadata_string_value_any(dispatch.get("groundingContextId"))),
            read_scope: metadata_string_list_any(Some(&row.try_get::<Value, _>("read_scope")?)),
            write_scope: metadata_string_list_any(Some(&row.try_get::<Value, _>("write_scope")?)),
            must_not_touch: metadata_string_list_any(Some(
                &row.try_get::<Value, _>("must_not_touch")?,
            )),
            capability_grant_ids: metadata_string_list_any(Some(
                &row.try_get::<Value, _>("capability_grant_ids")?,
            )),
            sandbox_profile: row.try_get::<Option<String>, _>("sandbox_profile")?,
            completion_materialization_policy: row
                .try_get::<Option<String>, _>("completion_materialization_policy")?,
        })
    }

    async fn verify_completion_scope(
        &self,
        task_id: &str,
        args: &Value,
        details: &Value,
        contract: &TaskRuntimeContract,
    ) -> Result<()> {
        let changed_paths = changed_paths_from_payload(args, details);
        let verification_present = verification_evidence_present(args, details);
        if contract.write_scope.is_empty() && changed_paths.is_empty() {
            return Ok(());
        }
        if contract.write_scope.is_empty() && !changed_paths.is_empty() {
            return Err(control_error_details(
                WRITE_SCOPE_VIOLATION_CODE,
                format!("task {task_id} reports changed paths but has no write_scope grant"),
                json!({
                    "task_id": task_id,
                    "changed_paths": changed_paths,
                    "write_scope": contract.write_scope,
                    "must_not_touch": contract.must_not_touch
                }),
            ));
        }
        if changed_paths.is_empty() {
            return Err(control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!(
                    "task {task_id} completed a write-scoped job without changed-path evidence"
                ),
                json!({
                    "task_id": task_id,
                    "write_scope": contract.write_scope,
                    "required": "changed_paths or files_changed"
                }),
            ));
        }
        if !verification_present {
            return Err(control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!(
                    "task {task_id} completed a write-scoped job without verification evidence"
                ),
                json!({
                    "task_id": task_id,
                    "required": "verification evidence"
                }),
            ));
        }
        let mut violations = Vec::new();
        for path in &changed_paths {
            if contract
                .must_not_touch
                .iter()
                .any(|scope| scope_matches_path(scope, path))
            {
                violations.push(json!({
                    "path": path,
                    "reason": "must_not_touch",
                    "scope": contract.must_not_touch
                }));
                continue;
            }
            if !contract
                .write_scope
                .iter()
                .any(|scope| scope_matches_path(scope, path))
            {
                violations.push(json!({
                    "path": path,
                    "reason": "outside_write_scope",
                    "scope": contract.write_scope
                }));
            }
        }
        if !violations.is_empty() {
            let details = json!({
                "task_id": task_id,
                "violations": violations,
                "write_scope": contract.write_scope,
                "must_not_touch": contract.must_not_touch
            });
            let _ = self
                .record_control_plane_event(
                    contract.project_id.as_deref(),
                    task_id,
                    "capability.denied",
                    "post-run-verifier",
                    json!({
                        "code": WRITE_SCOPE_VIOLATION_CODE,
                        "details": details
                    }),
                    Some("blocked"),
                    None,
                )
                .await;
            return Err(control_error_details(
                WRITE_SCOPE_VIOLATION_CODE,
                format!("task {task_id} changed files outside its write_scope"),
                details,
            ));
        }
        self.verify_actual_worktree_changes(task_id, &changed_paths, contract)
            .await?;
        Ok(())
    }

    async fn verify_actual_worktree_changes(
        &self,
        task_id: &str,
        reported_changed_paths: &[String],
        contract: &TaskRuntimeContract,
    ) -> Result<()> {
        if contract.write_scope.is_empty() {
            return Ok(());
        }
        let Some(project_root) = contract
            .project_root
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Err(control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!("task {task_id} completed write-scoped work without project_root metadata"),
                json!({
                    "task_id": task_id,
                    "required": "runtime_metadata.dispatch_metadata.project_root",
                }),
            ));
        };
        let (job_id, attempt_id) = self.current_job_attempt_for_task(task_id).await?;
        let Some(attempt_id) = attempt_id else {
            return Err(control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!("task {task_id} completed write-scoped work without a current attempt"),
                json!({
                    "task_id": task_id,
                    "required": "jobs.current_attempt_id",
                    "verifier": "attempt baseline diff"
                }),
            ));
        };
        let Some(pre_manifest) = self
            .latest_worktree_manifest(task_id, attempt_id.as_str(), "pre")
            .await?
        else {
            return Err(control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!(
                    "task {task_id} completed write-scoped work without a pre worktree manifest for attempt {attempt_id}"
                ),
                json!({
                    "task_id": task_id,
                    "attempt_id": attempt_id,
                    "required": "worktree_manifests phase=pre",
                    "verifier": "attempt baseline diff"
                }),
            ));
        };
        let status_changed_paths = git_status_changed_paths(project_root).map_err(|err| {
            control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!("task {task_id} post-run verifier failed: {err}"),
                json!({
                    "task_id": task_id,
                    "project_root": project_root,
                    "verifier": "git status --porcelain=v1"
                }),
            )
        })?;
        let post_head = git_head(project_root).ok();
        let head_changed_paths = match (pre_manifest.head.as_deref(), post_head.as_deref()) {
            (Some(pre_head), Some(post_head)) if pre_head != post_head => {
                git_changed_paths_between(project_root, pre_head, post_head).map_err(|err| {
                    control_error_details(
                        COMPLETION_ARTIFACT_INVALID_CODE,
                        format!(
                            "task {task_id} post-run verifier failed to diff attempt heads: {err}"
                        ),
                        json!({
                            "task_id": task_id,
                            "project_root": project_root,
                            "pre_head": pre_head,
                            "post_head": post_head,
                            "verifier": "git diff --name-only"
                        }),
                    )
                })?
            }
            _ => Vec::new(),
        };
        let actual_changed_paths = attempt_actual_changed_paths(
            &pre_manifest.changed_paths,
            &status_changed_paths,
            &head_changed_paths,
        );
        let _ = self
            .record_worktree_manifest(
                task_id,
                contract.project_id.as_deref(),
                job_id.as_deref(),
                Some(attempt_id.as_str()),
                project_root,
                "post",
                &actual_changed_paths,
                json!({
                    "source": "attempt-diff-verifier",
                    "phase_role": "post_attempt_diff",
                    "attempt_id": attempt_id,
                    "pre_head": pre_manifest.head,
                    "post_head": post_head,
                    "pre_changed_paths": pre_manifest.changed_paths,
                    "status_changed_paths": status_changed_paths,
                    "head_changed_paths": head_changed_paths,
                    "algorithm": "actual = committed paths between pre/post HEAD + current dirty paths not present in pre manifest"
                }),
            )
            .await;
        if actual_changed_paths.is_empty() {
            return Err(control_error_details(
                COMPLETION_ARTIFACT_INVALID_CODE,
                format!(
                    "task {task_id} reported changed paths but attempt diff found no actual post-run change"
                ),
                json!({
                    "task_id": task_id,
                    "project_root": project_root,
                    "reported_changed_paths": reported_changed_paths,
                    "verifier": "attempt baseline diff",
                    "required": "post manifest dirty diff or committed HEAD diff"
                }),
            ));
        }
        let mut violations = Vec::new();
        for path in &actual_changed_paths {
            if contract
                .must_not_touch
                .iter()
                .any(|scope| scope_matches_path(scope, path))
            {
                violations.push(json!({
                    "path": path,
                    "reason": "actual_must_not_touch",
                    "scope": contract.must_not_touch
                }));
                continue;
            }
            if !contract
                .write_scope
                .iter()
                .any(|scope| scope_matches_path(scope, path))
            {
                violations.push(json!({
                    "path": path,
                    "reason": "actual_outside_write_scope",
                    "scope": contract.write_scope
                }));
                continue;
            }
            if !reported_changed_paths
                .iter()
                .any(|reported| scope_matches_path(reported, path))
            {
                violations.push(json!({
                    "path": path,
                    "reason": "actual_path_not_reported_by_artifact",
                    "reported_changed_paths": reported_changed_paths
                }));
            }
        }
        if violations.is_empty() {
            return Ok(());
        }
        let details = json!({
            "task_id": task_id,
            "project_root": project_root,
            "actual_changed_paths": actual_changed_paths,
            "reported_changed_paths": reported_changed_paths,
            "violations": violations,
            "write_scope": contract.write_scope,
            "must_not_touch": contract.must_not_touch
        });
        let _ = self
            .record_control_plane_event(
                contract.project_id.as_deref(),
                task_id,
                "capability.denied",
                "post-run-verifier",
                json!({
                    "code": WRITE_SCOPE_VIOLATION_CODE,
                    "details": details
                }),
                Some("blocked"),
                None,
            )
            .await;
        Err(control_error_details(
            WRITE_SCOPE_VIOLATION_CODE,
            format!("task {task_id} actual worktree changes violate write_scope"),
            details,
        ))
    }

    async fn record_worktree_manifest(
        &self,
        task_id: &str,
        project_id: Option<&str>,
        job_id: Option<&str>,
        attempt_id: Option<&str>,
        project_root: &str,
        phase: &str,
        changed_paths: &[String],
        extra: Value,
    ) -> Result<()> {
        let head = git_head(project_root).ok();
        let dirty = !changed_paths.is_empty();
        let mut manifest = json!({
            "schema": "missiond.worktree-manifest.v1",
            "source": "post-run-verifier",
            "changed_paths": changed_paths,
            "head": head,
            "dirty": dirty,
        });
        merge_json_object(&mut manifest, extra);
        sqlx::query(
            r#"
            INSERT INTO worktree_manifests
              (id, job_id, attempt_id, task_id, project_id, project_root, phase,
               manifest, changed_paths)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(format!(
            "manifest:{task_id}:{phase}:{}",
            Utc::now().timestamp_millis()
        ))
        .bind(job_id)
        .bind(attempt_id)
        .bind(task_id)
        .bind(project_id)
        .bind(project_root)
        .bind(phase)
        .bind(manifest)
        .bind(json!(changed_paths))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn latest_worktree_manifest(
        &self,
        task_id: &str,
        attempt_id: &str,
        phase: &str,
    ) -> Result<Option<WorktreeManifestSnapshot>> {
        let row = sqlx::query(
            r#"
            SELECT manifest, changed_paths
            FROM worktree_manifests
            WHERE task_id = $1
              AND attempt_id = $2
              AND phase = $3
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(task_id)
        .bind(attempt_id)
        .bind(phase)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let manifest: Value = row.try_get("manifest")?;
            let changed_paths: Value = row.try_get("changed_paths")?;
            Ok(WorktreeManifestSnapshot {
                head: manifest
                    .get("head")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                changed_paths: metadata_string_list_any(Some(&changed_paths)),
            })
        })
        .transpose()
    }

    async fn current_job_attempt_for_task(
        &self,
        task_id: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        let row = sqlx::query("SELECT id, current_attempt_id FROM jobs WHERE task_id = $1")
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .map(|row| {
                (
                    row.try_get::<String, _>("id").ok(),
                    row.try_get::<Option<String>, _>("current_attempt_id")
                        .ok()
                        .flatten(),
                )
            })
            .unwrap_or((None, None)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn audit_capability(
        &self,
        grant_id: Option<&str>,
        subject_kind: Option<&str>,
        subject_id: Option<&str>,
        operation: &str,
        scope_kind: &str,
        scope_key: &str,
        decision: &str,
        code: Option<&str>,
        details: Value,
    ) -> Result<()> {
        let reason = details
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_owned);
        sqlx::query(
            r#"
            INSERT INTO capability_audit_events
              (id, grant_id, subject_kind, subject_id, operation, scope_kind,
               scope_key, decision, code, reason, details)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(grant_id)
        .bind(subject_kind)
        .bind(subject_id)
        .bind(operation)
        .bind(scope_kind)
        .bind(scope_key)
        .bind(decision)
        .bind(code)
        .bind(reason.as_deref())
        .bind(details)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn status_snapshot(&self) -> Value {
        let expired_stale_claims = self.expire_stale_claims().await.unwrap_or(0);
        let active_claims = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM work_leases WHERE status = 'active' AND lease_expires_at >= now()",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let stale_claims = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM work_leases WHERE status = 'active' AND lease_expires_at < now()",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let artifacts = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM shared_artifacts")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let task_result_artifacts =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM task_result_artifacts")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
        let runtime_artifacts = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM runtime_artifacts WHERE status = 'active'",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let expired_runtime_artifacts = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM runtime_artifacts WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < now()",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let active_workflow_runs = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM workflow_runs WHERE status IN ('running','blocked')",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let latest_seq = sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(seq) FROM shared_events")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        let cursor_lag = sqlx::query(
            r#"
            SELECT agent_id, stream_id, last_seq, GREATEST($1::bigint - last_seq, 0) AS lag
            FROM agent_cursors
            ORDER BY lag DESC, updated_at DESC
            LIMIT 10
            "#,
        )
        .bind(latest_seq)
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| {
                    json!({
                        "agent_id": row.get::<String, _>("agent_id"),
                        "stream_id": row.get::<String, _>("stream_id"),
                        "last_seq": row.get::<i64, _>("last_seq"),
                        "lag": row.get::<i64, _>("lag")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

        json!({
            "schema": "missiond.shared-memory.status.v1",
            "health": "ok",
            "latestSeq": latest_seq,
            "activeClaims": active_claims,
            "staleClaims": stale_claims,
            "expiredStaleClaims": expired_stale_claims,
            "artifactCount": artifacts,
            "taskResultArtifactCount": task_result_artifacts,
            "runtimeArtifactCount": runtime_artifacts,
            "expiredRuntimeArtifactCount": expired_runtime_artifacts,
            "runtimeArtifactRetention": {
                "compiled": "current compiled outputs plus recent historical snapshots; no time expiry",
                "reports": "14 days or last 200 per kind",
                "masterContextPacks": "7 days or last 100",
                "canonicalTaskEvidence": "indexed without automatic deletion"
            },
            "activeWorkflowRuns": active_workflow_runs,
            "cursorLag": cursor_lag
        })
    }

    pub(crate) async fn evidence_health_summary(&self, limit: i64) -> Value {
        self.task_evidence_summary(&json!({ "limit": limit }))
            .await
            .unwrap_or_else(|err| {
                json!({
                    "schema": "missiond.task-evidence-summary.v1",
                    "degraded": true,
                    "error": err.to_string(),
                    "items": []
                })
            })
    }

    pub(crate) async fn workflow_runs_summary(&self, limit: i64) -> Value {
        let limit = limit.clamp(1, 100);
        let stale_after_secs: i64 = 15 * 60;
        let counts = sqlx::query(
            r#"
            SELECT
              COUNT(*) FILTER (WHERE status = 'running') AS running,
              COUNT(*) FILTER (WHERE status = 'blocked') AS blocked,
              COUNT(*) FILTER (WHERE status = 'failed') AS failed,
              COUNT(*) FILTER (
                WHERE status IN ('running','blocked')
                  AND updated_at < now() - ($1::bigint * interval '1 second')
              ) AS stale,
              COALESCE(
                EXTRACT(EPOCH FROM (now() - (MIN(updated_at) FILTER (WHERE status IN ('running','blocked'))))),
                0
              )::bigint AS oldest_updated_age_secs
            FROM workflow_runs
            "#,
        )
        .bind(stale_after_secs)
        .fetch_optional(&self.pool)
        .await;
        let rows = sqlx::query(
            r#"
            SELECT id, workflow_id, workflow_path, project_id, parent_task_id, status,
                   cursor, checkpoint, max_inflight, active_task_ids, artifact_hashes,
                   started_at, updated_at, finished_at,
                   EXTRACT(EPOCH FROM (now() - updated_at))::bigint AS updated_age_secs
            FROM workflow_runs
            WHERE status IN ('running','blocked','failed')
            ORDER BY
              CASE status WHEN 'blocked' THEN 0 WHEN 'failed' THEN 1 ELSE 2 END,
              updated_at ASC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await;

        match (counts, rows) {
            (Ok(counts), Ok(rows)) => {
                let (running, blocked, failed, stale, oldest_updated_age_secs) =
                    counts.map_or((0, 0, 0, 0, 0), |row| {
                        (
                            row.try_get::<i64, _>("running").unwrap_or(0),
                            row.try_get::<i64, _>("blocked").unwrap_or(0),
                            row.try_get::<i64, _>("failed").unwrap_or(0),
                            row.try_get::<i64, _>("stale").unwrap_or(0),
                            row.try_get::<i64, _>("oldest_updated_age_secs")
                                .unwrap_or(0),
                        )
                    });
                json!({
                    "schema": "missiond.workflow-runs-summary.v1",
                    "running": running,
                    "blocked": blocked,
                    "failed": failed,
                    "stale": stale,
                    "staleAfterSecs": stale_after_secs,
                    "oldestUpdatedAgeSecs": oldest_updated_age_secs,
                    "items": rows.into_iter().map(workflow_run_summary_row_json).collect::<Vec<_>>()
                })
            }
            (counts_result, rows_result) => {
                let err = counts_result
                    .err()
                    .map(|err| err.to_string())
                    .or_else(|| rows_result.err().map(|err| err.to_string()))
                    .unwrap_or_else(|| "workflow_runs summary unavailable".to_string());
                json!({
                    "schema": "missiond.workflow-runs-summary.v1",
                    "degraded": true,
                    "error": err,
                    "running": 0,
                    "blocked": 0,
                    "failed": 0,
                    "stale": 0,
                    "items": []
                })
            }
        }
    }

    pub(crate) async fn startup_recover_workflow_runs(&self) -> Result<Value> {
        let rows = sqlx::query(
            r#"
            UPDATE workflow_runs
            SET status = 'blocked',
                checkpoint = checkpoint || jsonb_build_object(
                  'blocked_reason', 'startup recovery requires workflow_id, workflow_path, and parent_task_id',
                  'blocked_at', now()
                ),
                updated_at = now()
            WHERE status = 'running'
              AND (workflow_id IS NULL OR workflow_path IS NULL OR parent_task_id IS NULL)
            RETURNING id, workflow_id, workflow_path, project_id, parent_task_id, status,
                      cursor, checkpoint, max_inflight, active_task_ids, artifact_hashes,
                      started_at, updated_at, finished_at
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.workflow-startup-recovery.v1",
            "blockedUnrecoverable": rows.len(),
            "items": rows.into_iter().map(workflow_run_row_json).collect::<Vec<_>>()
        }))
    }

    pub(crate) async fn claim_status(&self, args: &Value) -> Result<Value> {
        self.expire_stale_claims().await?;
        let limit = bounded_limit(args);
        let scope_kind = string_arg(args, "scope_kind").or_else(|| string_arg(args, "scopeKind"));
        let scope_key = string_arg(args, "scope_key").or_else(|| string_arg(args, "scopeKey"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));

        let rows = sqlx::query(
            r#"
            SELECT id, project_id, task_id, holder_id AS owner_id, scope_kind, scope_key, status,
                   acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            FROM work_leases
            WHERE ($1::text IS NULL OR project_id = $1)
              AND ($2::text IS NULL OR scope_kind = $2)
              AND ($3::text IS NULL OR scope_key = $3)
            ORDER BY acquired_at DESC
            LIMIT $4
            "#,
        )
        .bind(project_id)
        .bind(scope_kind)
        .bind(scope_key)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(json!({
            "schema": "missiond.shared-memory.claim-status.v1",
            "claims": rows.into_iter().map(claim_row_json).collect::<Vec<_>>()
        }))
    }

    pub(crate) async fn context_slice(&self, args: &Value) -> Result<Value> {
        let project_id = string_arg(args, "project")
            .or_else(|| string_arg(args, "project_id"))
            .or_else(|| string_arg(args, "projectId"))
            .unwrap_or("missiond");
        let task_id = string_arg(args, "taskId").or_else(|| string_arg(args, "task_id"));
        let accepted_shard_id =
            string_arg(args, "acceptedShardId").or_else(|| string_arg(args, "accepted_shard_id"));
        let intent = string_arg(args, "intent").or_else(|| string_arg(args, "query"));
        let entry_id = string_arg(args, "entryId").or_else(|| string_arg(args, "entry_id"));
        let surface = string_arg(args, "surface");
        let compiled = self.read_compiled_json("compiled-semantic-ir.json").ok();
        let wants_agent_entry =
            intent.is_some() || entry_id.is_some() || surface.is_some() || project_id != "missiond";
        let agent_slices = if wants_agent_entry && project_id == "missiond" {
            self.read_compiled_json("compiled-agent-slices.json").ok()
        } else {
            None
        };
        let project_agent_navigation = if wants_agent_entry && project_id != "missiond" {
            self.read_compiled_json("compiled-project-agent-navigation.json")
                .ok()
        } else {
            None
        };
        let agent_entry = agent_slices
            .as_ref()
            .and_then(|compiled| select_agent_entry(compiled, entry_id, surface, intent));
        let project_agent_entry = project_agent_navigation
            .as_ref()
            .and_then(|compiled| select_project_agent_entry(compiled, project_id));
        let coverage_diagnostic = if project_id != "missiond" {
            Some(match (&project_agent_entry, &project_agent_navigation) {
                (Some(entry), _) => json!({
                    "code": "PROJECT_AGENT_NAVIGATION_DERIVED",
                    "coverageState": entry.get("coverageState").cloned().unwrap_or(Value::Null),
                    "message": "Project navigation card is derived read-only from MissionD project registry."
                }),
                (None, Some(_)) => json!({
                    "code": "PROJECT_AGENT_ENTRY_NOT_FOUND",
                    "message": "No registered project navigation card matched project_id."
                }),
                (None, None) => json!({
                    "code": "PROJECT_AGENT_NAVIGATION_UNAVAILABLE",
                    "message": "compiled-project-agent-navigation.json is missing or unreadable."
                }),
            })
        } else {
            None
        };
        let facts = compiled
            .as_ref()
            .and_then(|value| value.pointer("/payload/facts"))
            .and_then(|value| value.as_array())
            .map(|facts| {
                facts
                    .iter()
                    .filter(|fact| fact_relevant_to_project(fact, project_id))
                    .take(80)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let artifacts = self
            .recent_artifacts_for_task(project_id, task_id, accepted_shard_id)
            .await?;

        Ok(json!({
            "schema": "missiond.context-slice.v1",
            "project": project_id,
            "taskId": task_id,
            "acceptedShardId": accepted_shard_id,
            "source": {
                "semanticIr": compiled.as_ref().map(|v| v.pointer("/source_hash").cloned()).flatten(),
                "agentSlices": agent_slices.as_ref().map(|v| v.pointer("/source_hash").cloned()).flatten(),
                "projectAgentNavigation": project_agent_navigation.as_ref().map(|v| v.pointer("/source_hash").cloned()).flatten(),
                "artifactStore": "shared_artifacts"
            },
            "agentEntry": agent_entry.or(project_agent_entry),
            "coverageDiagnostic": coverage_diagnostic,
            "facts": facts,
            "artifacts": artifacts,
            "note": "Agents should read this slice before full Lisp. Use source_file/source_line in each fact for focused lookup."
        }))
    }

    pub(crate) async fn claim_write_scope(
        &self,
        project_id: Option<String>,
        task_id: Option<String>,
        owner_id: String,
        write_scope: &[String],
        accepted_shard_id: Option<String>,
    ) -> Result<Vec<Value>> {
        let mut claims = Vec::new();
        for scope in write_scope {
            let req = ClaimRequest {
                project_id: project_id.clone(),
                task_id: task_id.clone(),
                owner_id: owner_id.clone(),
                grant_id: None,
                subject_kind: "daemon".to_string(),
                subject_id: "mission_task_delegate".to_string(),
                scope_kind: "write_scope".to_string(),
                scope_key: scope.to_string(),
                lease_secs: DEFAULT_LEASE_SECS,
                metadata: json!({
                    "accepted_shard_id": accepted_shard_id,
                    "source": "mission_task_delegate"
                }),
                allow_system_bypass: true,
                bypass_reason: Some(
                    "mission_task_delegate pre-claims write_scope before worker subject binding"
                        .to_string(),
                ),
            };
            match self.claim_lease_typed(req).await {
                Ok(claim) => claims.push(claim),
                Err(err) => {
                    if let Some(conflict) = claim_conflict_projection_from_error(&err) {
                        claims.push(conflict);
                    } else {
                        return Err(err);
                    }
                }
            }
        }
        Ok(claims)
    }

    async fn append_event(&self, args: &Value) -> Result<Value> {
        let stream_id = string_arg(args, "stream_id")
            .or_else(|| string_arg(args, "streamId"))
            .unwrap_or("default");
        let event_kind = string_arg(args, "event_kind")
            .or_else(|| string_arg(args, "eventKind"))
            .unwrap_or("shared_memory_event");
        let id = Uuid::new_v4().to_string();
        let payload = args.get("payload").cloned().unwrap_or_else(|| json!({}));
        let idempotency_key =
            string_arg(args, "idempotency_key").or_else(|| string_arg(args, "idempotencyKey"));
        let correlation_id =
            string_arg(args, "correlation_id").or_else(|| string_arg(args, "correlationId"));
        let parent_event_ids = args
            .get("parent_event_ids")
            .or_else(|| args.get("parentEventIds"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let agent_id = string_arg(args, "agent_id").or_else(|| string_arg(args, "agentId"));

        let row = sqlx::query(
            r#"
            INSERT INTO shared_events
              (id, stream_id, project_id, task_id, agent_id, event_kind, payload,
               idempotency_key, correlation_id, parent_event_ids)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            ON CONFLICT (stream_id, idempotency_key) WHERE idempotency_key IS NOT NULL
            DO UPDATE SET payload = shared_events.payload
            RETURNING id, seq, created_at
            "#,
        )
        .bind(&id)
        .bind(stream_id)
        .bind(project_id)
        .bind(task_id)
        .bind(agent_id)
        .bind(event_kind)
        .bind(&payload)
        .bind(idempotency_key)
        .bind(correlation_id)
        .bind(&parent_event_ids)
        .fetch_one(&self.pool)
        .await?;

        let event_id: String = row.get("id");
        let seq: i64 = row.get("seq");
        let created_at: DateTime<Utc> = row.get("created_at");
        let result = json!({
            "schema": "missiond.shared-event.v1",
            "id": event_id,
            "stream_id": stream_id,
            "seq": seq,
            "event_kind": event_kind,
            "created_at": created_at.to_rfc3339()
        });

        let _ = self
            .bus
            .publish_system_webhook(SystemEvent::ExternalServiceEvent {
                service_id: "missiond-shared-memory".to_string(),
                event_id,
                event_kind: event_kind.to_string(),
                summary: format!("shared-memory event {event_kind} on {stream_id}"),
                trace_id: string_arg(args, "trace_id")
                    .or_else(|| string_arg(args, "traceId"))
                    .map(str::to_string),
                payload_json: serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()),
            })
            .await;

        Ok(result)
    }

    async fn query(&self, args: &Value) -> Result<Value> {
        self.expire_stale_claims().await?;
        let limit = bounded_limit(args);
        let stream_id = string_arg(args, "stream_id").or_else(|| string_arg(args, "streamId"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let since_seq = args
            .get("since_seq")
            .or_else(|| args.get("sinceSeq"))
            .and_then(Value::as_i64)
            .unwrap_or(0);

        let rows = sqlx::query(
            r#"
            SELECT id, stream_id, seq, project_id, task_id, agent_id, event_kind,
                   payload, idempotency_key, correlation_id, parent_event_ids, created_at
            FROM shared_events
            WHERE seq > $1
              AND ($2::text IS NULL OR stream_id = $2)
              AND ($3::text IS NULL OR project_id = $3)
              AND ($4::text IS NULL OR task_id = $4)
            ORDER BY seq ASC
            LIMIT $5
            "#,
        )
        .bind(since_seq)
        .bind(stream_id)
        .bind(project_id)
        .bind(task_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(json!({
            "schema": "missiond.shared-memory.query.v1",
            "events": rows.into_iter().map(event_row_json).collect::<Vec<_>>()
        }))
    }

    async fn artifact_put(&self, args: &Value) -> Result<Value> {
        let kind = string_arg(args, "kind").unwrap_or("artifact");
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let media_type = string_arg(args, "media_type")
            .or_else(|| string_arg(args, "mediaType"))
            .unwrap_or("application/json");
        let content = if let Some(s) = string_arg(args, "content") {
            s.as_bytes().to_vec()
        } else {
            serde_json::to_vec(args.get("json").unwrap_or(&Value::Null))?
        };
        let metadata = args.get("metadata").cloned().unwrap_or_else(|| json!({}));
        let artifact = self
            .put_artifact_bytes(kind, project_id, task_id, media_type, content, metadata)
            .await?;

        Ok(json!({
            "schema": "missiond.shared-artifact.v1",
            "hash": artifact.hash,
            "kind": kind,
            "size_bytes": artifact.size_bytes,
            "media_type": media_type
        }))
    }

    async fn put_artifact_bytes(
        &self,
        kind: &str,
        project_id: Option<&str>,
        task_id: Option<&str>,
        media_type: &str,
        content: Vec<u8>,
        metadata: Value,
    ) -> Result<StoredArtifact> {
        let hash = sha256_hex(&content);
        let size_bytes = i64::try_from(content.len()).unwrap_or(i64::MAX);

        sqlx::query(
            r#"
            INSERT INTO shared_artifacts
              (hash, kind, project_id, task_id, media_type, bytes, size_bytes, metadata)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            ON CONFLICT (hash) DO NOTHING
            "#,
        )
        .bind(&hash)
        .bind(kind)
        .bind(project_id)
        .bind(task_id)
        .bind(media_type)
        .bind(&content)
        .bind(size_bytes)
        .bind(&metadata)
        .execute(&self.pool)
        .await?;

        Ok(StoredArtifact { hash, size_bytes })
    }

    pub(crate) async fn put_json_artifact(
        &self,
        kind: &str,
        project_id: Option<&str>,
        task_id: Option<&str>,
        body: &Value,
        metadata: Value,
    ) -> Result<Value> {
        let content = serde_json::to_vec(body)?;
        let artifact = self
            .put_artifact_bytes(
                kind,
                project_id,
                task_id,
                "application/json",
                content,
                metadata,
            )
            .await?;
        Ok(json!({
            "schema": "missiond.shared-artifact.v1",
            "hash": artifact.hash,
            "kind": kind,
            "size_bytes": artifact.size_bytes,
            "media_type": "application/json"
        }))
    }

    async fn artifact_get(&self, args: &Value) -> Result<Value> {
        let hash = string_arg(args, "hash").ok_or_else(|| anyhow!("hash is required"))?;
        let row = sqlx::query(
            "SELECT hash, kind, project_id, task_id, media_type, bytes, size_bytes, metadata, created_at FROM shared_artifacts WHERE hash = $1",
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(artifact_row_json(row, true)),
            None => {
                Ok(json!({"schema": "missiond.shared-artifact.v1", "found": false, "hash": hash}))
            }
        }
    }

    pub(crate) fn task_result_put_request_from_args(
        &self,
        args: &Value,
    ) -> Result<TaskResultPutRequest> {
        let task_id = string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .ok_or_else(|| anyhow!("task_id is required"))?
            .to_string();
        let project_id = string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .unwrap_or("missiond")
            .to_string();
        let slot_id = string_arg(args, "slot_id")
            .or_else(|| string_arg(args, "slotId"))
            .map(str::to_string);
        let conversation_id = string_arg(args, "conversation_id")
            .or_else(|| string_arg(args, "conversationId"))
            .map(str::to_string);
        let provider = string_arg(args, "provider")
            .unwrap_or("unknown")
            .to_string();
        let result_status = string_arg(args, "result_status")
            .or_else(|| string_arg(args, "resultStatus"))
            .or_else(|| string_arg(args, "result_kind"))
            .or_else(|| string_arg(args, "resultKind"))
            .unwrap_or("completed")
            .to_string();
        let summary = string_arg(args, "summary")
            .map(str::to_string)
            .unwrap_or_else(|| summary_from_result_payload(args));
        let details = args.get("json").cloned().unwrap_or_else(|| json!({}));
        let content = args
            .get("content")
            .cloned()
            .filter(|value| !value.is_null())
            .unwrap_or_else(|| details.clone());
        let raw_evidence = args
            .get("raw_evidence")
            .cloned()
            .or_else(|| args.get("rawEvidence").cloned())
            .or_else(|| {
                details
                    .get("raw_evidence")
                    .cloned()
                    .or_else(|| details.get("rawEvidence").cloned())
            });
        let evidence_refs = task_result_evidence_refs(
            args,
            &details,
            raw_evidence.as_ref(),
            &provider,
            slot_id.as_deref(),
            conversation_id.as_deref(),
        );
        let has_explicit_evidence =
            task_result_has_explicit_evidence(args, &details, raw_evidence.as_ref());
        let producer = args
            .get("producer")
            .cloned()
            .or_else(|| details.get("producer").cloned())
            .unwrap_or_else(|| {
                json!({
                    "kind": "worker-completion-producer",
                    "provider": provider,
                    "slot_id": slot_id,
                    "conversation_id": conversation_id
                })
            });
        let created_at = string_arg(args, "created_at")
            .or_else(|| string_arg(args, "createdAt"))
            .map(str::to_string)
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let attempt_id = string_arg(args, "attempt_id")
            .or_else(|| string_arg(args, "attemptId"))
            .map(str::to_string);
        let accepted_shard_id = string_arg(args, "accepted_shard_id")
            .or_else(|| string_arg(args, "acceptedShardId"))
            .map(str::to_string);
        let subject_kind = string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or_else(|| {
                if slot_id.is_some() {
                    "worker"
                } else if conversation_id.is_some() {
                    "conversation"
                } else {
                    "task"
                }
            })
            .to_string();
        let subject_id = string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .map(str::to_string)
            .or_else(|| slot_id.clone())
            .or_else(|| conversation_id.clone())
            .unwrap_or_else(|| task_id.clone());
        let grant_id = string_arg(args, "grant_id")
            .or_else(|| string_arg(args, "grantId"))
            .or_else(|| string_arg(args, "capability_grant_id"))
            .or_else(|| string_arg(args, "capabilityGrantId"))
            .map(str::to_string);

        Ok(TaskResultPutRequest {
            task_id,
            project_id,
            slot_id,
            conversation_id,
            provider,
            result_status,
            summary,
            content,
            details,
            accepted_shard_id,
            attempt_id,
            grant_id,
            subject_kind,
            subject_id,
            producer,
            raw_evidence,
            evidence_refs,
            has_explicit_evidence,
            created_at,
            allow_system_bypass: system_or_operator_bypass_allowed(args),
            raw_args: args.clone(),
        })
    }

    async fn task_result_put(&self, req: TaskResultPutRequest) -> Result<Value> {
        let task_id = req.task_id;
        let project_id = req.project_id;
        let slot_id = req.slot_id;
        let conversation_id = req.conversation_id;
        let provider = req.provider;
        let result_status = req.result_status;
        let summary = req.summary;
        let details = req.details;
        let content = req.content;
        let raw_evidence = req.raw_evidence;
        let evidence_refs = req.evidence_refs;
        let has_explicit_evidence = req.has_explicit_evidence;
        let producer = req.producer;
        let created_at = req.created_at;
        let attempt_id = req.attempt_id;
        let subject_kind = req.subject_kind;
        let subject_id = req.subject_id;
        let requested_grant_id = req.grant_id;
        let accepted_shard_id = req.accepted_shard_id;
        let allow_system_bypass = req.allow_system_bypass;
        let raw_args = req.raw_args;
        let runtime_contract = self.task_runtime_contract(&task_id).await?;
        if is_completed_result_status(&result_status) && !runtime_contract.write_scope.is_empty() {
            let Some(attempt_id) = attempt_id.as_deref() else {
                return Err(control_error_details(
                    CAPABILITY_DENIED_CODE,
                    format!(
                        "task_result_put for write-scoped task {task_id} requires current attempt_id"
                    ),
                    json!({
                        "task_id": task_id,
                        "required": "attempt_id",
                        "write_scope": runtime_contract.write_scope.clone()
                    }),
                ));
            };
            let (_job_id, current_attempt_id) = self.current_job_attempt_for_task(&task_id).await?;
            if current_attempt_id.as_deref() != Some(attempt_id) {
                return Err(control_error_details(
                    CAPABILITY_DENIED_CODE,
                    format!(
                        "task_result_put attempt_id {attempt_id} is not current for task {task_id}"
                    ),
                    json!({
                        "task_id": task_id,
                        "attempt_id": attempt_id,
                        "current_attempt_id": current_attempt_id
                    }),
                ));
            }
        }
        let capability_grant_id = self
            .require_capability(CapabilityCheckRequest {
                grant_id: requested_grant_id.clone(),
                subject_kind: subject_kind.clone(),
                subject_id: subject_id.clone(),
                operation: "write".to_string(),
                scope_kind: "task".to_string(),
                scope_key: task_id.clone(),
                task_id: Some(task_id.clone()),
                allow_system_bypass,
                bypass_reason: Some("task_result_put system/operator authority".to_string()),
                details: json!({
                    "provider": provider,
                    "attempt_id": attempt_id.clone(),
                    "task_contract_id": runtime_contract.task_contract_id.clone(),
                    "runtime_contract_grants": runtime_contract.capability_grant_ids.clone()
                }),
            })
            .await?;
        if is_completed_result_status(&result_status) {
            self.verify_completion_scope(&task_id, &raw_args, &details, &runtime_contract)
                .await?;
        }
        validate_task_result_artifact_payload(
            &task_id,
            &project_id,
            &provider,
            &result_status,
            &summary,
            &content,
            &evidence_refs,
            has_explicit_evidence,
        )?;
        if let Some(row) = sqlx::query(
            r#"
            SELECT id, artifact_hash
            FROM task_result_artifacts
            WHERE task_id = $1
              AND slot_id IS NOT DISTINCT FROM $2
              AND conversation_id IS NOT DISTINCT FROM $3
              AND provider IS NOT DISTINCT FROM $4
              AND result_status = $5
              AND summary = $6
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(&task_id)
        .bind(slot_id.as_deref())
        .bind(conversation_id.as_deref())
        .bind(&provider)
        .bind(&result_status)
        .bind(&summary)
        .fetch_optional(&self.pool)
        .await?
        {
            let id: String = row.try_get("id")?;
            let artifact_hash: String = row.try_get("artifact_hash")?;
            let _ = self
                .record_control_plane_event(
                    Some(project_id.as_str()),
                    task_id.as_str(),
                    "artifact.accepted",
                    slot_id
                        .as_deref()
                        .or(conversation_id.as_deref())
                        .unwrap_or(provider.as_str()),
                    json!({
                        "schema": "missiond.artifact-accepted.v1",
                        "artifact_hash": artifact_hash.clone(),
                        "result_status": result_status.clone(),
                        "deduped": true
                    }),
                    Some("running"),
                    Some(artifact_hash.as_str()),
                )
                .await?;
            return Ok(json!({
                "schema": "missiond.task-result-artifact.v1",
                "ok": true,
                "id": id,
                "artifact_hash": artifact_hash,
                "deduped": true
            }));
        }
        let body = json!({
            "schema": "missiond.task-result-artifact.v1",
            "task_id": task_id,
            "project_id": project_id,
            "slot_id": slot_id,
            "conversation_id": conversation_id,
            "provider": provider,
            "producer": producer,
            "producer_subject_kind": subject_kind,
            "producer_subject_id": subject_id,
            "capability_grant_id": requested_grant_id,
            "attempt_id": attempt_id,
            "result_status": result_status,
            "result_kind": result_status,
            "summary": summary,
            "content": content,
            "details": details,
            "evidence_refs": evidence_refs,
            "raw_evidence": raw_evidence,
            "created_at": created_at
        });
        let metadata = json!({
            "schema": "missiond.task-result-artifact.v1",
            "task_id": task_id,
            "project_id": project_id,
            "slot_id": slot_id,
            "conversation_id": conversation_id,
            "provider": provider,
            "result_status": result_status,
            "producer_subject_kind": subject_kind,
            "producer_subject_id": subject_id,
            "capability_grant_id": requested_grant_id,
            "attempt_id": attempt_id,
            "accepted_shard_id": accepted_shard_id
        });
        let artifact = self
            .put_artifact_bytes(
                "task-result-artifact",
                Some(project_id.as_str()),
                Some(task_id.as_str()),
                "application/json",
                serde_json::to_vec(&body)?,
                metadata,
            )
            .await?;

        let id = Uuid::new_v4().to_string();
        let (job_id, _current_attempt_id) = self.current_job_attempt_for_task(&task_id).await?;
        sqlx::query(
            r#"
            INSERT INTO task_result_artifacts
              (id, artifact_hash, project_id, task_id, slot_id, conversation_id,
               provider, result_status, summary, job_id, attempt_id,
               producer_subject_kind, producer_subject_id, capability_grant_id)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
            ON CONFLICT(task_id, artifact_hash)
            DO UPDATE SET summary = EXCLUDED.summary,
                          job_id = COALESCE(EXCLUDED.job_id, task_result_artifacts.job_id),
                          attempt_id = COALESCE(EXCLUDED.attempt_id, task_result_artifacts.attempt_id),
                          producer_subject_kind = COALESCE(EXCLUDED.producer_subject_kind, task_result_artifacts.producer_subject_kind),
                          producer_subject_id = COALESCE(EXCLUDED.producer_subject_id, task_result_artifacts.producer_subject_id),
                          capability_grant_id = COALESCE(EXCLUDED.capability_grant_id, task_result_artifacts.capability_grant_id)
            "#,
        )
        .bind(&id)
        .bind(&artifact.hash)
        .bind(&project_id)
        .bind(&task_id)
        .bind(slot_id.as_deref())
        .bind(conversation_id.as_deref())
        .bind(&provider)
        .bind(&result_status)
        .bind(&summary)
        .bind(job_id.as_deref())
        .bind(attempt_id.as_deref())
        .bind(subject_kind.as_str())
        .bind(subject_id.as_str())
        .bind(if capability_grant_id.starts_with("bypass:") {
            None
        } else {
            Some(capability_grant_id.as_str())
        })
        .execute(&self.pool)
        .await?;

        let event = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": "task_result_artifact.created",
                "project_id": project_id,
                "task_id": task_id,
                "agent_id": slot_id.as_deref().or(conversation_id.as_deref()).unwrap_or(provider.as_str()),
                "idempotency_key": format!("task-result:{task_id}:{}", artifact.hash),
                "payload": {
                    "artifact_hash": artifact.hash,
                    "summary": summary,
                    "result_status": result_status
                }
            }))
            .await?;
        let _ = self
            .record_control_plane_event(
                Some(project_id.as_str()),
                task_id.as_str(),
                "artifact.accepted",
                slot_id
                    .as_deref()
                    .or(conversation_id.as_deref())
                    .unwrap_or(provider.as_str()),
                json!({
                    "schema": "missiond.artifact-accepted.v1",
                    "artifact_hash": artifact.hash.clone(),
                    "result_status": result_status.clone(),
                    "summary": summary.clone()
                }),
                Some("running"),
                Some(artifact.hash.as_str()),
            )
            .await?;

        Ok(json!({
            "schema": "missiond.task-result-artifact.v1",
            "ok": true,
            "id": id,
            "artifact_hash": artifact.hash,
            "size_bytes": artifact.size_bytes,
            "event": event
        }))
    }

    async fn task_result_get(&self, args: &Value) -> Result<Value> {
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let hash = string_arg(args, "hash").or_else(|| string_arg(args, "artifact_hash"));
        let limit = bounded_limit(args);
        let rows = sqlx::query(
            r#"
            SELECT id, artifact_hash, project_id, task_id, job_id, attempt_id,
                   slot_id, conversation_id, provider, result_status, summary,
                   producer_subject_kind, producer_subject_id, capability_grant_id,
                   created_at
            FROM task_result_artifacts
            WHERE ($1::text IS NULL OR task_id = $1)
              AND ($2::text IS NULL OR artifact_hash = $2)
            ORDER BY created_at DESC
            LIMIT $3
            "#,
        )
        .bind(task_id)
        .bind(hash)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.task-result-artifacts.v1",
            "results": rows.into_iter().map(task_result_row_json).collect::<Vec<_>>()
        }))
    }

    async fn task_evidence_summary(&self, args: &Value) -> Result<Value> {
        let task_ids = args
            .get("task_ids")
            .or_else(|| args.get("taskIds"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let limit = bounded_limit(args);
        let rows = if task_ids.is_empty() {
            sqlx::query(
                r#"
                SELECT DISTINCT ON (task_id)
                       task_id, artifact_hash, project_id, slot_id, conversation_id,
                       provider, result_status, summary, created_at
                FROM task_result_artifacts
                ORDER BY task_id, created_at DESC
                LIMIT $1
                "#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT DISTINCT ON (task_id)
                       task_id, artifact_hash, project_id, slot_id, conversation_id,
                       provider, result_status, summary, created_at
                FROM task_result_artifacts
                WHERE task_id = ANY($1)
                ORDER BY task_id, created_at DESC
                LIMIT $2
                "#,
            )
            .bind(&task_ids)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        let mut items = rows
            .into_iter()
            .map(task_evidence_summary_row_json)
            .collect::<Vec<_>>();
        if !task_ids.is_empty() {
            let found = items
                .iter()
                .filter_map(|item| item.get("taskId").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<std::collections::HashSet<_>>();
            for task_id in task_ids.iter().filter(|id| !found.contains(id.as_str())) {
                items.push(json!({
                    "taskId": task_id,
                    "complete": false,
                    "missingReasons": ["task-result-artifact-missing"],
                    "artifactHash": null,
                    "resultStatus": null,
                    "summary": "",
                    "verifierStatus": null,
                    "gateStatus": "blocked",
                    "updatedAt": null
                }));
            }
        }
        let total_artifacts =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM task_result_artifacts")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
        let tasks_with_evidence = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT task_id) FROM task_result_artifacts",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let completed = items
            .iter()
            .filter(|item| {
                item.get("complete")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        let missing = items.len().saturating_sub(completed);
        Ok(json!({
            "schema": "missiond.task-evidence-summary.v1",
            "degraded": false,
            "gate": {
                "requiredForDone": true,
                "status": if missing == 0 { "ok" } else { "blocked" },
                "missing": missing,
            },
            "artifacts": total_artifacts,
            "tasksWithEvidence": tasks_with_evidence,
            "completed": completed,
            "missing": missing,
            "items": items,
        }))
    }

    async fn workflow_start(&self, args: &Value) -> Result<Value> {
        let id = string_arg(args, "workflow_run_id")
            .or_else(|| string_arg(args, "workflowRunId"))
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let workflow_id =
            string_arg(args, "workflow_id").or_else(|| string_arg(args, "workflowId"));
        let workflow_path =
            string_arg(args, "workflow_path").or_else(|| string_arg(args, "workflowPath"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let parent_task_id =
            string_arg(args, "parent_task_id").or_else(|| string_arg(args, "parentTaskId"));
        let max_inflight = args
            .get("max_inflight")
            .or_else(|| args.get("maxInflight"))
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .clamp(1, 64) as i32;
        let cursor = args.get("cursor").cloned().unwrap_or_else(|| json!({}));
        let checkpoint = args.get("checkpoint").cloned().unwrap_or_else(|| json!({}));
        sqlx::query(
            r#"
            INSERT INTO workflow_runs
              (id, workflow_id, workflow_path, project_id, parent_task_id,
               status, cursor, checkpoint, max_inflight)
            VALUES ($1,$2,$3,$4,$5,'running',$6,$7,$8)
            ON CONFLICT(id)
            DO UPDATE SET updated_at = now()
            "#,
        )
        .bind(&id)
        .bind(workflow_id)
        .bind(workflow_path)
        .bind(project_id)
        .bind(parent_task_id)
        .bind(&cursor)
        .bind(&checkpoint)
        .bind(max_inflight)
        .execute(&self.pool)
        .await?;
        let event = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": "workflow_run.started",
                "project_id": project_id,
                "task_id": parent_task_id,
                "idempotency_key": format!("workflow-run-start:{id}"),
                "payload": {
                    "workflow_run_id": id,
                    "workflow_id": workflow_id,
                    "workflow_path": workflow_path,
                    "max_inflight": max_inflight
                }
            }))
            .await?;
        Ok(json!({
            "schema": "missiond.workflow-run.v1",
            "ok": true,
            "workflow_run_id": id,
            "event": event
        }))
    }

    async fn workflow_checkpoint(&self, args: &Value) -> Result<Value> {
        let id = string_arg(args, "workflow_run_id")
            .or_else(|| string_arg(args, "workflowRunId"))
            .or_else(|| string_arg(args, "id"))
            .ok_or_else(|| anyhow!("workflow_run_id is required"))?;
        let status = string_arg(args, "status").unwrap_or("running");
        let cursor = args.get("cursor").cloned().unwrap_or_else(|| json!({}));
        let checkpoint = args.get("checkpoint").cloned().unwrap_or_else(|| json!({}));
        let active_task_ids = args
            .get("active_task_ids")
            .or_else(|| args.get("activeTaskIds"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        let artifact_hashes = args
            .get("artifact_hashes")
            .or_else(|| args.get("artifactHashes"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        let row = sqlx::query(
            r#"
            UPDATE workflow_runs
            SET status = $2,
                cursor = $3,
                checkpoint = $4,
                active_task_ids = $5,
                artifact_hashes = $6,
                updated_at = now(),
                finished_at = CASE WHEN $2 IN ('done','failed','blocked') THEN now() ELSE finished_at END
            WHERE id = $1
            RETURNING id, workflow_id, workflow_path, project_id, parent_task_id, status,
                      cursor, checkpoint, max_inflight, active_task_ids, artifact_hashes,
                      started_at, updated_at, finished_at
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(&cursor)
        .bind(&checkpoint)
        .bind(&active_task_ids)
        .bind(&artifact_hashes)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(json!({
                "schema": "missiond.workflow-run.v1",
                "ok": false,
                "error": "workflow_run_not_found",
                "workflow_run_id": id
            }));
        };
        let event = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": "workflow_run.checkpointed",
                "idempotency_key": format!("workflow-run-checkpoint:{id}:{}", Utc::now().timestamp_millis()),
                "payload": {
                    "workflow_run_id": id,
                    "status": status,
                    "cursor": cursor,
                    "active_task_ids": active_task_ids,
                    "artifact_hashes": artifact_hashes
                }
            }))
            .await?;
        Ok(json!({
            "schema": "missiond.workflow-run.v1",
            "ok": true,
            "workflow_run": workflow_run_row_json(row),
            "event": event
        }))
    }

    async fn workflow_status(&self, args: &Value) -> Result<Value> {
        let id = string_arg(args, "workflow_run_id")
            .or_else(|| string_arg(args, "workflowRunId"))
            .or_else(|| string_arg(args, "id"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let status = string_arg(args, "status");
        let limit = bounded_limit(args);
        let rows = sqlx::query(
            r#"
            SELECT id, workflow_id, workflow_path, project_id, parent_task_id, status,
                   cursor, checkpoint, max_inflight, active_task_ids, artifact_hashes,
                   started_at, updated_at, finished_at
            FROM workflow_runs
            WHERE ($1::text IS NULL OR id = $1)
              AND ($2::text IS NULL OR project_id = $2)
              AND ($3::text IS NULL OR status = $3)
            ORDER BY updated_at DESC
            LIMIT $4
            "#,
        )
        .bind(id)
        .bind(project_id)
        .bind(status)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.workflow-runs.v1",
            "workflow_runs": rows.into_iter().map(workflow_run_row_json).collect::<Vec<_>>()
        }))
    }

    async fn runtime_artifact_index(&self, args: &Value) -> Result<Value> {
        let path_arg = string_arg(args, "path").ok_or_else(|| anyhow!("path is required"))?;
        let path = self.resolve_runtime_artifact_path(path_arg)?;
        let rel_path = self.runtime_artifact_catalog_path(&path);
        let bytes = fs::read(&path)?;
        let hash = string_arg(args, "hash")
            .map(str::to_string)
            .unwrap_or_else(|| sha256_hex(&bytes));
        let size_bytes = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        let kind = string_arg(args, "kind")
            .map(str::to_string)
            .unwrap_or_else(|| infer_runtime_artifact_kind(&rel_path));
        let source_surface =
            string_arg(args, "source_surface").or_else(|| string_arg(args, "sourceSurface"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let media_type = string_arg(args, "media_type")
            .or_else(|| string_arg(args, "mediaType"))
            .unwrap_or_else(|| infer_media_type(&rel_path));
        let metadata = args.get("metadata").cloned().unwrap_or_else(|| json!({}));
        let expires_at = runtime_artifact_expires_at(&kind, &rel_path);

        let row = sqlx::query(
            r#"
            INSERT INTO runtime_artifacts
              (hash, path, kind, source_surface, project_id, task_id,
               media_type, size_bytes, status, metadata, expires_at)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'active',$9,$10)
            ON CONFLICT(path, hash)
            DO UPDATE SET
              kind = EXCLUDED.kind,
              source_surface = EXCLUDED.source_surface,
              project_id = EXCLUDED.project_id,
              task_id = EXCLUDED.task_id,
              media_type = EXCLUDED.media_type,
              size_bytes = EXCLUDED.size_bytes,
              status = 'active',
              metadata = EXCLUDED.metadata,
              indexed_at = now(),
              expires_at = EXCLUDED.expires_at
            RETURNING id::text, hash, path, kind, source_surface, project_id,
                      task_id, media_type, size_bytes, status, metadata,
                      created_at, indexed_at, expires_at
            "#,
        )
        .bind(&hash)
        .bind(&rel_path)
        .bind(&kind)
        .bind(source_surface)
        .bind(project_id)
        .bind(task_id)
        .bind(media_type)
        .bind(size_bytes)
        .bind(&metadata)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;

        Ok(json!({
            "schema": "missiond.runtime-artifact.v1",
            "ok": true,
            "artifact": runtime_artifact_row_json(row)
        }))
    }

    async fn runtime_artifact_list(&self, args: &Value) -> Result<Value> {
        let project_id = string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .or_else(|| string_arg(args, "project"));
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let kind = string_arg(args, "kind");
        let include_expired = args
            .get("include_expired")
            .or_else(|| args.get("includeExpired"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let limit = bounded_limit(args).min(100);
        let rows = self
            .runtime_artifacts_for_scope(project_id, task_id, kind, include_expired, limit)
            .await?;
        Ok(json!({
            "schema": "missiond.runtime-artifacts.v1",
            "artifacts": rows
        }))
    }

    async fn runtime_artifact_prune(&self, args: &Value) -> Result<Value> {
        let dry_run = args
            .get("dry_run")
            .or_else(|| args.get("dryRun"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let expired = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM runtime_artifacts WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < now()",
        )
        .fetch_one(&self.pool)
        .await?;
        let marked = if dry_run {
            0
        } else {
            sqlx::query(
                "UPDATE runtime_artifacts SET status = 'expired' WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < now()",
            )
            .execute(&self.pool)
            .await?
            .rows_affected() as i64
        };
        Ok(json!({
            "schema": "missiond.runtime-artifact-retention.v1",
            "dryRun": dry_run,
            "expiredActiveCount": expired,
            "markedExpired": marked,
            "note": "Retention marks catalog rows expired; destructive file deletion remains an explicit maintenance workflow."
        }))
    }

    async fn evidence_view(&self, args: &Value) -> Result<Value> {
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let project_id = string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .or_else(|| string_arg(args, "project"));
        if task_id.is_none() && project_id.is_none() {
            return Err(anyhow!("task_id or project_id is required"));
        }
        let limit = bounded_limit(args).min(100);

        let board_task = match task_id {
            Some(id) => {
                let row = sqlx::query(
                    r#"
                    SELECT id, title, status, category, project, project_id, assignee,
                           updated_at, dedupe_key
                    FROM board_tasks
                    WHERE id = $1
                    "#,
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
                row.map(board_task_evidence_row_json)
            }
            None => None,
        };

        let task_results = sqlx::query(
            r#"
            SELECT id, artifact_hash, project_id, task_id, job_id, attempt_id,
                   slot_id, conversation_id, provider, result_status, summary,
                   producer_subject_kind, producer_subject_id, capability_grant_id,
                   created_at
            FROM task_result_artifacts
            WHERE ($1::text IS NULL OR task_id = $1)
              AND ($2::text IS NULL OR project_id = $2)
            ORDER BY created_at DESC
            LIMIT $3
            "#,
        )
        .bind(task_id)
        .bind(project_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let conversations = sqlx::query(
            r#"
            SELECT id, project, project_id, slot_id, source, model, status,
                   conversation_type, task_id, message_count, started_at, ended_at,
                   updated_at, llm_summary
            FROM conversations
            WHERE ($1::text IS NULL OR task_id = $1)
              AND ($2::text IS NULL OR project_id = $2 OR project = $2)
            ORDER BY COALESCE(updated_at, ended_at, started_at) DESC
            LIMIT $3
            "#,
        )
        .bind(task_id)
        .bind(project_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let shared_events = sqlx::query(
            r#"
            SELECT id, stream_id, seq, project_id, task_id, agent_id, event_kind,
                   payload, idempotency_key, correlation_id, parent_event_ids, created_at
            FROM shared_events
            WHERE ($1::text IS NULL OR task_id = $1)
              AND ($2::text IS NULL OR project_id = $2)
            ORDER BY seq DESC
            LIMIT $3
            "#,
        )
        .bind(task_id)
        .bind(project_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let timeline_events = sqlx::query(
            r#"
            SELECT seq, domain, kind, payload_inline, payload_ref, producer_id,
                   trace_id::text AS trace_id, ts, ephemeral
            FROM event_log
            WHERE ($1::text IS NULL OR payload_inline::text ILIKE ('%' || $1 || '%'))
              AND ($2::text IS NULL OR payload_inline::text ILIKE ('%' || $2 || '%') OR domain = $2)
            ORDER BY seq DESC
            LIMIT $3
            "#,
        )
        .bind(task_id)
        .bind(project_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let kb_entries = sqlx::query(
            r#"
            SELECT k.id, k.category, k.key, k.summary, k.source, k.project_id,
                   k.linked_task_id, k.scope_task_id, k.updated_at,
                   rs.state AS review_state
            FROM knowledge k
            LEFT JOIN knowledge_review_state rs
              ON rs.knowledge_id = k.id AND rs.is_current = true
            WHERE ($1::text IS NULL OR k.linked_task_id = $1 OR k.scope_task_id = $1)
              AND ($2::text IS NULL OR k.project_id = $2 OR k.project_id IS NULL)
            ORDER BY k.updated_at DESC
            LIMIT $3
            "#,
        )
        .bind(task_id)
        .bind(project_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let runtime_artifacts = self
            .runtime_artifacts_for_scope(project_id, task_id, None, false, limit)
            .await?;

        Ok(json!({
            "schema": "missiond.evidence-governance-view.v1",
            "taskId": task_id,
            "projectId": project_id,
            "model": {
                "taskResultArtifacts": "canonical worker outputs and workflow batch results",
                "conversations": "provider/user turn read model; useful for audit and retrieval, not worker completion authority",
                "timelineEvents": "event causality and external/system event projection",
                "kbMemory": "curated reviewed long-term knowledge; active retrieval is controlled by knowledge_review_state",
                "board": "coordination projection and operator-facing task state",
                "runtimeArtifacts": "indexed cold .missiond/v3/runtime/** diagnostics; queryable by path/hash/kind, excluded from broad SSOT search"
            },
            "authorityOrder": [
                "task_result_artifacts",
                "provider_durable_conversation",
                "event_log",
                "knowledge_review_state",
                "board_projection",
                "runtime_artifacts_catalog"
            ],
            "lanes": {
                "board": board_task,
                "taskResults": task_results.into_iter().map(task_result_row_json).collect::<Vec<_>>(),
                "conversations": conversations.into_iter().map(conversation_evidence_row_json).collect::<Vec<_>>(),
                "sharedEvents": shared_events.into_iter().map(event_row_json).collect::<Vec<_>>(),
                "timelineEvents": timeline_events.into_iter().map(timeline_event_row_json).collect::<Vec<_>>(),
                "kbMemory": kb_entries.into_iter().map(kb_evidence_row_json).collect::<Vec<_>>(),
                "runtimeArtifacts": runtime_artifacts
            }
        }))
    }

    pub(crate) fn worker_settle_request_from_args(
        &self,
        args: &Value,
    ) -> Result<WorkerSettleRequest> {
        let task_id = string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .ok_or_else(|| anyhow!("task_id is required"))?
            .to_string();
        let project_id = string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .map(str::to_string);
        let slot_id = string_arg(args, "slot_id")
            .or_else(|| string_arg(args, "slotId"))
            .map(str::to_string);
        let conversation_id = string_arg(args, "conversation_id")
            .or_else(|| string_arg(args, "conversationId"))
            .map(str::to_string);
        let artifact_hash = string_arg(args, "artifact_hash")
            .or_else(|| string_arg(args, "artifactHash"))
            .map(str::to_string);
        let status = normalize_worker_settle_status(string_arg(args, "status"))?.to_string();
        let grant_id = string_arg(args, "grant_id")
            .or_else(|| string_arg(args, "grantId"))
            .or_else(|| string_arg(args, "capability_grant_id"))
            .or_else(|| string_arg(args, "capabilityGrantId"))
            .map(str::to_string);
        let subject_kind = string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .map(str::to_string)
            .unwrap_or_else(|| {
                if slot_id.is_some() {
                    "worker".to_string()
                } else if conversation_id.is_some() {
                    "conversation".to_string()
                } else {
                    "task".to_string()
                }
            });
        let subject_id = string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .map(str::to_string)
            .or_else(|| slot_id.clone())
            .or_else(|| conversation_id.clone())
            .unwrap_or_else(|| task_id.clone());
        let attempt_id = string_arg(args, "attempt_id")
            .or_else(|| string_arg(args, "attemptId"))
            .map(str::to_string);

        Ok(WorkerSettleRequest {
            task_id,
            project_id,
            slot_id,
            conversation_id,
            artifact_hash,
            status,
            summary: string_arg(args, "summary").map(str::to_string),
            grant_id,
            subject_kind,
            subject_id,
            attempt_id,
            allow_system_bypass: system_or_operator_bypass_allowed(args),
        })
    }

    async fn worker_settle(&self, req: WorkerSettleRequest) -> Result<Value> {
        let task_id = req.task_id.as_str();
        let project_id = req.project_id.as_deref();
        let slot_id = req.slot_id.as_deref();
        let conversation_id = req.conversation_id.as_deref();
        let artifact_hash_owned = req.artifact_hash.clone();
        let target_status = normalize_worker_settle_status(Some(req.status.as_str()))?;
        let note_summary = req
            .summary
            .clone()
            .unwrap_or_else(|| "Worker durable final settled.".to_string());

        if target_status.eq_ignore_ascii_case("done") && artifact_hash_owned.is_none() {
            return Err(control_error_details(
                EVIDENCE_REQUIRED_CODE,
                format!("worker_settle(done) for task {task_id} requires artifact_hash"),
                json!({
                    "task_id": task_id,
                    "required": "artifact_hash"
                }),
            ));
        }
        let runtime_contract = self.task_runtime_contract(task_id).await?;
        let settle_capability_grant_id = self
            .require_capability(CapabilityCheckRequest {
                grant_id: req.grant_id.clone(),
                subject_kind: req.subject_kind.clone(),
                subject_id: req.subject_id.clone(),
                operation: "settle".to_string(),
                scope_kind: "task".to_string(),
                scope_key: task_id.to_string(),
                task_id: Some(task_id.to_string()),
                allow_system_bypass: req.allow_system_bypass,
                bypass_reason: Some("worker_settle system/operator authority".to_string()),
                details: json!({
                    "artifact_hash": artifact_hash_owned.clone(),
                    "task_contract_id": runtime_contract.task_contract_id.clone(),
                    "runtime_contract_grants": runtime_contract.capability_grant_ids.clone()
                }),
            })
            .await?;
        if target_status.eq_ignore_ascii_case("done") && !runtime_contract.write_scope.is_empty() {
            let artifact_hash = artifact_hash_owned.as_deref().ok_or_else(|| {
                control_error_details(
                    EVIDENCE_REQUIRED_CODE,
                    format!(
                        "worker_settle(done) for write-scoped task {task_id} requires artifact_hash"
                    ),
                    json!({"task_id": task_id, "required": "artifact_hash"}),
                )
            })?;
            let artifact_attempt_id = sqlx::query_scalar::<_, Option<String>>(
                "SELECT attempt_id FROM task_result_artifacts WHERE task_id = $1 AND artifact_hash = $2",
            )
            .bind(task_id)
            .bind(artifact_hash)
            .fetch_optional(&self.pool)
            .await?
            .flatten();
            let (_job_id, current_attempt_id) = self.current_job_attempt_for_task(task_id).await?;
            validate_write_scoped_settle_attempt(
                task_id,
                artifact_hash,
                artifact_attempt_id.as_deref(),
                current_attempt_id.as_deref(),
                req.attempt_id.as_deref(),
            )?;
        }

        if target_status.eq_ignore_ascii_case("done") {
            self.consume_capability_grant(
                settle_capability_grant_id.as_str(),
                "settle",
                task_id,
                req.subject_kind.as_str(),
                req.subject_id.as_str(),
            )
            .await?;
        }

        let conversation_rows = self
            .mark_worker_conversation_completed(task_id, conversation_id, slot_id)
            .await?;
        let released_claims = self.release_claims_for_task(task_id, slot_id).await?;
        let board = self
            .settle_board_task(
                task_id,
                target_status,
                artifact_hash_owned.as_deref(),
                &note_summary,
            )
            .await?;

        if let Some(slot) = slot_id {
            let _ = self
                .bus
                .publish_slot(SlotEvent::BecameIdle {
                    slot_id: slot.to_string(),
                })
                .await;
        }

        let event = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": "worker_completion.settled",
                "project_id": project_id,
                "task_id": task_id,
                "agent_id": slot_id.or(conversation_id).unwrap_or("worker"),
                "idempotency_key": format!("worker-settle:{task_id}:{}", artifact_hash_owned.as_deref().unwrap_or("no-artifact")),
                "payload": {
                    "task_id": task_id,
                    "slot_id": slot_id,
                    "conversation_id": conversation_id,
                    "artifact_hash": artifact_hash_owned,
                    "board": board,
                    "conversation_rows_completed": conversation_rows,
                    "released_claims": released_claims
                }
            }))
            .await?;

        Ok(json!({
            "schema": "missiond.worker-completion-settle.v1",
            "ok": true,
            "task_id": task_id,
            "artifact_hash": artifact_hash_owned,
            "conversation_rows_completed": conversation_rows,
            "released_claims": released_claims,
            "board": board,
            "event": event
        }))
    }

    async fn claim(&self, req: ClaimRequest) -> Result<Value> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2::text, 0))",
        )
        .bind(&req.scope_kind)
        .bind(&req.scope_key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE work_leases
            SET status = 'expired'
            WHERE status = 'active'
              AND scope_kind = $1
              AND scope_key = $2
              AND lease_expires_at < now()
            "#,
        )
        .bind(&req.scope_kind)
        .bind(&req.scope_key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE shared_claims
            SET status = 'expired'
            WHERE status = 'active'
              AND scope_kind = $1
              AND scope_key = $2
              AND lease_expires_at < now()
            "#,
        )
        .bind(&req.scope_kind)
        .bind(&req.scope_key)
        .execute(&mut *tx)
        .await?;
        let active = sqlx::query(
            r#"
            SELECT id, project_id, task_id, holder_id AS owner_id, scope_kind, scope_key, status,
                   acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            FROM work_leases
            WHERE status = 'active'
              AND scope_kind = $1
              AND scope_key = $2
              AND lease_expires_at >= now()
            ORDER BY acquired_at ASC
            LIMIT 1
            FOR UPDATE
            "#,
        )
        .bind(&req.scope_kind)
        .bind(&req.scope_key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = active {
            let conflict = claim_row_json(row);
            tx.commit().await?;
            let holder = conflict
                .get("owner_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let lease_expires_at = conflict
                .get("lease_expires_at")
                .and_then(Value::as_str)
                .map(str::to_string);
            return Err(control_error_details(
                CLAIM_CONFLICT_CODE,
                format!(
                    "active work lease conflict for {}:{}",
                    req.scope_kind, req.scope_key
                ),
                json!({
                    "scope_kind": req.scope_kind,
                    "scope_key": req.scope_key,
                    "holder": holder,
                    "lease_expires_at": lease_expires_at,
                    "authority": "work_leases",
                    "conflict": conflict
                }),
            ));
        }

        let id = Uuid::new_v4().to_string();
        let lease_secs = req.lease_secs.clamp(30, MAX_LEASE_SECS);
        let lease_expires_at = Utc::now() + Duration::seconds(lease_secs);
        let row = sqlx::query(
            r#"
            INSERT INTO work_leases
              (id, project_id, task_id, holder_id, holder_kind, scope_kind, scope_key, status,
               lease_expires_at, heartbeat_at, metadata)
            VALUES ($1,$2,$3,$4,$5,$6,$7,'active',$8,now(),$9)
            RETURNING id, project_id, task_id, holder_id AS owner_id, scope_kind, scope_key, status,
                      acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            "#,
        )
        .bind(&id)
        .bind(req.project_id.as_deref())
        .bind(req.task_id.as_deref())
        .bind(req.owner_id.as_str())
        .bind(req.subject_kind.as_str())
        .bind(req.scope_kind.as_str())
        .bind(req.scope_key.as_str())
        .bind(lease_expires_at)
        .bind(req.metadata.clone())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE shared_claims
            SET status = 'expired'
            WHERE status = 'active'
              AND scope_kind = $1
              AND scope_key = $2
            "#,
        )
        .bind(req.scope_kind.as_str())
        .bind(req.scope_key.as_str())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO shared_claims
              (id, project_id, task_id, owner_id, scope_kind, scope_key, status,
               lease_expires_at, heartbeat_at, metadata)
            VALUES ($1,$2,$3,$4,$5,$6,'active',$7,now(),$8)
            ON CONFLICT (id)
            DO UPDATE SET status = EXCLUDED.status,
                          lease_expires_at = EXCLUDED.lease_expires_at,
                          heartbeat_at = EXCLUDED.heartbeat_at,
                          metadata = EXCLUDED.metadata
            "#,
        )
        .bind(&id)
        .bind(req.project_id.as_deref())
        .bind(req.task_id.as_deref())
        .bind(req.owner_id.as_str())
        .bind(req.scope_kind.as_str())
        .bind(req.scope_key.as_str())
        .bind(lease_expires_at)
        .bind(req.metadata)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(json!({
            "schema": "missiond.shared-claim.v1",
            "ok": true,
            "claim": claim_row_json(row)
        }))
    }

    async fn release(&self, req: ReleaseLeaseRequest) -> Result<Value> {
        let id = req.claim_id.trim().to_string();
        if id.is_empty() {
            return Err(anyhow!("claim_id is required"));
        }
        let owner_id = req.owner_id.clone();
        let lease = sqlx::query(
            "SELECT task_id, holder_id, scope_kind, scope_key, status, lease_expires_at, metadata FROM work_leases WHERE id = $1",
        )
        .bind(&id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(lease) = lease else {
            return Err(control_error_details(
                CLAIM_CONFLICT_CODE,
                format!("work lease {id} does not exist; release is fail-closed"),
                json!({
                    "claim_id": id,
                    "owner_id": owner_id,
                    "reason": "lease_not_found",
                    "authority": "work_leases"
                }),
            ));
        };
        let task_id = lease.try_get::<Option<String>, _>("task_id")?;
        let holder_id: String = lease.try_get("holder_id")?;
        let scope_kind: String = lease.try_get("scope_kind")?;
        let scope_key: String = lease.try_get("scope_key")?;
        let lease_status: String = lease.try_get("status")?;
        let lease_expires_at = lease
            .try_get::<DateTime<Utc>, _>("lease_expires_at")
            .ok()
            .map(|ts| ts.to_rfc3339());
        let lease_metadata = lease
            .try_get::<Value, _>("metadata")
            .unwrap_or_else(|_| json!({}));
        let subject_id = if req.subject_id.trim().is_empty() {
            owner_id
                .as_deref()
                .unwrap_or(holder_id.as_str())
                .to_string()
        } else {
            req.subject_id.clone()
        };
        let capability_details = if req
            .details
            .as_object()
            .is_some_and(|fields| !fields.is_empty())
        {
            json!({
                "lease_metadata": lease_metadata,
                "request_details": req.details
            })
        } else {
            lease_metadata
        };
        self.require_capability(CapabilityCheckRequest {
            grant_id: req.grant_id,
            subject_kind: req.subject_kind,
            subject_id,
            operation: "claim".to_string(),
            scope_kind: scope_kind.clone(),
            scope_key: scope_key.clone(),
            task_id: task_id.clone(),
            allow_system_bypass: req.allow_system_bypass,
            bypass_reason: req.bypass_reason.or_else(|| {
                Some("mission_shared_memory release system/operator authority".to_string())
            }),
            details: capability_details,
        })
        .await?;
        let row = sqlx::query(
            r#"
            UPDATE work_leases
            SET status = 'released', released_at = now()
            WHERE id = $1
              AND status = 'active'
              AND ($2::text IS NULL OR holder_id = $2)
            RETURNING id, project_id, task_id, holder_id AS owner_id, scope_kind, scope_key, status,
                      acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            "#,
        )
        .bind(&id)
        .bind(owner_id.as_deref())
        .fetch_optional(&self.pool)
        .await?;
        if row.is_some() {
            let _ = sqlx::query(
                r#"
                UPDATE shared_claims
                SET status = 'released', released_at = now()
                WHERE id = $1
                  AND ($2::text IS NULL OR owner_id = $2)
                "#,
            )
            .bind(&id)
            .bind(owner_id.as_deref())
            .execute(&self.pool)
            .await;
        }
        let Some(row) = row else {
            return Err(control_error_details(
                CLAIM_CONFLICT_CODE,
                format!("work lease {id} was not released; holder or status does not match"),
                json!({
                    "claim_id": id,
                    "owner_id": owner_id,
                    "holder": holder_id,
                    "scope_kind": scope_kind,
                    "scope_key": scope_key,
                    "lease_status": lease_status,
                    "lease_expires_at": lease_expires_at,
                    "authority": "work_leases"
                }),
            ));
        };
        Ok(json!({
            "schema": "missiond.shared-claim-release.v1",
            "ok": true,
            "claim": claim_row_json(row)
        }))
    }

    async fn heartbeat(&self, req: HeartbeatLeaseRequest) -> Result<Value> {
        let id = req.claim_id.trim().to_string();
        if id.is_empty() {
            return Err(anyhow!("claim_id is required"));
        }
        let owner_id = req.owner_id.clone();
        let lease_secs = req.lease_secs.clamp(30, MAX_LEASE_SECS);
        let lease_expires_at = Utc::now() + Duration::seconds(lease_secs);
        let lease = sqlx::query(
            "SELECT task_id, holder_id, scope_kind, scope_key, status, lease_expires_at, metadata FROM work_leases WHERE id = $1",
        )
        .bind(&id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(lease) = lease else {
            return Err(control_error_details(
                CLAIM_CONFLICT_CODE,
                format!("work lease {id} does not exist; heartbeat is fail-closed"),
                json!({
                    "claim_id": id,
                    "owner_id": owner_id,
                    "reason": "lease_not_found",
                    "authority": "work_leases"
                }),
            ));
        };
        let task_id = lease.try_get::<Option<String>, _>("task_id")?;
        let holder_id: String = lease.try_get("holder_id")?;
        let scope_kind: String = lease.try_get("scope_kind")?;
        let scope_key: String = lease.try_get("scope_key")?;
        let lease_status: String = lease.try_get("status")?;
        let current_lease_expires_at = lease
            .try_get::<DateTime<Utc>, _>("lease_expires_at")
            .ok()
            .map(|ts| ts.to_rfc3339());
        let lease_metadata = lease
            .try_get::<Value, _>("metadata")
            .unwrap_or_else(|_| json!({}));
        let subject_id = if req.subject_id.trim().is_empty() {
            owner_id
                .as_deref()
                .unwrap_or(holder_id.as_str())
                .to_string()
        } else {
            req.subject_id.clone()
        };
        let capability_details = if req
            .details
            .as_object()
            .is_some_and(|fields| !fields.is_empty())
        {
            json!({
                "lease_metadata": lease_metadata,
                "request_details": req.details
            })
        } else {
            lease_metadata
        };
        self.require_capability(CapabilityCheckRequest {
            grant_id: req.grant_id,
            subject_kind: req.subject_kind,
            subject_id,
            operation: "claim".to_string(),
            scope_kind: scope_kind.clone(),
            scope_key: scope_key.clone(),
            task_id: task_id.clone(),
            allow_system_bypass: req.allow_system_bypass,
            bypass_reason: req.bypass_reason.or_else(|| {
                Some("mission_shared_memory heartbeat system/operator authority".to_string())
            }),
            details: capability_details,
        })
        .await?;
        let row = sqlx::query(
            r#"
            UPDATE work_leases
            SET lease_expires_at = $3, heartbeat_at = now()
            WHERE id = $1
              AND status = 'active'
              AND ($2::text IS NULL OR holder_id = $2)
            RETURNING id, project_id, task_id, holder_id AS owner_id, scope_kind, scope_key, status,
                      acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            "#,
        )
        .bind(&id)
        .bind(owner_id.as_deref())
        .bind(lease_expires_at)
        .fetch_optional(&self.pool)
        .await?;
        if row.is_some() {
            let _ = sqlx::query(
                r#"
                UPDATE shared_claims
                SET lease_expires_at = $3, heartbeat_at = now()
                WHERE id = $1
                  AND status = 'active'
                  AND ($2::text IS NULL OR owner_id = $2)
                "#,
            )
            .bind(&id)
            .bind(owner_id.as_deref())
            .bind(lease_expires_at)
            .execute(&self.pool)
            .await;
        }
        let Some(row) = row else {
            return Err(control_error_details(
                CLAIM_CONFLICT_CODE,
                format!("work lease {id} was not heartbeated; holder or status does not match"),
                json!({
                    "claim_id": id,
                    "owner_id": owner_id,
                    "holder": holder_id,
                    "scope_kind": scope_kind,
                    "scope_key": scope_key,
                    "lease_status": lease_status,
                    "lease_expires_at": current_lease_expires_at,
                    "authority": "work_leases"
                }),
            ));
        };
        Ok(json!({
            "schema": "missiond.shared-claim-heartbeat.v1",
            "ok": true,
            "claim": claim_row_json(row)
        }))
    }

    async fn cursor(&self, args: &Value) -> Result<Value> {
        let agent_id = string_arg(args, "agent_id")
            .or_else(|| string_arg(args, "agentId"))
            .ok_or_else(|| anyhow!("agent_id is required"))?;
        let stream_id = string_arg(args, "stream_id")
            .or_else(|| string_arg(args, "streamId"))
            .unwrap_or("default");
        if let Some(last_seq) = args
            .get("last_seq")
            .or_else(|| args.get("lastSeq"))
            .and_then(Value::as_i64)
        {
            sqlx::query(
                r#"
                INSERT INTO agent_cursors(agent_id, stream_id, last_seq, updated_at)
                VALUES ($1,$2,$3,now())
                ON CONFLICT(agent_id, stream_id)
                DO UPDATE SET last_seq = EXCLUDED.last_seq, updated_at = now()
                "#,
            )
            .bind(agent_id)
            .bind(stream_id)
            .bind(last_seq)
            .execute(&self.pool)
            .await?;
        }
        let row = sqlx::query(
            "SELECT agent_id, stream_id, last_seq, updated_at FROM agent_cursors WHERE agent_id = $1 AND stream_id = $2",
        )
        .bind(agent_id)
        .bind(stream_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.agent-cursor.v1",
            "cursor": row.map(cursor_row_json)
        }))
    }

    async fn expire_stale_claims(&self) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE work_leases SET status = 'expired' WHERE status = 'active' AND lease_expires_at < now()",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE shared_claims SET status = 'expired' WHERE status = 'active' AND lease_expires_at < now()",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn mark_worker_conversation_completed(
        &self,
        task_id: &str,
        conversation_id: Option<&str>,
        slot_id: Option<&str>,
    ) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        let result = if let Some(conversation_id) = conversation_id {
            sqlx::query(
                r#"
                UPDATE conversations
                SET task_id = COALESCE(task_id, $1),
                    slot_id = COALESCE(slot_id, $2),
                    status = 'completed',
                    ended_at = COALESCE(ended_at, $3),
                    conversation_type = 'worker',
                    updated_at = $3
                WHERE id = $4
                "#,
            )
            .bind(task_id)
            .bind(slot_id)
            .bind(&now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                UPDATE conversations
                SET status = 'completed',
                    ended_at = COALESCE(ended_at, $2),
                    conversation_type = 'worker',
                    updated_at = $2
                WHERE task_id = $1
                  AND status <> 'completed'
                "#,
            )
            .bind(task_id)
            .bind(&now)
            .execute(&self.pool)
            .await?
        };
        Ok(result.rows_affected())
    }

    async fn release_claims_for_task(&self, task_id: &str, owner_id: Option<&str>) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE work_leases
            SET status = 'released', released_at = now()
            WHERE status = 'active'
              AND task_id = $1
              AND ($2::text IS NULL OR holder_id = $2)
            "#,
        )
        .bind(task_id)
        .bind(owner_id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            UPDATE shared_claims
            SET status = 'released', released_at = now()
            WHERE status = 'active'
              AND task_id = $1
              AND ($2::text IS NULL OR owner_id = $2)
            "#,
        )
        .bind(task_id)
        .bind(owner_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn settle_board_task(
        &self,
        task_id: &str,
        target_status: &str,
        artifact_hash: Option<&str>,
        summary: &str,
    ) -> Result<Value> {
        if target_status.eq_ignore_ascii_case("done") {
            let artifact_hash = artifact_hash.ok_or_else(|| {
                control_error_details(
                    EVIDENCE_REQUIRED_CODE,
                    format!("BoardTask {task_id} done requires a canonical completed task-result-artifact hash"),
                    json!({
                        "task_id": task_id,
                        "required": "artifact_hash"
                    }),
                )
            })?;
            let artifact_matches = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                  SELECT 1
                  FROM task_result_artifacts
                  WHERE task_id = $1
                    AND artifact_hash = $2
                    AND lower(result_status) IN ('completed', 'complete', 'verified', 'pass', 'passed')
                )
                "#,
            )
            .bind(task_id)
            .bind(artifact_hash)
            .fetch_one(&self.pool)
            .await?;
            if !artifact_matches {
                return Err(control_error_details(
                    EVIDENCE_REQUIRED_CODE,
                    format!(
                        "artifact_hash {artifact_hash} is not a completed task-result-artifact for task {task_id}"
                    ),
                    json!({
                        "task_id": task_id,
                        "artifact_hash": artifact_hash
                    }),
                ));
            }
        }
        let old = self.store.get_board_task(task_id).await?;
        let old_status = old.as_ref().map(|task| task.status.as_str().to_string());
        let project_id_owned = old
            .as_ref()
            .and_then(|task| task.project.as_deref())
            .map(str::to_string);
        let _ = self
            .record_control_plane_event(
                project_id_owned.as_deref(),
                task_id,
                "settle.requested",
                "missiond.worker-completion-settle",
                json!({
                    "schema": "missiond.settle-requested.v1",
                    "target_status": target_status,
                    "artifact_hash": artifact_hash,
                    "summary": summary
                }),
                None,
                artifact_hash,
            )
            .await?;
        let update = UpdateBoardTaskInput {
            status: Some(target_status.to_string()),
            artifact_hash: artifact_hash.map(str::to_string),
            ..Default::default()
        };
        let updated = self.store.update_board_task(task_id, &update).await?;
        let projected_status = updated
            .as_ref()
            .map(|task| task.status.as_str().to_string())
            .unwrap_or_else(|| target_status.to_string());
        let terminal_event = match target_status.to_ascii_lowercase().as_str() {
            "done" | "completed" => Some(("job.completed", "completed")),
            "failed" => Some(("job.failed", "failed")),
            "blocked" => Some(("job.blocked", "blocked")),
            "skipped" => Some(("job.blocked", "skipped")),
            _ => None,
        };
        let job_id = if let Some((event_kind, job_state)) = terminal_event {
            Some(
                self.record_control_plane_event(
                    updated
                        .as_ref()
                        .and_then(|task| task.project.as_deref())
                        .or(project_id_owned.as_deref()),
                    task_id,
                    event_kind,
                    "missiond.worker-completion-settle",
                    json!({
                        "schema": "missiond.job-terminal.v1",
                        "target_status": target_status,
                        "artifact_hash": artifact_hash,
                        "summary": summary
                    }),
                    Some(job_state),
                    artifact_hash,
                )
                .await?,
            )
        } else {
            None
        };
        self.project_board_task_view(
            task_id,
            job_id.as_deref(),
            projected_status.as_str(),
            artifact_hash,
            json!({
                "schema": "missiond.board-task-view.v1",
                "source": "job_state_machine",
                "projected_status": projected_status,
                "artifact_hash": artifact_hash,
                "summary": summary
            }),
        )
        .await?;
        let note_content = match artifact_hash {
            Some(hash) => format!("{summary}\n\ntask_result_artifact: {hash}"),
            None => summary.to_string(),
        };
        let note = self
            .store
            .add_board_task_note(&AddBoardTaskNoteInput {
                task_id: task_id.to_string(),
                content: note_content.clone(),
                note_type: Some("summary".to_string()),
                author: Some("missiond.worker-completion-settle".to_string()),
            })
            .await
            .ok();

        if let Some(task) = &updated {
            if old_status.as_deref() != Some(task.status.as_str()) {
                let ev = BoardEvent::StatusChanged {
                    task_id: task.id.to_string(),
                    old_status: old_status.unwrap_or_else(|| "unknown".to_string()),
                    new_status: task.status.as_str().to_string(),
                };
                crate::engine::master_control::notify_board_event_direct(&ev);
                let _ = self.bus.publish_board(ev).await;
                if task.status == missiond_core::types::BoardTaskStatus::Done {
                    let _ = self
                        .bus
                        .publish_task(TaskEvent::Completed {
                            task_id: task.id.to_string(),
                        })
                        .await;
                }
            } else {
                let ev = BoardEvent::Updated {
                    task_id: task.id.to_string(),
                    status: task.status.as_str().to_string(),
                    category: task.category.clone(),
                };
                crate::engine::master_control::notify_board_event_direct(&ev);
                let _ = self.bus.publish_board(ev).await;
            }
        }

        if let Some(note) = &note {
            let ev = BoardEvent::NoteAdded {
                task_id: task_id.to_string(),
                note_id: note.id.clone(),
                content_preview: note_content.chars().take(160).collect(),
            };
            crate::engine::master_control::notify_board_event_direct(&ev);
            let _ = self.bus.publish_board(ev).await;
        }

        let updated_payload = updated.as_ref().map(|task| {
            json!({
                "id": task.id.to_string(),
                "status": task.status.as_str(),
                "title": task.title
            })
        });
        let note_id = note.as_ref().map(|note| note.id.clone());
        Ok(json!({
            "updated": updated_payload,
            "note_id": note_id
        }))
    }

    async fn recent_artifacts_for_task(
        &self,
        project_id: &str,
        task_id: Option<&str>,
        accepted_shard_id: Option<&str>,
    ) -> Result<Vec<Value>> {
        let rows = sqlx::query(
            r#"
            SELECT hash, kind, project_id, task_id, media_type, bytes, size_bytes, metadata, created_at
            FROM shared_artifacts
            WHERE ($1::text IS NULL OR project_id = $1)
              AND ($2::text IS NULL OR task_id = $2)
              AND ($3::text IS NULL OR metadata->>'accepted_shard_id' = $3)
            ORDER BY created_at DESC
            LIMIT 20
            "#,
        )
        .bind(Some(project_id))
        .bind(task_id)
        .bind(accepted_shard_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| artifact_row_json(row, false))
            .collect())
    }

    async fn runtime_artifacts_for_scope(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        kind: Option<&str>,
        include_expired: bool,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let rows = sqlx::query(
            r#"
            SELECT id::text, hash, path, kind, source_surface, project_id, task_id,
                   media_type, size_bytes, status, metadata, created_at, indexed_at, expires_at
            FROM runtime_artifacts
            WHERE ($1::text IS NULL OR project_id = $1)
              AND ($2::text IS NULL OR task_id = $2)
              AND ($3::text IS NULL OR kind = $3)
              AND ($4::bool OR status = 'active')
            ORDER BY indexed_at DESC
            LIMIT $5
            "#,
        )
        .bind(project_id)
        .bind(task_id)
        .bind(kind)
        .bind(include_expired)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(runtime_artifact_row_json).collect())
    }

    fn resolve_runtime_artifact_path(&self, value: &str) -> Result<PathBuf> {
        let path = PathBuf::from(value);
        let path = if path.is_absolute() {
            path
        } else {
            self.missiond_root.join(path)
        };
        let runtime_root = self
            .missiond_root
            .join(".missiond")
            .join("v3")
            .join("runtime");
        let normalized = normalize_path_for_prefix_check(&path);
        let normalized_runtime_root = normalize_path_for_prefix_check(&runtime_root);
        if !normalized.starts_with(&normalized_runtime_root) {
            return Err(anyhow!(
                "runtime artifact path must be under {}",
                runtime_root.display()
            ));
        }
        Ok(path)
    }

    fn runtime_artifact_catalog_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.missiond_root)
            .map(|rel| rel.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string())
    }

    fn read_compiled_json(&self, name: &str) -> Result<Value> {
        let path = self
            .missiond_root
            .join(".missiond/v3/runtime/compiled")
            .join(name);
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn bounded_limit(args: &Value) -> i64 {
    args.get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .clamp(1, MAX_QUERY_LIMIT)
}

struct StoredArtifact {
    hash: String,
    size_bytes: i64,
}

fn summary_from_result_payload(args: &Value) -> String {
    if let Some(content) = string_arg(args, "content") {
        return content.chars().take(500).collect();
    }
    args.get("json")
        .map(|value| {
            let text = value.to_string();
            text.chars().take(500).collect()
        })
        .unwrap_or_else(|| "Worker result artifact".to_string())
}

fn task_result_evidence_refs(
    args: &Value,
    details: &Value,
    raw_evidence: Option<&Value>,
    provider: &str,
    slot_id: Option<&str>,
    conversation_id: Option<&str>,
) -> Vec<Value> {
    for key in ["evidence_refs", "evidenceRefs"] {
        if let Some(values) = args
            .get(key)
            .or_else(|| details.get(key))
            .and_then(Value::as_array)
        {
            return values.clone();
        }
    }
    let mut refs = Vec::new();
    if let Some(conversation_id) = conversation_id {
        refs.push(json!({
            "kind": "provider_conversation",
            "provider": provider,
            "conversation_id": conversation_id
        }));
    }
    if let Some(slot_id) = slot_id {
        refs.push(json!({
            "kind": "pty_observation",
            "slot_id": slot_id
        }));
    }
    if raw_evidence.is_some() {
        refs.push(json!({
            "kind": "raw_evidence_inline"
        }));
    }
    if refs.is_empty() {
        refs.push(json!({
            "kind": "completion_content_inline"
        }));
    }
    refs
}

fn task_result_has_explicit_evidence(
    args: &Value,
    details: &Value,
    raw_evidence: Option<&Value>,
) -> bool {
    let has_refs = ["evidence_refs", "evidenceRefs"].iter().any(|key| {
        args.get(*key)
            .or_else(|| details.get(*key))
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty())
    });
    has_refs || raw_evidence.is_some()
}

fn validate_task_result_artifact_payload(
    task_id: &str,
    project_id: &str,
    provider: &str,
    result_status: &str,
    summary: &str,
    content: &Value,
    evidence_refs: &[Value],
    has_explicit_evidence: bool,
) -> Result<()> {
    let invalid = |reason: String| {
        control_error_details(
            COMPLETION_ARTIFACT_INVALID_CODE,
            format!("task_result_artifact invalid for task {task_id}: {reason}"),
            json!({
                "task_id": task_id,
                "reason": reason
            }),
        )
    };
    if task_id.trim().is_empty() {
        return Err(invalid("task_id is required".to_string()));
    }
    if project_id.trim().is_empty() {
        return Err(invalid("project_id is required".to_string()));
    }
    if provider.trim().is_empty() {
        return Err(invalid("producer.provider is required".to_string()));
    }
    if !is_completed_result_status(result_status)
        && !matches!(
            result_status.to_ascii_lowercase().as_str(),
            "failed" | "blocked" | "skipped"
        )
    {
        return Err(invalid(format!(
            "unsupported result_status `{result_status}`"
        )));
    }
    if summary.trim().is_empty() {
        return Err(invalid("summary is required".to_string()));
    }
    let content_empty = match content {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    };
    if content_empty {
        return Err(invalid("content is required".to_string()));
    }
    if evidence_refs.is_empty() || !has_explicit_evidence {
        return Err(invalid(
            "evidence_refs or raw_evidence must be explicitly provided".to_string(),
        ));
    }
    Ok(())
}

fn normalize_worker_settle_status(status: Option<&str>) -> Result<&str> {
    let status = status.unwrap_or("done");
    match status {
        "done" | "failed" | "blocked" | "skipped" => Ok(status),
        other => Err(anyhow!(
            "unsupported worker_settle status `{other}`; expected done, failed, blocked, or skipped"
        )),
    }
}

fn validate_write_scoped_settle_attempt(
    task_id: &str,
    artifact_hash: &str,
    artifact_attempt_id: Option<&str>,
    current_attempt_id: Option<&str>,
    provided_attempt_id: Option<&str>,
) -> Result<()> {
    let attempt_ok = match (artifact_attempt_id, current_attempt_id, provided_attempt_id) {
        (Some(artifact), Some(current), Some(provided)) => {
            artifact == current && provided == current
        }
        _ => false,
    };
    if attempt_ok {
        return Ok(());
    }
    Err(control_error_details(
        EVIDENCE_REQUIRED_CODE,
        format!(
            "worker_settle(done) for write-scoped task {task_id} requires artifact, current, and provided attempt_id to match"
        ),
        json!({
            "task_id": task_id,
            "artifact_hash": artifact_hash,
            "artifact_attempt_id": artifact_attempt_id,
            "current_attempt_id": current_attempt_id,
            "provided_attempt_id": provided_attempt_id,
            "required": "artifact_attempt_id == jobs.current_attempt_id == worker_settle.attempt_id"
        }),
    ))
}

fn metadata_string_list_any(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "[]")
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn first_non_empty_metadata_list(values: &[Option<&Value>]) -> Vec<String> {
    values
        .iter()
        .find_map(|value| {
            let values = metadata_string_list_any(*value);
            if values.is_empty() {
                None
            } else {
                Some(values)
            }
        })
        .unwrap_or_default()
}

fn metadata_string_value_any(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn merge_json_object(target: &mut Value, extra: Value) {
    let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) else {
        return;
    };
    for (key, value) in extra {
        target.insert(key.clone(), value.clone());
    }
}

fn bool_arg_any(args: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        let Some(value) = args.get(*key) else {
            return false;
        };
        value.as_bool().unwrap_or_else(|| {
            value.as_str().is_some_and(|text| {
                matches!(
                    text.trim().to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                )
            })
        })
    })
}

fn system_or_operator_bypass_allowed(args: &Value) -> bool {
    let subject_kind = string_arg(args, "subject_kind")
        .or_else(|| string_arg(args, "subjectKind"))
        .unwrap_or("");
    matches!(subject_kind, "system" | "daemon")
        || (subject_kind == "operator"
            && bool_arg_any(
                args,
                &[
                    "confirm",
                    "operator_confirm",
                    "operatorConfirm",
                    "operator_confirmed",
                    "operatorConfirmed",
                ],
            ))
}

fn runtime_feature_enabled(env_key: &str) -> bool {
    std::env::var(env_key)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn route_outcome_status(outcome: &str) -> &'static str {
    match outcome.trim().to_ascii_lowercase().as_str() {
        "completed" | "complete" | "success" | "succeeded" | "accepted" | "stop" => "succeeded",
        "blocked" | "cancelled" | "canceled" => "blocked",
        "failed" | "failure" | "error" | "errored" | "length" | "max_tokens" => "failed",
        _ => "recorded",
    }
}

fn task_contract_projection_from_metadata(
    task_id: &str,
    project_id: Option<&str>,
    runtime_metadata: &Value,
) -> TaskContractMetadataProjection {
    let dispatch = runtime_metadata
        .get("dispatch_metadata")
        .or_else(|| runtime_metadata.get("swarm_metadata"))
        .or_else(|| runtime_metadata.get("metadata"))
        .unwrap_or(runtime_metadata);
    let task_contract_id = metadata_string_value_any(runtime_metadata.get("task_contract_id"))
        .or_else(|| metadata_string_value_any(dispatch.get("task_contract_id")))
        .unwrap_or_else(|| format!("board-task:{task_id}"));
    let project_id = metadata_string_value_any(dispatch.get("project_id")).or_else(|| {
        project_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    });
    let read_scope = first_non_empty_metadata_list(&[
        runtime_metadata.get("read_scope"),
        dispatch.get("read_scope"),
    ]);
    let write_scope = first_non_empty_metadata_list(&[
        runtime_metadata.get("write_scope"),
        dispatch.get("write_scope"),
    ]);
    let must_not_touch = first_non_empty_metadata_list(&[
        runtime_metadata.get("must_not_touch"),
        dispatch.get("must_not_touch"),
    ]);
    let capability_grant_ids =
        metadata_string_list_any(runtime_metadata.get("capability_grant_ids"))
            .into_iter()
            .chain(metadata_string_list_any(
                dispatch.get("capability_grant_ids"),
            ))
            .collect::<Vec<_>>();
    let sandbox_profile = metadata_string_value_any(runtime_metadata.get("sandbox_profile"))
        .or_else(|| metadata_string_value_any(dispatch.get("sandbox_profile")));
    let completion_materialization_policy =
        metadata_string_value_any(runtime_metadata.get("completion_materialization_policy"))
            .or_else(|| {
                metadata_string_value_any(dispatch.get("completion_materialization_policy"))
            });
    let grounding_refs = runtime_metadata
        .get("grounding_refs")
        .or_else(|| dispatch.get("grounding_refs"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let context_refs = runtime_metadata
        .get("context_refs")
        .or_else(|| dispatch.get("context_refs"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    TaskContractMetadataProjection {
        id: format!("task-contract:{task_id}"),
        task_contract_id,
        project_id,
        dispatch_metadata: dispatch.clone(),
        read_scope,
        write_scope,
        must_not_touch,
        capability_grant_ids,
        sandbox_profile,
        completion_materialization_policy,
        grounding_refs,
        context_refs,
    }
}

fn changed_paths_from_payload(args: &Value, details: &Value) -> Vec<String> {
    for key in [
        "changed_paths",
        "changedPaths",
        "files_changed",
        "filesChanged",
    ] {
        let values = metadata_string_list_any(args.get(key));
        if !values.is_empty() {
            return values;
        }
        let values = metadata_string_list_any(details.get(key));
        if !values.is_empty() {
            return values;
        }
    }
    details
        .get("verification")
        .and_then(|value| {
            value
                .get("changed_paths")
                .or_else(|| value.get("files_changed"))
        })
        .map(|value| metadata_string_list_any(Some(value)))
        .unwrap_or_default()
}

fn verification_evidence_present(args: &Value, details: &Value) -> bool {
    args.get("verification").is_some()
        || args.get("verification_evidence").is_some()
        || args.get("verificationEvidence").is_some()
        || details.get("verification").is_some()
        || details.get("verification_evidence").is_some()
        || details
            .get("evidence_refs")
            .and_then(Value::as_array)
            .is_some_and(|refs| {
                refs.iter().any(|item| {
                    item.get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| kind.contains("verification"))
                })
            })
}

fn git_status_changed_paths(project_root: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["status", "--porcelain=v1"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("git status failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    for line in stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let payload = line[3..].trim();
        if payload.is_empty() {
            continue;
        }
        if let Some((old_path, new_path)) = payload.split_once(" -> ") {
            push_changed_path(&mut paths, old_path);
            push_changed_path(&mut paths, new_path);
        } else {
            push_changed_path(&mut paths, payload);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn git_changed_paths_between(
    project_root: &str,
    from_head: &str,
    to_head: &str,
) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["diff", "--name-only", &format!("{from_head}..{to_head}")])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("git diff --name-only failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    for line in stdout.lines() {
        push_changed_path(&mut paths, line);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn git_head(project_root: &str) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("git rev-parse HEAD failed: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn push_changed_path(paths: &mut Vec<String>, path: &str) {
    let normalized = path.trim().trim_matches('"').trim_start_matches("./");
    if !normalized.is_empty() {
        paths.push(normalized.to_string());
    }
}

fn normalize_changed_path(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

fn attempt_actual_changed_paths(
    pre_changed_paths: &[String],
    status_changed_paths: &[String],
    head_changed_paths: &[String],
) -> Vec<String> {
    let pre_dirty: BTreeSet<String> = pre_changed_paths
        .iter()
        .map(|path| normalize_changed_path(path))
        .filter(|path| !path.is_empty())
        .collect();
    let mut actual = BTreeSet::new();
    for path in head_changed_paths {
        let path = normalize_changed_path(path);
        if !path.is_empty() {
            actual.insert(path);
        }
    }
    for path in status_changed_paths {
        let path = normalize_changed_path(path);
        if !path.is_empty() && !pre_dirty.contains(&path) {
            actual.insert(path);
        }
    }
    actual.into_iter().collect()
}

fn is_completed_result_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "complete" | "verified" | "pass" | "passed"
    )
}

fn scope_matches_path(scope: &str, path: &str) -> bool {
    let scope = scope.trim().trim_start_matches("./").trim_end_matches('/');
    let path = path.trim().trim_start_matches("./").trim_end_matches('/');
    if scope.is_empty() || path.is_empty() {
        return false;
    }
    if scope == "*" || scope == "**/*" {
        return true;
    }
    if let Some(prefix) = scope.strip_suffix("/**") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    if let Some(prefix) = scope.strip_suffix("/*") {
        return path.starts_with(&format!("{prefix}/")) && !path[prefix.len() + 1..].contains('/');
    }
    path == scope || path.starts_with(&format!("{scope}/"))
}

fn job_state_for_event(event_kind: &str) -> Option<&'static str> {
    match event_kind {
        "job.created" => Some("created"),
        "job.claimed" => Some("claimed"),
        "attempt.started" => Some("running"),
        "observation.recorded" => Some("running"),
        "artifact.accepted" => Some("running"),
        "settle.requested" => Some("running"),
        "job.completed" => Some("completed"),
        "job.blocked" => Some("blocked"),
        "job.failed" => Some("failed"),
        "lease.expired" => Some("blocked"),
        "capability.denied" => Some("blocked"),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn normalize_path_for_prefix_check(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn infer_runtime_artifact_kind(path: &str) -> String {
    if path.contains("/compiled/") {
        "compiled-output".to_string()
    } else if path.contains("/lisp-code-sync/") {
        "lisp-code-sync-report".to_string()
    } else if path.contains("/commit-lisp-convergence/") {
        "commit-convergence-report".to_string()
    } else if path.contains("/nightly-evolution/") {
        "nightly-evolution-report".to_string()
    } else if path.contains("/jarvis-smoke/") {
        "jarvis-smoke-report".to_string()
    } else if path.contains("/master-control/context-packs/") {
        "master-control-context-pack".to_string()
    } else if path.contains("/plans/") || path.contains("/executions/") {
        "canonical-task-evidence".to_string()
    } else {
        "runtime-diagnostic".to_string()
    }
}

fn infer_media_type(path: &str) -> &'static str {
    if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".lisp") {
        "application/x-lisp"
    } else if path.ends_with(".md") {
        "text/markdown"
    } else {
        "application/octet-stream"
    }
}

fn runtime_artifact_expires_at(kind: &str, path: &str) -> Option<DateTime<Utc>> {
    if kind == "master-control-context-pack" || path.contains("/master-control/context-packs/") {
        Some(Utc::now() + Duration::days(7))
    } else if matches!(
        kind,
        "lisp-code-sync-report"
            | "commit-convergence-report"
            | "nightly-evolution-report"
            | "jarvis-smoke-report"
            | "runtime-diagnostic"
    ) {
        Some(Utc::now() + Duration::days(14))
    } else {
        None
    }
}

fn fact_relevant_to_project(fact: &Value, project_id: &str) -> bool {
    if project_id == "missiond" {
        return true;
    }
    fact.get("project_id")
        .and_then(Value::as_str)
        .map(|id| id == project_id)
        .unwrap_or(false)
}

fn event_row_json(row: sqlx::postgres::PgRow) -> Value {
    let created_at: DateTime<Utc> = row.get("created_at");
    json!({
        "id": row.get::<String, _>("id"),
        "stream_id": row.get::<String, _>("stream_id"),
        "seq": row.get::<i64, _>("seq"),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "task_id": row.try_get::<Option<String>, _>("task_id").ok().flatten(),
        "agent_id": row.try_get::<Option<String>, _>("agent_id").ok().flatten(),
        "event_kind": row.get::<String, _>("event_kind"),
        "payload": row.get::<Value, _>("payload"),
        "idempotency_key": row.try_get::<Option<String>, _>("idempotency_key").ok().flatten(),
        "correlation_id": row.try_get::<Option<String>, _>("correlation_id").ok().flatten(),
        "parent_event_ids": row.get::<Value, _>("parent_event_ids"),
        "created_at": created_at.to_rfc3339()
    })
}

fn artifact_row_json(row: sqlx::postgres::PgRow, include_content: bool) -> Value {
    let created_at: DateTime<Utc> = row.get("created_at");
    let bytes: Vec<u8> = row.get("bytes");
    let mut value = json!({
        "found": true,
        "hash": row.get::<String, _>("hash"),
        "kind": row.get::<String, _>("kind"),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "task_id": row.try_get::<Option<String>, _>("task_id").ok().flatten(),
        "media_type": row.get::<String, _>("media_type"),
        "size_bytes": row.get::<i64, _>("size_bytes"),
        "metadata": row.get::<Value, _>("metadata"),
        "created_at": created_at.to_rfc3339()
    });
    if include_content {
        let content = String::from_utf8_lossy(&bytes).to_string();
        value["content"] = json!(content);
        if row.get::<String, _>("media_type") == "application/json" {
            if let Ok(parsed) = serde_json::from_slice::<Value>(&bytes) {
                value["json"] = parsed;
            }
        }
    }
    value
}

fn claim_conflict_projection_from_error(err: &anyhow::Error) -> Option<Value> {
    let control = err.downcast_ref::<StructuredControlError>()?;
    if control.code != CLAIM_CONFLICT_CODE {
        return None;
    }
    let conflict = control
        .details
        .get("conflict")
        .cloned()
        .unwrap_or_else(|| control.details.clone());
    Some(json!({
        "schema": "missiond.shared-claim.v1",
        "ok": false,
        "status": "conflict",
        "code": CLAIM_CONFLICT_CODE,
        "error_code": CLAIM_CONFLICT_CODE,
        "message": control.message.clone(),
        "details": control.details.clone(),
        "conflict": conflict
    }))
}

fn claim_row_json(row: sqlx::postgres::PgRow) -> Value {
    let acquired_at: DateTime<Utc> = row.get("acquired_at");
    let lease_expires_at: DateTime<Utc> = row.get("lease_expires_at");
    let released_at: Option<DateTime<Utc>> = row.try_get("released_at").ok().flatten();
    let heartbeat_at: Option<DateTime<Utc>> = row.try_get("heartbeat_at").ok().flatten();
    json!({
        "id": row.get::<String, _>("id"),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "task_id": row.try_get::<Option<String>, _>("task_id").ok().flatten(),
        "owner_id": row.get::<String, _>("owner_id"),
        "scope_kind": row.get::<String, _>("scope_kind"),
        "scope_key": row.get::<String, _>("scope_key"),
        "status": row.get::<String, _>("status"),
        "acquired_at": acquired_at.to_rfc3339(),
        "lease_expires_at": lease_expires_at.to_rfc3339(),
        "released_at": released_at.map(|dt| dt.to_rfc3339()),
        "heartbeat_at": heartbeat_at.map(|dt| dt.to_rfc3339()),
        "metadata": row.get::<Value, _>("metadata")
    })
}

fn cursor_row_json(row: sqlx::postgres::PgRow) -> Value {
    let updated_at: DateTime<Utc> = row.get("updated_at");
    json!({
        "agent_id": row.get::<String, _>("agent_id"),
        "stream_id": row.get::<String, _>("stream_id"),
        "last_seq": row.get::<i64, _>("last_seq"),
        "updated_at": updated_at.to_rfc3339()
    })
}

fn task_result_row_json(row: sqlx::postgres::PgRow) -> Value {
    let created_at: DateTime<Utc> = row.get("created_at");
    json!({
        "id": row.get::<String, _>("id"),
        "artifact_hash": row.get::<String, _>("artifact_hash"),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "task_id": row.get::<String, _>("task_id"),
        "job_id": row.try_get::<Option<String>, _>("job_id").ok().flatten(),
        "attempt_id": row.try_get::<Option<String>, _>("attempt_id").ok().flatten(),
        "slot_id": row.try_get::<Option<String>, _>("slot_id").ok().flatten(),
        "conversation_id": row.try_get::<Option<String>, _>("conversation_id").ok().flatten(),
        "provider": row.try_get::<Option<String>, _>("provider").ok().flatten(),
        "result_status": row.get::<String, _>("result_status"),
        "summary": row.get::<String, _>("summary"),
        "producer_subject_kind": row.try_get::<Option<String>, _>("producer_subject_kind").ok().flatten(),
        "producer_subject_id": row.try_get::<Option<String>, _>("producer_subject_id").ok().flatten(),
        "capability_grant_id": row.try_get::<Option<String>, _>("capability_grant_id").ok().flatten(),
        "created_at": created_at.to_rfc3339()
    })
}

fn task_evidence_summary_row_json(row: sqlx::postgres::PgRow) -> Value {
    let created_at: DateTime<Utc> = row.get("created_at");
    let result_status = row
        .try_get::<String, _>("result_status")
        .unwrap_or_else(|_| "unknown".to_string());
    let complete = matches!(
        result_status.as_str(),
        "completed" | "complete" | "verified" | "pass" | "passed"
    );
    json!({
        "taskId": row.get::<String, _>("task_id"),
        "artifactHash": row.get::<String, _>("artifact_hash"),
        "projectId": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "slotId": row.try_get::<Option<String>, _>("slot_id").ok().flatten(),
        "conversationId": row.try_get::<Option<String>, _>("conversation_id").ok().flatten(),
        "provider": row.try_get::<Option<String>, _>("provider").ok().flatten(),
        "resultStatus": result_status,
        "summary": row.get::<String, _>("summary"),
        "verifierStatus": null,
        "gateStatus": if complete { "ok" } else { "blocked" },
        "complete": complete,
        "missingReasons": if complete {
            Vec::<String>::new()
        } else {
            vec!["result-status-not-complete".to_string()]
        },
        "updatedAt": created_at.to_rfc3339()
    })
}

fn runtime_artifact_row_json(row: sqlx::postgres::PgRow) -> Value {
    let created_at: DateTime<Utc> = row.get("created_at");
    let indexed_at: DateTime<Utc> = row.get("indexed_at");
    let expires_at: Option<DateTime<Utc>> = row.try_get("expires_at").ok().flatten();
    json!({
        "id": row.get::<String, _>("id"),
        "hash": row.get::<String, _>("hash"),
        "path": row.get::<String, _>("path"),
        "kind": row.get::<String, _>("kind"),
        "source_surface": row.try_get::<Option<String>, _>("source_surface").ok().flatten(),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "task_id": row.try_get::<Option<String>, _>("task_id").ok().flatten(),
        "media_type": row.get::<String, _>("media_type"),
        "size_bytes": row.get::<i64, _>("size_bytes"),
        "status": row.get::<String, _>("status"),
        "metadata": row.get::<Value, _>("metadata"),
        "created_at": created_at.to_rfc3339(),
        "indexed_at": indexed_at.to_rfc3339(),
        "expires_at": expires_at.map(|dt| dt.to_rfc3339()),
        "role": "runtime_diagnostic_catalog"
    })
}

fn board_task_evidence_row_json(row: sqlx::postgres::PgRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "title": row.get::<String, _>("title"),
        "status": row.get::<String, _>("status"),
        "category": row.get::<String, _>("category"),
        "project": row.try_get::<Option<String>, _>("project").ok().flatten(),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "assignee": row.try_get::<Option<String>, _>("assignee").ok().flatten(),
        "updated_at": row.get::<String, _>("updated_at"),
        "dedupe_key": row.try_get::<Option<String>, _>("dedupe_key").ok().flatten(),
        "role": "coordination_projection"
    })
}

fn conversation_evidence_row_json(row: sqlx::postgres::PgRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "project": row.try_get::<Option<String>, _>("project").ok().flatten(),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "slot_id": row.try_get::<Option<String>, _>("slot_id").ok().flatten(),
        "source": row.get::<String, _>("source"),
        "model": row.try_get::<Option<String>, _>("model").ok().flatten(),
        "status": row.try_get::<Option<String>, _>("status").ok().flatten(),
        "conversation_type": row.get::<String, _>("conversation_type"),
        "task_id": row.try_get::<Option<String>, _>("task_id").ok().flatten(),
        "message_count": row.try_get::<Option<i32>, _>("message_count").ok().flatten(),
        "started_at": row.get::<String, _>("started_at"),
        "ended_at": row.try_get::<Option<String>, _>("ended_at").ok().flatten(),
        "updated_at": row.try_get::<Option<String>, _>("updated_at").ok().flatten(),
        "llm_summary": row.try_get::<Option<String>, _>("llm_summary").ok().flatten(),
        "role": "provider_turn_read_model"
    })
}

fn timeline_event_row_json(row: sqlx::postgres::PgRow) -> Value {
    let ts: DateTime<Utc> = row.get("ts");
    json!({
        "seq": row.get::<i64, _>("seq"),
        "domain": row.get::<String, _>("domain"),
        "kind": row.get::<String, _>("kind"),
        "payload_inline": row.try_get::<Option<Value>, _>("payload_inline").ok().flatten(),
        "payload_ref": row.try_get::<Option<String>, _>("payload_ref").ok().flatten(),
        "producer_id": row.get::<String, _>("producer_id"),
        "trace_id": row.try_get::<Option<String>, _>("trace_id").ok().flatten(),
        "ts": ts.to_rfc3339(),
        "ephemeral": row.get::<bool, _>("ephemeral"),
        "role": "event_causality_projection"
    })
}

fn kb_evidence_row_json(row: sqlx::postgres::PgRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "category": row.get::<String, _>("category"),
        "key": row.get::<String, _>("key"),
        "summary": row.get::<String, _>("summary"),
        "source": row.try_get::<Option<String>, _>("source").ok().flatten(),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "linked_task_id": row.try_get::<Option<String>, _>("linked_task_id").ok().flatten(),
        "scope_task_id": row.try_get::<Option<String>, _>("scope_task_id").ok().flatten(),
        "review_state": row.try_get::<Option<String>, _>("review_state").ok().flatten().unwrap_or_else(|| "unreviewed".to_string()),
        "updated_at": row.get::<String, _>("updated_at"),
        "role": "curated_long_term_knowledge"
    })
}

fn workflow_run_row_json(row: sqlx::postgres::PgRow) -> Value {
    let started_at: DateTime<Utc> = row.get("started_at");
    let updated_at: DateTime<Utc> = row.get("updated_at");
    let finished_at: Option<DateTime<Utc>> = row.try_get("finished_at").ok().flatten();
    json!({
        "id": row.get::<String, _>("id"),
        "workflow_id": row.try_get::<Option<String>, _>("workflow_id").ok().flatten(),
        "workflow_path": row.try_get::<Option<String>, _>("workflow_path").ok().flatten(),
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "parent_task_id": row.try_get::<Option<String>, _>("parent_task_id").ok().flatten(),
        "status": row.get::<String, _>("status"),
        "cursor": row.get::<Value, _>("cursor"),
        "checkpoint": row.get::<Value, _>("checkpoint"),
        "max_inflight": row.get::<i32, _>("max_inflight"),
        "active_task_ids": row.get::<Value, _>("active_task_ids"),
        "artifact_hashes": row.get::<Value, _>("artifact_hashes"),
        "started_at": started_at.to_rfc3339(),
        "updated_at": updated_at.to_rfc3339(),
        "finished_at": finished_at.map(|dt| dt.to_rfc3339())
    })
}

fn workflow_run_summary_row_json(row: sqlx::postgres::PgRow) -> Value {
    let started_at: DateTime<Utc> = row.get("started_at");
    let updated_at: DateTime<Utc> = row.get("updated_at");
    let finished_at: Option<DateTime<Utc>> = row.try_get("finished_at").ok().flatten();
    let workflow_id = row
        .try_get::<Option<String>, _>("workflow_id")
        .ok()
        .flatten();
    let workflow_path = row
        .try_get::<Option<String>, _>("workflow_path")
        .ok()
        .flatten();
    let parent_task_id = row
        .try_get::<Option<String>, _>("parent_task_id")
        .ok()
        .flatten();
    let recoverable = workflow_id.is_some() && workflow_path.is_some() && parent_task_id.is_some();
    let checkpoint = row.get::<Value, _>("checkpoint");
    json!({
        "id": row.get::<String, _>("id"),
        "workflow_id": workflow_id,
        "workflow_path": workflow_path,
        "project_id": row.try_get::<Option<String>, _>("project_id").ok().flatten(),
        "parent_task_id": parent_task_id,
        "status": row.get::<String, _>("status"),
        "cursor": row.get::<Value, _>("cursor"),
        "checkpoint": checkpoint,
        "checkpointExcerpt": workflow_checkpoint_excerpt(&checkpoint),
        "max_inflight": row.get::<i32, _>("max_inflight"),
        "active_task_ids": row.get::<Value, _>("active_task_ids"),
        "artifact_hashes": row.get::<Value, _>("artifact_hashes"),
        "started_at": started_at.to_rfc3339(),
        "updated_at": updated_at.to_rfc3339(),
        "finished_at": finished_at.map(|dt| dt.to_rfc3339()),
        "updatedAgeSecs": row.try_get::<i64, _>("updated_age_secs").unwrap_or(0),
        "recoverable": recoverable
    })
}

fn workflow_checkpoint_excerpt(checkpoint: &Value) -> String {
    let mut text = checkpoint.to_string();
    if text.len() > 240 {
        text.truncate(240);
        text.push_str("...");
    }
    text
}

#[allow(dead_code)]
fn normalize_path(path: &str) -> String {
    Path::new(path).to_string_lossy().to_string()
}

fn select_agent_entry(
    compiled: &Value,
    entry_id: Option<&str>,
    surface: Option<&str>,
    intent: Option<&str>,
) -> Option<Value> {
    let entries = compiled.pointer("/payload/entries")?.as_array()?;
    if let Some(entry_id) = entry_id.filter(|value| !value.trim().is_empty()) {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(entry_id))
        {
            return Some(entry.clone());
        }
    }
    if let Some(surface) = surface.filter(|value| !value.trim().is_empty()) {
        if let Some(entry) = entries.iter().find(|entry| {
            entry
                .get("surfaces")
                .and_then(Value::as_array)
                .is_some_and(|surfaces| surfaces.iter().any(|item| item.as_str() == Some(surface)))
        }) {
            return Some(entry.clone());
        }
    }
    let normalized = intent.unwrap_or_default().to_lowercase();
    entries
        .iter()
        .filter_map(|entry| {
            let score = score_agent_entry(entry, &normalized);
            (score > 0).then_some((score, entry))
        })
        .max_by(|(left_score, left), (right_score, right)| {
            left_score.cmp(right_score).then_with(|| {
                right
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .cmp(left.get("id").and_then(Value::as_str).unwrap_or(""))
            })
        })
        .map(|(_, entry)| entry.clone())
}

fn select_project_agent_entry(compiled: &Value, project_id: &str) -> Option<Value> {
    compiled
        .pointer("/payload/projects")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("projectId").and_then(Value::as_str) == Some(project_id))
        .cloned()
}

fn score_agent_entry(entry: &Value, normalized_intent: &str) -> i64 {
    let mut score = 0;
    if let Some(id) = entry.get("id").and_then(Value::as_str) {
        if normalized_intent.contains(&id.to_lowercase()) {
            score += 100;
        }
    }
    for key in ["intentKeywords", "surfaces"] {
        if let Some(values) = entry.get(key).and_then(Value::as_array) {
            for value in values.iter().filter_map(Value::as_str) {
                let normalized = value.to_lowercase();
                if !normalized.is_empty() && normalized_intent.contains(&normalized) {
                    score += 25 + normalized.len() as i64;
                }
            }
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_lease_request_from_args(args: &Value) -> Result<ReleaseLeaseRequest> {
        Ok(ReleaseLeaseRequest {
            claim_id: string_arg(args, "claim_id")
                .or_else(|| string_arg(args, "claimId"))
                .or_else(|| string_arg(args, "id"))
                .ok_or_else(|| anyhow!("claim_id is required"))?
                .to_string(),
            owner_id: string_arg(args, "owner_id")
                .or_else(|| string_arg(args, "ownerId"))
                .map(str::to_string),
            grant_id: string_arg(args, "grant_id")
                .or_else(|| string_arg(args, "grantId"))
                .or_else(|| string_arg(args, "capability_grant_id"))
                .or_else(|| string_arg(args, "capabilityGrantId"))
                .map(str::to_string),
            subject_kind: string_arg(args, "subject_kind")
                .or_else(|| string_arg(args, "subjectKind"))
                .unwrap_or("worker")
                .to_string(),
            subject_id: string_arg(args, "subject_id")
                .or_else(|| string_arg(args, "subjectId"))
                .or_else(|| string_arg(args, "owner_id"))
                .or_else(|| string_arg(args, "ownerId"))
                .unwrap_or("")
                .to_string(),
            details: args.get("details").cloned().unwrap_or_else(|| json!({})),
            allow_system_bypass: system_or_operator_bypass_allowed(args),
            bypass_reason: Some(
                "mission_shared_memory release system/operator authority".to_string(),
            ),
        })
    }

    fn heartbeat_lease_request_from_args(args: &Value) -> Result<HeartbeatLeaseRequest> {
        Ok(HeartbeatLeaseRequest {
            claim_id: string_arg(args, "claim_id")
                .or_else(|| string_arg(args, "claimId"))
                .or_else(|| string_arg(args, "id"))
                .ok_or_else(|| anyhow!("claim_id is required"))?
                .to_string(),
            owner_id: string_arg(args, "owner_id")
                .or_else(|| string_arg(args, "ownerId"))
                .map(str::to_string),
            grant_id: string_arg(args, "grant_id")
                .or_else(|| string_arg(args, "grantId"))
                .or_else(|| string_arg(args, "capability_grant_id"))
                .or_else(|| string_arg(args, "capabilityGrantId"))
                .map(str::to_string),
            subject_kind: string_arg(args, "subject_kind")
                .or_else(|| string_arg(args, "subjectKind"))
                .unwrap_or("worker")
                .to_string(),
            subject_id: string_arg(args, "subject_id")
                .or_else(|| string_arg(args, "subjectId"))
                .or_else(|| string_arg(args, "owner_id"))
                .or_else(|| string_arg(args, "ownerId"))
                .unwrap_or("")
                .to_string(),
            lease_secs: args
                .get("lease_secs")
                .or_else(|| args.get("leaseSecs"))
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_LEASE_SECS),
            details: args.get("details").cloned().unwrap_or_else(|| json!({})),
            allow_system_bypass: system_or_operator_bypass_allowed(args),
            bypass_reason: Some(
                "mission_shared_memory heartbeat system/operator authority".to_string(),
            ),
        })
    }

    #[test]
    fn runtime_artifact_kind_is_inferred_from_v3_runtime_path() {
        assert_eq!(
            infer_runtime_artifact_kind(
                ".missiond/v3/runtime/compiled/compiled-runtime-config.json"
            ),
            "compiled-output"
        );
        assert_eq!(
            infer_runtime_artifact_kind(".missiond/v3/runtime/lisp-code-sync/20260523.report.lisp"),
            "lisp-code-sync-report"
        );
        assert_eq!(
            infer_runtime_artifact_kind(".missiond/v3/runtime/jarvis-smoke/smoke.json"),
            "jarvis-smoke-report"
        );
        assert_eq!(
            infer_runtime_artifact_kind(
                ".missiond/v3/runtime/master-control/context-packs/pack.lisp"
            ),
            "master-control-context-pack"
        );
        assert_eq!(
            infer_runtime_artifact_kind(".missiond/v3/runtime/plans/plan.evidence.json"),
            "canonical-task-evidence"
        );
    }

    #[test]
    fn runtime_artifact_retention_keeps_canonical_and_compiled_outputs() {
        assert!(runtime_artifact_expires_at(
            "compiled-output",
            ".missiond/v3/runtime/compiled/compiled-runtime-config.json"
        )
        .is_none());
        assert!(runtime_artifact_expires_at(
            "canonical-task-evidence",
            ".missiond/v3/runtime/plans/plan.evidence.json"
        )
        .is_none());
        assert!(runtime_artifact_expires_at(
            "jarvis-smoke-report",
            ".missiond/v3/runtime/jarvis-smoke/smoke.json"
        )
        .is_some());
    }

    #[test]
    fn runtime_artifact_media_type_tracks_file_extension() {
        assert_eq!(
            infer_media_type("compiled-runtime-config.json"),
            "application/json"
        );
        assert_eq!(infer_media_type("report.lisp"), "application/x-lisp");
        assert_eq!(infer_media_type("notes.md"), "text/markdown");
        assert_eq!(infer_media_type("screen.bin"), "application/octet-stream");
    }

    #[test]
    fn task_result_artifact_validation_rejects_empty_completed_content() {
        let err = validate_task_result_artifact_payload(
            "task-1",
            "missiond",
            "autopilot",
            "completed",
            "summary",
            &json!({}),
            &[json!({"kind": "provider_conversation"})],
            true,
        )
        .expect_err("empty content must be rejected");
        assert!(err.to_string().starts_with("COMPLETION_ARTIFACT_INVALID"));
    }

    #[test]
    fn task_result_evidence_refs_default_to_structured_observation_refs() {
        let refs = task_result_evidence_refs(
            &json!({}),
            &json!({}),
            Some(&json!("final")),
            "codex",
            Some("slot-codex"),
            Some("conv-1"),
        );
        assert!(refs
            .iter()
            .any(|value| value["kind"] == "provider_conversation"));
        assert!(refs.iter().any(|value| value["kind"] == "pty_observation"));
        assert!(refs
            .iter()
            .any(|value| value["kind"] == "raw_evidence_inline"));
    }

    #[test]
    fn attempt_actual_changed_paths_uses_head_diff_and_subtracts_pre_dirty_status_paths() {
        let actual = attempt_actual_changed_paths(
            &[
                "already-dirty.rs".to_string(),
                "./pre-existing.md".to_string(),
            ],
            &[
                "already-dirty.rs".to_string(),
                "new-dirty.rs".to_string(),
                "./pre-existing.md".to_string(),
            ],
            &[
                "committed.rs".to_string(),
                "already-dirty.rs".to_string(),
                "nested/committed.md".to_string(),
            ],
        );
        assert_eq!(
            actual,
            vec![
                "already-dirty.rs".to_string(),
                "committed.rs".to_string(),
                "nested/committed.md".to_string(),
                "new-dirty.rs".to_string()
            ]
        );
    }

    #[test]
    fn context_slice_agent_entry_selects_by_intent() {
        let compiled = json!({
            "payload": {
                "entries": [
                    {
                        "id": "modify-plan-execution",
                        "intentKeywords": ["plan execution"],
                        "surfaces": ["mission_plan"]
                    },
                    {
                        "id": "modify-workstation-autopilot",
                        "intentKeywords": ["autopilot"],
                        "surfaces": ["autopilot-runtime"]
                    }
                ]
            }
        });
        let entry = select_agent_entry(&compiled, None, None, Some("change plan execution"))
            .expect("entry should match");
        assert_eq!(
            entry.get("id").and_then(Value::as_str),
            Some("modify-plan-execution")
        );
    }

    #[test]
    fn context_slice_project_agent_entry_selects_registered_project() {
        let compiled = json!({
            "payload": {
                "projects": [
                    {
                        "id": "project:jarvis",
                        "projectId": "jarvis",
                        "coverageState": "native-ssot-present",
                        "readFirst": ["/Users/jinchen/Projects/jarvis/.missiond/intent.lisp"]
                    }
                ]
            }
        });
        let entry = select_project_agent_entry(&compiled, "jarvis").expect("project should match");
        assert_eq!(
            entry.get("projectId").and_then(Value::as_str),
            Some("jarvis")
        );
    }

    #[test]
    fn release_lease_request_from_args_preserves_authority_and_scope() {
        let req = release_lease_request_from_args(&json!({
            "claim_id": "claim-1",
            "owner_id": "slot-xjpcode",
            "grant_id": "grant-lease",
            "subject_kind": "worker",
            "details": {"source": "worker-shutdown"}
        }))
        .expect("release request");

        assert_eq!(req.claim_id, "claim-1");
        assert_eq!(req.owner_id.as_deref(), Some("slot-xjpcode"));
        assert_eq!(req.grant_id.as_deref(), Some("grant-lease"));
        assert_eq!(req.subject_kind, "worker");
        assert_eq!(req.subject_id, "slot-xjpcode");
        assert!(!req.allow_system_bypass);
        assert_eq!(req.details["source"], "worker-shutdown");
    }

    #[test]
    fn heartbeat_lease_request_from_args_requires_explicit_operator_confirm_for_bypass() {
        let unconfirmed = heartbeat_lease_request_from_args(&json!({
            "claim_id": "claim-1",
            "owner_id": "slot-xjpcode",
            "subject_kind": "operator"
        }))
        .expect("heartbeat request");
        assert!(!unconfirmed.allow_system_bypass);

        let confirmed = heartbeat_lease_request_from_args(&json!({
            "claim_id": "claim-1",
            "owner_id": "slot-xjpcode",
            "subject_kind": "operator",
            "operator_confirmed": true,
            "lease_secs": 90
        }))
        .expect("confirmed heartbeat request");
        assert_eq!(confirmed.subject_id, "slot-xjpcode");
        assert_eq!(confirmed.lease_secs, 90);
        assert!(confirmed.allow_system_bypass);
    }

    #[test]
    fn write_scoped_settle_attempt_requires_exact_three_way_match() {
        assert!(validate_write_scoped_settle_attempt(
            "task-1",
            "hash-1",
            Some("attempt-1"),
            Some("attempt-1"),
            Some("attempt-1"),
        )
        .is_ok());

        let missing_current = validate_write_scoped_settle_attempt(
            "task-1",
            "hash-1",
            Some("attempt-1"),
            None,
            Some("attempt-1"),
        )
        .expect_err("current attempt is required");
        assert_write_scoped_attempt_error(&missing_current, None, Some("attempt-1"));

        let missing_provided = validate_write_scoped_settle_attempt(
            "task-1",
            "hash-1",
            Some("attempt-1"),
            Some("attempt-1"),
            None,
        )
        .expect_err("worker_settle attempt_id is required");
        assert_write_scoped_attempt_error(&missing_provided, Some("attempt-1"), None);

        let wrong_artifact = validate_write_scoped_settle_attempt(
            "task-1",
            "hash-1",
            Some("attempt-old"),
            Some("attempt-1"),
            Some("attempt-1"),
        )
        .expect_err("artifact attempt must match current attempt");
        assert_write_scoped_attempt_error(&wrong_artifact, Some("attempt-1"), Some("attempt-1"));
    }

    fn assert_write_scoped_attempt_error(
        err: &anyhow::Error,
        current_attempt_id: Option<&str>,
        provided_attempt_id: Option<&str>,
    ) {
        let structured = err
            .downcast_ref::<StructuredControlError>()
            .expect("structured control error");
        assert_eq!(structured.code, EVIDENCE_REQUIRED_CODE);
        assert_eq!(
            structured.details["required"],
            "artifact_attempt_id == jobs.current_attempt_id == worker_settle.attempt_id"
        );
        assert_eq!(
            structured.details["current_attempt_id"],
            current_attempt_id.map(Value::from).unwrap_or(Value::Null)
        );
        assert_eq!(
            structured.details["provided_attempt_id"],
            provided_attempt_id.map(Value::from).unwrap_or(Value::Null)
        );
    }

    #[test]
    fn claim_conflict_projection_preserves_structured_error_details() {
        let err: anyhow::Error = StructuredControlError::new(
            CLAIM_CONFLICT_CODE,
            "active work lease conflict for write_scope:src",
        )
        .with_details(json!({
            "scope_kind": "write_scope",
            "scope_key": "src",
            "holder": "worker-1",
            "lease_expires_at": "2026-05-28T00:00:00Z",
            "authority": "work_leases",
            "conflict": {
                "id": "lease-1",
                "owner_id": "worker-1"
            }
        }))
        .into();

        let projection = claim_conflict_projection_from_error(&err).expect("conflict projection");
        assert_eq!(projection["ok"], false);
        assert_eq!(projection["code"], CLAIM_CONFLICT_CODE);
        assert_eq!(projection["error_code"], CLAIM_CONFLICT_CODE);
        assert_eq!(projection["details"]["authority"], "work_leases");
        assert_eq!(projection["conflict"]["id"], "lease-1");
    }

    #[test]
    fn claim_conflict_projection_ignores_non_conflict_errors() {
        let err: anyhow::Error =
            StructuredControlError::new(CAPABILITY_DENIED_CODE, "denied").into();
        assert!(claim_conflict_projection_from_error(&err).is_none());
    }
}
