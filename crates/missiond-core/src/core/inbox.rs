//! Inbox - Message inbox management
//!
//! Manages messages from completed tasks.

#[cfg(feature = "sqlite")]
use crate::db::MissionDB;
use crate::types::InboxMessage;
#[cfg(feature = "sqlite")]
use std::sync::Arc;
#[cfg(feature = "sqlite")]
use tracing::{debug, info};
#[cfg(feature = "sqlite")]
use uuid::Uuid;

/// Inbox Manager
///
/// Manages the message inbox for task results.
pub struct Inbox {
    #[cfg(feature = "sqlite")]
    db: Option<Arc<MissionDB>>,
}

impl Inbox {
    /// Create a new Inbox (SQLite mode). `db` is optional — when None,
    /// all operations are no-ops (handled by async store instead).
    #[cfg(feature = "sqlite")]
    pub fn new(db: Option<Arc<MissionDB>>) -> Self {
        Self { db }
    }

    /// Create a new Inbox (PG mode — no SQLite DB).
    #[cfg(not(feature = "sqlite"))]
    pub fn new() -> Self {
        Self {}
    }

    /// Get messages
    ///
    /// # Arguments
    /// * `unread_only` - If true, only return unread messages
    /// * `limit` - Maximum number of messages to return
    #[cfg(feature = "sqlite")]
    pub fn get_messages(&self, unread_only: bool, limit: usize) -> Vec<InboxMessage> {
        self.db.as_ref()
            .and_then(|db| db.get_inbox_messages(unread_only, limit as i64).ok())
            .unwrap_or_default()
    }

    #[cfg(not(feature = "sqlite"))]
    pub fn get_messages(&self, _unread_only: bool, _limit: usize) -> Vec<InboxMessage> {
        Vec::new()
    }

    /// Mark a message as read
    #[cfg(feature = "sqlite")]
    pub fn mark_read(&self, message_id: &str) {
        if let Some(ref db) = self.db {
            let _ = db.mark_inbox_read(message_id);
            debug!(message_id = %message_id, "Message marked as read");
        }
    }

    #[cfg(not(feature = "sqlite"))]
    pub fn mark_read(&self, _message_id: &str) {}

    /// Mark all messages as read
    #[cfg(feature = "sqlite")]
    pub fn mark_all_read(&self) {
        let Some(ref db) = self.db else { return };
        let messages = db.get_inbox_messages(true, 10000).unwrap_or_default();
        let count = messages.len();
        for msg in messages {
            let _ = db.mark_inbox_read(&msg.id);
        }
        info!(count = count, "All messages marked as read");
    }

    #[cfg(not(feature = "sqlite"))]
    pub fn mark_all_read(&self) {}

    /// Get unread message count
    #[cfg(feature = "sqlite")]
    pub fn get_unread_count(&self) -> usize {
        self.db.as_ref()
            .and_then(|db| db.get_inbox_messages(true, 10000).ok())
            .map(|v| v.len())
            .unwrap_or(0)
    }

    #[cfg(not(feature = "sqlite"))]
    pub fn get_unread_count(&self) -> usize { 0 }

    /// Add a message to the inbox
    #[cfg(feature = "sqlite")]
    pub fn add_message(&self, task_id: &str, from_role: &str, content: &str) {
        let Some(ref db) = self.db else { return };
        let msg = InboxMessage {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            from_role: from_role.to_string(),
            content: content.to_string(),
            read: false,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        let _ = db.insert_inbox_message(&msg);
        debug!(message_id = %msg.id, task_id = %task_id, "Message added to inbox");
    }

    #[cfg(not(feature = "sqlite"))]
    pub fn add_message(&self, _task_id: &str, _from_role: &str, _content: &str) {}
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    fn create_test_db() -> Arc<MissionDB> {
        Arc::new(MissionDB::in_memory().unwrap())
    }

    #[test]
    fn test_add_and_get_messages() {
        let db = create_test_db();
        let inbox = Inbox::new(Some(db));

        // Add messages
        inbox.add_message("task-1", "worker", "Result 1");
        inbox.add_message("task-2", "specialist", "Result 2");

        // Get all messages
        let messages = inbox.get_messages(false, 10);
        assert_eq!(messages.len(), 2);

        // Get unread messages
        let unread = inbox.get_messages(true, 10);
        assert_eq!(unread.len(), 2);

        // Check content
        assert!(messages.iter().any(|m| m.content == "Result 1"));
        assert!(messages.iter().any(|m| m.content == "Result 2"));
    }

    #[test]
    fn test_mark_read() {
        let db = create_test_db();
        let inbox = Inbox::new(Some(db));

        // Add a message
        inbox.add_message("task-1", "worker", "Result");

        // Get the message ID
        let messages = inbox.get_messages(true, 10);
        assert_eq!(messages.len(), 1);
        let msg_id = &messages[0].id;

        // Mark as read
        inbox.mark_read(msg_id);

        // Should no longer appear in unread
        let unread = inbox.get_messages(true, 10);
        assert_eq!(unread.len(), 0);

        // But still appears in all messages
        let all = inbox.get_messages(false, 10);
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_mark_all_read() {
        let db = create_test_db();
        let inbox = Inbox::new(Some(db));

        // Add multiple messages
        inbox.add_message("task-1", "worker", "Result 1");
        inbox.add_message("task-2", "worker", "Result 2");
        inbox.add_message("task-3", "worker", "Result 3");

        // All should be unread
        assert_eq!(inbox.get_unread_count(), 3);

        // Mark all read
        inbox.mark_all_read();

        // None should be unread
        assert_eq!(inbox.get_unread_count(), 0);
    }

    #[test]
    fn test_get_unread_count() {
        let db = create_test_db();
        let inbox = Inbox::new(Some(db));

        assert_eq!(inbox.get_unread_count(), 0);

        inbox.add_message("task-1", "worker", "Result 1");
        assert_eq!(inbox.get_unread_count(), 1);

        inbox.add_message("task-2", "worker", "Result 2");
        assert_eq!(inbox.get_unread_count(), 2);

        // Mark one as read
        let messages = inbox.get_messages(true, 1);
        inbox.mark_read(&messages[0].id);

        assert_eq!(inbox.get_unread_count(), 1);
    }

    #[test]
    fn test_limit() {
        let db = create_test_db();
        let inbox = Inbox::new(Some(db));

        // Add 5 messages
        for i in 0..5 {
            inbox.add_message(&format!("task-{}", i), "worker", &format!("Result {}", i));
        }

        // Request only 3
        let messages = inbox.get_messages(true, 3);
        assert_eq!(messages.len(), 3);
    }
}
