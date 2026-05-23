//! DirectiveLayerStore — PostgreSQL implementation.
//!
//! Covers the directive → plan → workflow pipeline. Schema truth:
//! `migrations/20260420000000_directive_plan_workflow.sql`.
//!
//! `avg_cost_usd` is NUMERIC(10,4) in PG; we cast to `float8` on read and
//! bind as `f64` on write (relies on PG's implicit `double precision → numeric`
//! cast) to avoid pulling in `rust_decimal` / `bigdecimal`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::db::directive::{DIRECTIVE_COLS, PLAN_COLS, WORKFLOW_COLS_WITH_CAST};
use crate::db::error::DbResult;
use crate::db::traits::DirectiveLayerStore;
use crate::types::{Directive, DirectiveStatus, Plan, PlanStatus, Workflow};
use std::str::FromStr;

use super::PgMissionStore;

// ----------------------------------------------------------------------------
// Row tuple type aliases
// ----------------------------------------------------------------------------

type DirectiveRow = (
    Uuid,
    String,                // utterance_text
    String,                // sexp_text
    i32,                   // version
    String,                // status
    Option<String>,        // compiler_model
    Option<JsonValue>,     // references_json (JSONB)
    DateTime<Utc>,         // created_at
    Option<DateTime<Utc>>, // approved_at
);

fn directive_row_to_directive(r: DirectiveRow) -> Directive {
    Directive {
        id: r.0,
        utterance_text: r.1,
        sexp_text: r.2,
        version: r.3,
        status: DirectiveStatus::from_str(&r.4).unwrap_or(DirectiveStatus::Draft),
        compiler_model: r.5,
        references_json: r.6,
        created_at: r.7,
        approved_at: r.8,
    }
}

type PlanRow = (
    Uuid,
    String,                // board_task_id
    Option<Uuid>,          // source_directive_id
    i32,                   // version
    String,                // sexp_text
    String,                // sexp_hash
    String,                // status
    Option<String>,        // compiler_model
    Option<String>,        // compiled_from
    JsonValue,             // contract_json
    DateTime<Utc>,         // created_at
    Option<DateTime<Utc>>, // approved_at
    Option<DateTime<Utc>>, // finished_at
);

#[derive(sqlx::FromRow)]
struct LispCodeSyncJobRow {
    id: Uuid,
    project_id: String,
    root_path: String,
    changed_path: String,
    content_hash: String,
    event_kind: String,
    status: String,
    attempts: i32,
    next_run_at: DateTime<Utc>,
    lease_owner: Option<String>,
    lease_expires_at: Option<DateTime<Utc>>,
    checker_ok: Option<bool>,
    checker_command: Option<String>,
    checker_tail: Option<String>,
    sync_task_id: Option<String>,
    dedupe_key: String,
    storm_circuit: bool,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const LISP_CODE_SYNC_JOB_COLS: &str = "job.id, job.project_id, job.root_path, job.changed_path, job.content_hash, job.event_kind, job.status, job.attempts, job.next_run_at, job.lease_owner, job.lease_expires_at, job.checker_ok, job.checker_command, job.checker_tail, job.sync_task_id, job.dedupe_key, job.storm_circuit, job.last_error, job.created_at, job.updated_at";

fn lisp_code_sync_job_row_to_job(r: LispCodeSyncJobRow) -> crate::types::LispCodeSyncJob {
    crate::types::LispCodeSyncJob {
        id: r.id,
        project_id: r.project_id,
        root_path: r.root_path,
        changed_path: r.changed_path,
        content_hash: r.content_hash,
        event_kind: r.event_kind,
        status: r.status,
        attempts: r.attempts,
        next_run_at: r.next_run_at,
        lease_owner: r.lease_owner,
        lease_expires_at: r.lease_expires_at,
        checker_ok: r.checker_ok,
        checker_command: r.checker_command,
        checker_tail: r.checker_tail,
        sync_task_id: r.sync_task_id,
        dedupe_key: r.dedupe_key,
        storm_circuit: r.storm_circuit,
        last_error: r.last_error,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

fn plan_row_to_plan(r: PlanRow) -> Plan {
    Plan {
        id: r.0,
        board_task_id: r.1,
        source_directive_id: r.2,
        version: r.3,
        sexp_text: r.4,
        sexp_hash: r.5,
        status: PlanStatus::from_str(&r.6).unwrap_or(PlanStatus::Draft),
        compiler_model: r.7,
        compiled_from: r.8,
        contract_json: r.9,
        created_at: r.10,
        approved_at: r.11,
        finished_at: r.12,
    }
}

type WorkflowRow = (
    Uuid,
    String,                // name
    String,                // sexp_text
    JsonValue,             // match_rules (JSONB)
    Option<Uuid>,          // learned_from
    i32,                   // executions
    i32,                   // success_count
    Option<f64>,           // avg_cost_usd (cast to float8)
    Option<DateTime<Utc>>, // last_used_at
    DateTime<Utc>,         // created_at
);

fn workflow_row_to_workflow(r: WorkflowRow) -> Workflow {
    Workflow {
        id: r.0,
        name: r.1,
        sexp_text: r.2,
        match_rules: r.3,
        learned_from: r.4,
        executions: r.5,
        success_count: r.6,
        avg_cost_usd: r.7,
        last_used_at: r.8,
        created_at: r.9,
    }
}

// ----------------------------------------------------------------------------
// DirectiveLayerStore impl
// ----------------------------------------------------------------------------

#[cfg(feature = "postgres")]
#[async_trait]
impl DirectiveLayerStore for PgMissionStore {
    // ================================================================
    // directive 表 (6 方法)
    // ================================================================

    async fn directive_insert(
        &self,
        utterance_text: &str,
        sexp_text: &str,
        version: i32,
        status: DirectiveStatus,
        compiler_model: Option<&str>,
        references_json: Option<&JsonValue>,
    ) -> DbResult<Uuid> {
        let references_str = references_json
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()));

        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO directive (utterance_text, sexp_text, version, status, compiler_model, references_json)
             VALUES ($1, $2, $3, $4, $5, $6::jsonb)
             RETURNING id",
        )
        .bind(utterance_text)
        .bind(sexp_text)
        .bind(version)
        .bind(status.as_str())
        .bind(compiler_model)
        .bind(references_str)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn directive_get(&self, id: Uuid, version: i32) -> DbResult<Option<Directive>> {
        let row: Option<DirectiveRow> = sqlx::query_as(&format!(
            "SELECT {} FROM directive WHERE id = $1 AND version = $2",
            DIRECTIVE_COLS
        ))
        .bind(id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(directive_row_to_directive))
    }

    async fn directive_update_status(
        &self,
        id: Uuid,
        version: i32,
        new_status: DirectiveStatus,
    ) -> DbResult<()> {
        sqlx::query("UPDATE directive SET status = $3 WHERE id = $1 AND version = $2")
            .bind(id)
            .bind(version)
            .bind(new_status.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn directive_approve(&self, id: Uuid, version: i32) -> DbResult<()> {
        sqlx::query(
            "UPDATE directive SET status = 'approved', approved_at = NOW()
             WHERE id = $1 AND version = $2",
        )
        .bind(id)
        .bind(version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn directive_list_by_status(
        &self,
        status: DirectiveStatus,
        limit: i64,
    ) -> DbResult<Vec<Directive>> {
        let rows: Vec<DirectiveRow> = sqlx::query_as(&format!(
            "SELECT {} FROM directive WHERE status = $1 ORDER BY created_at DESC LIMIT $2",
            DIRECTIVE_COLS
        ))
        .bind(status.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(directive_row_to_directive).collect())
    }

    async fn directive_get_version_chain(&self, id: Uuid) -> DbResult<Vec<Directive>> {
        let rows: Vec<DirectiveRow> = sqlx::query_as(&format!(
            "SELECT {} FROM directive WHERE id = $1 ORDER BY version ASC",
            DIRECTIVE_COLS
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(directive_row_to_directive).collect())
    }

    async fn directive_list_recent(
        &self,
        status: Option<DirectiveStatus>,
        limit: i64,
    ) -> DbResult<Vec<Directive>> {
        let rows: Vec<DirectiveRow> = match status {
            Some(s) => {
                sqlx::query_as(&format!(
                    "SELECT {} FROM directive WHERE status = $1 ORDER BY created_at DESC LIMIT $2",
                    DIRECTIVE_COLS
                ))
                .bind(s.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as(&format!(
                    "SELECT {} FROM directive ORDER BY created_at DESC LIMIT $1",
                    DIRECTIVE_COLS
                ))
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(directive_row_to_directive).collect())
    }

    // ================================================================
    // plan 表 (6 方法)
    // ================================================================

    async fn plan_insert(
        &self,
        board_task_id: &str,
        source_directive_id: Option<Uuid>,
        version: i32,
        sexp_text: &str,
        sexp_hash: &str,
        status: PlanStatus,
        compiler_model: Option<&str>,
        compiled_from: Option<&str>,
    ) -> DbResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO plan (board_task_id, source_directive_id, version, sexp_text, sexp_hash, status, compiler_model, compiled_from, contract_json)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, '{}'::jsonb)
             RETURNING id",
        )
        .bind(board_task_id)
        .bind(source_directive_id)
        .bind(version)
        .bind(sexp_text)
        .bind(sexp_hash)
        .bind(status.as_str())
        .bind(compiler_model)
        .bind(compiled_from)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn plan_get(&self, id: Uuid) -> DbResult<Option<Plan>> {
        let row: Option<PlanRow> =
            sqlx::query_as(&format!("SELECT {} FROM plan WHERE id = $1", PLAN_COLS))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(plan_row_to_plan))
    }

    async fn plan_update_contract_json(&self, id: Uuid, contract_json: &JsonValue) -> DbResult<()> {
        let contract_str =
            serde_json::to_string(contract_json).unwrap_or_else(|_| "{}".to_string());
        sqlx::query("UPDATE plan SET contract_json = $2::jsonb WHERE id = $1")
            .bind(id)
            .bind(contract_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn plan_update_status(&self, id: Uuid, new_status: PlanStatus) -> DbResult<()> {
        // When transitioning to a terminal state, stamp finished_at.
        let is_terminal = matches!(
            new_status,
            PlanStatus::Succeeded | PlanStatus::Failed | PlanStatus::Superseded
        );
        if is_terminal {
            sqlx::query(
                "UPDATE plan SET status = $2, finished_at = COALESCE(finished_at, NOW())
                 WHERE id = $1",
            )
            .bind(id)
            .bind(new_status.as_str())
            .execute(&self.pool)
            .await?;
        } else if matches!(new_status, PlanStatus::Approved) {
            sqlx::query(
                "UPDATE plan SET status = $2, approved_at = COALESCE(approved_at, NOW())
                 WHERE id = $1",
            )
            .bind(id)
            .bind(new_status.as_str())
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("UPDATE plan SET status = $2 WHERE id = $1")
                .bind(id)
                .bind(new_status.as_str())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn plan_supersede(&self, old_id: Uuid, _new_id: Uuid) -> DbResult<()> {
        sqlx::query(
            "UPDATE plan SET status = 'superseded', finished_at = COALESCE(finished_at, NOW())
             WHERE id = $1",
        )
        .bind(old_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn plan_list_by_task(&self, board_task_id: &str) -> DbResult<Vec<Plan>> {
        let rows: Vec<PlanRow> = sqlx::query_as(&format!(
            "SELECT {} FROM plan WHERE board_task_id = $1 ORDER BY version DESC",
            PLAN_COLS
        ))
        .bind(board_task_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(plan_row_to_plan).collect())
    }

    async fn plan_get_latest(&self, board_task_id: &str) -> DbResult<Option<Plan>> {
        let row: Option<PlanRow> = sqlx::query_as(&format!(
            "SELECT {} FROM plan WHERE board_task_id = $1 ORDER BY version DESC LIMIT 1",
            PLAN_COLS
        ))
        .bind(board_task_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(plan_row_to_plan))
    }

    async fn plan_list_recent(
        &self,
        status: Option<PlanStatus>,
        limit: i64,
    ) -> DbResult<Vec<Plan>> {
        let rows: Vec<PlanRow> = match status {
            Some(s) => {
                sqlx::query_as(&format!(
                    "SELECT {} FROM plan WHERE status = $1 ORDER BY created_at DESC LIMIT $2",
                    PLAN_COLS
                ))
                .bind(s.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as(&format!(
                    "SELECT {} FROM plan ORDER BY created_at DESC LIMIT $1",
                    PLAN_COLS
                ))
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(plan_row_to_plan).collect())
    }

    async fn lisp_code_sync_enqueue_job(
        &self,
        input: &crate::types::EnqueueLispCodeSyncJob,
    ) -> DbResult<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO lisp_code_sync_jobs
                (project_id, root_path, changed_path, content_hash, event_kind, dedupe_key, storm_circuit)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (dedupe_key) DO UPDATE SET
                project_id = EXCLUDED.project_id,
                root_path = EXCLUDED.root_path,
                changed_path = EXCLUDED.changed_path,
                content_hash = EXCLUDED.content_hash,
                event_kind = EXCLUDED.event_kind,
                status = CASE
                    WHEN lisp_code_sync_jobs.status IN ('synced', 'cancelled') THEN 'queued'
                    WHEN lisp_code_sync_jobs.status = 'failed' THEN 'queued'
                    ELSE lisp_code_sync_jobs.status
                END,
                next_run_at = LEAST(lisp_code_sync_jobs.next_run_at, now()),
                checker_ok = NULL,
                checker_command = NULL,
                checker_tail = NULL,
                storm_circuit = EXCLUDED.storm_circuit,
                last_error = NULL,
                updated_at = now()
             RETURNING id",
        )
        .bind(&input.project_id)
        .bind(&input.root_path)
        .bind(&input.changed_path)
        .bind(&input.content_hash)
        .bind(&input.event_kind)
        .bind(&input.dedupe_key)
        .bind(input.storm_circuit)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn lisp_code_sync_claim_due_jobs(
        &self,
        lease_owner: &str,
        limit: i64,
        lease_secs: i64,
    ) -> DbResult<Vec<crate::types::LispCodeSyncJob>> {
        let rows: Vec<LispCodeSyncJobRow> = sqlx::query_as(&format!(
            "WITH due AS (
                SELECT id
                FROM lisp_code_sync_jobs
                WHERE next_run_at <= now()
                  AND (
                    status IN ('queued', 'failed')
                    OR (status = 'running' AND lease_expires_at < now())
                  )
                ORDER BY next_run_at ASC, created_at ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
             )
             UPDATE lisp_code_sync_jobs AS job
             SET status = 'running',
                 attempts = attempts + 1,
                 lease_owner = $1,
                 lease_expires_at = now() + ($3::text || ' seconds')::interval,
                 updated_at = now()
             FROM due
             WHERE job.id = due.id
             RETURNING {}",
            LISP_CODE_SYNC_JOB_COLS
        ))
        .bind(lease_owner)
        .bind(limit.max(1))
        .bind(lease_secs.max(1))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(lisp_code_sync_job_row_to_job)
            .collect())
    }

    async fn lisp_code_sync_complete_job(
        &self,
        id: Uuid,
        status: &str,
        checker_ok: Option<bool>,
        checker_command: Option<&str>,
        checker_tail: Option<&str>,
        sync_task_id: Option<&str>,
        last_error: Option<&str>,
        retry_after_secs: Option<i64>,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE lisp_code_sync_jobs
             SET status = $2,
                 checker_ok = $3,
                 checker_command = $4,
                 checker_tail = $5,
                 sync_task_id = $6,
                 last_error = $7,
                 next_run_at = CASE
                    WHEN $8::bigint IS NULL THEN next_run_at
                    ELSE now() + ($8::text || ' seconds')::interval
                 END,
                 lease_owner = NULL,
                 lease_expires_at = NULL,
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(checker_ok)
        .bind(checker_command)
        .bind(checker_tail)
        .bind(sync_task_id)
        .bind(last_error)
        .bind(retry_after_secs)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn lisp_code_sync_queue_stats(&self) -> DbResult<crate::types::LispCodeSyncQueueStats> {
        let counts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, COUNT(*)::bigint FROM lisp_code_sync_jobs GROUP BY status",
        )
        .fetch_all(&self.pool)
        .await?;
        let due: (i64, Option<i64>) = sqlx::query_as(
            "SELECT COUNT(*)::bigint,
                    EXTRACT(EPOCH FROM (now() - MIN(next_run_at)))::bigint
             FROM lisp_code_sync_jobs
             WHERE next_run_at <= now() AND status IN ('queued', 'failed')",
        )
        .fetch_one(&self.pool)
        .await?;
        let active_leases: (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint FROM lisp_code_sync_jobs
             WHERE status = 'running' AND lease_expires_at > now()",
        )
        .fetch_one(&self.pool)
        .await?;
        let last: Option<(String,)> = sqlx::query_as(
            "SELECT CONCAT(status, COALESCE(': ' || last_error, ''))
             FROM lisp_code_sync_jobs
             WHERE status IN ('synced', 'failed', 'observed-only', 'unknown-project')
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let mut stats = crate::types::LispCodeSyncQueueStats {
            due: due.0,
            oldest_due_age_secs: due.1,
            active_leases: active_leases.0,
            batch_last_result: last.map(|row| row.0),
            ..Default::default()
        };
        for (status, count) in counts {
            match status.as_str() {
                "queued" => stats.queued = count,
                "running" => stats.running = count,
                "failed" => stats.failed = count,
                _ => {}
            }
        }
        Ok(stats)
    }

    // ================================================================
    // workflow 表 (5 方法)
    // ================================================================

    async fn workflow_insert(
        &self,
        name: &str,
        sexp_text: &str,
        match_rules: &JsonValue,
        learned_from: Option<Uuid>,
    ) -> DbResult<Uuid> {
        let match_rules_str =
            serde_json::to_string(match_rules).unwrap_or_else(|_| "{}".to_string());
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO workflow (name, sexp_text, match_rules, learned_from)
             VALUES ($1, $2, $3::jsonb, $4)
             RETURNING id",
        )
        .bind(name)
        .bind(sexp_text)
        .bind(match_rules_str)
        .bind(learned_from)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn workflow_get_by_name(&self, name: &str) -> DbResult<Option<Workflow>> {
        let row: Option<WorkflowRow> = sqlx::query_as(&format!(
            "SELECT {} FROM workflow WHERE name = $1",
            WORKFLOW_COLS_WITH_CAST
        ))
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(workflow_row_to_workflow))
    }

    async fn workflow_get_by_id(&self, id: Uuid) -> DbResult<Option<Workflow>> {
        let row: Option<WorkflowRow> = sqlx::query_as(&format!(
            "SELECT {} FROM workflow WHERE id = $1",
            WORKFLOW_COLS_WITH_CAST
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(workflow_row_to_workflow))
    }

    async fn workflow_find_by_match(&self, query_utterance: &str) -> DbResult<Vec<Workflow>> {
        // Simplified: substring match over the serialized JSONB representation.
        // Callers are expected to refine this via real match_rules keys later.
        let pattern = format!("%{}%", query_utterance);
        let rows: Vec<WorkflowRow> = sqlx::query_as(&format!(
            "SELECT {} FROM workflow
             WHERE match_rules::text ILIKE $1
             ORDER BY executions DESC, success_count DESC
             LIMIT 50",
            WORKFLOW_COLS_WITH_CAST
        ))
        .bind(pattern)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(workflow_row_to_workflow).collect())
    }

    async fn workflow_record_execution(
        &self,
        id: Uuid,
        success: bool,
        cost_usd: Option<f64>,
    ) -> DbResult<()> {
        // Rolling average: new_avg = (old_avg * old_exec + cost) / (old_exec + 1).
        // Done inline in SQL to stay atomic.
        sqlx::query(
            "UPDATE workflow
             SET executions     = executions + 1,
                 success_count  = success_count + CASE WHEN $2 THEN 1 ELSE 0 END,
                 avg_cost_usd   = CASE
                     WHEN $3::float8 IS NULL THEN avg_cost_usd
                     WHEN avg_cost_usd IS NULL THEN $3::float8::numeric
                     ELSE ((avg_cost_usd * executions) + $3::float8) / (executions + 1)
                 END,
                 last_used_at   = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(success)
        .bind(cost_usd)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn workflow_list_top_n(&self, n: i64) -> DbResult<Vec<Workflow>> {
        let rows: Vec<WorkflowRow> = sqlx::query_as(&format!(
            "SELECT {} FROM workflow
             ORDER BY executions DESC, success_count DESC, last_used_at DESC NULLS LAST
             LIMIT $1",
            WORKFLOW_COLS_WITH_CAST
        ))
        .bind(n)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(workflow_row_to_workflow).collect())
    }
}
