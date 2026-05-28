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

/// Historical ClaudeCode sessions sometimes predate durable slot/session
/// binding. Their only reliable evidence is the first provider message:
/// MissionD worker prompts are highly structured, while real human sessions
/// are conversational. This input lets the repair path audit those rows
/// without re-parsing JSONL in the handler.
#[derive(Debug, Clone)]
pub struct HistoricalClassificationInput<'a> {
    pub session_id: &'a str,
    pub stored_conversation_type: &'a str,
    pub source: &'a str,
    pub slot_id: Option<&'a str>,
    pub slot_category: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub raw_role_present: bool,
    pub first_message: Option<&'a str>,
}

/// Provider-aware historical classification verdict. `confidence`
/// deliberately stays coarse so maintenance surfaces can apply only
/// high-confidence worker repairs and leave ambiguous sessions for review.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoricalClassificationFinding {
    pub session_id: String,
    pub stored_conversation_type: String,
    pub expected_conversation_type: String,
    pub reason: &'static str,
    pub confidence: f32,
    pub raw_role_present: bool,
}

const WORKER_PROMPT_PREFIXES: &[&str] = &[
    "Clean retry for parent BoardTask",
    "Execute MissionD task ",
    "Fix MissionD-side swarm ",
    "Implement accepted swarm shard",
    "Survey exact shards for swarm objective",
    "Smoke test post-",
    "Upgrade project ",
    "Bring XJP monorepo service SSOT",
    "M10 ladder infra shard",
    "# Forge Deep Cartographer Protocol",
    "[realtime-extract]",
    "[Matched Skills",
    "你是意图提升器",
];

const WORKER_PROMPT_NEEDLES: &[&str] = &[
    "BoardTask",
    "Board Task ID",
    "write_scope",
    "must_not_touch",
    "completion protocol",
    "READ ONLY. Do not edit files",
    "Do not edit files, do not stage, do not commit",
    "context-pack",
    "swarm objective",
    "MissionD maturity gate",
];

fn looks_like_historical_worker_prompt(content: &str) -> Option<&'static str> {
    let trimmed = content.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    for prefix in WORKER_PROMPT_PREFIXES {
        if trimmed.starts_with(prefix) {
            return Some("claude_worker_prompt_prefix");
        }
    }
    let lower = trimmed.to_ascii_lowercase();
    let has_worker_term = WORKER_PROMPT_NEEDLES
        .iter()
        .any(|needle| lower.contains(&needle.to_ascii_lowercase()));
    let has_guardrail = lower.contains("do not edit")
        || lower.contains("read only")
        || lower.contains("write scope")
        || lower.contains("must_not_touch")
        || lower.contains("boardtask")
        || lower.contains("swarm");
    if has_worker_term && has_guardrail {
        return Some("claude_worker_prompt_signature");
    }
    None
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

/// Historical-row audit that augments provider/slot classification with
/// first-message prompt signatures. This is the bridge for sessions created
/// before dispatch-time `slot_id` / `task_id` rebinding existed.
pub fn audit_historical_classification(
    input: &HistoricalClassificationInput<'_>,
) -> Option<HistoricalClassificationFinding> {
    if input.source == "claude_code" && input.slot_id.is_none() {
        if let Some(reason) = input
            .first_message
            .and_then(looks_like_historical_worker_prompt)
        {
            if input.stored_conversation_type == "worker" {
                return None;
            }
            return Some(HistoricalClassificationFinding {
                session_id: input.session_id.to_string(),
                stored_conversation_type: input.stored_conversation_type.to_string(),
                expected_conversation_type: "worker".to_string(),
                reason,
                confidence: if input.task_id.is_some() { 0.98 } else { 0.93 },
                raw_role_present: input.raw_role_present,
            });
        }
    }

    let direct = audit_classification(&ClassificationAuditInput {
        session_id: input.session_id,
        stored_conversation_type: input.stored_conversation_type,
        source: input.source,
        slot_id: input.slot_id,
        slot_category: input.slot_category,
        raw_role_present: input.raw_role_present,
    });
    if let Some(f) = direct {
        if f.reason == "codex_raw_role_missing" && input.first_message.is_none() {
            return None;
        }
        return Some(HistoricalClassificationFinding {
            session_id: f.session_id,
            stored_conversation_type: f.stored_conversation_type,
            expected_conversation_type: f.expected_conversation_type,
            reason: f.reason,
            confidence: if input.slot_id.is_some() { 1.0 } else { 0.95 },
            raw_role_present: f.raw_role_present,
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
        let prefix = if alias.is_empty() {
            String::new()
        } else {
            format!("{}.", alias)
        };
        match self {
            Self::All => String::new(),
            Self::User => format!(" AND {}conversation_type = 'user'", prefix),
            Self::Meta => format!(" AND {}conversation_type = 'meta'", prefix),
            Self::Worker => format!(" AND {}conversation_type = 'worker'", prefix),
            Self::Jarvis => format!(" AND {}conversation_type = 'jarvis'", prefix),
            Self::System => format!(" AND {}conversation_type IN ('meta', 'worker')", prefix),
            Self::Gemini => format!(
                " AND {}conversation_type IN ('gemini_chat', 'router_chat')",
                prefix
            ),
            Self::Subagent => format!(" AND {}conversation_type = 'subagent'", prefix),
            Self::Compaction => format!(" AND {}conversation_type = 'compaction'", prefix),
            Self::Custom(s) => format!(
                " AND {}conversation_type = '{}'",
                prefix,
                s.replace('\'', "''")
            ),
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
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
    pub application_id: Option<String>,
    pub channel: Option<String>,
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

    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    pub fn tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn application_id(mut self, application_id: impl Into<String>) -> Self {
        self.application_id = Some(application_id.into());
        self
    }

    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
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
    fn historical_audit_ignores_empty_codex_placeholder_without_raw_role() {
        let input = HistoricalClassificationInput {
            session_id: "pty-slot-codex-master-control",
            stored_conversation_type: "worker",
            source: "codex_cli",
            slot_id: Some("slot-codex-master-control"),
            slot_category: Some("worker"),
            task_id: None,
            raw_role_present: false,
            first_message: None,
        };
        assert!(audit_historical_classification(&input).is_none());
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

    #[test]
    fn historical_audit_promotes_boardtask_prompt_to_worker() {
        let input = HistoricalClassificationInput {
            session_id: "old-worker",
            stored_conversation_type: "user",
            source: "claude_code",
            slot_id: None,
            slot_category: None,
            task_id: None,
            raw_role_present: true,
            first_message: Some(
                "Survey exact shards for swarm objective (1/2)\nParent BoardTask abc requires READ ONLY inspection.",
            ),
        };
        let finding = audit_historical_classification(&input).expect("worker prompt must flag");
        assert_eq!(finding.expected_conversation_type, "worker");
        assert_eq!(finding.reason, "claude_worker_prompt_prefix");
        assert!(finding.confidence >= 0.9);
    }

    #[test]
    fn historical_audit_accepts_already_backfilled_worker_prompt() {
        let input = HistoricalClassificationInput {
            session_id: "old-worker",
            stored_conversation_type: "worker",
            source: "claude_code",
            slot_id: None,
            slot_category: None,
            task_id: None,
            raw_role_present: true,
            first_message: Some(
                "Clean retry for parent BoardTask 9f343270: fix mission_task_query visibility.",
            ),
        };
        assert!(audit_historical_classification(&input).is_none());
    }

    #[test]
    fn historical_audit_keeps_human_question_as_user() {
        let input = HistoricalClassificationInput {
            session_id: "human-session",
            stored_conversation_type: "user",
            source: "claude_code",
            slot_id: None,
            slot_category: None,
            task_id: None,
            raw_role_present: true,
            first_message: Some("看一下 auth 的架构还有什么问题"),
        };
        assert!(audit_historical_classification(&input).is_none());
    }
}
