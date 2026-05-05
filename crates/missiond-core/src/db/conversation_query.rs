use serde::{Deserialize, Serialize};

// ───────────────────────────────────────────────────────────────────────
// BoardTask e1a5ac1f :: provider-aware conversation classification.
//
// The legacy `db::derive_conversation_type` knows about Claude and Gemini
// flows but treats everything from the Codex CLI as anonymous user input.
// That mis-routes background-ingested Codex worker sessions into the
// human Logs tab and silently strips slotId/taskId linkage from the
// resulting rows.
//
// `classify_conversation_type` wraps `derive_conversation_type` with an
// explicit provider intercept so:
//   * Codex sessions with a slot binding   → "worker"   (durable
//     slot/task linkage; mirrors Claude worker semantics).
//   * Codex sessions without a slot         → "codex_chat" (parallel to
//     "gemini_chat" — background-ingested provider chat surfaced under
//     its own tab rather than buried in Logs).
//   * Claude / Gemini callers              → unchanged; defers to
//     `derive_conversation_type` so existing flows keep working byte for
//     byte.
//
// Real human Jarvis conversations stay `conversation_type=user` because
// they have no slot binding AND their `source` is the Claude CLI, which
// the inner classifier already projects to "user" for the no-slot
// fallthrough branch.
//
// Pure function + no I/O, so callers can drop it into background workers
// and the historical-row audit alike.
// ───────────────────────────────────────────────────────────────────────

/// Provider-aware conversation classifier. See module doc-comment for
/// authority and rationale.
pub fn classify_conversation_type(
    slot_category: Option<&str>,
    slot_id: Option<&str>,
    session_id: &str,
    source: &str,
) -> String {
    // Codex CLI: provider-aware intercept that the legacy derive_*
    // function does not yet cover. Slot-bound Codex traffic is a worker
    // chain; the rest is a Codex-side chat surface.
    if source == "codex_cli" {
        return match slot_id {
            Some(_) => "worker".to_string(),
            None => "codex_chat".to_string(),
        };
    }
    crate::db::derive_conversation_type(slot_category, slot_id, session_id, source)
}

/// Audit verdict for a single historical conversation row in dry-run mode.
/// Carries enough information to surface mis-classified rows without
/// mutating anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationAuditFinding {
    /// Conversation id of the suspect row.
    pub session_id: String,
    /// What `conversation_type` is currently stored on the row.
    pub stored_conversation_type: String,
    /// What the provider-aware classifier believes the row should be.
    pub expected_conversation_type: String,
    /// Why the audit flagged this row (`codex_user_without_slot`,
    /// `codex_chat_without_slot`, `worker_loses_slot_linkage`, ...).
    pub reason: &'static str,
    /// Whether the stored row preserves provider metadata. Codex
    /// ingestion historically dropped `raw_role`; mark those rows so the
    /// audit can reconcile them without bulk DB mutation.
    pub raw_role_present: bool,
}

/// Inputs to a single classification audit check. Pure value type so
/// the caller (background worker, dry-run report, test fixture) can
/// drive the audit from any data source it has on hand without coupling
/// to `MissionStore` or to any specific row shape.
#[derive(Debug, Clone)]
pub struct ClassificationAuditInput<'a> {
    pub session_id: &'a str,
    pub stored_conversation_type: &'a str,
    pub source: &'a str,
    pub slot_id: Option<&'a str>,
    pub slot_category: Option<&'a str>,
    pub raw_role_present: bool,
}

/// Pure dry-run check: returns `Some(finding)` when the stored row
/// disagrees with the provider-aware classifier OR when a Codex row is
/// missing `raw_role`. Returns `None` for clean rows.
///
/// This is intentionally side-effect-free so callers can drive the
/// audit from a fixture, an MCP query, or a CLI report without granting
/// the audit DB-write permission. Bulk DB mutation is explicitly NOT
/// part of the dedup contract — `mission_conversation_reconcile` is the
/// repair surface, this is the diagnose surface.
pub fn audit_classification(
    input: &ClassificationAuditInput<'_>,
) -> Option<ClassificationAuditFinding> {
    let expected = classify_conversation_type(
        input.slot_category,
        input.slot_id,
        input.session_id,
        input.source,
    );
    if expected != input.stored_conversation_type {
        let reason = match (input.source, input.slot_id, input.stored_conversation_type) {
            ("codex_cli", None, ct) if ct == "user" => "codex_user_without_slot",
            ("codex_cli", Some(_), ct) if ct != "worker" => "codex_slot_not_worker",
            ("codex_cli", _, _) => "codex_classification_mismatch",
            (_, Some(_), ct) if ct == "user" => "worker_loses_slot_linkage",
            _ => "classification_mismatch",
        };
        return Some(ClassificationAuditFinding {
            session_id: input.session_id.to_string(),
            stored_conversation_type: input.stored_conversation_type.to_string(),
            expected_conversation_type: expected,
            reason,
            raw_role_present: input.raw_role_present,
        });
    }
    if input.source == "codex_cli" && !input.raw_role_present {
        return Some(ClassificationAuditFinding {
            session_id: input.session_id.to_string(),
            stored_conversation_type: input.stored_conversation_type.to_string(),
            expected_conversation_type: expected,
            reason: "codex_raw_role_missing",
            raw_role_present: false,
        });
    }
    None
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ConversationTypeFilter {
    User,
    Meta,
    Worker,
    Jarvis,
    System, // 组合：Meta + Worker (不含 Jarvis)
    Gemini, // 组合：gemini_chat + router_chat
    Subagent,
    Compaction,
    All,
    Custom(String),
}

impl ConversationTypeFilter {
    pub fn to_sql_clause(&self, alias: &str) -> String {
        let prefix = if alias.is_empty() { String::new() } else { format!("{}.", alias) };
        match self {
            Self::All => String::new(),
            Self::User => format!(" AND {}conversation_type = 'user'", prefix),
            Self::Meta => format!(" AND {}conversation_type = 'meta'", prefix),
            Self::Worker => format!(" AND {}conversation_type = 'worker'", prefix),
            Self::Jarvis => format!(" AND {}conversation_type = 'jarvis'", prefix),
            Self::System => format!(" AND {}conversation_type IN ('meta', 'worker')", prefix),
            Self::Gemini => format!(" AND {}conversation_type IN ('gemini_chat', 'router_chat')", prefix),
            Self::Subagent => format!(" AND {}conversation_type = 'subagent'", prefix),
            Self::Compaction => format!(" AND {}conversation_type = 'compaction'", prefix),
            Self::Custom(s) => format!(" AND {}conversation_type = '{}'", prefix, s.replace('\'', "''")),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "all" => Self::All,
            "user" => Self::User,
            "meta" => Self::Meta,
            "worker" => Self::Worker,
            "jarvis" => Self::Jarvis,
            "system" => Self::System,
            "gemini" => Self::Gemini,
            "subagent" => Self::Subagent,
            "compaction" => Self::Compaction,
            _ => Self::Custom(s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConversationQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub conv_type: Option<ConversationTypeFilter>,
    pub task_id: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub source: Option<String>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub parent_session_id: Option<String>,
}

impl ConversationQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn conv_type(mut self, conv_type: ConversationTypeFilter) -> Self {
        self.conv_type = Some(conv_type);
        self
    }

    pub fn task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    pub fn since(mut self, since: impl Into<String>) -> Self {
        self.since = Some(since.into());
        self
    }

    pub fn until(mut self, until: impl Into<String>) -> Self {
        self.until = Some(until.into());
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    pub fn project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }
}

#[cfg(test)]
mod conversation_classification_tests {
    use super::*;

    // ── classify_conversation_type ──────────────────────────────────────
    //
    // Pins the provider-aware classifier contract for BoardTask
    // e1a5ac1f: ClaudeCode/Codex/Gemini worker sessions are
    // `conversation_type=worker` (or their provider-chat equivalent
    // when running headless), real human Jarvis user input remains
    // `user`, and raw provider metadata is preserved.

    #[test]
    fn human_jarvis_user_session_remains_user() {
        let v = classify_conversation_type(None, None, "abc-123", "claude_code");
        assert_eq!(v, "user");
    }

    #[test]
    fn jarvis_master_input_remains_user() {
        // Jarvis master prompts the daemon directly; no slot binding,
        // raw source is the Claude CLI. Classification must keep these
        // visible in the user/Jarvis tab rather than collapsing them
        // into the worker pool.
        let v = classify_conversation_type(None, None, "jarvis-session-uuid", "claude_code");
        assert_eq!(v, "user");
    }

    #[test]
    fn claudecode_worker_session_classifies_as_worker_via_slot_category() {
        // Slot-bound Claude session inherits the slot's declared
        // category. `worker` for coder/operator/etc. slots.
        let v = classify_conversation_type(
            Some("worker"),
            Some("slot-coder-1"),
            "session-id",
            "claude_code",
        );
        assert_eq!(v, "worker");
    }

    #[test]
    fn claudecode_slot_session_without_category_falls_back_to_worker() {
        // Slot binding present but the slot config disappeared (orphan).
        // The classifier must still treat the session as worker rather
        // than human user — losing the slot category is a soft failure,
        // not a reclassification trigger.
        let v = classify_conversation_type(None, Some("slot-orphan"), "session-id", "claude_code");
        assert_eq!(v, "worker");
    }

    #[test]
    fn codex_worker_with_slot_classifies_as_worker() {
        // BoardTask e1a5ac1f core fix: a Codex-CLI-backed slot is a
        // worker session, not a human user row.
        let v = classify_conversation_type(
            Some("worker"),
            Some("slot-codex-1"),
            "thread-uuid",
            "codex_cli",
        );
        assert_eq!(v, "worker");
    }

    #[test]
    fn codex_background_session_without_slot_classifies_as_codex_chat() {
        // BoardTask e1a5ac1f core fix: background-ingested Codex chats
        // (no slot binding) get their own provider-chat tab, mirroring
        // gemini_chat. They MUST NOT be silently labelled `user` —
        // doing so was the original misclassification that drove
        // historical Codex ingestion to land in the human Logs view.
        let v = classify_conversation_type(None, None, "thread-uuid", "codex_cli");
        assert_eq!(v, "codex_chat");
    }

    #[test]
    fn gemini_classification_routes_to_gemini_chat() {
        // Existing Gemini routing behaviour preserved: regardless of
        // slot binding, gemini_cli/router_chat sources map to the
        // gemini_chat tab. The classifier does not relitigate that.
        assert_eq!(
            classify_conversation_type(None, None, "session", "gemini_cli"),
            "gemini_chat"
        );
        assert_eq!(
            classify_conversation_type(None, None, "session", "router_chat"),
            "gemini_chat"
        );
    }

    #[test]
    fn subagent_session_without_slot_remains_subagent() {
        // Sub-agent prefix (`agent-*`) plus no slot binding → subagent
        // category. Classification must not promote sub-agents to
        // worker — they ride on the parent's slot lineage instead.
        let v = classify_conversation_type(None, None, "agent-12345-foo", "claude_code");
        assert_eq!(v, "subagent");
    }

    #[test]
    fn compaction_session_keeps_compaction_label() {
        let v = classify_conversation_type(None, None, "abc-acompact-123", "claude_code");
        assert_eq!(v, "compaction");
    }

    // ── audit_classification ────────────────────────────────────────────
    //
    // Pins the historical-row dry-run contract: returns `Some(finding)`
    // for rows that disagree with the provider-aware classifier; never
    // mutates the input. Used by the conversation-classification audit
    // to surface suspicious rows without bulk DB writes.

    #[test]
    fn audit_passes_clean_rows() {
        let input = ClassificationAuditInput {
            session_id: "user-session",
            stored_conversation_type: "user",
            source: "claude_code",
            slot_id: None,
            slot_category: None,
            raw_role_present: true,
        };
        assert!(audit_classification(&input).is_none());
    }

    #[test]
    fn audit_flags_codex_user_without_slot_as_misclassification() {
        let input = ClassificationAuditInput {
            session_id: "thread-uuid",
            stored_conversation_type: "user",
            source: "codex_cli",
            slot_id: None,
            slot_category: None,
            raw_role_present: true,
        };
        let finding = audit_classification(&input).expect("must flag misclassification");
        assert_eq!(finding.expected_conversation_type, "codex_chat");
        assert_eq!(finding.reason, "codex_user_without_slot");
    }

    #[test]
    fn audit_flags_codex_slot_misclassified_as_user() {
        let input = ClassificationAuditInput {
            session_id: "slot-codex-thread",
            stored_conversation_type: "user",
            source: "codex_cli",
            slot_id: Some("slot-codex-1"),
            slot_category: Some("worker"),
            raw_role_present: true,
        };
        let finding = audit_classification(&input).expect("slot codex must flag");
        assert_eq!(finding.expected_conversation_type, "worker");
        assert_eq!(finding.reason, "codex_slot_not_worker");
    }

    #[test]
    fn audit_flags_codex_row_without_raw_role() {
        let input = ClassificationAuditInput {
            session_id: "thread-uuid",
            stored_conversation_type: "codex_chat",
            source: "codex_cli",
            slot_id: None,
            slot_category: None,
            raw_role_present: false,
        };
        let finding = audit_classification(&input).expect("missing raw_role must flag");
        assert_eq!(finding.reason, "codex_raw_role_missing");
        assert!(!finding.raw_role_present);
    }

    #[test]
    fn audit_does_not_mutate_or_misflag_compaction_rows() {
        let input = ClassificationAuditInput {
            session_id: "abc-acompact-1",
            stored_conversation_type: "compaction",
            source: "claude_code",
            slot_id: None,
            slot_category: None,
            raw_role_present: true,
        };
        assert!(audit_classification(&input).is_none());
    }

    #[test]
    fn audit_flags_worker_row_that_lost_its_slot_linkage() {
        let input = ClassificationAuditInput {
            session_id: "session-x",
            stored_conversation_type: "user",
            source: "claude_code",
            slot_id: Some("slot-coder-1"),
            slot_category: Some("worker"),
            raw_role_present: true,
        };
        let finding = audit_classification(&input).expect("slot linkage loss must flag");
        assert_eq!(finding.expected_conversation_type, "worker");
        assert_eq!(finding.reason, "worker_loses_slot_linkage");
    }
}
