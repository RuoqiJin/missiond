//! Narrow application ports over the legacy `MissionStore` supertrait.
//!
//! Phase 1 keeps `Arc<dyn MissionStore>` as the backing implementation, but
//! new orchestration code should depend on these small traits instead of the
//! whole database surface.

use std::sync::Arc;

use async_trait::async_trait;
use missiond_core::db::error::DbResult;
use missiond_core::db::traits::MissionStore;
use missiond_core::db::TimelineRow;
use missiond_core::types::{
    AddBoardTaskNoteInput, BoardTask, BoardTaskNote, BoardTaskWithNotes, Conversation,
    CreateBoardTaskInput, DependencyStatus, TaskId, UpdateBoardTaskInput,
};

#[derive(Clone)]
pub(crate) struct StorePorts {
    store: Arc<dyn MissionStore>,
}

impl StorePorts {
    pub(crate) fn new(store: Arc<dyn MissionStore>) -> Self {
        Self { store }
    }

    #[allow(dead_code)]
    pub(crate) fn legacy_store(&self) -> Arc<dyn MissionStore> {
        Arc::clone(&self.store)
    }
}

#[async_trait]
pub(crate) trait BoardTaskRepo: Send + Sync {
    async fn create_board_task(&self, input: &CreateBoardTaskInput) -> DbResult<BoardTask>;
    async fn get_board_task(&self, id: &str) -> DbResult<Option<BoardTask>>;
    async fn update_board_task(
        &self,
        id: &str,
        update: &UpdateBoardTaskInput,
    ) -> DbResult<Option<BoardTask>>;
    async fn get_board_task_with_notes(&self, id: &str) -> DbResult<Option<BoardTaskWithNotes>>;
    async fn list_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>>;
    async fn list_running_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>>;
    async fn add_board_task_note(&self, input: &AddBoardTaskNoteInput) -> DbResult<BoardTaskNote>;
    async fn check_dependencies(&self, depends_on: &[TaskId]) -> DbResult<DependencyStatus>;
}

#[async_trait]
impl BoardTaskRepo for StorePorts {
    async fn create_board_task(&self, input: &CreateBoardTaskInput) -> DbResult<BoardTask> {
        self.store.create_board_task(input).await
    }

    async fn get_board_task(&self, id: &str) -> DbResult<Option<BoardTask>> {
        self.store.get_board_task(id).await
    }

    async fn update_board_task(
        &self,
        id: &str,
        update: &UpdateBoardTaskInput,
    ) -> DbResult<Option<BoardTask>> {
        self.store.update_board_task(id, update).await
    }

    async fn get_board_task_with_notes(&self, id: &str) -> DbResult<Option<BoardTaskWithNotes>> {
        self.store.get_board_task_with_notes(id).await
    }

    async fn list_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        self.store.list_autopilot_tasks().await
    }

    async fn list_running_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>> {
        self.store.list_running_autopilot_tasks().await
    }

    async fn add_board_task_note(&self, input: &AddBoardTaskNoteInput) -> DbResult<BoardTaskNote> {
        self.store.add_board_task_note(input).await
    }

    async fn check_dependencies(&self, depends_on: &[TaskId]) -> DbResult<DependencyStatus> {
        self.store.check_dependencies(depends_on).await
    }
}

#[async_trait]
pub(crate) trait SlotLeaseRepo: Send + Sync {
    async fn claim_board_task(
        &self,
        id: &str,
        executor_id: &str,
        executor_type: &str,
    ) -> DbResult<Option<BoardTask>>;
    async fn set_board_task_lease(&self, task_id: &str, lease_expires_at: &str) -> DbResult<usize>;
    async fn unclaim_board_task(&self, task_id: &str) -> DbResult<()>;
    async fn release_board_claims_by_executor(&self, executor_id: &str) -> DbResult<usize>;
}

#[async_trait]
impl SlotLeaseRepo for StorePorts {
    async fn claim_board_task(
        &self,
        id: &str,
        executor_id: &str,
        executor_type: &str,
    ) -> DbResult<Option<BoardTask>> {
        self.store
            .claim_board_task(id, executor_id, executor_type)
            .await
    }

    async fn set_board_task_lease(&self, task_id: &str, lease_expires_at: &str) -> DbResult<usize> {
        self.store
            .set_board_task_lease(task_id, lease_expires_at)
            .await
    }

    async fn unclaim_board_task(&self, task_id: &str) -> DbResult<()> {
        self.store.unclaim_board_task(task_id).await
    }

    async fn release_board_claims_by_executor(&self, executor_id: &str) -> DbResult<usize> {
        self.store
            .release_board_claims_by_executor(executor_id)
            .await
    }
}

#[async_trait]
pub(crate) trait ConversationFinalRepo: Send + Sync {
    async fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>>;
    async fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>>;
    async fn complete_stale_conversations(&self, cutoff: &str) -> DbResult<usize>;
}

#[async_trait]
impl ConversationFinalRepo for StorePorts {
    async fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>> {
        self.store.get_last_assistant_content(session_id).await
    }

    async fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>> {
        self.store.get_conversations_by_task_id(task_id).await
    }

    async fn complete_stale_conversations(&self, cutoff: &str) -> DbResult<usize> {
        self.store.complete_stale_conversations(cutoff).await
    }
}

#[async_trait]
pub(crate) trait EventLogRepo: Send + Sync {
    async fn query_timeline_since(
        &self,
        since_seq: i64,
        limit: usize,
    ) -> DbResult<Vec<TimelineRow>>;
    async fn timeline_latest_seq(&self) -> DbResult<i64>;
    async fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>>;
}

#[async_trait]
impl EventLogRepo for StorePorts {
    async fn query_timeline_since(
        &self,
        since_seq: i64,
        limit: usize,
    ) -> DbResult<Vec<TimelineRow>> {
        self.store.query_timeline_since(since_seq, limit).await
    }

    async fn timeline_latest_seq(&self) -> DbResult<i64> {
        self.store.timeline_latest_seq().await
    }

    async fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        self.store.query_timeline_by_trace(trace_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_ports_is_cloneable_facade() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<StorePorts>();
    }
}
