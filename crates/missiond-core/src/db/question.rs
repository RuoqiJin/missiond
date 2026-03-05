use rusqlite::params;
use super::error::DbResult;
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ Agent Questions (Pending Decisions) ============

    pub fn create_agent_question(
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
            decision_type: input.decision_type.clone().unwrap_or_else(|| "implementation".to_string()),
            retry_count: 0,
            routing_trace: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO agent_questions (id, task_id, slot_id, session_id, question, context, status, answer, target, options, decision_type, retry_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                q.id, q.task_id, q.slot_id, q.session_id,
                q.question, q.context, q.status.as_str(),
                q.answer, q.target, q.options, q.decision_type,
                q.retry_count, q.created_at, q.updated_at,
            ],
        )?;

        // Auto-block: if question is linked to a board task, mark task as blocked
        if let Some(ref tid) = q.task_id {
            conn.execute(
                "UPDATE board_tasks SET status = 'blocked', updated_at = ?1 WHERE id = ?2 AND status IN ('open', 'running')",
                params![q.created_at, tid],
            )?;
            tracing::info!(task_id = %tid, question_id = %q.id, "Question created → task auto-blocked");
        }

        Ok(q)
    }

    pub fn get_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>> {
        let conn = self.read_conn();
        let mut stmt = conn
            .prepare("SELECT * FROM agent_questions WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_agent_question(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_agent_questions(
        &self,
        status: Option<&str>,
        target: Option<&str>,
        limit: Option<usize>,
    ) -> DbResult<Vec<AgentQuestion>> {
        let conn = self.read_conn();
        let mut sql = "SELECT * FROM agent_questions WHERE 1=1".to_string();
        let mut param_values: Vec<String> = Vec::new();

        if let Some(s) = status {
            param_values.push(s.to_string());
            sql.push_str(&format!(" AND status = ?{}", param_values.len()));
        }
        if let Some(t) = target {
            param_values.push(t.to_string());
            sql.push_str(&format!(" AND target = ?{}", param_values.len()));
        }
        sql.push_str(" ORDER BY created_at DESC");
        if let Some(l) = limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| Self::row_to_agent_question(row))?;
        let mut questions = Vec::new();
        for q in rows {
            questions.push(q?);
        }
        Ok(questions)
    }

    /// List answered questions linked to a specific board task
    pub fn list_questions_for_task(&self, task_id: &str) -> DbResult<Vec<AgentQuestion>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM agent_questions WHERE task_id = ?1 AND status = 'answered' ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| Self::row_to_agent_question(row))?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Increment retry_count and set status back to open for retry.
    /// Also clears claim fields so CAS re-claim can succeed.
    pub fn increment_board_task_retry(&self, task_id: &str, new_retry: i64) -> DbResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE board_tasks SET retry_count = ?1, status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = ?2 WHERE id = ?3",
            params![new_retry, now, task_id],
        )?;
        Ok(())
    }

    /// Release CAS claim and revert task to open.
    /// Used when PTY spawn times out or other pre-send failures occur.
    pub fn unclaim_board_task(&self, task_id: &str) -> DbResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )?;
        Ok(())
    }

    pub fn answer_agent_question(
        &self,
        id: &str,
        answer: &str,
    ) -> DbResult<Option<AgentQuestion>> {
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = self.conn();
            conn.execute(
                "UPDATE agent_questions SET answer = ?1, status = 'answered', updated_at = ?2 WHERE id = ?3",
                params![answer, now, id],
            )?;

            // Auto-unblock: if all questions for the linked task are resolved, unblock the task
            // First get the task_id from this question
            let task_id: Option<String> = conn.query_row(
                "SELECT task_id FROM agent_questions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            ).ok().flatten();

            if let Some(ref tid) = task_id {
                // Check if any pending questions remain for this task
                let pending_count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM agent_questions WHERE task_id = ?1 AND status = 'pending'",
                    params![tid],
                    |row| row.get(0),
                ).unwrap_or(0);

                if pending_count == 0 {
                    conn.execute(
                        "UPDATE board_tasks SET status = 'open', updated_at = ?1 WHERE id = ?2 AND status = 'blocked'",
                        params![now, tid],
                    )?;
                    tracing::info!(task_id = %tid, "All questions answered → task auto-unblocked");
                }
            }
        }
        self.get_agent_question(id)
    }

    pub fn dismiss_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>> {
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = self.conn();
            conn.execute(
                "UPDATE agent_questions SET status = 'dismissed', updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        self.get_agent_question(id)
    }

    /// Decision Engine: set routing trace JSON for a question
    pub fn set_question_routing_trace(&self, id: &str, trace_json: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE agent_questions SET routing_trace = ?1, updated_at = ?2 WHERE id = ?3",
            params![trace_json, now, id],
        )?;
        Ok(())
    }

    /// Decision Engine: downgrade question target from master to user
    pub fn downgrade_question_to_user(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE agent_questions SET target = 'user', updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Decision Engine: increment retry_count for a question
    pub fn increment_question_retry(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE agent_questions SET retry_count = retry_count + 1, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Decision Engine: find pending master questions older than max_age_secs
    pub fn find_stale_master_questions(&self, max_age_secs: i64) -> DbResult<Vec<AgentQuestion>> {
        let conn = self.read_conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT * FROM agent_questions
             WHERE target = 'master' AND status = 'pending'
               AND julianday(?1) - julianday(created_at) > ?2 / 86400.0"
        )?;
        let rows = stmt.query_map(params![now, max_age_secs as f64], |row| {
            Self::row_to_agent_question(row)
        })?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Find board tasks with ≥ min_count answered master questions (for checkpoint harvesting)
    pub fn find_tasks_with_unharvested_decisions(&self, min_count: usize) -> DbResult<Vec<(String, String, usize)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT bt.id, bt.title, COUNT(aq.id) as q_count
             FROM board_tasks bt
             JOIN agent_questions aq ON aq.task_id = bt.id
             WHERE bt.status NOT IN ('done', 'skipped', 'failed')
               AND aq.target = 'master' AND aq.status = 'answered'
             GROUP BY bt.id
             HAVING q_count >= ?1"
        )?;
        let rows = stmt.query_map(params![min_count as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as usize,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Decision Engine: aggregate statistics for monitoring dashboard
    pub fn decision_stats(&self, hours: i64) -> DbResult<serde_json::Value> {
        let conn = self.read_conn();
        let cutoff = (chrono::Utc::now() - chrono::TimeDelta::hours(hours)).to_rfc3339();

        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

        let answered: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND status = 'answered' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND status = 'pending'",
            [], |row| row.get(0),
        ).unwrap_or(0);

        let dismissed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE target = 'master' AND status = 'dismissed' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

        // Count by resolved_tier from routing_trace
        let t1_hits: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"T1\"%' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

        let t2_hits: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"T2\"%' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

        let t3_hits: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"T3\"%' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

        let downgraded: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE routing_trace LIKE '%\"resolved_tier\":\"downgraded\"%' AND created_at > ?1",
            params![cutoff], |row| row.get(0),
        ).unwrap_or(0);

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

    fn row_to_agent_question(row: &rusqlite::Row) -> rusqlite::Result<AgentQuestion> {
        let status_str: String = row.get("status")?;
        let status =
            AgentQuestionStatus::from_str(&status_str).unwrap_or(AgentQuestionStatus::Pending);
        Ok(AgentQuestion {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            slot_id: row.get("slot_id")?,
            session_id: row.get("session_id")?,
            question: row.get("question")?,
            context: row.get("context")?,
            status,
            answer: row.get("answer")?,
            target: row.get::<_, Option<String>>("target")?.unwrap_or_else(|| "user".to_string()),
            options: row.get("options")?,
            decision_type: row.get::<_, Option<String>>("decision_type")?.unwrap_or_else(|| "implementation".to_string()),
            retry_count: row.get::<_, Option<i64>>("retry_count")?.unwrap_or(0),
            routing_trace: row.get("routing_trace")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }


}
