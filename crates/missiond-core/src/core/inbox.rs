//! Inbox - Message inbox management
//!
//! Manages messages from completed tasks.

use crate::db::MissionDB;
use crate::types::InboxMessage;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

/// Inbox Manager
///
/// Manages the message inbox for task results.
pub struct Inbox {
    db: Arc<MissionDB>,
}

impl Inbox {
    /// Create a new Inbox
    pub fn new(db: Arc<MissionDB>) -> Self {
        Self { db }
    }

    /// Get messages
    ///
    /// # Arguments
    /// * `unread_only` - If true, only return unread messages
    /// * `limit` - Maximum number of messages to return
    pub fn get_messages(&self, unread_only: bool, limit: usize) -> Vec<InboxMessage> {
        self.db
            .get_inbox_messages(unread_only, limit as i64)
            .unwrap_or_default()
    }

    /// Mark a message as read
    pub fn mark_read(&self, message_id: &str) {
        let _ = self.db.mark_inbox_read(message_id);
        debug!(message_id = %message_id, "Message marked as read");
    }

    /// Mark all messages as read
    pub fn mark_all_read(&self) {
        let messages = self
            .db
            .get_inbox_messages(true, 10000)
            .unwrap_or_default();
        let count = messages.len();

        for msg in messages {
            let _ = self.db.mark_inbox_read(&msg.id);
        }

        info!(count = count, "All messages marked as read");
    }

    /// Get unread message count
    pub fn get_unread_count(&self) -> usize {
        self.db
            .get_inbox_messages(true, 10000)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Add a message to the inbox
    pub fn add_message(&self, task_id: &str, from_role: &str, content: &str) {
        let msg = InboxMessage {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            from_role: from_role.to_string(),
            content: content.to_string(),
            read: false,
            created_at: chrono::Utc::now().timestamp_millis(),
        };

        let _ = self.db.insert_inbox_message(&msg);
        debug!(message_id = %msg.id, task_id = %task_id, "Message added to inbox");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_db() -> Arc<MissionDB> {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        Arc::new(MissionDB::open(db_path).unwrap())
    }

    #[test]
    fn test_add_and_get_messages() {
        let db = create_test_db();
        let inbox = Inbox::new(db);

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
        let inbox = Inbox::new(db);

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
        let inbox = Inbox::new(db);

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
        let inbox = Inbox::new(db);

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
        let inbox = Inbox::new(db);

        // Add 5 messages
        for i in 0..5 {
            inbox.add_message(&format!("task-{}", i), "worker", &format!("Result {}", i));
        }

        // Request only 3
        let messages = inbox.get_messages(true, 3);
        assert_eq!(messages.len(), 3);
    }
}
