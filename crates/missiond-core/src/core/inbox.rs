//! Inbox - Message inbox management
//!
//! Manages messages from completed tasks.
//!
//! Post-v0.4.23 Stage 2E: all operations are no-ops — the async `MessageStore`
//! trait on the PG-backed `MissionStore` is the source of truth.

use crate::types::InboxMessage;

/// Inbox Manager
///
/// Manages the message inbox for task results.
pub struct Inbox {}

impl Inbox {
    /// Create a new Inbox (PG mode — no SQLite DB).
    pub fn new() -> Self {
        Self {}
    }

    /// Get messages
    ///
    /// # Arguments
    /// * `unread_only` - If true, only return unread messages
    /// * `limit` - Maximum number of messages to return
    pub fn get_messages(&self, _unread_only: bool, _limit: usize) -> Vec<InboxMessage> {
        Vec::new()
    }

    /// Mark a message as read
    pub fn mark_read(&self, _message_id: &str) {}

    /// Mark all messages as read
    pub fn mark_all_read(&self) {}

    /// Get unread message count
    pub fn get_unread_count(&self) -> usize { 0 }

    /// Add a message to the inbox
    pub fn add_message(&self, _task_id: &str, _from_role: &str, _content: &str) {}
}
