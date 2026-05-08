use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use missiond_core::event::events::SystemEvent;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::bus::BusServices;

const DEFAULT_QUERY_LIMIT: i64 = 50;
const MAX_QUERY_LIMIT: i64 = 500;
const DEFAULT_LEASE_SECS: i64 = 1800;
const MAX_LEASE_SECS: i64 = 7200;

#[derive(Clone)]
pub(crate) struct SharedMemoryService {
    pool: PgPool,
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

impl SharedMemoryService {
    pub(crate) fn new(pool: PgPool, bus: Arc<BusServices>, missiond_root: PathBuf) -> Self {
        Self {
            pool,
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
            "claim" => self.claim_from_args(args).await,
            "release" => self.release(args).await,
            "heartbeat" => self.heartbeat(args).await,
            "cursor" => self.cursor(args).await,
            other => Err(anyhow!("unknown shared memory action: {other}")),
        }
    }

    pub(crate) async fn status_snapshot(&self) -> Value {
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
            "artifactCount": artifacts,
            "cursorLag": cursor_lag
        })
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
        let compiled = self.read_compiled_json("compiled-semantic-ir.json").ok();
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
                "artifactStore": "shared_artifacts"
            },
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
        let payload = args
            .get("payload")
            .cloned()
            .unwrap_or_else(|| json!({}));
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
        let metadata = args
            .get("metadata")
            .cloned()
            .unwrap_or_else(|| json!({}));
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

        Ok(json!({
            "schema": "missiond.shared-artifact.v1",
            "hash": hash,
            "kind": kind,
            "size_bytes": size_bytes,
            "media_type": media_type
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
            None => Ok(json!({"schema": "missiond.shared-artifact.v1", "found": false, "hash": hash})),
        }
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
            metadata: args
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| json!({})),
        };
        self.claim(req).await
    }

    async fn claim(&self, req: ClaimRequest) -> Result<Value> {
        self.expire_stale_claims().await?;
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
            "#,
        )
        .bind(&req.scope_kind)
        .bind(&req.scope_key)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = active {
            return Ok(json!({
                "schema": "missiond.shared-claim.v1",
                "ok": false,
                "status": "conflict",
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
        .bind(req.project_id)
        .bind(req.task_id)
        .bind(req.owner_id)
        .bind(req.scope_kind)
        .bind(req.scope_key)
        .bind(lease_expires_at)
        .bind(req.metadata)
        .fetch_one(&self.pool)
        .await?;
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
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn bounded_limit(args: &Value) -> i64 {
    args.get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .clamp(1, MAX_QUERY_LIMIT)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
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
        value["content"] = json!(String::from_utf8_lossy(&bytes).to_string());
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

#[allow(dead_code)]
fn normalize_path(path: &str) -> String {
    Path::new(path).to_string_lossy().to_string()
}
