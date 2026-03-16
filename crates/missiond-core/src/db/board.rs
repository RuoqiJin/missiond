use rusqlite::{params, OptionalExtension};
use super::error::DbResult;
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ Board Tasks ============

    /// Insert a new board task
    pub fn insert_board_task(&self, task: &BoardTask) -> DbResult<()> {
        let conn = self.conn();
        let depends_on_json = serde_json::to_string(&task.depends_on).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO board_tasks (id, title, description, status, priority, category, project, server, due_date, parent_id, assignee, auto_execute, prompt_template, hidden, retry_count, max_retries, order_idx, created_at, updated_at, depends_on, dedupe_key, timeout_secs, context_intent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            params![
                task.id,
                task.title,
                task.description,
                task.status.as_str(),
                task.priority,
                task.category,
                task.project,
                task.server,
                task.due_date,
                task.parent_id,
                task.assignee,
                task.auto_execute as i32,
                task.prompt_template,
                task.hidden as i32,
                task.retry_count,
                task.max_retries,
                task.order_idx,
                task.created_at,
                task.updated_at,
                depends_on_json,
                task.dedupe_key,
                task.timeout_secs,
                task.context_intent,
            ],
        )?;
        Ok(())
    }

    /// Create a board task from input
    pub fn create_board_task(&self, input: &CreateBoardTaskInput) -> DbResult<BoardTask> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();

        // Phase 6.6: Enforce description size limit (50KB) — DB-layer safety net
        const MAX_DESC_LEN: usize = 50_000;
        let description = match &input.description {
            Some(d) if d.len() > MAX_DESC_LEN => {
                tracing::warn!(len = d.len(), "Board task description exceeds 50KB, truncating");
                let end = crate::embedding::char_boundary_at(d, MAX_DESC_LEN);
                format!("{}...(truncated from {} bytes)", &d[..end], d.len())
            }
            Some(d) => d.clone(),
            None => String::new(),
        };

        // Get max order for siblings
        let conn = self.conn();
        let max_order: i64 = if let Some(ref pid) = input.parent_id {
            conn.query_row(
                "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id = ?1",
                params![pid],
                |row| row.get(0),
            )
            .unwrap_or(-1)
        } else {
            conn.query_row(
                "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1)
        };
        drop(conn);

        let task = BoardTask {
            id,
            title: input.title.clone(),
            description,
            status: BoardTaskStatus::Open,
            priority: input.priority.clone().unwrap_or_else(|| "medium".to_string()),
            category: input.category.clone().unwrap_or_else(|| "other".to_string()),
            project: input.project.clone(),
            server: input.server.clone(),
            due_date: input.due_date.clone(),
            parent_id: input.parent_id.clone(),
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
            depends_on: input.depends_on.clone().unwrap_or_default(),
            lease_expires_at: None,
            dedupe_key: input.dedupe_key.clone(),
            timeout_secs: input.timeout_secs,
            context_intent: input.context_intent.clone(),
            notes_count: 0,
        };

        self.insert_board_task(&task)?;
        Ok(task)
    }

    /// Find an open board task by dedupe_key (for AIOps alert aggregation).
    /// Returns the first open task matching the key, or None.
    pub fn find_open_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM board_tasks WHERE dedupe_key = ?1 AND status NOT IN ('done', 'failed', 'skipped') LIMIT 1",
        )?;
        let mut rows = stmt.query(params![dedupe_key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_board_task(row)?))
        } else {
            Ok(None)
        }
    }

    /// Close an open board task by dedupe_key (for AIOps auto-recovery).
    /// Returns the closed task, or None if no matching open task found.
    pub fn close_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>> {
        let conn = self.read_conn();
        let task_id: Option<String> = conn.query_row(
            "SELECT id FROM board_tasks WHERE dedupe_key = ?1 AND status = 'open' LIMIT 1",
            params![dedupe_key],
            |row| row.get(0),
        ).optional()?;
        drop(conn);

        if let Some(id) = task_id {
            let update = UpdateBoardTaskInput {
                status: Some("done".to_string()),
                ..Default::default()
            };
            self.update_board_task(&id, &update)
        } else {
            Ok(None)
        }
    }

    /// Get a board task by ID
    /// Resolve a (possibly short) board task ID to a full UUID.
    /// Supports exact match and unique prefix match (>= 6 chars).
    pub fn resolve_board_task_id(&self, id: &str) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        // Exact match
        let exact: Option<String> = conn.query_row(
            "SELECT id FROM board_tasks WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ).optional()?;
        if exact.is_some() {
            return Ok(exact);
        }
        // Prefix match for short IDs
        if id.len() >= 6 && id.len() < 36 {
            let prefix = format!("{}%", id);
            let mut stmt = conn.prepare("SELECT id FROM board_tasks WHERE id LIKE ?1")?;
            let matches: Vec<String> = stmt
                .query_map(params![prefix], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if matches.len() == 1 {
                return Ok(Some(matches.into_iter().next().unwrap()));
            }
        }
        Ok(None)
    }

    pub fn get_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let conn = self.read_conn();
        let mut stmt = conn
            .prepare("SELECT * FROM board_tasks WHERE id = ?")?;
        let mut rows = stmt.query(params![full_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_board_task(row)?))
        } else {
            Ok(None)
        }
    }

    /// List all board tasks (optionally filtered by status, hidden excluded by default)
    pub fn list_board_tasks(&self, status: Option<&str>, include_hidden: bool) -> DbResult<Vec<BoardTask>> {
        let conn = self.read_conn();
        let mut tasks = Vec::new();
        let hidden_clause = if include_hidden { "" } else { " AND hidden = 0" };
        let notes_sub = "(SELECT COUNT(*) FROM board_task_notes WHERE task_id = board_tasks.id) AS notes_count";

        if let Some(s) = status {
            let sql = format!("SELECT *, {} FROM board_tasks WHERE status = ?1{} ORDER BY order_idx ASC", notes_sub, hidden_clause);
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![s], |row| Self::row_to_board_task(row))?;
            for task in rows {
                tasks.push(task?);
            }
        } else {
            let sql = format!(
                "SELECT *, {} FROM board_tasks{} ORDER BY order_idx ASC",
                notes_sub,
                if include_hidden { "" } else { " WHERE hidden = 0" }
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], |row| Self::row_to_board_task(row))?;
            for task in rows {
                tasks.push(task?);
            }
        }

        Ok(tasks)
    }

    /// Update a board task
    pub fn update_board_task(&self, id: &str, update: &UpdateBoardTaskInput) -> DbResult<Option<BoardTask>> {
        let id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let id = id.as_str();
        let now = chrono::Utc::now().to_rfc3339();
        let mut fields = vec!["updated_at = ?".to_string()];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        macro_rules! push_field {
            ($name:ident, $col:expr) => {
                if let Some(ref v) = update.$name {
                    fields.push(format!("{} = ?", $col));
                    values.push(Box::new(v.clone()));
                }
            };
        }

        push_field!(title, "title");
        push_field!(description, "description");
        push_field!(status, "status");
        push_field!(priority, "priority");
        push_field!(category, "category");
        push_field!(project, "project");
        push_field!(server, "server");
        push_field!(due_date, "due_date");
        push_field!(parent_id, "parent_id");
        push_field!(assignee, "assignee");
        push_field!(prompt_template, "prompt_template");

        // Explicit claim update (for manual claim via MCP tool)
        if let Some(ref eid) = update.claim_executor_id {
            fields.push("claim_executor_id = ?".to_string());
            values.push(Box::new(eid.clone()));
        }
        if let Some(ref etype) = update.claim_executor_type {
            fields.push("claim_executor_type = ?".to_string());
            values.push(Box::new(etype.clone()));
        }

        // Auto-clear claim when status changes away from 'running'
        if let Some(ref status_str) = update.status {
            if status_str != "running" {
                // Clear claim fields when transitioning to any non-running status
                if update.claim_executor_id.is_none() {
                    fields.push("claim_executor_id = NULL".to_string());
                    fields.push("claim_executor_type = NULL".to_string());
                    fields.push("claimed_at = NULL".to_string());
                }
            }
        }

        if let Some(auto_exec) = update.auto_execute {
            fields.push("auto_execute = ?".to_string());
            values.push(Box::new(auto_exec as i32));
        }

        if let Some(hidden) = update.hidden {
            fields.push("hidden = ?".to_string());
            values.push(Box::new(hidden as i32));
        }

        if let Some(idx) = update.order_idx {
            fields.push("order_idx = ?".to_string());
            values.push(Box::new(idx));
        }

        // Flow Engine fields
        push_field!(flow_phase, "flow_phase");
        push_field!(flow_context, "flow_context");
        push_field!(flow_template, "flow_template");

        // DAG dependency
        if let Some(ref deps) = update.depends_on {
            fields.push("depends_on = ?".to_string());
            values.push(Box::new(serde_json::to_string(deps).unwrap_or_else(|_| "[]".to_string())));
        }

        let sql = format!(
            "UPDATE board_tasks SET {} WHERE id = ?",
            fields.join(", ")
        );
        values.push(Box::new(id.to_string()));

        let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        {
            let conn = self.conn();
            conn.execute(&sql, params.as_slice())?;
        }

        self.get_board_task(id)
    }

    /// Delete a board task and all descendants
    pub fn delete_board_task(&self, id: &str) -> DbResult<i64> {
        let id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(0),
        };
        let conn = self.conn();
        // Collect all descendant IDs recursively
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let current = to_delete[i].clone();
            let mut stmt = conn
                .prepare("SELECT id FROM board_tasks WHERE parent_id = ?")?;
            let children: Vec<String> = stmt
                .query_map(params![current], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            to_delete.extend(children);
            i += 1;
        }

        let mut deleted = 0i64;
        for tid in &to_delete {
            conn.execute("DELETE FROM board_task_notes WHERE task_id = ?", params![tid])?;
            let r = conn
                .execute("DELETE FROM board_tasks WHERE id = ?", params![tid])?;
            deleted += r as i64;
        }
        Ok(deleted)
    }

    /// Toggle a board task status (open <-> done)
    pub fn toggle_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        if let Some(task) = self.get_board_task(id)? {
            let new_status = match task.status {
                BoardTaskStatus::Done => "open",
                _ => "done",
            };
            let update = UpdateBoardTaskInput {
                status: Some(new_status.to_string()),
                ..Default::default()
            };
            self.update_board_task(id, &update)
        } else {
            Ok(None)
        }
    }

    /// Atomically claim a board task (CAS: only succeeds if open + unclaimed).
    /// Returns Ok(Some(task)) on success, Ok(None) if task not found or already claimed.
    pub fn claim_board_task(
        &self,
        id: &str,
        executor_id: &str,
        executor_type: &str,
    ) -> DbResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        let rows = conn.execute(
            "UPDATE board_tasks
             SET claim_executor_id = ?1, claim_executor_type = ?2, claimed_at = ?3,
                 status = 'running', updated_at = ?3
             WHERE id = ?4 AND status = 'open' AND claim_executor_id IS NULL",
            params![executor_id, executor_type, now, full_id],
        )?;
        drop(conn);
        if rows == 0 {
            return Ok(None); // Already claimed or not open
        }
        self.get_board_task(&full_id)
    }

    /// Release all claims held by a specific executor (zombie cleanup on disconnect/exit).
    /// Resets claimed tasks back to 'open'.
    pub fn release_board_claims_by_executor(&self, executor_id: &str) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE board_tasks
             SET claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL,
                 status = 'open', updated_at = ?1
             WHERE claim_executor_id = ?2 AND status = 'running'",
            params![now, executor_id],
        )?;
        if count > 0 {
            tracing::info!(count, executor_id, "Released board claims for disconnected executor");
        }
        Ok(count)
    }

    /// Recover stale running tasks using lease-based expiration.
    /// Tasks with an expired lease (lease_expires_at < now) are reset to 'open'.
    /// Tasks without a lease fall back to the old N-minute timeout based on updated_at.
    pub fn recover_stale_running_tasks(&self, fallback_stale_minutes: i64) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        // Recover tasks with expired lease
        let leased = conn.execute(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL,
                 claim_executor_type = NULL, claimed_at = NULL, lease_expires_at = NULL, updated_at = ?1
             WHERE status = 'running'
               AND lease_expires_at IS NOT NULL
               AND lease_expires_at < ?1",
            params![now],
        )?;
        // Fallback: recover tasks without lease using time-based check.
        // Respects per-task timeout_secs (from task_delegate), else uses fallback_stale_minutes.
        let unleased = conn.execute(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL,
                 claim_executor_type = NULL, claimed_at = NULL, updated_at = ?1
             WHERE status = 'running'
               AND lease_expires_at IS NULL
               AND julianday('now') - julianday(updated_at) >
                   COALESCE(timeout_secs, ?2 * 60) / 86400.0",
            params![now, fallback_stale_minutes as f64],
        )?;
        let count = leased + unleased;
        if count > 0 {
            tracing::warn!(count, leased, unleased, "Recovered stale running tasks (lease-based + fallback)");
        }
        Ok(count)
    }

    /// Set or renew the lease expiration for a running board task.
    pub fn set_board_task_lease(&self, task_id: &str, lease_expires_at: &str) -> DbResult<usize> {
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE board_tasks SET lease_expires_at = ?1, updated_at = ?2
             WHERE id = ?3 AND status = 'running'",
            params![lease_expires_at, chrono::Utc::now().to_rfc3339(), task_id],
        )?;
        Ok(count)
    }

    /// List board tasks currently in 'running' state with auto_execute.
    /// Used by smart watchdog to detect orphaned tasks (slot idle but task still running).
    pub fn list_running_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM board_tasks
             WHERE auto_execute = 1
               AND status = 'running'
               AND claim_executor_id IS NOT NULL
             ORDER BY claimed_at ASC"
        )?;
        let rows = stmt.query_map([], |row| Self::row_to_board_task(row))?;
        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// List board tasks eligible for autopilot execution
    /// (auto_execute=true, status=open, due_date <= today local)
    /// Tasks with assignee=NULL are also returned — autopilot dynamically assigns a slot.
    pub fn list_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM board_tasks
             WHERE auto_execute = 1
               AND status = 'open'
               AND (due_date IS NULL OR due_date <= ?1)
             ORDER BY
               CASE WHEN assignee IS NOT NULL THEN 0 ELSE 1 END,
               order_idx ASC"
        )?;
        let rows = stmt.query_map(params![today], |row| Self::row_to_board_task(row))?;
        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Count open board tasks with priority in the given set (for idle exploration gating).
    pub fn count_open_tasks_by_priority(&self, priorities: &[&str]) -> DbResult<i64> {
        let conn = self.read_conn();
        let placeholders = priorities.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT COUNT(*) FROM board_tasks WHERE status IN ('open','running') AND priority IN ({}) AND hidden = 0",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = priorities.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let count: i64 = stmt.query_row(params.as_slice(), |r| r.get(0))?;
        Ok(count)
    }

    /// Count open/running board tasks with the given category.
    pub fn count_tasks_by_category(&self, category: &str) -> DbResult<i64> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM board_tasks WHERE category = ?1 AND status IN ('open','running') AND hidden = 0",
            params![category],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Board summary: status counts + pending questions + recent activity
    pub fn board_summary(&self, since: Option<&str>) -> DbResult<serde_json::Value> {
        tokio::task::block_in_place(|| {
            let conn = self.read_conn();
            let since_clause = since.unwrap_or("2000-01-01T00:00:00Z");

            // Status counts (since timestamp for completed/failed, all-time for open/running/blocked)
            let open: i64 = conn.query_row("SELECT COUNT(*) FROM board_tasks WHERE status = 'open'", [], |r| r.get(0))?;
            let running: i64 = conn.query_row("SELECT COUNT(*) FROM board_tasks WHERE status = 'running'", [], |r| r.get(0))?;
            let blocked: i64 = conn.query_row("SELECT COUNT(*) FROM board_tasks WHERE status = 'blocked'", [], |r| r.get(0))?;
            let completed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM board_tasks WHERE status = 'done' AND updated_at >= ?1",
                params![since_clause], |r| r.get(0),
            )?;
            let failed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM board_tasks WHERE status = 'failed' AND updated_at >= ?1",
                params![since_clause], |r| r.get(0),
            )?;

            // Pending questions
            let pending_questions: i64 = conn.query_row(
                "SELECT COUNT(*) FROM agent_questions WHERE status = 'pending'", [], |r| r.get(0),
            )?;

            // Recent KB entries (since timestamp)
            let new_kb: i64 = conn.query_row(
                "SELECT COUNT(*) FROM knowledge WHERE created_at >= ?1",
                params![since_clause], |r| r.get(0),
            ).unwrap_or(0);

            Ok(serde_json::json!({
                "open": open,
                "running": running,
                "blocked": blocked,
                "completed": completed,
                "failed": failed,
                "pendingQuestions": pending_questions,
                "newKBEntries": new_kb,
            }))
        })
    }

    /// Clear all done board tasks
    pub fn clear_done_board_tasks(&self) -> DbResult<i64> {
        let conn = self.conn();
        let result = conn
            .execute("DELETE FROM board_tasks WHERE status = 'done'", [])?;
        Ok(result as i64)
    }

    /// Check whether all DAG dependencies of a task are satisfied.
    pub fn check_dependencies(&self, depends_on: &[String]) -> DbResult<crate::types::DependencyStatus> {
        use crate::types::DependencyStatus;
        if depends_on.is_empty() {
            return Ok(DependencyStatus::Ready);
        }
        let conn = self.read_conn();
        for dep_id in depends_on {
            let row: Option<String> = conn.query_row(
                "SELECT status FROM board_tasks WHERE id = ?1",
                params![dep_id],
                |row| row.get(0),
            ).optional()?;
            match row.as_deref() {
                Some("done") => {} // satisfied
                Some("failed") | Some("skipped") => {
                    // Get title for human-readable reason
                    let title: String = conn.query_row(
                        "SELECT title FROM board_tasks WHERE id = ?1",
                        params![dep_id],
                        |row| row.get(0),
                    ).unwrap_or_else(|_| dep_id.clone());
                    return Ok(DependencyStatus::Blocked(format!("{} ({})", title, row.unwrap())));
                }
                Some(_) => return Ok(DependencyStatus::Pending), // open, running, blocked, verifying
                None => return Ok(DependencyStatus::Blocked(format!("依赖任务 {} 不存在", dep_id))),
            }
        }
        Ok(DependencyStatus::Ready)
    }

    /// Find all downstream tasks that depend on the given task (direct + transitive).
    /// Used by mission_board_retry to cascade-reset downstream tasks.
    pub fn find_downstream_tasks(&self, task_id: &str) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut all_tasks: Vec<BoardTask> = Vec::new();
        let mut stmt = conn.prepare("SELECT * FROM board_tasks WHERE depends_on != '[]'")?;
        let rows = stmt.query_map([], |row| Self::row_to_board_task(row))?;
        for row in rows {
            all_tasks.push(row?);
        }

        let mut downstream = Vec::new();
        let mut queue = vec![task_id.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(task_id.to_string());

        while let Some(current) = queue.pop() {
            for t in &all_tasks {
                if t.depends_on.iter().any(|d| d.starts_with(&current) || current.starts_with(d)) {
                    if visited.insert(t.id.clone()) {
                        downstream.push(t.id.clone());
                        queue.push(t.id.clone());
                    }
                }
            }
        }
        Ok(downstream)
    }

    /// Reset a task and optionally all downstream tasks to open status.
    pub fn retry_board_task(&self, task_id: &str, reset_downstream: bool) -> DbResult<Vec<String>> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();

        // Reset target task
        conn.execute(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )?;

        let mut reset_ids = vec![task_id.to_string()];

        if reset_downstream {
            // Need read_conn for find_downstream, but we hold conn. Use a separate scope.
            drop(conn);
            let downstream = self.find_downstream_tasks(task_id)?;
            if !downstream.is_empty() {
                let conn = self.conn();
                for ds_id in &downstream {
                    conn.execute(
                        "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = ?1 WHERE id = ?2",
                        params![now, ds_id],
                    )?;
                }
                reset_ids.extend(downstream);
            }
        }

        Ok(reset_ids)
    }

    fn row_to_board_task(row: &rusqlite::Row) -> rusqlite::Result<BoardTask> {
        let status_str: String = row.get("status")?;
        let status = BoardTaskStatus::from_str(&status_str).unwrap_or(BoardTaskStatus::Open);
        let auto_execute: i32 = row.get("auto_execute").unwrap_or(0);
        let hidden: i32 = row.get("hidden").unwrap_or(0);

        Ok(BoardTask {
            id: row.get("id")?,
            title: row.get("title")?,
            description: row.get("description")?,
            status,
            priority: row.get("priority")?,
            category: row.get("category")?,
            project: row.get("project")?,
            server: row.get("server")?,
            due_date: row.get("due_date")?,
            parent_id: row.get("parent_id")?,
            assignee: row.get("assignee")?,
            auto_execute: auto_execute != 0,
            prompt_template: row.get("prompt_template")?,
            hidden: hidden != 0,
            retry_count: row.get("retry_count").unwrap_or(0),
            max_retries: row.get("max_retries").unwrap_or(2),
            order_idx: row.get("order_idx")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            claim_executor_id: row.get("claim_executor_id").unwrap_or(None),
            claim_executor_type: row.get("claim_executor_type").unwrap_or(None),
            claimed_at: row.get("claimed_at").unwrap_or(None),
            flow_phase: row.get("flow_phase").unwrap_or(None),
            flow_context: row.get("flow_context").unwrap_or(None),
            flow_template: row.get("flow_template").unwrap_or(None),
            depends_on: {
                let raw: String = row.get("depends_on").unwrap_or_else(|_| "[]".to_string());
                serde_json::from_str(&raw).unwrap_or_default()
            },
            lease_expires_at: row.get("lease_expires_at").unwrap_or(None),
            dedupe_key: row.get("dedupe_key").unwrap_or(None),
            timeout_secs: row.get("timeout_secs").unwrap_or(None),
            context_intent: row.get("context_intent").unwrap_or(None),
            notes_count: row.get("notes_count").unwrap_or(0),
        })
    }


    // ============ Board Search ============

    /// Search board tasks with filters, returning compact results
    pub fn search_board_tasks(&self, input: &crate::types::BoardSearchInput) -> DbResult<crate::types::BoardSearchResult> {
        // Resolve short parent_id before acquiring read conn
        let resolved_parent_id = if let Some(ref pid) = input.parent_id {
            self.resolve_board_task_id(pid)?
        } else {
            None
        };

        let conn = self.read_conn();
        let limit = input.limit.unwrap_or(50);

        // Build dynamic WHERE clause
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !input.include_hidden {
            conditions.push("hidden = 0".to_string());
        }

        if let Some(ref q) = input.query {
            // Split query into words, each word matches independently (AND logic)
            // This enables multi-keyword search: "Phase Darwin" matches title containing both
            let words: Vec<&str> = q.split_whitespace().filter(|w| !w.is_empty()).collect();
            if !words.is_empty() {
                let mut word_conditions = Vec::new();
                for word in &words {
                    let param_idx = params.len() + 1;
                    word_conditions.push(format!(
                        "(title LIKE ?{p} OR description LIKE ?{p})", p = param_idx
                    ));
                    params.push(Box::new(format!("%{}%", word)));
                }
                conditions.push(format!("({})", word_conditions.join(" AND ")));
            }
        }
        if let Some(ref project) = input.project {
            let param_idx = params.len() + 1;
            conditions.push(format!("project = ?{}", param_idx));
            params.push(Box::new(project.clone()));
        }
        if let Some(ref category) = input.category {
            let param_idx = params.len() + 1;
            conditions.push(format!("category = ?{}", param_idx));
            params.push(Box::new(category.clone()));
        }
        if let Some(ref status) = input.status {
            let param_idx = params.len() + 1;
            conditions.push(format!("status = ?{}", param_idx));
            params.push(Box::new(status.clone()));
        }
        if let Some(ref full_pid) = resolved_parent_id {
            let param_idx = params.len() + 1;
            conditions.push(format!("parent_id = ?{}", param_idx));
            params.push(Box::new(full_pid.clone()));
        } else if let Some(ref parent_id) = input.parent_id {
            // Fallback: resolve failed (no match), use original value
            let param_idx = params.len() + 1;
            conditions.push(format!("parent_id = ?{}", param_idx));
            params.push(Box::new(parent_id.clone()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        // Count total matching
        let count_sql = format!("SELECT COUNT(*) FROM board_tasks{}", where_clause);
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|v| v.as_ref()).collect();
        let total: usize = conn.query_row(&count_sql, param_refs.as_slice(), |row| row.get(0))?;

        // Fetch compact results with limit
        let select_sql = format!(
            "SELECT id, title, status, priority, parent_id, project, category FROM board_tasks{} ORDER BY order_idx ASC LIMIT {}",
            where_clause, limit
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|v| v.as_ref()).collect();
        let mut stmt = conn.prepare(&select_sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let status_str: String = row.get("status")?;
            let status = crate::types::BoardTaskStatus::from_str(&status_str).unwrap_or(crate::types::BoardTaskStatus::Open);
            Ok(crate::types::CompactBoardTask {
                id: row.get("id")?,
                title: row.get("title")?,
                status,
                priority: row.get("priority")?,
                parent_id: row.get("parent_id")?,
                project: row.get("project")?,
                category: row.get("category")?,
            })
        })?;

        let mut data = Vec::new();
        for row in rows {
            data.push(row?);
        }
        let returned = data.len();

        let message = if returned < total {
            Some(format!("Showing {} of {} results. Refine query for more.", returned, total))
        } else {
            None
        };

        Ok(crate::types::BoardSearchResult {
            meta: crate::types::BoardSearchMeta { total, returned, message },
            data,
        })
    }

    /// Get multiple board tasks by IDs with optional children, using max 3 SQL queries
    pub fn get_board_tasks_with_context(
        &self,
        ids: &[String],
        include_children: bool,
    ) -> DbResult<Vec<crate::types::BoardTaskWithContext>> {
        use std::collections::HashMap;

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Resolve short IDs BEFORE acquiring conn (resolve_board_task_id also uses read_conn)
        let mut full_ids = Vec::new();
        for id in ids {
            if let Some(fid) = self.resolve_board_task_id(id)? {
                full_ids.push(fid);
            }
        }

        if full_ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.read_conn();

        // Query 1: Fetch main tasks
        let placeholders: Vec<String> = (1..=full_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!("SELECT * FROM board_tasks WHERE id IN ({})", placeholders.join(","));
        let params: Vec<&dyn rusqlite::ToSql> = full_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |row| Self::row_to_board_task(row))?;
        let mut main_tasks: Vec<BoardTask> = Vec::new();
        for row in rows {
            main_tasks.push(row?);
        }

        // Query 2: Fetch children (if requested)
        let mut children_by_parent: HashMap<String, Vec<BoardTask>> = HashMap::new();
        if include_children && !main_tasks.is_empty() {
            let parent_placeholders: Vec<String> = (1..=main_tasks.len()).map(|i| format!("?{}", i)).collect();
            let child_sql = format!(
                "SELECT * FROM board_tasks WHERE parent_id IN ({}) ORDER BY order_idx ASC",
                parent_placeholders.join(",")
            );
            let parent_ids: Vec<String> = main_tasks.iter().map(|t| t.id.clone()).collect();
            let params: Vec<&dyn rusqlite::ToSql> = parent_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
            let mut stmt = conn.prepare(&child_sql)?;
            let rows = stmt.query_map(params.as_slice(), |row| Self::row_to_board_task(row))?;
            for row in rows {
                let task = row?;
                if let Some(ref pid) = task.parent_id {
                    children_by_parent.entry(pid.clone()).or_default().push(task);
                }
            }
        }

        // Query 3: Fetch all notes in one batch
        let mut all_task_ids: Vec<String> = main_tasks.iter().map(|t| t.id.clone()).collect();
        for children in children_by_parent.values() {
            for child in children {
                all_task_ids.push(child.id.clone());
            }
        }

        let mut notes_by_task: HashMap<String, Vec<BoardTaskNote>> = HashMap::new();
        if !all_task_ids.is_empty() {
            let note_placeholders: Vec<String> = (1..=all_task_ids.len()).map(|i| format!("?{}", i)).collect();
            let note_sql = format!(
                "SELECT id, task_id, content, note_type, author, created_at FROM board_task_notes WHERE task_id IN ({}) ORDER BY created_at ASC",
                note_placeholders.join(",")
            );
            let params: Vec<&dyn rusqlite::ToSql> = all_task_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
            let mut stmt = conn.prepare(&note_sql)?;
            let rows = stmt.query_map(params.as_slice(), Self::row_to_board_task_note)?;
            for row in rows {
                let note = row?;
                notes_by_task.entry(note.task_id.clone()).or_default().push(note);
            }
        }

        // Assemble: build BoardTaskWithContext tree
        let results: Vec<crate::types::BoardTaskWithContext> = main_tasks
            .into_iter()
            .map(|task| {
                let task_id = task.id.clone();
                let notes = notes_by_task.remove(&task_id).unwrap_or_default();
                let children = if include_children {
                    let child_tasks = children_by_parent.remove(&task_id).unwrap_or_default();
                    Some(child_tasks.into_iter().map(|child| {
                        let child_id = child.id.clone();
                        let child_notes = notes_by_task.remove(&child_id).unwrap_or_default();
                        crate::types::BoardTaskWithContext {
                            task: child,
                            notes: child_notes,
                            children: None, // single level
                        }
                    }).collect())
                } else {
                    None
                };
                crate::types::BoardTaskWithContext {
                    task,
                    notes,
                    children,
                }
            })
            .collect();

        Ok(results)
    }

    // ============ Board Task Notes ============

    /// Add a note to a board task
    pub fn add_board_task_note(
        &self,
        input: &AddBoardTaskNoteInput,
    ) -> DbResult<BoardTaskNote> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let note_type_str = input.note_type.as_deref().unwrap_or("note");

        let conn = self.conn();

        // Verify task exists
        let task_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM board_tasks WHERE id = ?1",
            params![input.task_id],
            |row| row.get(0),
        )?;
        if !task_exists {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }

        let note_type = BoardNoteType::from_str(note_type_str).unwrap_or(BoardNoteType::Note);

        conn.execute(
            "INSERT INTO board_task_notes (id, task_id, content, note_type, author, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, input.task_id, input.content, note_type.as_str(), input.author, now],
        )?;

        Ok(BoardTaskNote {
            id,
            task_id: input.task_id.clone(),
            content: input.content.clone(),
            note_type,
            author: input.author.clone(),
            created_at: now,
        })
    }

    /// Get all notes for a board task (ordered by creation time ASC)
    pub fn get_board_task_notes(&self, task_id: &str) -> DbResult<Vec<BoardTaskNote>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, content, note_type, author, created_at
             FROM board_task_notes WHERE task_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![task_id], Self::row_to_board_task_note)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    /// Get a board task with its notes
    pub fn get_board_task_with_notes(
        &self,
        id: &str,
    ) -> DbResult<Option<BoardTaskWithNotes>> {
        if let Some(task) = self.get_board_task(id)? {
            let notes = self.get_board_task_notes(&task.id)?;
            Ok(Some(BoardTaskWithNotes { task, notes }))
        } else {
            Ok(None)
        }
    }

    fn row_to_board_task_note(row: &rusqlite::Row) -> rusqlite::Result<BoardTaskNote> {
        let note_type_str: String = row.get("note_type")?;
        let note_type = BoardNoteType::from_str(&note_type_str).unwrap_or(BoardNoteType::Note);

        Ok(BoardTaskNote {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            content: row.get("content")?,
            note_type,
            author: row.get("author")?,
            created_at: row.get("created_at")?,
        })
    }

    /// Query board tasks where a time column falls within a range.
    /// Used by activity_report to find created/updated tasks in a date range.
    pub fn query_board_tasks_in_range(&self, time_col: &str, since: &str, until: &str) -> DbResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        // Only allow safe column names
        let col = match time_col {
            "created_at" | "updated_at" => time_col,
            _ => return Ok(Vec::new()),
        };
        let sql = format!(
            "SELECT id, title, status, priority, project, {} as ts FROM board_tasks WHERE {} >= ?1 AND {} <= ?2 ORDER BY {} DESC LIMIT 50",
            col, col, col, col
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![since, until], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "priority": row.get::<_, String>(3)?,
                "project": row.get::<_, Option<String>>(4)?,
                "time": row.get::<_, String>(5)?,
            }))
        })?;
        let mut tasks = Vec::new();
        for r in rows { tasks.push(r?); }
        Ok(tasks)
    }

    /// Query board tasks that reached a specific status within a time range.
    pub fn query_board_tasks_in_range_with_status(&self, status: &str, since: &str, until: &str) -> DbResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, title, status, priority, project, updated_at FROM board_tasks WHERE status = ?1 AND updated_at >= ?2 AND updated_at <= ?3 ORDER BY updated_at DESC LIMIT 50"
        )?;
        let rows = stmt.query_map(rusqlite::params![status, since, until], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "priority": row.get::<_, String>(3)?,
                "project": row.get::<_, Option<String>>(4)?,
                "completed_at": row.get::<_, String>(5)?,
            }))
        })?;
        let mut tasks = Vec::new();
        for r in rows { tasks.push(r?); }
        Ok(tasks)
    }


}
