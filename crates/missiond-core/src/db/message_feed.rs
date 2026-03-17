//! MessageFeed: unified query interface for conversation messages (Layer 3).
//! Builder pattern — 80% unified + 20% specialization for FTS/vector search.

use super::error::DbResult;
use super::MissionDB;
use crate::types::ConversationMessage;
use std::collections::HashMap;

/// Unified cursor for pagination / checkpoint resumption.
#[derive(Debug, Clone)]
pub struct MessageCursor {
    /// Last message ID in the result set (for since_id continuation)
    pub last_id: Option<i64>,
    /// Total messages matching the query (before limit)
    pub total_matched: usize,
    /// Whether there are more results beyond the limit
    pub has_more: bool,
}

/// Result of a MessageFeed query.
#[derive(Debug)]
pub struct MessageFeed {
    pub messages: Vec<ConversationMessage>,
    /// Labels for each message: message_id → [(label, value)]
    pub labels: HashMap<i64, Vec<(String, String)>>,
    pub cursor: MessageCursor,
}

/// Builder for constructing message queries.
pub struct MessageFeedBuilder {
    session_ids: Option<Vec<String>>,
    roles: Option<Vec<String>>,
    labels_filter: Vec<(String, Option<String>)>,
    exclude_labels: Vec<String>,
    since_id: Option<i64>,
    since_time: Option<String>,
    conversation_type: Option<String>,
    limit: i64,
    include_raw: bool,
    include_labels: bool,
}

impl MessageFeedBuilder {
    fn new() -> Self {
        Self {
            session_ids: None,
            roles: None,
            labels_filter: Vec::new(),
            exclude_labels: Vec::new(),
            since_id: None,
            since_time: None,
            conversation_type: None,
            limit: 50,
            include_raw: false,
            include_labels: false,
        }
    }

    /// Filter by session ID(s).
    pub fn session(mut self, session_id: &str) -> Self {
        self.session_ids = Some(vec![session_id.to_string()]);
        self
    }

    pub fn sessions(mut self, ids: Vec<String>) -> Self {
        self.session_ids = Some(ids);
        self
    }

    /// Filter by role(s).
    pub fn roles(mut self, roles: Vec<&str>) -> Self {
        self.roles = Some(roles.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Require messages to have a specific label (optionally with value).
    pub fn with_label(mut self, label: &str, value: Option<&str>) -> Self {
        self.labels_filter.push((label.to_string(), value.map(|s| s.to_string())));
        self
    }

    /// Exclude messages that have a specific label.
    pub fn exclude_label(mut self, label: &str) -> Self {
        self.exclude_labels.push(label.to_string());
        self
    }

    /// Only messages after this ID (watermark).
    pub fn since_id(mut self, id: i64) -> Self {
        self.since_id = Some(id);
        self
    }

    /// Only messages after this timestamp.
    pub fn since_time(mut self, ts: &str) -> Self {
        self.since_time = Some(ts.to_string());
        self
    }

    /// Filter by conversation type (user/meta/worker).
    pub fn conversation_type(mut self, ct: &str) -> Self {
        self.conversation_type = Some(ct.to_string());
        self
    }

    pub fn limit(mut self, n: i64) -> Self {
        self.limit = n;
        self
    }

    pub fn include_raw(mut self, yes: bool) -> Self {
        self.include_raw = yes;
        self
    }

    /// Populate labels in the result.
    pub fn include_labels(mut self, yes: bool) -> Self {
        self.include_labels = yes;
        self
    }

    /// Execute the query.
    pub fn fetch(self, db: &MissionDB) -> DbResult<MessageFeed> {
        let conn = db.read_conn();

        // Build dynamic SQL
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut param_idx = 1;

        // Session filter
        if let Some(ref sids) = self.session_ids {
            if sids.len() == 1 {
                conditions.push(format!("m.session_id = ?{}", param_idx));
                param_values.push(Box::new(sids[0].clone()));
                param_idx += 1;
            } else {
                let placeholders: Vec<String> = sids.iter().enumerate()
                    .map(|(i, _)| format!("?{}", param_idx + i))
                    .collect();
                conditions.push(format!("m.session_id IN ({})", placeholders.join(",")));
                for sid in sids {
                    param_values.push(Box::new(sid.clone()));
                    param_idx += 1;
                }
            }
        }

        // Conversation type filter (requires JOIN)
        let needs_conv_join = self.conversation_type.is_some();
        if let Some(ref ct) = self.conversation_type {
            conditions.push(format!("c.conversation_type = ?{}", param_idx));
            param_values.push(Box::new(ct.clone()));
            param_idx += 1;
        }

        // Role filter
        if let Some(ref roles) = self.roles {
            let placeholders: Vec<String> = roles.iter().enumerate()
                .map(|(i, _)| format!("?{}", param_idx + i))
                .collect();
            conditions.push(format!("m.role IN ({})", placeholders.join(",")));
            for r in roles {
                param_values.push(Box::new(r.clone()));
                param_idx += 1;
            }
        }

        // Since ID
        if let Some(sid) = self.since_id {
            conditions.push(format!("m.id > ?{}", param_idx));
            param_values.push(Box::new(sid));
            param_idx += 1;
        }

        // Since time
        if let Some(ref st) = self.since_time {
            conditions.push(format!("m.timestamp > ?{}", param_idx));
            param_values.push(Box::new(st.clone()));
            param_idx += 1;
        }

        // Label inclusion filter (EXISTS subquery)
        for (label, value) in &self.labels_filter {
            if let Some(v) = value {
                conditions.push(format!(
                    "EXISTS (SELECT 1 FROM message_labels ml WHERE ml.message_id = m.id AND ml.label = ?{} AND ml.value = ?{})",
                    param_idx, param_idx + 1
                ));
                param_values.push(Box::new(label.clone()));
                param_values.push(Box::new(v.clone()));
                param_idx += 2;
            } else {
                conditions.push(format!(
                    "EXISTS (SELECT 1 FROM message_labels ml WHERE ml.message_id = m.id AND ml.label = ?{})",
                    param_idx
                ));
                param_values.push(Box::new(label.clone()));
                param_idx += 1;
            }
        }

        // Label exclusion filter (NOT EXISTS subquery)
        for label in &self.exclude_labels {
            conditions.push(format!(
                "NOT EXISTS (SELECT 1 FROM message_labels ml WHERE ml.message_id = m.id AND ml.label = ?{})",
                param_idx
            ));
            param_values.push(Box::new(label.clone()));
            param_idx += 1;
        }

        // Build SQL
        let from_clause = if needs_conv_join {
            "FROM conversation_messages m JOIN conversations c ON m.session_id = c.id"
        } else {
            "FROM conversation_messages m"
        };

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Count query (for cursor.has_more)
        let count_sql = format!("SELECT COUNT(*) {} {}", from_clause, where_clause);
        let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let total: usize = conn.query_row(&count_sql, params_ref.as_slice(), |row| row.get(0))?;

        // Data query
        let limit_clause = format!("ORDER BY m.id ASC LIMIT ?{}", param_idx);
        let sql = format!("SELECT m.* {} {} {}", from_clause, where_clause, limit_clause);
        param_values.push(Box::new(self.limit));
        let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            MissionDB::row_to_conversation_message(row)
        })?;

        let mut messages = Vec::new();
        for r in rows {
            messages.push(r?);
        }

        let last_id = messages.last().map(|m| m.id);
        let has_more = total > messages.len();

        // Strip raw_content if not requested
        if !self.include_raw {
            for msg in &mut messages {
                msg.raw_content = None;
            }
        }

        // Fetch labels if requested
        let labels = if self.include_labels && !messages.is_empty() {
            let msg_ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
            db.label_get_batch(&msg_ids)?
        } else {
            HashMap::new()
        };

        Ok(MessageFeed {
            messages,
            labels,
            cursor: MessageCursor {
                last_id,
                total_matched: total,
                has_more,
            },
        })
    }
}

impl MissionDB {
    /// Create a MessageFeed builder for unified message queries.
    pub fn message_feed() -> MessageFeedBuilder {
        MessageFeedBuilder::new()
    }
}
