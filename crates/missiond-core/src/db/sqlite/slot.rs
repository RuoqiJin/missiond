use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::SlotStore;
use crate::types::*;

#[async_trait]
impl SlotStore for SqliteMissionStore {
    // -- slot.rs: session mapping --

    async fn get_slot_session(&self, slot_id: &str) -> DbResult<Option<String>> {
        let slot_id = slot_id.to_owned();
        self.executor.run(move |db| db.get_slot_session(&slot_id)).await
    }

    async fn set_slot_session(&self, slot_id: &str, session_id: &str) -> DbResult<()> {
        let slot_id = slot_id.to_owned();
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.set_slot_session(&slot_id, &session_id)).await
    }

    async fn cleanup_pty_placeholder(&self, slot_id: &str) -> DbResult<()> {
        let slot_id = slot_id.to_owned();
        self.executor.run(move |db| db.cleanup_pty_placeholder(&slot_id)).await
    }

    async fn delete_slot_session(&self, slot_id: &str) -> DbResult<()> {
        let slot_id = slot_id.to_owned();
        self.executor.run(move |db| db.delete_slot_session(&slot_id)).await
    }

    async fn get_all_slot_sessions(&self) -> DbResult<Vec<(String, String)>> {
        self.executor.run(move |db| db.get_all_slot_sessions()).await
    }

    async fn get_slot_for_session(&self, session_id: &str) -> DbResult<Option<String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_slot_for_session(&session_id)).await
    }

    // -- slot.rs: slot tasks --

    async fn insert_slot_task(&self, task: &SlotTask) -> DbResult<()> {
        let task = task.clone();
        self.executor.run(move |db| db.insert_slot_task(&task)).await
    }

    async fn slot_task_set_running(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.slot_task_set_running(&id)).await
    }

    async fn slot_task_set_completed(&self, id: &str, output_count: i64) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.slot_task_set_completed(&id, output_count)).await
    }

    async fn slot_task_set_failed(&self, id: &str, error: &str) -> DbResult<()> {
        let id = id.to_owned();
        let error = error.to_owned();
        self.executor.run(move |db| db.slot_task_set_failed(&id, &error)).await
    }

    async fn list_slot_tasks(&self, slot_id: Option<&str>, task_type: Option<&str>, status: Option<&str>, limit: i64) -> DbResult<Vec<SlotTask>> {
        let slot_id = slot_id.map(|s| s.to_owned());
        let task_type = task_type.map(|s| s.to_owned());
        let status = status.map(|s| s.to_owned());
        self.executor.run(move |db| db.list_slot_tasks(slot_id.as_deref(), task_type.as_deref(), status.as_deref(), limit)).await
    }

    async fn slot_task_stats(&self, slot_id: Option<&str>) -> DbResult<serde_json::Value> {
        let slot_id = slot_id.map(|s| s.to_owned());
        self.executor.run(move |db| db.slot_task_stats(slot_id.as_deref())).await
    }

    async fn reap_stale_slot_tasks(&self, max_age_secs: i64) -> DbResult<usize> {
        self.executor.run(move |db| db.reap_stale_slot_tasks(max_age_secs)).await
    }

    async fn find_stale_decision_tasks(&self, max_age_secs: i64) -> DbResult<Vec<SlotTask>> {
        self.executor.run(move |db| db.find_stale_decision_tasks(max_age_secs)).await
    }

    async fn cleanup_orphan_slot_tasks(&self) -> DbResult<usize> {
        self.executor.run(move |db| db.cleanup_orphan_slot_tasks()).await
    }

    async fn last_completed_slot_task_at(&self, task_type: &str) -> DbResult<Option<i64>> {
        let task_type = task_type.to_owned();
        self.executor.run(move |db| db.last_completed_slot_task_at(&task_type)).await
    }

    async fn get_running_slot_task(&self, slot_id: &str) -> DbResult<Option<String>> {
        let slot_id = slot_id.to_owned();
        self.executor.run(move |db| db.get_running_slot_task(&slot_id)).await
    }

    // -- slot.rs: daemon state --

    async fn daemon_state_get(&self, key: &str) -> DbResult<Option<i64>> {
        let key = key.to_owned();
        self.executor.run(move |db| db.daemon_state_get(&key)).await
    }

    async fn daemon_state_set(&self, key: &str, value: i64) -> DbResult<()> {
        let key = key.to_owned();
        self.executor.run(move |db| db.daemon_state_set(&key, value)).await
    }

    // -- task.rs: generic tasks --

    async fn insert_task(&self, task: &Task) -> DbResult<()> {
        let task = task.clone();
        self.executor.run(move |db| db.insert_task(&task)).await
    }

    async fn update_task(&self, id: &str, update: &TaskUpdate) -> DbResult<()> {
        let id = id.to_owned();
        let update = update.clone();
        self.executor.run(move |db| db.update_task(&id, &update)).await
    }

    async fn get_task(&self, id: &str) -> DbResult<Option<Task>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.get_task(&id)).await
    }

    async fn get_tasks_by_status(&self, status: TaskStatus) -> DbResult<Vec<Task>> {
        self.executor.run(move |db| db.get_tasks_by_status(status)).await
    }

    async fn get_queued_tasks_by_role(&self, role: &str) -> DbResult<Vec<Task>> {
        let role = role.to_owned();
        self.executor.run(move |db| db.get_queued_tasks_by_role(&role)).await
    }

    async fn requeue_running_tasks_for_slot(&self, slot_id: &str) -> DbResult<usize> {
        let slot_id = slot_id.to_owned();
        self.executor.run(move |db| db.requeue_running_tasks_for_slot(&slot_id)).await
    }

    async fn get_all_tasks(&self, limit: i64) -> DbResult<Vec<Task>> {
        self.executor.run(move |db| db.get_all_tasks(limit)).await
    }

    async fn ack_completed_tasks(&self, since: Option<i64>) -> DbResult<Vec<Task>> {
        self.executor.run(move |db| db.ack_completed_tasks(since)).await
    }

    // -- task.rs: inbox --

    async fn insert_inbox_message(&self, msg: &InboxMessage) -> DbResult<()> {
        let msg = msg.clone();
        self.executor.run(move |db| db.insert_inbox_message(&msg)).await
    }

    async fn get_inbox_messages(&self, unread_only: bool, limit: i64) -> DbResult<Vec<InboxMessage>> {
        self.executor.run(move |db| db.get_inbox_messages(unread_only, limit)).await
    }

    async fn mark_inbox_read(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.mark_inbox_read(&id)).await
    }

    // -- task.rs: events (legacy) --

    async fn insert_event(&self, task_id: &str, event_type: EventType, data: Option<&serde_json::Value>, timestamp: i64) -> DbResult<i64> {
        let task_id = task_id.to_owned();
        let data = data.cloned();
        self.executor.run(move |db| db.insert_event(&task_id, event_type, data.as_ref(), timestamp)).await
    }

    async fn get_events_by_task(&self, task_id: &str) -> DbResult<Vec<TaskEvent>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.get_events_by_task(&task_id)).await
    }

    // -- dynamic_slot.rs --

    async fn create_dynamic_slot(&self, slot: &DynamicSlot) -> DbResult<()> {
        let slot = slot.clone();
        self.executor.run(move |db| db.create_dynamic_slot(&slot)).await
    }

    async fn get_dynamic_slot(&self, id: &str) -> DbResult<Option<DynamicSlot>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.get_dynamic_slot(&id)).await
    }

    async fn list_dynamic_slots(&self, status: Option<&str>) -> DbResult<Vec<DynamicSlot>> {
        let status = status.map(|s| s.to_owned());
        self.executor.run(move |db| db.list_dynamic_slots(status.as_deref())).await
    }

    async fn count_active_dynamic_slots(&self) -> DbResult<i64> {
        self.executor.run(move |db| db.count_active_dynamic_slots()).await
    }

    async fn terminate_dynamic_slot(&self, id: &str, reason: &str) -> DbResult<bool> {
        let id = id.to_owned();
        let reason = reason.to_owned();
        self.executor.run(move |db| db.terminate_dynamic_slot(&id, &reason)).await
    }

    async fn extend_dynamic_slot(&self, id: &str, additional_seconds: i64) -> DbResult<Option<String>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.extend_dynamic_slot(&id, additional_seconds)).await
    }

    async fn find_expired_dynamic_slots(&self) -> DbResult<Vec<DynamicSlot>> {
        self.executor.run(move |db| db.find_expired_dynamic_slots()).await
    }

    async fn find_expiring_dynamic_slots(&self, within_seconds: i64) -> DbResult<Vec<DynamicSlot>> {
        self.executor.run(move |db| db.find_expiring_dynamic_slots(within_seconds)).await
    }
}
