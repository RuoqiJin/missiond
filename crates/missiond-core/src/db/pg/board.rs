//! BoardStore — PostgreSQL implementation.
//! Covers board task CRUD, claiming, dependencies, notes, search, and agent questions.

use super::PgMissionStore;
use crate::db::error::{DbError, DbResult};
use crate::db::traits::BoardStore;
use crate::types::*;
use async_trait::async_trait;
use sqlx::Row;

const MAX_BOARD_TASK_DESCRIPTION_BYTES: usize = 50_000;

fn compact_board_task_description(raw: &str) -> String {
    if raw.len() <= MAX_BOARD_TASK_DESCRIPTION_BYTES {
        return raw.to_string();
    }
    tracing::warn!(
        len = raw.len(),
        max = MAX_BOARD_TASK_DESCRIPTION_BYTES,
        "Board task description exceeds limit, truncating"
    );
    let end = crate::embedding::char_boundary_at(raw, MAX_BOARD_TASK_DESCRIPTION_BYTES);
    format!("{}...(truncated from {} bytes)", &raw[..end], raw.len())
}

fn board_create_runtime_metadata(
    task_id: &TaskId,
    input: &CreateBoardTaskInput,
) -> serde_json::Value {
    let mut metadata = input
        .runtime_metadata
        .clone()
        .filter(|value| value.as_object().is_some_and(|fields| !fields.is_empty()))
        .unwrap_or_else(|| serde_json::json!({}));
    if !metadata.is_object() {
        metadata = serde_json::json!({});
    }
    let fields = metadata.as_object_mut().expect("metadata object");
    fields
        .entry("schema".to_string())
        .or_insert_with(|| serde_json::json!("missiond.board-task-runtime-metadata.v1"));
    fields
        .entry("source".to_string())
        .or_insert_with(|| serde_json::json!("mission_board_create"));
    fields
        .entry("control_state".to_string())
        .or_insert_with(|| serde_json::json!("runtime_metadata"));
    fields
        .entry("task_contract_id".to_string())
        .or_insert_with(|| serde_json::json!(format!("board-task:{}", task_id.as_str())));
    fields
        .entry("dispatch_metadata".to_string())
        .or_insert_with(|| {
            serde_json::json!({
                "task_class": input
                    .context_intent
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(input.category.as_deref().unwrap_or("general"))
            })
        });
    fields
        .entry("read_scope".to_string())
        .or_insert_with(|| serde_json::json!([]));
    fields
        .entry("write_scope".to_string())
        .or_insert_with(|| serde_json::json!([]));
    fields
        .entry("must_not_touch".to_string())
        .or_insert_with(|| serde_json::json!([]));
    fields
        .entry("capability_grant_ids".to_string())
        .or_insert_with(|| serde_json::json!([]));
    fields
        .entry("sandbox_profile".to_string())
        .or_insert_with(|| serde_json::json!("read-only"));
    fields
        .entry("projection_policy".to_string())
        .or_insert_with(|| serde_json::json!("description_notes_are_projection_only"));
    metadata
}

#[cfg(feature = "postgres")]
async fn resolve_existing_board_task_id(
    store: &PgMissionStore,
    field: &'static str,
    raw: &str,
) -> DbResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DbError::Constraint(format!("{field} must be non-empty")));
    }
    match BoardStore::resolve_board_task_id(store, trimmed).await? {
        Some(id) => Ok(id),
        None => Err(DbError::Constraint(format!(
            "{field} references unknown BoardTask id `{trimmed}`"
        ))),
    }
}

#[cfg(feature = "postgres")]
async fn require_task_completion_evidence(
    store: &PgMissionStore,
    task_id: &str,
    artifact_hash: Option<&str>,
) -> DbResult<()> {
    let Some(artifact_hash) = artifact_hash.map(str::trim).filter(|hash| !hash.is_empty()) else {
        return Err(DbError::EvidenceRequired {
            task_id: task_id.to_string(),
        });
    };
    let has_evidence = sqlx::query_scalar::<_, bool>(
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
    .fetch_one(&store.pool)
    .await?;
    if has_evidence {
        Ok(())
    } else {
        Err(DbError::EvidenceRequired {
            task_id: task_id.to_string(),
        })
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl BoardStore for PgMissionStore {
    // ========== board.rs: task CRUD ==========

    async fn insert_board_task(&self, task: &BoardTask) -> DbResult<()> {
        let depends_on_json =
            serde_json::to_string(&task.depends_on).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "INSERT INTO board_tasks (id, title, description, status, priority, category, project, server, due_date, parent_id, assignee, auto_execute, prompt_template, hidden, retry_count, max_retries, order_idx, created_at, updated_at, depends_on, dedupe_key, timeout_secs, context_intent, trigger_source, runtime_metadata)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)"
        )
        .bind(task.id.as_str())
        .bind(&task.title)
        .bind(&task.description)
        .bind(task.status.as_str())
        .bind(&task.priority)
        .bind(&task.category)
        .bind(&task.project)
        .bind(&task.server)
        .bind(&task.due_date)
        .bind(task.parent_id.as_ref().map(|p| p.as_str()))
        .bind(&task.assignee)
        .bind(task.auto_execute as i32)
        .bind(&task.prompt_template)
        .bind(task.hidden as i32)
        .bind(task.retry_count)
        .bind(task.max_retries)
        .bind(task.order_idx)
        .bind(&task.created_at)
        .bind(&task.updated_at)
        .bind(&depends_on_json)
        .bind(&task.dedupe_key)
        .bind(task.timeout_secs)
        .bind(&task.context_intent)
        .bind(&task.trigger_source)
        .bind(&task.runtime_metadata)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_board_task(&self, input: &CreateBoardTaskInput) -> DbResult<BoardTask> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = TaskId::new();

        let description = match &input.description {
            Some(d) => compact_board_task_description(d),
            None => String::new(),
        };

        // Resolve parent_id before sibling ordering so short IDs and aliases
        // cannot leak raw strings into Board hierarchy state.
        let resolved_parent_id = if let Some(ref raw) = input.parent_id {
            Some(TaskId::from_trusted(
                resolve_existing_board_task_id(self, "parentId", raw).await?,
            ))
        } else {
            None
        };

        // Get max order for siblings
        let max_order: i64 = if let Some(ref pid) = resolved_parent_id {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id = $1",
            )
            .bind(pid.as_str())
            .fetch_optional(&self.pool)
            .await?;
            row.map(|r| r.0).unwrap_or(-1)
        } else {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id IS NULL",
            )
            .fetch_optional(&self.pool)
            .await?;
            row.map(|r| r.0).unwrap_or(-1)
        };

        // Resolve depends_on
        let mut resolved_depends_on: Vec<TaskId> = Vec::new();
        if let Some(ref deps) = input.depends_on {
            for raw in deps {
                let full = resolve_existing_board_task_id(self, "dependsOn", raw).await?;
                if !resolved_depends_on.iter().any(|d| d.as_str() == full) {
                    resolved_depends_on.push(TaskId::from_trusted(full));
                }
            }
        }

        let runtime_metadata = board_create_runtime_metadata(&id, input);
        let task = BoardTask {
            id,
            title: input.title.clone(),
            description,
            status: BoardTaskStatus::Open,
            priority: input
                .priority
                .clone()
                .unwrap_or_else(|| "medium".to_string()),
            category: input
                .category
                .clone()
                .unwrap_or_else(|| "other".to_string()),
            project: input.project.clone(),
            server: input.server.clone(),
            due_date: input.due_date.clone(),
            parent_id: resolved_parent_id,
            assignee: input.assignee.clone(),
            auto_execute: input.auto_execute.unwrap_or(false),
            prompt_template: input.prompt_template.clone(),
            hidden: input.hidden.unwrap_or(false),
            retry_count: 0,
            max_retries: 2,
            order_idx: max_order + 1,
            created_at: now.clone(),
            updated_at: now,
            claim_executor_id: None,
            claim_executor_type: None,
            claimed_at: None,
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: resolved_depends_on,
            lease_expires_at: None,
            dedupe_key: input.dedupe_key.clone(),
            timeout_secs: input.timeout_secs,
            context_intent: input.context_intent.clone(),
            trigger_source: None,
            runtime_metadata,
            notes_count: 0,
        };

        self.insert_board_task(&task).await?;
        Ok(task)
    }

    async fn get_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id).await? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let row = sqlx::query_as::<_, PgBoardTaskRow>(
            "SELECT *, (SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count FROM board_tasks WHERE id = $1"
        )
        .bind(&full_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.into_board_task()))
    }

    async fn list_board_tasks(
        &self,
        status: Option<&str>,
        include_hidden: bool,
    ) -> DbResult<Vec<BoardTask>> {
        let rows = if let Some(s) = status {
            if include_hidden {
                sqlx::query_as::<_, PgBoardTaskRow>(
                    "SELECT *, (SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count FROM board_tasks WHERE status = $1 ORDER BY order_idx ASC"
                )
                .bind(s)
                .fetch_all(&self.pool)
                .await?
            } else {
                sqlx::query_as::<_, PgBoardTaskRow>(
                    "SELECT *, (SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count FROM board_tasks WHERE status = $1 AND hidden = 0 ORDER BY order_idx ASC"
                )
                .bind(s)
                .fetch_all(&self.pool)
                .await?
            }
        } else if include_hidden {
            sqlx::query_as::<_, PgBoardTaskRow>(
                "SELECT *, (SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count FROM board_tasks ORDER BY order_idx ASC"
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, PgBoardTaskRow>(
                "SELECT *, (SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count FROM board_tasks WHERE hidden = 0 ORDER BY order_idx ASC"
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(|r| r.into_board_task()).collect())
    }

    async fn list_board_tasks_by_trigger(
        &self,
        trigger_source: &str,
        status: &str,
        limit: i64,
    ) -> DbResult<Vec<BoardTask>> {
        let rows = sqlx::query_as::<_, PgBoardTaskRow>(
            "SELECT *, (SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count
             FROM board_tasks
             WHERE trigger_source = $1 AND status = $2
             ORDER BY order_idx ASC, created_at ASC
             LIMIT $3"
        )
        .bind(trigger_source)
        .bind(status)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_board_task()).collect())
    }

    async fn update_board_task(
        &self,
        id: &str,
        update: &UpdateBoardTaskInput,
    ) -> DbResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id).await? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let now = chrono::Utc::now().to_rfc3339();
        if update
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("done"))
        {
            require_task_completion_evidence(self, &full_id, update.artifact_hash.as_deref())
                .await?;
        }

        // Build dynamic SET clause with numbered params
        let mut set_parts: Vec<String> = Vec::new();
        let mut param_idx: usize = 0;
        // We use a Vec of boxed dyn Encode values via manual binding below.
        // Since sqlx doesn't support dynamic param lists easily, we build
        // the full SQL and use raw query with manual binds.

        // Collect all (column, value) pairs to update
        struct UpdateField {
            column: &'static str,
            value: String,
        }
        let mut fields: Vec<UpdateField> = Vec::new();

        fields.push(UpdateField {
            column: "updated_at",
            value: now.clone(),
        });

        if let Some(ref v) = update.title {
            fields.push(UpdateField {
                column: "title",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.description {
            fields.push(UpdateField {
                column: "description",
                value: compact_board_task_description(v),
            });
        }
        if let Some(ref v) = update.status {
            fields.push(UpdateField {
                column: "status",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.priority {
            fields.push(UpdateField {
                column: "priority",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.category {
            fields.push(UpdateField {
                column: "category",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.project {
            fields.push(UpdateField {
                column: "project",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.server {
            fields.push(UpdateField {
                column: "server",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.due_date {
            fields.push(UpdateField {
                column: "due_date",
                value: v.clone(),
            });
        }
        if let Some(ref raw_pid) = update.parent_id {
            let resolved = resolve_existing_board_task_id(self, "parentId", raw_pid).await?;
            if resolved == full_id {
                return Err(DbError::Constraint(
                    "parentId cannot reference the task itself".to_string(),
                ));
            }
            fields.push(UpdateField {
                column: "parent_id",
                value: resolved,
            });
        }
        if let Some(ref v) = update.assignee {
            fields.push(UpdateField {
                column: "assignee",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.prompt_template {
            fields.push(UpdateField {
                column: "prompt_template",
                value: v.clone(),
            });
        }

        if let Some(ref eid) = update.claim_executor_id {
            fields.push(UpdateField {
                column: "claim_executor_id",
                value: eid.clone(),
            });
        }
        if let Some(ref etype) = update.claim_executor_type {
            fields.push(UpdateField {
                column: "claim_executor_type",
                value: etype.clone(),
            });
        }

        if let Some(ref v) = update.flow_phase {
            fields.push(UpdateField {
                column: "flow_phase",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.flow_context {
            fields.push(UpdateField {
                column: "flow_context",
                value: v.clone(),
            });
        }
        if let Some(ref v) = update.flow_template {
            fields.push(UpdateField {
                column: "flow_template",
                value: v.clone(),
            });
        }

        if let Some(ref deps) = update.depends_on {
            let mut resolved = Vec::new();
            for raw in deps {
                let full = resolve_existing_board_task_id(self, "dependsOn", raw).await?;
                if full == full_id {
                    return Err(DbError::Constraint(
                        "dependsOn cannot reference the task itself".to_string(),
                    ));
                }
                if !resolved.iter().any(|id| id == &full) {
                    resolved.push(full);
                }
            }
            let json = serde_json::to_string(&resolved).unwrap_or_else(|_| "[]".to_string());
            fields.push(UpdateField {
                column: "depends_on",
                value: json,
            });
        }

        // Build SET clause from fields
        for f in &fields {
            param_idx += 1;
            set_parts.push(format!("{} = ${}", f.column, param_idx));
        }

        // Handle auto_execute (bool -> int)
        let auto_exec_val: Option<i32> = update.auto_execute.map(|b| b as i32);
        if auto_exec_val.is_some() {
            param_idx += 1;
            set_parts.push(format!("auto_execute = ${}", param_idx));
        }

        // Handle hidden (bool -> int)
        let hidden_val: Option<i32> = update.hidden.map(|b| b as i32);
        if hidden_val.is_some() {
            param_idx += 1;
            set_parts.push(format!("hidden = ${}", param_idx));
        }

        // Handle order_idx
        if update.order_idx.is_some() {
            param_idx += 1;
            set_parts.push(format!("order_idx = ${}", param_idx));
        }

        // Handle runtime_metadata (JSONB). This is the control-plane contract;
        // descriptions are prompt projections only.
        if update.runtime_metadata.is_some() {
            param_idx += 1;
            set_parts.push(format!("runtime_metadata = ${}", param_idx));
        }

        // Auto-clear claim when status changes away from 'running'
        let mut clear_claim = false;
        if let Some(ref status_str) = update.status {
            if status_str != "running" && update.claim_executor_id.is_none() {
                clear_claim = true;
                set_parts.push("claim_executor_id = NULL".to_string());
                set_parts.push("claim_executor_type = NULL".to_string());
                set_parts.push("claimed_at = NULL".to_string());
            }
        }
        let _ = clear_claim;

        // WHERE clause
        param_idx += 1;
        let id_param = param_idx;
        let sql = format!(
            "UPDATE board_tasks SET {} WHERE id = ${}",
            set_parts.join(", "),
            id_param
        );

        // Build and execute query with dynamic binds
        let mut query = sqlx::query(&sql);
        for f in &fields {
            query = query.bind(&f.value);
        }
        if let Some(v) = auto_exec_val {
            query = query.bind(v);
        }
        if let Some(v) = hidden_val {
            query = query.bind(v);
        }
        if let Some(v) = update.order_idx {
            query = query.bind(v);
        }
        if let Some(ref v) = update.runtime_metadata {
            query = query.bind(v);
        }
        query = query.bind(&full_id);

        query.execute(&self.pool).await?;

        self.get_board_task(&full_id).await
    }

    async fn delete_board_task(&self, id: &str) -> DbResult<i64> {
        let full_id = match self.resolve_board_task_id(id).await? {
            Some(fid) => fid,
            None => return Ok(0),
        };

        // Collect all descendant IDs recursively
        let mut to_delete = vec![full_id.clone()];
        let mut i = 0;
        while i < to_delete.len() {
            let current = to_delete[i].clone();
            let children: Vec<(String,)> =
                sqlx::query_as("SELECT id FROM board_tasks WHERE parent_id = $1")
                    .bind(&current)
                    .fetch_all(&self.pool)
                    .await?;
            for (child_id,) in children {
                to_delete.push(child_id);
            }
            i += 1;
        }

        let mut deleted: i64 = 0;
        for tid in &to_delete {
            sqlx::query("DELETE FROM board_task_notes WHERE task_id = $1")
                .bind(tid)
                .execute(&self.pool)
                .await?;
            let result = sqlx::query("DELETE FROM board_tasks WHERE id = $1")
                .bind(tid)
                .execute(&self.pool)
                .await?;
            deleted += result.rows_affected() as i64;
        }
        Ok(deleted)
    }

    async fn toggle_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        if let Some(task) = self.get_board_task(id).await? {
            let new_status = match task.status {
                BoardTaskStatus::Done => "open",
                _ => "done",
            };
            let update = UpdateBoardTaskInput {
                status: Some(new_status.to_string()),
                ..Default::default()
            };
            self.update_board_task(id, &update).await
        } else {
            Ok(None)
        }
    }

    async fn resolve_board_task_id(&self, id: &str) -> DbResult<Option<String>> {
        // Exact match
        let exact: Option<(String,)> = sqlx::query_as("SELECT id FROM board_tasks WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        if let Some((fid,)) = exact {
            return Ok(Some(fid));
        }
        // Prefix match for short IDs
        if id.len() >= 6 && id.len() < 36 {
            let prefix = format!("{}%", id);
            let matches: Vec<(String,)> =
                sqlx::query_as("SELECT id FROM board_tasks WHERE id LIKE $1")
                    .bind(&prefix)
                    .fetch_all(&self.pool)
                    .await?;
            if matches.len() == 1 {
                return Ok(Some(matches[0].0.clone()));
            }
        }
        Ok(None)
    }

    async fn find_open_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>> {
        let row = sqlx::query_as::<_, PgBoardTaskRow>(
            "SELECT *, 0::bigint AS notes_count FROM board_tasks WHERE dedupe_key = $1 AND status NOT IN ('done', 'failed', 'skipped') LIMIT 1"
        )
        .bind(dedupe_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.into_board_task()))
    }

    async fn close_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>> {
        let task_id: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM board_tasks WHERE dedupe_key = $1 AND status = 'open' LIMIT 1",
        )
        .bind(dedupe_key)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id,)) = task_id {
            let update = UpdateBoardTaskInput {
                status: Some("done".to_string()),
                ..Default::default()
            };
            self.update_board_task(&id, &update).await
        } else {
            Ok(None)
        }
    }

    async fn search_board_tasks(&self, input: &BoardSearchInput) -> DbResult<BoardSearchResult> {
        let resolved_parent_id = if let Some(ref pid) = input.parent_id {
            self.resolve_board_task_id(pid).await?
        } else {
            None
        };

        let limit = input.limit.unwrap_or(50);
        let mut conditions: Vec<String> = Vec::new();
        let mut param_values: Vec<String> = Vec::new();
        let active_filter_applied = input.apply_active_status_filter();
        let historical_included = input.include_historical_results();

        if !input.include_hidden {
            conditions.push("hidden = 0".to_string());
        }
        if active_filter_applied {
            let quoted_statuses = ACTIVE_BOARD_SEARCH_STATUSES
                .iter()
                .map(|s| format!("'{}'", s))
                .collect::<Vec<_>>()
                .join(", ");
            conditions.push(format!("status IN ({})", quoted_statuses));
        }

        if let Some(ref q) = input.query {
            let words: Vec<&str> = q.split_whitespace().filter(|w| !w.is_empty()).collect();
            if !words.is_empty() {
                let mut word_conditions = Vec::new();
                for word in &words {
                    let param_idx = param_values.len() + 1;
                    word_conditions.push(format!(
                        "(title LIKE ${p} OR description LIKE ${p})",
                        p = param_idx
                    ));
                    param_values.push(format!("%{}%", word));
                }
                conditions.push(format!("({})", word_conditions.join(" AND ")));
            }
        }
        if let Some(ref project) = input.project {
            let param_idx = param_values.len() + 1;
            conditions.push(format!("project = ${}", param_idx));
            param_values.push(project.clone());
        }
        if let Some(ref category) = input.category {
            let param_idx = param_values.len() + 1;
            conditions.push(format!("category = ${}", param_idx));
            param_values.push(category.clone());
        }
        if let Some(ref status) = input.status {
            let param_idx = param_values.len() + 1;
            conditions.push(format!("status = ${}", param_idx));
            param_values.push(status.clone());
        }
        if let Some(ref full_pid) = resolved_parent_id {
            let param_idx = param_values.len() + 1;
            conditions.push(format!("parent_id = ${}", param_idx));
            param_values.push(full_pid.clone());
        } else if let Some(ref parent_id) = input.parent_id {
            let param_idx = param_values.len() + 1;
            conditions.push(format!("parent_id = ${}", param_idx));
            param_values.push(parent_id.clone());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        // Count total
        let count_sql = format!("SELECT COUNT(*) FROM board_tasks{}", where_clause);
        let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql);
        for v in &param_values {
            count_query = count_query.bind(v);
        }
        let (total_i64,) = count_query.fetch_one(&self.pool).await?;
        let total = total_i64 as usize;

        // Fetch compact results
        let select_sql = format!(
            "SELECT id, title, status, priority, parent_id, project, category FROM board_tasks{} ORDER BY order_idx ASC LIMIT {}",
            where_clause, limit
        );
        let mut select_query = sqlx::query_as::<_, PgCompactBoardTaskRow>(&select_sql);
        for v in &param_values {
            select_query = select_query.bind(v);
        }
        let rows = select_query.fetch_all(&self.pool).await?;

        let data: Vec<CompactBoardTask> = rows
            .into_iter()
            .map(|r| CompactBoardTask {
                id: TaskId::from_trusted(r.id),
                title: r.title,
                status: BoardTaskStatus::from_str(&r.status).unwrap_or(BoardTaskStatus::Open),
                priority: r.priority,
                parent_id: r.parent_id.map(TaskId::from_trusted),
                project: r.project,
                category: r.category,
            })
            .collect();

        let returned = data.len();
        let mut message_parts = Vec::new();
        if returned < total {
            message_parts.push(format!(
                "Showing {} of {} results. Refine query for more.",
                returned, total
            ));
        }
        if active_filter_applied {
            message_parts.push(
                "Default search scope excludes done/skipped historical tasks; pass includeHistorical=true, scope=all, or status=done/skipped to review history."
                    .to_string(),
            );
        }
        let message = if message_parts.is_empty() {
            None
        } else {
            Some(message_parts.join(" "))
        };

        Ok(BoardSearchResult {
            meta: BoardSearchMeta {
                total,
                returned,
                active_filter_applied,
                historical_included,
                message,
            },
            data,
        })
    }

    async fn board_summary(&self, since: Option<&str>) -> DbResult<serde_json::Value> {
        let since_clause = since.unwrap_or("2000-01-01T00:00:00Z");

        let (open,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM board_tasks WHERE status = 'open'")
                .fetch_one(&self.pool)
                .await?;
        let (running,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM board_tasks WHERE status = 'running'")
                .fetch_one(&self.pool)
                .await?;
        let (blocked,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM board_tasks WHERE status = 'blocked'")
                .fetch_one(&self.pool)
                .await?;
        let (completed,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM board_tasks WHERE status = 'done' AND updated_at >= $1",
        )
        .bind(since_clause)
        .fetch_one(&self.pool)
        .await?;
        let (failed,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM board_tasks WHERE status = 'failed' AND updated_at >= $1",
        )
        .bind(since_clause)
        .fetch_one(&self.pool)
        .await?;

        let (pending_questions,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM agent_questions WHERE status = 'pending'")
                .fetch_one(&self.pool)
                .await?;

        let new_kb: i64 =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM knowledge WHERE created_at >= $1")
                .bind(since_clause)
                .fetch_optional(&self.pool)
                .await?
                .map(|r| r.0)
                .unwrap_or(0);

        Ok(serde_json::json!({
            "open": open,
            "running": running,
            "blocked": blocked,
            "completed": completed,
            "failed": failed,
            "pendingQuestions": pending_questions,
            "newKBEntries": new_kb,
        }))
    }

    // ========== board.rs: claiming & autopilot ==========

    async fn claim_board_task(
        &self,
        id: &str,
        executor_id: &str,
        executor_type: &str,
    ) -> DbResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id).await? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
            .bind(format!("board-task-claim:{full_id}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            UPDATE board_tasks
            SET claim_executor_id = NULL,
                claim_executor_type = NULL,
                claimed_at = NULL,
                lease_expires_at = NULL,
                status = CASE WHEN status = 'running' THEN 'open' ELSE status END,
                updated_at = $2
            WHERE id = $1
              AND claim_executor_id IS NOT NULL
              AND lease_expires_at IS NOT NULL
              AND NULLIF(lease_expires_at, '')::timestamptz < now()
            "#,
        )
        .bind(&full_id)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        let existing = sqlx::query(
            r#"
            SELECT claim_executor_id, claim_executor_type, lease_expires_at, status
            FROM board_tasks
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(&full_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(existing) = existing else {
            tx.commit().await?;
            return Ok(None);
        };
        let existing_status: String = existing.try_get("status")?;
        let existing_holder: Option<String> = existing.try_get("claim_executor_id")?;
        if existing_status != "open" || existing_holder.is_some() {
            let holder = existing_holder.or_else(|| existing.try_get("claim_executor_type").ok());
            let lease_expires_at: Option<String> = existing.try_get("lease_expires_at")?;
            tx.commit().await?;
            return Err(DbError::ClaimConflict {
                scope_kind: "board_task".to_string(),
                scope_key: full_id,
                holder,
                lease_expires_at,
            });
        }
        let now = chrono::Utc::now();
        let now_s = now.to_rfc3339();
        let lease_expires_at = (now + chrono::Duration::minutes(30)).to_rfc3339();
        sqlx::query(
            r#"
            UPDATE board_tasks
            SET claim_executor_id = $1,
                claim_executor_type = $2,
                claimed_at = $3,
                lease_expires_at = $4,
                status = 'running',
                updated_at = $3
            WHERE id = $5
            "#,
        )
        .bind(executor_id)
        .bind(executor_type)
        .bind(&now_s)
        .bind(&lease_expires_at)
        .bind(&full_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO work_leases
              (id, task_id, holder_id, holder_kind, scope_kind, scope_key,
               lease_expires_at, metadata)
            VALUES ($1,$2,$3,$4,'board_task',$2,$5,'{}'::jsonb)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&full_id)
        .bind(executor_id)
        .bind(executor_type)
        .bind(&lease_expires_at)
        .execute(&mut *tx)
        .await
        .ok();
        tx.commit().await?;
        self.get_board_task(&full_id).await
    }

    async fn clear_board_task_assignee(
        &self,
        task_id: &str,
        expected_assignee: &str,
    ) -> DbResult<usize> {
        let full_id = match self.resolve_board_task_id(task_id).await? {
            Some(fid) => fid,
            None => return Ok(0),
        };
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE board_tasks
             SET assignee = NULL, updated_at = $1
             WHERE id = $2 AND status = 'open' AND assignee = $3",
        )
        .bind(&now)
        .bind(&full_id)
        .bind(expected_assignee)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn release_board_claims_by_executor(&self, executor_id: &str) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE board_tasks
             SET claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL,
                 status = 'open', updated_at = $1
             WHERE claim_executor_id = $2 AND status = 'running'",
        )
        .bind(&now)
        .bind(executor_id)
        .execute(&self.pool)
        .await?;
        let count = result.rows_affected() as usize;
        if count > 0 {
            tracing::info!(
                count,
                executor_id,
                "Released board claims for disconnected executor"
            );
        }
        Ok(count)
    }

    async fn recover_stale_running_tasks(&self, fallback_stale_minutes: i64) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();

        // Recover tasks with expired lease
        let leased = sqlx::query(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL,
                 claim_executor_type = NULL, claimed_at = NULL, lease_expires_at = NULL, updated_at = $1
             WHERE status = 'running'
               AND lease_expires_at IS NOT NULL
               AND lease_expires_at < $1"
        )
        .bind(&now)
        .execute(&self.pool)
        .await?
        .rows_affected();

        // Fallback: recover tasks without lease using time-based check
        let fallback_secs = fallback_stale_minutes * 60;
        let unleased = sqlx::query(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL,
                 claim_executor_type = NULL, claimed_at = NULL, updated_at = $1
             WHERE status = 'running'
               AND lease_expires_at IS NULL
               AND EXTRACT(EPOCH FROM (NOW() - updated_at::timestamp)) > COALESCE(timeout_secs, $2)"
        )
        .bind(&now)
        .bind(fallback_secs)
        .execute(&self.pool)
        .await?
        .rows_affected();

        let count = (leased + unleased) as usize;
        if count > 0 {
            tracing::warn!(
                count,
                leased,
                unleased,
                "Recovered stale running tasks (lease-based + fallback)"
            );
        }
        Ok(count)
    }

    async fn clear_dangling_dynamic_slot_assignees(&self) -> DbResult<usize> {
        // Companion to recover_stale_running_tasks: clears the `assignee`
        // pointer that recover intentionally leaves alone. Targets only
        // dynamic slots (slot-dyn-*) — never touches static slot pins because
        // those refer to long-lived configured slot ids that survive restart.
        //
        // NOT EXISTS catches both:
        //   * dynamic_slots row exists but status != 'active' (terminated)
        //   * dynamic_slots row was removed (GC'd) — no row to match
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE board_tasks
             SET assignee = NULL, updated_at = $1
             WHERE assignee LIKE 'slot-dyn-%'
               AND NOT EXISTS (
                 SELECT 1 FROM dynamic_slots
                 WHERE id = board_tasks.assignee
                   AND status = 'active'
               )",
        )
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let count = result.rows_affected() as usize;
        if count > 0 {
            tracing::warn!(
                count,
                "Cleared BoardTask assignees pointing to terminated/missing dynamic slots"
            );
        }
        Ok(count)
    }

    async fn set_board_task_lease(&self, task_id: &str, lease_expires_at: &str) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE board_tasks SET lease_expires_at = $1, updated_at = $2
             WHERE id = $3 AND status = 'running'",
        )
        .bind(lease_expires_at)
        .bind(&now)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn list_running_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        let rows = sqlx::query_as::<_, PgBoardTaskRow>(
            "SELECT *, 0::bigint AS notes_count FROM board_tasks
             WHERE auto_execute = 1
               AND status = 'running'
               AND claim_executor_id IS NOT NULL
             ORDER BY claimed_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_board_task()).collect())
    }

    async fn list_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let rows = sqlx::query_as::<_, PgBoardTaskRow>(
            "SELECT *, 0::bigint AS notes_count FROM board_tasks
             WHERE auto_execute = 1
               AND status = 'open'
               AND (due_date IS NULL OR due_date <= $1)
             ORDER BY
               CASE WHEN assignee IS NOT NULL THEN 0 ELSE 1 END,
               order_idx ASC",
        )
        .bind(&today)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_board_task()).collect())
    }

    async fn count_open_tasks_by_priority(&self, priorities: &[&str]) -> DbResult<i64> {
        if priorities.is_empty() {
            return Ok(0);
        }
        // Build placeholders $1, $2, ...
        let placeholders: Vec<String> = (1..=priorities.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "SELECT COUNT(*) FROM board_tasks WHERE status IN ('open','running') AND priority IN ({}) AND hidden = 0",
            placeholders.join(",")
        );
        let mut query = sqlx::query_as::<_, (i64,)>(&sql);
        for p in priorities {
            query = query.bind(*p);
        }
        let (count,) = query.fetch_one(&self.pool).await?;
        Ok(count)
    }

    async fn count_tasks_by_category(&self, category: &str) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM board_tasks WHERE category = $1 AND status IN ('open','running') AND hidden = 0"
        )
        .bind(category)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn clear_done_board_tasks(&self) -> DbResult<i64> {
        let result = sqlx::query("DELETE FROM board_tasks WHERE status = 'done'")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as i64)
    }

    // ========== board.rs: dependencies (DAG) ==========

    async fn check_dependencies(&self, depends_on: &[TaskId]) -> DbResult<DependencyStatus> {
        if depends_on.is_empty() {
            return Ok(DependencyStatus::Ready);
        }
        for dep_id in depends_on {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT status FROM board_tasks WHERE id = $1")
                    .bind(dep_id.as_str())
                    .fetch_optional(&self.pool)
                    .await?;
            match row.as_ref().map(|r| r.0.as_str()) {
                Some("done") => {} // satisfied
                Some("failed") | Some("skipped") => {
                    let title: String = sqlx::query_as::<_, (String,)>(
                        "SELECT title FROM board_tasks WHERE id = $1",
                    )
                    .bind(dep_id.as_str())
                    .fetch_optional(&self.pool)
                    .await?
                    .map(|r| r.0)
                    .unwrap_or_else(|| dep_id.to_string());
                    return Ok(DependencyStatus::Blocked(format!(
                        "{} ({})",
                        title,
                        row.unwrap().0
                    )));
                }
                Some(_) => return Ok(DependencyStatus::Pending),
                None => {
                    return Ok(DependencyStatus::Blocked(format!(
                        "\u{4f9d}\u{8d56}\u{4efb}\u{52a1} {} \u{4e0d}\u{5b58}\u{5728}",
                        dep_id
                    )))
                }
            }
        }
        Ok(DependencyStatus::Ready)
    }

    async fn find_downstream_tasks(&self, task_id: &str) -> DbResult<Vec<String>> {
        // Fetch all tasks with non-empty depends_on
        let rows = sqlx::query_as::<_, PgBoardTaskRow>(
            "SELECT *, 0::bigint AS notes_count FROM board_tasks WHERE depends_on != '[]'",
        )
        .fetch_all(&self.pool)
        .await?;
        let all_tasks: Vec<BoardTask> = rows.into_iter().map(|r| r.into_board_task()).collect();

        let mut downstream = Vec::new();
        let mut queue = vec![task_id.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(task_id.to_string());

        while let Some(current) = queue.pop() {
            for t in &all_tasks {
                if t.depends_on.iter().any(|d| d.as_str() == current) {
                    let tid_str = t.id.as_str().to_string();
                    if visited.insert(tid_str.clone()) {
                        downstream.push(tid_str.clone());
                        queue.push(tid_str);
                    }
                }
            }
        }
        Ok(downstream)
    }

    async fn retry_board_task(
        &self,
        task_id: &str,
        reset_downstream: bool,
    ) -> DbResult<Vec<String>> {
        let now = chrono::Utc::now().to_rfc3339();

        // Reset target task
        sqlx::query(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = $1 WHERE id = $2"
        )
        .bind(&now)
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        let mut reset_ids = vec![task_id.to_string()];

        if reset_downstream {
            let downstream = self.find_downstream_tasks(task_id).await?;
            for ds_id in &downstream {
                sqlx::query(
                    "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = $1 WHERE id = $2"
                )
                .bind(&now)
                .bind(ds_id)
                .execute(&self.pool)
                .await?;
            }
            reset_ids.extend(downstream);
        }

        Ok(reset_ids)
    }

    // ========== board.rs: context & notes ==========

    async fn get_board_tasks_with_context(
        &self,
        ids: &[String],
        include_children: bool,
    ) -> DbResult<Vec<BoardTaskWithContext>> {
        use std::collections::HashMap;

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Resolve short IDs
        let mut full_ids = Vec::new();
        for id in ids {
            if let Some(fid) = self.resolve_board_task_id(id).await? {
                full_ids.push(fid);
            }
        }
        if full_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Query 1: Fetch main tasks
        let placeholders: Vec<String> = (1..=full_ids.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "SELECT *, 0::bigint AS notes_count FROM board_tasks WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut query = sqlx::query_as::<_, PgBoardTaskRow>(&sql);
        for id in &full_ids {
            query = query.bind(id);
        }
        let main_rows = query.fetch_all(&self.pool).await?;
        let main_tasks: Vec<BoardTask> =
            main_rows.into_iter().map(|r| r.into_board_task()).collect();

        // Query 2: Fetch children (if requested)
        let mut children_by_parent: HashMap<String, Vec<BoardTask>> = HashMap::new();
        if include_children && !main_tasks.is_empty() {
            let parent_ids: Vec<String> = main_tasks
                .iter()
                .map(|t| t.id.as_str().to_string())
                .collect();
            let parent_placeholders: Vec<String> =
                (1..=parent_ids.len()).map(|i| format!("${}", i)).collect();
            let child_sql = format!(
                "SELECT *, 0::bigint AS notes_count FROM board_tasks WHERE parent_id IN ({}) ORDER BY order_idx ASC",
                parent_placeholders.join(",")
            );
            let mut child_query = sqlx::query_as::<_, PgBoardTaskRow>(&child_sql);
            for pid in &parent_ids {
                child_query = child_query.bind(pid);
            }
            let child_rows = child_query.fetch_all(&self.pool).await?;
            for row in child_rows {
                let task = row.into_board_task();
                if let Some(ref pid) = task.parent_id {
                    children_by_parent
                        .entry(pid.as_str().to_string())
                        .or_default()
                        .push(task);
                }
            }
        }

        // Query 3: Fetch all notes in one batch
        let mut all_task_ids: Vec<String> = main_tasks
            .iter()
            .map(|t| t.id.as_str().to_string())
            .collect();
        for children in children_by_parent.values() {
            for child in children {
                all_task_ids.push(child.id.as_str().to_string());
            }
        }

        let mut notes_by_task: HashMap<String, Vec<BoardTaskNote>> = HashMap::new();
        if !all_task_ids.is_empty() {
            let note_placeholders: Vec<String> = (1..=all_task_ids.len())
                .map(|i| format!("${}", i))
                .collect();
            let note_sql = format!(
                "SELECT id, task_id, content, note_type, author, created_at FROM board_task_notes WHERE task_id IN ({}) ORDER BY created_at ASC",
                note_placeholders.join(",")
            );
            let mut note_query = sqlx::query_as::<_, PgBoardTaskNoteRow>(&note_sql);
            for tid in &all_task_ids {
                note_query = note_query.bind(tid);
            }
            let note_rows = note_query.fetch_all(&self.pool).await?;
            for row in note_rows {
                let note = row.into_note();
                notes_by_task
                    .entry(note.task_id.clone())
                    .or_default()
                    .push(note);
            }
        }

        // Assemble tree
        let results = main_tasks
            .into_iter()
            .map(|task| {
                let task_id_str = task.id.as_str().to_string();
                let notes = notes_by_task.remove(&task_id_str).unwrap_or_default();
                let children = if include_children {
                    let child_tasks = children_by_parent.remove(&task_id_str).unwrap_or_default();
                    Some(
                        child_tasks
                            .into_iter()
                            .map(|child| {
                                let child_id_str = child.id.as_str().to_string();
                                let child_notes =
                                    notes_by_task.remove(&child_id_str).unwrap_or_default();
                                BoardTaskWithContext {
                                    task: child,
                                    notes: child_notes,
                                    children: None,
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                };
                BoardTaskWithContext {
                    task,
                    notes,
                    children,
                }
            })
            .collect();

        Ok(results)
    }

    async fn add_board_task_note(&self, input: &AddBoardTaskNoteInput) -> DbResult<BoardTaskNote> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let note_type_str = input.note_type.as_deref().unwrap_or("note");

        // Verify task exists
        let (exists,): (bool,) =
            sqlx::query_as("SELECT COUNT(*) > 0 FROM board_tasks WHERE id = $1")
                .bind(&input.task_id)
                .fetch_one(&self.pool)
                .await?;
        if !exists {
            return Err(DbError::NotFound {
                entity: "board_task",
                id: input.task_id.clone(),
            });
        }

        let note_type = BoardNoteType::from_str(note_type_str).unwrap_or(BoardNoteType::Note);

        sqlx::query(
            "INSERT INTO board_task_notes (id, task_id, content, note_type, author, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(&input.task_id)
        .bind(&input.content)
        .bind(note_type.as_str())
        .bind(&input.author)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(BoardTaskNote {
            id,
            task_id: input.task_id.clone(),
            content: input.content.clone(),
            note_type,
            author: input.author.clone(),
            created_at: now,
        })
    }

    async fn get_board_task_notes(&self, task_id: &str) -> DbResult<Vec<BoardTaskNote>> {
        let rows = sqlx::query_as::<_, PgBoardTaskNoteRow>(
            "SELECT id, task_id, content, note_type, author, created_at
             FROM board_task_notes WHERE task_id = $1 ORDER BY created_at ASC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_note()).collect())
    }

    async fn get_board_task_with_notes(&self, id: &str) -> DbResult<Option<BoardTaskWithNotes>> {
        if let Some(task) = self.get_board_task(id).await? {
            let notes = self.get_board_task_notes(task.id.as_str()).await?;
            Ok(Some(BoardTaskWithNotes { task, notes }))
        } else {
            Ok(None)
        }
    }

    async fn query_board_tasks_in_range(
        &self,
        time_col: &str,
        since: &str,
        until: &str,
    ) -> DbResult<Vec<serde_json::Value>> {
        let col = match time_col {
            "created_at" | "updated_at" => time_col,
            _ => return Ok(Vec::new()),
        };
        let sql = format!(
            "SELECT id, title, status, priority, project, {} as ts FROM board_tasks WHERE {} >= $1 AND {} <= $2 ORDER BY {} DESC LIMIT 50",
            col, col, col, col
        );
        let rows =
            sqlx::query_as::<_, (String, String, String, String, Option<String>, String)>(&sql)
                .bind(since)
                .bind(until)
                .fetch_all(&self.pool)
                .await?;
        let tasks: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, title, status, priority, project, ts)| {
                serde_json::json!({
                    "id": id,
                    "title": title,
                    "status": status,
                    "priority": priority,
                    "project": project,
                    "time": ts,
                })
            })
            .collect();
        Ok(tasks)
    }

    async fn query_board_tasks_in_range_with_status(
        &self,
        status: &str,
        since: &str,
        until: &str,
    ) -> DbResult<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, String)>(
            "SELECT id, title, status, priority, project, updated_at FROM board_tasks WHERE status = $1 AND updated_at >= $2 AND updated_at <= $3 ORDER BY updated_at DESC LIMIT 50"
        )
        .bind(status)
        .bind(since)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;
        let tasks: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, title, status, priority, project, updated_at)| {
                serde_json::json!({
                    "id": id,
                    "title": title,
                    "status": status,
                    "priority": priority,
                    "project": project,
                    "completed_at": updated_at,
                })
            })
            .collect();
        Ok(tasks)
    }

    // ========== question.rs: agent decision requests ==========

    async fn create_agent_question(
        &self,
        input: &CreateAgentQuestionInput,
    ) -> DbResult<AgentQuestion> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let q = AgentQuestion {
            id: id.clone(),
            task_id: input.task_id.clone(),
            slot_id: input.slot_id.clone(),
            session_id: input.session_id.clone(),
            question: input.question.clone(),
            context: input.context.clone().unwrap_or_default(),
            status: AgentQuestionStatus::Pending,
            answer: None,
            target: input.target.clone().unwrap_or_else(|| "user".to_string()),
            options: input.options.clone(),
            decision_type: input
                .decision_type
                .clone()
                .unwrap_or_else(|| "implementation".to_string()),
            retry_count: 0,
            routing_trace: None,
            revalidation_status: None,
            stale_reason: None,
            evidence_fresh_at: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        sqlx::query(
            "INSERT INTO agent_questions (id, task_id, slot_id, session_id, question, context, status, answer, target, options, decision_type, retry_count, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        )
        .bind(&q.id)
        .bind(&q.task_id)
        .bind(&q.slot_id)
        .bind(&q.session_id)
        .bind(&q.question)
        .bind(&q.context)
        .bind(q.status.as_str())
        .bind(&q.answer)
        .bind(&q.target)
        .bind(&q.options)
        .bind(&q.decision_type)
        .bind(q.retry_count)
        .bind(&q.created_at)
        .bind(&q.updated_at)
        .execute(&self.pool)
        .await?;

        // Auto-block: if question is linked to a board task, mark task as blocked
        if let Some(ref tid) = q.task_id {
            sqlx::query(
                "UPDATE board_tasks SET status = 'blocked', updated_at = $1 WHERE id = $2 AND status IN ('open', 'running')"
            )
            .bind(&q.created_at)
            .bind(tid)
            .execute(&self.pool)
            .await?;
            tracing::info!(task_id = %tid, question_id = %q.id, "Question created -> task auto-blocked");
        }

        Ok(q)
    }

    async fn get_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>> {
        let row =
            sqlx::query_as::<_, PgAgentQuestionRow>("SELECT * FROM agent_questions WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.into_question()))
    }

    async fn list_agent_questions(
        &self,
        status: Option<&str>,
        target: Option<&str>,
        limit: Option<usize>,
    ) -> DbResult<Vec<AgentQuestion>> {
        let mut conditions = Vec::new();
        let mut param_values: Vec<String> = Vec::new();

        if let Some(s) = status {
            let idx = param_values.len() + 1;
            conditions.push(format!("status = ${}", idx));
            param_values.push(s.to_string());
        }
        if let Some(t) = target {
            let idx = param_values.len() + 1;
            conditions.push(format!("target = ${}", idx));
            param_values.push(t.to_string());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let limit_clause = match limit {
            Some(l) => format!(" LIMIT {}", l),
            None => String::new(),
        };

        let sql = format!(
            "SELECT * FROM agent_questions{} ORDER BY created_at DESC{}",
            where_clause, limit_clause
        );
        let mut query = sqlx::query_as::<_, PgAgentQuestionRow>(&sql);
        for v in &param_values {
            query = query.bind(v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into_question()).collect())
    }

    async fn list_questions_for_task(&self, task_id: &str) -> DbResult<Vec<AgentQuestion>> {
        let rows = sqlx::query_as::<_, PgAgentQuestionRow>(
            "SELECT * FROM agent_questions WHERE task_id = $1 AND status = 'answered' ORDER BY created_at ASC"
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_question()).collect())
    }

    async fn answer_agent_question(
        &self,
        id: &str,
        answer: &str,
    ) -> DbResult<Option<AgentQuestion>> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "UPDATE agent_questions SET answer = $1, status = 'answered', updated_at = $2 WHERE id = $3"
        )
        .bind(answer)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        // Auto-unblock: if all questions for the linked task are resolved, unblock the task
        let task_id: Option<(Option<String>,)> =
            sqlx::query_as("SELECT task_id FROM agent_questions WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;

        if let Some((Some(tid),)) = task_id {
            let (pending_count,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM agent_questions WHERE task_id = $1 AND status = 'pending'",
            )
            .bind(&tid)
            .fetch_one(&self.pool)
            .await?;

            if pending_count == 0 {
                sqlx::query(
                    "UPDATE board_tasks SET status = 'open', updated_at = $1 WHERE id = $2 AND status = 'blocked'"
                )
                .bind(&now)
                .bind(&tid)
                .execute(&self.pool)
                .await?;
                tracing::info!(task_id = %tid, "All questions answered -> task auto-unblocked");
            }
        }

        self.get_agent_question(id).await
    }

    async fn dismiss_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE agent_questions SET status = 'dismissed', updated_at = $1 WHERE id = $2",
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get_agent_question(id).await
    }

    async fn set_question_routing_trace(&self, id: &str, trace_json: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE agent_questions SET routing_trace = $1, updated_at = $2 WHERE id = $3")
            .bind(trace_json)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn downgrade_question_to_user(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE agent_questions SET target = 'user', updated_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn increment_question_retry(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE agent_questions SET retry_count = retry_count + 1, updated_at = $1 WHERE id = $2"
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find_stale_master_questions(&self, max_age_secs: i64) -> DbResult<Vec<AgentQuestion>> {
        let rows = sqlx::query_as::<_, PgAgentQuestionRow>(
            "SELECT * FROM agent_questions
             WHERE target = 'master' AND status = 'pending'
               AND EXTRACT(EPOCH FROM (NOW() - created_at::timestamp)) > $1",
        )
        .bind(max_age_secs)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_question()).collect())
    }

    async fn find_tasks_with_unharvested_decisions(
        &self,
        min_count: usize,
    ) -> DbResult<Vec<(String, String, usize)>> {
        let rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT bt.id, bt.title, COUNT(aq.id) as q_count
             FROM board_tasks bt
             JOIN agent_questions aq ON aq.task_id = bt.id
             WHERE bt.status NOT IN ('done', 'skipped', 'failed')
               AND aq.target = 'master' AND aq.status = 'answered'
             GROUP BY bt.id, bt.title
             HAVING COUNT(aq.id) >= $1",
        )
        .bind(min_count as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, title, count)| (id, title, count as usize))
            .collect())
    }

    async fn decision_stats(&self, hours: i64) -> DbResult<serde_json::Value> {
        let cutoff = (chrono::Utc::now() - chrono::TimeDelta::hours(hours)).to_rfc3339();

        let (total,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND created_at > $1",
        )
        .bind(&cutoff)
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0,));

        let (answered,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND status = 'answered' AND created_at > $1"
        ).bind(&cutoff).fetch_one(&self.pool).await.unwrap_or((0,));

        let (pending,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND status = 'pending'",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0,));

        let (dismissed,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND status = 'dismissed' AND created_at > $1"
        ).bind(&cutoff).fetch_one(&self.pool).await.unwrap_or((0,));

        let (t1_hits,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"T1\"%' AND created_at > $1"
        ).bind(&cutoff).fetch_one(&self.pool).await.unwrap_or((0,));

        let (t2_hits,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"T2\"%' AND created_at > $1"
        ).bind(&cutoff).fetch_one(&self.pool).await.unwrap_or((0,));

        let (t3_hits,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"T3\"%' AND created_at > $1"
        ).bind(&cutoff).fetch_one(&self.pool).await.unwrap_or((0,));

        let (downgraded,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"downgraded\"%' AND created_at > $1"
        ).bind(&cutoff).fetch_one(&self.pool).await.unwrap_or((0,));

        let resolved = t1_hits + t2_hits + t3_hits;
        let t1_hit_rate = if resolved > 0 {
            format!("{:.0}%", (t1_hits as f64 / resolved as f64) * 100.0)
        } else {
            "N/A".to_string()
        };

        Ok(serde_json::json!({
            "total": total,
            "answered": answered,
            "pending": pending,
            "dismissed": dismissed,
            "downgraded": downgraded,
            "t1Hits": t1_hits,
            "t2Hits": t2_hits,
            "t3Hits": t3_hits,
            "t1HitRate": t1_hit_rate,
            "hours": hours,
        }))
    }

    async fn increment_board_task_retry(&self, task_id: &str, new_retry: i64) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE board_tasks SET retry_count = $1, status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = $2 WHERE id = $3"
        )
        .bind(new_retry)
        .bind(&now)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn unclaim_board_task(&self, task_id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = $1 WHERE id = $2"
        )
        .bind(&now)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod board_store_unit_tests {
    use super::*;

    #[test]
    fn compact_board_task_description_preserves_short_text() {
        assert_eq!(compact_board_task_description("short"), "short");
    }

    #[test]
    fn compact_board_task_description_truncates_on_char_boundary() {
        let raw = format!("{}{}", "好".repeat(20_000), "tail");
        let compact = compact_board_task_description(&raw);
        assert!(compact.len() <= MAX_BOARD_TASK_DESCRIPTION_BYTES + 64);
        assert!(compact.contains("truncated from"));
        assert!(std::str::from_utf8(compact.as_bytes()).is_ok());
    }
}

// ============ Row extraction helpers ============

/// Row struct for manual extraction of board_tasks rows via sqlx::FromRow.
#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgBoardTaskRow {
    id: String,
    title: String,
    description: String,
    status: String,
    priority: String,
    category: String,
    project: Option<String>,
    server: Option<String>,
    due_date: Option<String>,
    parent_id: Option<String>,
    assignee: Option<String>,
    auto_execute: i32,
    prompt_template: Option<String>,
    hidden: i32,
    retry_count: Option<i64>,
    max_retries: Option<i64>,
    order_idx: i64,
    created_at: String,
    updated_at: String,
    claim_executor_id: Option<String>,
    claim_executor_type: Option<String>,
    claimed_at: Option<String>,
    flow_phase: Option<String>,
    flow_context: Option<String>,
    flow_template: Option<String>,
    depends_on: Option<String>,
    lease_expires_at: Option<String>,
    dedupe_key: Option<String>,
    timeout_secs: Option<i64>,
    context_intent: Option<String>,
    trigger_source: Option<String>,
    runtime_metadata: Option<serde_json::Value>,
    notes_count: Option<i64>,
}

#[cfg(feature = "postgres")]
impl PgBoardTaskRow {
    fn into_board_task(self) -> BoardTask {
        let status = BoardTaskStatus::from_str(&self.status).unwrap_or(BoardTaskStatus::Open);
        let depends_on: Vec<TaskId> = self
            .depends_on
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();

        BoardTask {
            id: TaskId::from_trusted(self.id),
            title: self.title,
            description: self.description,
            status,
            priority: self.priority,
            category: self.category,
            project: self.project,
            server: self.server,
            due_date: self.due_date,
            parent_id: self.parent_id.map(TaskId::from_trusted),
            assignee: self.assignee,
            auto_execute: self.auto_execute != 0,
            prompt_template: self.prompt_template,
            hidden: self.hidden != 0,
            retry_count: self.retry_count.unwrap_or(0),
            max_retries: self.max_retries.unwrap_or(2),
            order_idx: self.order_idx,
            created_at: self.created_at,
            updated_at: self.updated_at,
            claim_executor_id: self.claim_executor_id,
            claim_executor_type: self.claim_executor_type,
            claimed_at: self.claimed_at,
            flow_phase: self.flow_phase,
            flow_context: self.flow_context,
            flow_template: self.flow_template,
            depends_on,
            lease_expires_at: self.lease_expires_at,
            dedupe_key: self.dedupe_key,
            timeout_secs: self.timeout_secs,
            context_intent: self.context_intent,
            trigger_source: self.trigger_source,
            runtime_metadata: self
                .runtime_metadata
                .unwrap_or_else(|| serde_json::json!({})),
            notes_count: self.notes_count.unwrap_or(0),
        }
    }
}

/// Row struct for compact board task search results.
#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgCompactBoardTaskRow {
    id: String,
    title: String,
    status: String,
    priority: String,
    parent_id: Option<String>,
    project: Option<String>,
    category: Option<String>,
}

/// Row struct for board_task_notes.
#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgBoardTaskNoteRow {
    id: String,
    task_id: String,
    content: String,
    note_type: String,
    author: Option<String>,
    created_at: String,
}

#[cfg(feature = "postgres")]
impl PgBoardTaskNoteRow {
    fn into_note(self) -> BoardTaskNote {
        let note_type = BoardNoteType::from_str(&self.note_type).unwrap_or(BoardNoteType::Note);
        BoardTaskNote {
            id: self.id,
            task_id: self.task_id,
            content: self.content,
            note_type,
            author: self.author,
            created_at: self.created_at,
        }
    }
}

/// Row struct for agent_questions.
#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct PgAgentQuestionRow {
    id: String,
    task_id: Option<String>,
    slot_id: Option<String>,
    session_id: Option<String>,
    question: String,
    context: String,
    status: String,
    answer: Option<String>,
    target: Option<String>,
    options: Option<String>,
    decision_type: Option<String>,
    retry_count: Option<i64>,
    routing_trace: Option<String>,
    created_at: String,
    updated_at: String,
}

#[cfg(feature = "postgres")]
impl PgAgentQuestionRow {
    fn into_question(self) -> AgentQuestion {
        let status =
            AgentQuestionStatus::from_str(&self.status).unwrap_or(AgentQuestionStatus::Pending);
        AgentQuestion {
            id: self.id,
            task_id: self.task_id,
            slot_id: self.slot_id,
            session_id: self.session_id,
            question: self.question,
            context: self.context,
            status,
            answer: self.answer,
            target: self.target.unwrap_or_else(|| "user".to_string()),
            options: self.options,
            decision_type: self
                .decision_type
                .unwrap_or_else(|| "implementation".to_string()),
            retry_count: self.retry_count.unwrap_or(0),
            routing_trace: self.routing_trace,
            revalidation_status: None,
            stale_reason: None,
            evidence_fresh_at: None,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}
