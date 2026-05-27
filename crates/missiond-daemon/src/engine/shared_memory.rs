use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use missiond_core::db::traits::MissionStore;
use missiond_core::event::events::{BoardEvent, SlotEvent, SystemEvent};
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
const WRITE_SCOPE_VIOLATION_CODE: &str = "WRITE_SCOPE_VIOLATION";

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
    pub scope_kind: String,
    pub scope_key: String,
    pub lease_secs: i64,
    pub metadata: Value,
}

struct CapabilityGrantInput<'a> {
    subject_kind: &'a str,
    subject_id: &'a str,
    operation: &'a str,
    scope_kind: &'a str,
    scope_key: &'a str,
    project_id: Option<&'a str>,
    task_id: Option<&'a str>,
    issuer: &'a str,
    evidence_requirement: Option<&'a str>,
    details: Value,
}

#[derive(Debug, Clone, Default)]
struct TaskRuntimeContract {
    project_id: Option<String>,
    project_root: Option<String>,
    write_scope: Vec<String>,
    must_not_touch: Vec<String>,
    capability_grant_ids: Vec<String>,
    sandbox_profile: Option<String>,
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
            "task_result_put" | "put_task_result" => self.task_result_put(args).await,
            "task_result_get" | "get_task_result" => self.task_result_get(args).await,
            "task_evidence_summary" | "evidence_summary" => self.task_evidence_summary(args).await,
            "workflow_start" | "start_workflow" => self.workflow_start(args).await,
            "workflow_checkpoint" | "checkpoint_workflow" => self.workflow_checkpoint(args).await,
            "workflow_status" | "get_workflow_status" => self.workflow_status(args).await,
            "workflow_summary" | "workflow_runs_summary" => {
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
            "worker_settle" | "completion_settle" | "settle_worker" => {
                self.worker_settle(args).await
            }
            "capability_grant" | "grant_capability" => self.capability_grant_from_args(args).await,
            "capability_check" | "check_capability" => self.capability_check_from_args(args).await,
            "job_event" | "record_job_event" => self.job_event_from_args(args).await,
            "claim" => self.claim_from_args(args).await,
            "release" => self.release(args).await,
            "heartbeat" => self.heartbeat(args).await,
            "cursor" => self.cursor(args).await,
            other => Err(anyhow!("unknown shared memory action: {other}")),
        }
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
        Ok(grant_ids)
    }

    async fn insert_capability_grant(&self, input: CapabilityGrantInput<'_>) -> Result<String> {
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

    async fn capability_grant_from_args(&self, args: &Value) -> Result<Value> {
        let subject_kind = string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or("task");
        let subject_id = string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .ok_or_else(|| anyhow!("subject_id is required"))?;
        let operation =
            string_arg(args, "operation").ok_or_else(|| anyhow!("operation is required"))?;
        let scope_kind = string_arg(args, "scope_kind")
            .or_else(|| string_arg(args, "scopeKind"))
            .ok_or_else(|| anyhow!("scope_kind is required"))?;
        let scope_key = string_arg(args, "scope_key")
            .or_else(|| string_arg(args, "scopeKey"))
            .ok_or_else(|| anyhow!("scope_key is required"))?;
        let task_id = string_arg(args, "task_id").or_else(|| string_arg(args, "taskId"));
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let id = self
            .insert_capability_grant(CapabilityGrantInput {
                subject_kind,
                subject_id,
                operation,
                scope_kind,
                scope_key,
                project_id,
                task_id,
                issuer: string_arg(args, "issuer").unwrap_or("missiond"),
                evidence_requirement: string_arg(args, "evidence_requirement")
                    .or_else(|| string_arg(args, "evidenceRequirement")),
                details: args.get("details").cloned().unwrap_or_else(|| json!({})),
            })
            .await?;
        Ok(json!({
            "schema": "missiond.capability-grant.v1",
            "ok": true,
            "grant_id": id
        }))
    }

    async fn capability_check_from_args(&self, args: &Value) -> Result<Value> {
        let task_id = string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .ok_or_else(|| anyhow!("task_id is required"))?;
        let operation =
            string_arg(args, "operation").ok_or_else(|| anyhow!("operation is required"))?;
        let scope_kind = string_arg(args, "scope_kind")
            .or_else(|| string_arg(args, "scopeKind"))
            .unwrap_or("task");
        let scope_key = string_arg(args, "scope_key")
            .or_else(|| string_arg(args, "scopeKey"))
            .unwrap_or(task_id);
        self.require_capability(task_id, operation, scope_kind, scope_key)
            .await?;
        Ok(json!({
            "schema": "missiond.capability-check.v1",
            "ok": true,
            "task_id": task_id,
            "operation": operation,
            "scope_kind": scope_kind,
            "scope_key": scope_key
        }))
    }

    async fn job_event_from_args(&self, args: &Value) -> Result<Value> {
        let task_id = string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .ok_or_else(|| anyhow!("task_id is required"))?;
        let event_kind = string_arg(args, "event_kind")
            .or_else(|| string_arg(args, "eventKind"))
            .unwrap_or("observation.recorded");
        let state = job_state_for_event(event_kind).unwrap_or("running");
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let job_id = self
            .ensure_job_for_task(
                project_id,
                task_id,
                state,
                args.get("runtime_metadata")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            )
            .await?;
        let event = self
            .append_event(&json!({
                "stream_id": "execution-control-plane",
                "event_kind": event_kind,
                "project_id": project_id,
                "task_id": task_id,
                "agent_id": string_arg(args, "agent_id").or_else(|| string_arg(args, "agentId")).unwrap_or("missiond"),
                "idempotency_key": format!("job-event:{task_id}:{event_kind}:{}", Utc::now().timestamp_millis()),
                "payload": args.get("payload").cloned().unwrap_or_else(|| json!({}))
            }))
            .await?;
        Ok(json!({
            "schema": "missiond.job-event.v1",
            "ok": true,
            "job_id": job_id,
            "state": state,
            "event": event
        }))
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

    async fn require_capability(
        &self,
        task_id: &str,
        operation: &str,
        scope_kind: &str,
        scope_key: &str,
    ) -> Result<String> {
        let grant = sqlx::query(
            r#"
            SELECT id
            FROM capability_grants
            WHERE task_id = $1
              AND operation = $2
              AND scope_kind = $3
              AND scope_key = $4
              AND status = 'active'
              AND (expires_at IS NULL OR expires_at > now())
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(task_id)
        .bind(operation)
        .bind(scope_kind)
        .bind(scope_key)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = grant {
            let grant_id: String = row.try_get("id")?;
            self.audit_capability(
                Some(&grant_id),
                Some("task"),
                Some(task_id),
                operation,
                scope_kind,
                scope_key,
                "allowed",
                None,
                json!({}),
            )
            .await?;
            return Ok(grant_id);
        }
        self.audit_capability(
            None,
            Some("task"),
            Some(task_id),
            operation,
            scope_kind,
            scope_key,
            "denied",
            Some(CAPABILITY_DENIED_CODE),
            json!({
                "reason": "no active capability grant",
                "task_id": task_id
            }),
        )
        .await?;
        Err(control_error_details(
            CAPABILITY_DENIED_CODE,
            format!("task {task_id} lacks active capability for {operation} on {scope_kind}:{scope_key}"),
            json!({
                "task_id": task_id,
                "operation": operation,
                "scope_kind": scope_kind,
                "scope_key": scope_key
            }),
        ))
    }

    async fn task_runtime_contract(&self, task_id: &str) -> Result<TaskRuntimeContract> {
        let Some(task) = self.store.get_board_task(task_id).await? else {
            return Err(control_error_details(
                RUNTIME_METADATA_REQUIRED_CODE,
                format!("BoardTask {task_id} is required for runtime_metadata-backed control"),
                json!({ "task_id": task_id }),
            ));
        };
        if task
            .runtime_metadata
            .as_object()
            .is_none_or(|obj| obj.is_empty())
        {
            return Err(control_error_details(
                RUNTIME_METADATA_REQUIRED_CODE,
                format!("BoardTask {task_id} has no runtime_metadata; legacy description fallback is disabled"),
                json!({ "task_id": task_id }),
            ));
        }
        let dispatch = task
            .runtime_metadata
            .get("dispatch_metadata")
            .or_else(|| task.runtime_metadata.get("swarm_metadata"))
            .or_else(|| task.runtime_metadata.get("metadata"))
            .unwrap_or(&task.runtime_metadata);
        let capability_grant_ids =
            metadata_string_list_any(task.runtime_metadata.get("capability_grant_ids"))
                .into_iter()
                .chain(metadata_string_list_any(
                    dispatch.get("capability_grant_ids"),
                ))
                .collect::<Vec<_>>();
        let sandbox_profile =
            metadata_string_value_any(task.runtime_metadata.get("sandbox_profile"))
                .or_else(|| metadata_string_value_any(dispatch.get("sandbox_profile")));
        Ok(TaskRuntimeContract {
            project_id: metadata_string_value_any(dispatch.get("project_id")).or(task.project),
            project_root: metadata_string_value_any(dispatch.get("project_root")),
            write_scope: metadata_string_list_any(dispatch.get("write_scope")),
            must_not_touch: metadata_string_list_any(dispatch.get("must_not_touch")),
            capability_grant_ids,
            sandbox_profile,
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
            return Err(control_error_details(
                WRITE_SCOPE_VIOLATION_CODE,
                format!("task {task_id} changed files outside its write_scope"),
                json!({
                    "task_id": task_id,
                    "violations": violations,
                    "write_scope": contract.write_scope,
                    "must_not_touch": contract.must_not_touch
                }),
            ));
        }
        Ok(())
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
            "SELECT COUNT(*) FROM shared_claims WHERE status = 'active' AND lease_expires_at >= now()",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        let stale_claims = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM shared_claims WHERE status = 'active' AND lease_expires_at < now()",
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
            SELECT id, project_id, task_id, owner_id, scope_kind, scope_key, status,
                   acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            FROM shared_claims
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
                scope_kind: "write_scope".to_string(),
                scope_key: scope.to_string(),
                lease_secs: DEFAULT_LEASE_SECS,
                metadata: json!({
                    "accepted_shard_id": accepted_shard_id,
                    "source": "mission_task_delegate"
                }),
            };
            claims.push(self.claim(req).await?);
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

    async fn task_result_put(&self, args: &Value) -> Result<Value> {
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
        let content = if let Some(s) = string_arg(args, "content") {
            Value::String(s.to_string())
        } else {
            details.clone()
        };
        let raw_evidence = details
            .get("raw_evidence")
            .cloned()
            .or_else(|| details.get("rawEvidence").cloned());
        let evidence_refs = task_result_evidence_refs(
            args,
            &details,
            raw_evidence.as_ref(),
            &provider,
            slot_id.as_deref(),
            conversation_id.as_deref(),
        );
        let runtime_contract = self.task_runtime_contract(&task_id).await?;
        self.require_capability(&task_id, "settle", "task", &task_id)
            .await?;
        if is_completed_result_status(&result_status) {
            self.verify_completion_scope(&task_id, args, &details, &runtime_contract)
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
            "producer": {
                "kind": "worker-completion-producer",
                "provider": provider,
                "slot_id": slot_id,
                "conversation_id": conversation_id
            },
            "result_status": result_status,
            "result_kind": result_status,
            "summary": summary,
            "content": content,
            "details": details,
            "evidence_refs": evidence_refs,
            "raw_evidence": raw_evidence,
            "created_at": Utc::now().to_rfc3339()
        });
        let metadata = json!({
            "schema": "missiond.task-result-artifact.v1",
            "task_id": task_id,
            "project_id": project_id,
            "slot_id": slot_id,
            "conversation_id": conversation_id,
            "provider": provider,
            "result_status": result_status,
            "accepted_shard_id": string_arg(args, "accepted_shard_id").or_else(|| string_arg(args, "acceptedShardId"))
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
        sqlx::query(
            r#"
            INSERT INTO task_result_artifacts
              (id, artifact_hash, project_id, task_id, slot_id, conversation_id,
               provider, result_status, summary)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            ON CONFLICT(task_id, artifact_hash)
            DO UPDATE SET summary = EXCLUDED.summary
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
            SELECT id, artifact_hash, project_id, task_id, slot_id, conversation_id,
                   provider, result_status, summary, created_at
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
            SELECT id, artifact_hash, project_id, task_id, slot_id, conversation_id,
                   provider, result_status, summary, created_at
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

    async fn worker_settle(&self, args: &Value) -> Result<Value> {
        let task_id = string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .ok_or_else(|| anyhow!("task_id is required"))?;
        let project_id = string_arg(args, "project_id").or_else(|| string_arg(args, "projectId"));
        let slot_id = string_arg(args, "slot_id").or_else(|| string_arg(args, "slotId"));
        let conversation_id =
            string_arg(args, "conversation_id").or_else(|| string_arg(args, "conversationId"));
        let artifact_hash_owned = string_arg(args, "artifact_hash")
            .or_else(|| string_arg(args, "artifactHash"))
            .map(str::to_string);
        let target_status = normalize_worker_settle_status(string_arg(args, "status"))?;
        let note_summary = string_arg(args, "summary")
            .map(str::to_string)
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
        let _runtime_contract = self.task_runtime_contract(task_id).await?;
        self.require_capability(task_id, "settle", "task", task_id)
            .await?;

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

    async fn claim_from_args(&self, args: &Value) -> Result<Value> {
        let req = ClaimRequest {
            project_id: string_arg(args, "project_id")
                .or_else(|| string_arg(args, "projectId"))
                .map(str::to_string),
            task_id: string_arg(args, "task_id")
                .or_else(|| string_arg(args, "taskId"))
                .map(str::to_string),
            owner_id: string_arg(args, "owner_id")
                .or_else(|| string_arg(args, "ownerId"))
                .unwrap_or("unknown")
                .to_string(),
            scope_kind: string_arg(args, "scope_kind")
                .or_else(|| string_arg(args, "scopeKind"))
                .unwrap_or("write_scope")
                .to_string(),
            scope_key: string_arg(args, "scope_key")
                .or_else(|| string_arg(args, "scopeKey"))
                .ok_or_else(|| anyhow!("scope_key is required"))?
                .to_string(),
            lease_secs: args
                .get("lease_secs")
                .or_else(|| args.get("leaseSecs"))
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_LEASE_SECS),
            metadata: args.get("metadata").cloned().unwrap_or_else(|| json!({})),
        };
        self.claim(req).await
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
            SELECT id, project_id, task_id, owner_id, scope_kind, scope_key, status,
                   acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            FROM shared_claims
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
            tx.commit().await?;
            return Ok(json!({
                "schema": "missiond.shared-claim.v1",
                "ok": false,
                "status": "conflict",
                "code": CLAIM_CONFLICT_CODE,
                "error_code": CLAIM_CONFLICT_CODE,
                "conflict": claim_row_json(row)
            }));
        }

        let id = Uuid::new_v4().to_string();
        let lease_secs = req.lease_secs.clamp(30, MAX_LEASE_SECS);
        let lease_expires_at = Utc::now() + Duration::seconds(lease_secs);
        let row = sqlx::query(
            r#"
            INSERT INTO shared_claims
              (id, project_id, task_id, owner_id, scope_kind, scope_key, status,
               lease_expires_at, heartbeat_at, metadata)
            VALUES ($1,$2,$3,$4,$5,$6,'active',$7,now(),$8)
            RETURNING id, project_id, task_id, owner_id, scope_kind, scope_key, status,
                      acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            "#,
        )
        .bind(&id)
        .bind(req.project_id.as_deref())
        .bind(req.task_id.as_deref())
        .bind(req.owner_id.as_str())
        .bind(req.scope_kind.as_str())
        .bind(req.scope_key.as_str())
        .bind(lease_expires_at)
        .bind(req.metadata.clone())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO work_leases
              (id, project_id, task_id, holder_id, holder_kind, scope_kind, scope_key,
               lease_expires_at, metadata)
            VALUES ($1,$2,$3,$4,'worker',$5,$6,$7,$8)
            ON CONFLICT (scope_kind, scope_key) WHERE status = 'active'
            DO NOTHING
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

    async fn release(&self, args: &Value) -> Result<Value> {
        let id = string_arg(args, "claim_id")
            .or_else(|| string_arg(args, "claimId"))
            .or_else(|| string_arg(args, "id"))
            .ok_or_else(|| anyhow!("claim_id is required"))?;
        let owner_id = string_arg(args, "owner_id").or_else(|| string_arg(args, "ownerId"));
        let row = sqlx::query(
            r#"
            UPDATE shared_claims
            SET status = 'released', released_at = now()
            WHERE id = $1
              AND ($2::text IS NULL OR owner_id = $2)
            RETURNING id, project_id, task_id, owner_id, scope_kind, scope_key, status,
                      acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            "#,
        )
        .bind(id)
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.shared-claim-release.v1",
            "ok": row.is_some(),
            "claim": row.map(claim_row_json)
        }))
    }

    async fn heartbeat(&self, args: &Value) -> Result<Value> {
        let id = string_arg(args, "claim_id")
            .or_else(|| string_arg(args, "claimId"))
            .or_else(|| string_arg(args, "id"))
            .ok_or_else(|| anyhow!("claim_id is required"))?;
        let owner_id = string_arg(args, "owner_id").or_else(|| string_arg(args, "ownerId"));
        let lease_secs = args
            .get("lease_secs")
            .or_else(|| args.get("leaseSecs"))
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_LEASE_SECS)
            .clamp(30, MAX_LEASE_SECS);
        let lease_expires_at = Utc::now() + Duration::seconds(lease_secs);
        let row = sqlx::query(
            r#"
            UPDATE shared_claims
            SET lease_expires_at = $3, heartbeat_at = now()
            WHERE id = $1
              AND status = 'active'
              AND ($2::text IS NULL OR owner_id = $2)
            RETURNING id, project_id, task_id, owner_id, scope_kind, scope_key, status,
                      acquired_at, lease_expires_at, released_at, heartbeat_at, metadata
            "#,
        )
        .bind(id)
        .bind(owner_id)
        .bind(lease_expires_at)
        .fetch_optional(&self.pool)
        .await?;
        Ok(json!({
            "schema": "missiond.shared-claim-heartbeat.v1",
            "ok": row.is_some(),
            "claim": row.map(claim_row_json)
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
                    format!("artifact_hash {artifact_hash} is not a completed task-result-artifact for task {task_id}"),
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

fn validate_task_result_artifact_payload(
    task_id: &str,
    project_id: &str,
    provider: &str,
    result_status: &str,
    summary: &str,
    content: &Value,
    evidence_refs: &[Value],
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
    if evidence_refs.is_empty() {
        return Err(invalid("evidence_refs must be non-empty".to_string()));
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

fn metadata_string_value_any(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
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
        "slot_id": row.try_get::<Option<String>, _>("slot_id").ok().flatten(),
        "conversation_id": row.try_get::<Option<String>, _>("conversation_id").ok().flatten(),
        "provider": row.try_get::<Option<String>, _>("provider").ok().flatten(),
        "result_status": row.get::<String, _>("result_status"),
        "summary": row.get::<String, _>("summary"),
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
}
