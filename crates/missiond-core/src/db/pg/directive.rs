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
    String,                  // utterance_text
    String,                  // sexp_text
    i32,                     // version
    String,                  // status
    Option<String>,          // compiler_model
    Option<JsonValue>,       // references_json (JSONB)
    DateTime<Utc>,           // created_at
    Option<DateTime<Utc>>,   // approved_at
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
    String,                  // board_task_id
    Option<Uuid>,            // source_directive_id
    i32,                     // version
    String,                  // sexp_text
    String,                  // sexp_hash
    String,                  // status
    Option<String>,          // compiler_model
    Option<String>,          // compiled_from
    DateTime<Utc>,           // created_at
    Option<DateTime<Utc>>,   // approved_at
    Option<DateTime<Utc>>,   // finished_at
);

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
        created_at: r.9,
        approved_at: r.10,
        finished_at: r.11,
    }
}

type WorkflowRow = (
    Uuid,
    String,                  // name
    String,                  // sexp_text
    JsonValue,               // match_rules (JSONB)
    Option<Uuid>,            // learned_from
    i32,                     // executions
    i32,                     // success_count
    Option<f64>,             // avg_cost_usd (cast to float8)
    Option<DateTime<Utc>>,   // last_used_at
    DateTime<Utc>,           // created_at
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
        sqlx::query(
            "UPDATE directive SET status = $3 WHERE id = $1 AND version = $2",
        )
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
            "INSERT INTO plan (board_task_id, source_directive_id, version, sexp_text, sexp_hash, status, compiler_model, compiled_from)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
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
        let row: Option<PlanRow> = sqlx::query_as(&format!(
            "SELECT {} FROM plan WHERE id = $1",
            PLAN_COLS
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(plan_row_to_plan))
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
        let match_rules_str = serde_json::to_string(match_rules).unwrap_or_else(|_| "{}".to_string());
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
