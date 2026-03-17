use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::BoardStore;
use crate::types::*;

#[async_trait]
impl BoardStore for SqliteMissionStore {
    // ========== board.rs: task CRUD ==========

    async fn insert_board_task(&self, task: &BoardTask) -> DbResult<()> {
        let task = task.clone();
        self.executor.run(move |db| db.insert_board_task(&task)).await
    }

    async fn create_board_task(&self, input: &CreateBoardTaskInput) -> DbResult<BoardTask> {
        let input = input.clone();
        self.executor.run(move |db| db.create_board_task(&input)).await
    }

    async fn get_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.get_board_task(&id)).await
    }

    async fn list_board_tasks(&self, status: Option<&str>, include_hidden: bool) -> DbResult<Vec<BoardTask>> {
        let status = status.map(|s| s.to_owned());
        self.executor.run(move |db| db.list_board_tasks(status.as_deref(), include_hidden)).await
    }

    async fn update_board_task(&self, id: &str, update: &UpdateBoardTaskInput) -> DbResult<Option<BoardTask>> {
        let id = id.to_owned();
        let update = update.clone();
        self.executor.run(move |db| db.update_board_task(&id, &update)).await
    }

    async fn delete_board_task(&self, id: &str) -> DbResult<i64> {
        let id = id.to_owned();
        self.executor.run(move |db| db.delete_board_task(&id)).await
    }

    async fn toggle_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.toggle_board_task(&id)).await
    }

    async fn resolve_board_task_id(&self, id: &str) -> DbResult<Option<String>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.resolve_board_task_id(&id)).await
    }

    async fn find_open_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>> {
        let dedupe_key = dedupe_key.to_owned();
        self.executor.run(move |db| db.find_open_task_by_dedupe_key(&dedupe_key)).await
    }

    async fn close_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>> {
        let dedupe_key = dedupe_key.to_owned();
        self.executor.run(move |db| db.close_task_by_dedupe_key(&dedupe_key)).await
    }

    async fn search_board_tasks(&self, input: &BoardSearchInput) -> DbResult<BoardSearchResult> {
        let input = input.clone();
        self.executor.run(move |db| db.search_board_tasks(&input)).await
    }

    async fn board_summary(&self, since: Option<&str>) -> DbResult<serde_json::Value> {
        let since = since.map(|s| s.to_owned());
        self.executor.run(move |db| db.board_summary(since.as_deref())).await
    }

    // ========== board.rs: claiming & autopilot ==========

    async fn claim_board_task(&self, id: &str, executor_id: &str, executor_type: &str) -> DbResult<Option<BoardTask>> {
        let id = id.to_owned();
        let executor_id = executor_id.to_owned();
        let executor_type = executor_type.to_owned();
        self.executor.run(move |db| db.claim_board_task(&id, &executor_id, &executor_type)).await
    }

    async fn release_board_claims_by_executor(&self, executor_id: &str) -> DbResult<usize> {
        let executor_id = executor_id.to_owned();
        self.executor.run(move |db| db.release_board_claims_by_executor(&executor_id)).await
    }

    async fn recover_stale_running_tasks(&self, fallback_stale_minutes: i64) -> DbResult<usize> {
        self.executor.run(move |db| db.recover_stale_running_tasks(fallback_stale_minutes)).await
    }

    async fn set_board_task_lease(&self, task_id: &str, lease_expires_at: &str) -> DbResult<usize> {
        let task_id = task_id.to_owned();
        let lease_expires_at = lease_expires_at.to_owned();
        self.executor.run(move |db| db.set_board_task_lease(&task_id, &lease_expires_at)).await
    }

    async fn list_running_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        self.executor.run(|db| db.list_running_autopilot_tasks()).await
    }

    async fn list_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        self.executor.run(|db| db.list_autopilot_tasks()).await
    }

    async fn count_open_tasks_by_priority(&self, priorities: &[&str]) -> DbResult<i64> {
        let priorities: Vec<String> = priorities.iter().map(|s| s.to_string()).collect();
        self.executor.run(move |db| {
            let refs: Vec<&str> = priorities.iter().map(|s| s.as_str()).collect();
            db.count_open_tasks_by_priority(&refs)
        }).await
    }

    async fn count_tasks_by_category(&self, category: &str) -> DbResult<i64> {
        let category = category.to_owned();
        self.executor.run(move |db| db.count_tasks_by_category(&category)).await
    }

    async fn clear_done_board_tasks(&self) -> DbResult<i64> {
        self.executor.run(|db| db.clear_done_board_tasks()).await
    }

    // ========== board.rs: dependencies (DAG) ==========

    async fn check_dependencies(&self, depends_on: &[TaskId]) -> DbResult<DependencyStatus> {
        let depends_on = depends_on.to_vec();
        self.executor.run(move |db| db.check_dependencies(&depends_on)).await
    }

    async fn find_downstream_tasks(&self, task_id: &str) -> DbResult<Vec<String>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.find_downstream_tasks(&task_id)).await
    }

    async fn retry_board_task(&self, task_id: &str, reset_downstream: bool) -> DbResult<Vec<String>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.retry_board_task(&task_id, reset_downstream)).await
    }

    // ========== board.rs: context & notes ==========

    async fn get_board_tasks_with_context(&self, ids: &[String], include_children: bool) -> DbResult<Vec<BoardTaskWithContext>> {
        let ids = ids.to_vec();
        self.executor.run(move |db| db.get_board_tasks_with_context(&ids, include_children)).await
    }

    async fn add_board_task_note(&self, input: &AddBoardTaskNoteInput) -> DbResult<BoardTaskNote> {
        let input = input.clone();
        self.executor.run(move |db| db.add_board_task_note(&input)).await
    }

    async fn get_board_task_notes(&self, task_id: &str) -> DbResult<Vec<BoardTaskNote>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.get_board_task_notes(&task_id)).await
    }

    async fn get_board_task_with_notes(&self, id: &str) -> DbResult<Option<BoardTaskWithNotes>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.get_board_task_with_notes(&id)).await
    }

    async fn query_board_tasks_in_range(&self, time_col: &str, since: &str, until: &str) -> DbResult<Vec<serde_json::Value>> {
        let time_col = time_col.to_owned();
        let since = since.to_owned();
        let until = until.to_owned();
        self.executor.run(move |db| db.query_board_tasks_in_range(&time_col, &since, &until)).await
    }

    async fn query_board_tasks_in_range_with_status(&self, status: &str, since: &str, until: &str) -> DbResult<Vec<serde_json::Value>> {
        let status = status.to_owned();
        let since = since.to_owned();
        let until = until.to_owned();
        self.executor.run(move |db| db.query_board_tasks_in_range_with_status(&status, &since, &until)).await
    }

    // ========== question.rs: agent decision requests ==========

    async fn create_agent_question(&self, input: &CreateAgentQuestionInput) -> DbResult<AgentQuestion> {
        let input = input.clone();
        self.executor.run(move |db| db.create_agent_question(&input)).await
    }

    async fn get_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.get_agent_question(&id)).await
    }

    async fn list_agent_questions(&self, status: Option<&str>, target: Option<&str>, limit: Option<usize>) -> DbResult<Vec<AgentQuestion>> {
        let status = status.map(|s| s.to_owned());
        let target = target.map(|s| s.to_owned());
        self.executor.run(move |db| db.list_agent_questions(status.as_deref(), target.as_deref(), limit)).await
    }

    async fn list_questions_for_task(&self, task_id: &str) -> DbResult<Vec<AgentQuestion>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.list_questions_for_task(&task_id)).await
    }

    async fn answer_agent_question(&self, id: &str, answer: &str) -> DbResult<Option<AgentQuestion>> {
        let id = id.to_owned();
        let answer = answer.to_owned();
        self.executor.run(move |db| db.answer_agent_question(&id, &answer)).await
    }

    async fn dismiss_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.dismiss_agent_question(&id)).await
    }

    async fn set_question_routing_trace(&self, id: &str, trace_json: &str) -> DbResult<()> {
        let id = id.to_owned();
        let trace_json = trace_json.to_owned();
        self.executor.run(move |db| db.set_question_routing_trace(&id, &trace_json)).await
    }

    async fn downgrade_question_to_user(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.downgrade_question_to_user(&id)).await
    }

    async fn increment_question_retry(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.increment_question_retry(&id)).await
    }

    async fn find_stale_master_questions(&self, max_age_secs: i64) -> DbResult<Vec<AgentQuestion>> {
        self.executor.run(move |db| db.find_stale_master_questions(max_age_secs)).await
    }

    async fn find_tasks_with_unharvested_decisions(&self, min_count: usize) -> DbResult<Vec<(String, String, usize)>> {
        self.executor.run(move |db| db.find_tasks_with_unharvested_decisions(min_count)).await
    }

    async fn decision_stats(&self, hours: i64) -> DbResult<serde_json::Value> {
        self.executor.run(move |db| db.decision_stats(hours)).await
    }

    async fn increment_board_task_retry(&self, task_id: &str, new_retry: i64) -> DbResult<()> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.increment_board_task_retry(&task_id, new_retry)).await
    }

    async fn unclaim_board_task(&self, task_id: &str) -> DbResult<()> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.unclaim_board_task(&task_id)).await
    }
}
