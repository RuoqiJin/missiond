//! Database trait abstraction layer for MissionD.
//!
//! Domain-specific async traits for MissionD's PostgreSQL-backed store.
//!
//! Provider-local indexes are read by ingestion workers and do not constitute a MissionD database backend.

use super::error::DbResult;
use crate::types::*;
use async_trait::async_trait;
use std::collections::HashMap;

// Re-export shared types (always available regardless of feature flags)
pub use super::artifact_commit::{ArtifactCommitOutboxInput, ArtifactCommitOutboxRecord};
pub use super::shared::BackfillPhaseStatus;
pub use super::shared::MessageRoleBackfillCandidate;
pub use super::shared::{AstNodeRow, AstSearchHit, AstStats, AstSyncResult, ModuleAstSummary};
pub use super::shared::{BeaconInfo, BeaconNode};
pub use super::shared::{LatencyStats, TimelineRow, TimelineStats};
pub use crate::ast::CodeNode;

// ============================================================================
// 1. ConversationStore — Session lifecycle, analysis, embeddings, extraction
// Source: conversation.rs + audit.rs (lifecycle/extraction methods)
// ============================================================================

#[async_trait]
pub trait ConversationStore: Send + Sync {
    // -- conversation.rs: session CRUD --
    async fn upsert_conversation(&self, conv: &Conversation) -> DbResult<()>;
    /// Insert conversation record if it doesn't exist; on conflict, backfill parent_session_id.
    /// Used by ReconcileWorker to avoid overwriting conversation_type/status/message_count
    /// while still ensuring parent linkage is populated.
    async fn ensure_conversation_exists(
        &self,
        session_id: &str,
        project_path: &str,
        jsonl_path: &str,
        status: &str,
        conversation_type: &str,
        parent_session_id: Option<&str>,
        started_at: Option<&str>,
    ) -> DbResult<()>;
    /// Refresh message_count from actual rows in conversation_messages.
    async fn refresh_conversation_message_count(&self, session_id: &str) -> DbResult<()>;
    /// Upsert non-destructive provider source-state coverage for a conversation.
    async fn upsert_conversation_source_state(
        &self,
        state: &ConversationSourceStateInput,
    ) -> DbResult<()>;
    async fn get_conversation(&self, id: &str) -> DbResult<Option<Conversation>>;
    async fn list_conversations(
        &self,
        status: Option<&str>,
        limit: i64,
        conv_type: Option<&str>,
        task_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        source: Option<&str>,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<Conversation>>;
    async fn get_child_conversations(&self, parent_session_id: &str)
        -> DbResult<Vec<Conversation>>;

    /// Resolve an active session by multi-tenant isolation dimensions and optional topic.
    /// Returns the most recently updated active session matching the isolation scope,
    /// enabling session-less entry (clients don't need to create conversations explicitly).
    async fn resolve_active_session(
        &self,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        topic_id: Option<&str>,
    ) -> DbResult<Option<Conversation>>;

    /// Bind a context capsule hash and optional topic to an existing conversation.
    async fn bind_context_capsule(
        &self,
        session_id: &str,
        context_capsule_hash: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
    ) -> DbResult<()>;

    /// Fix orphan subagent parent links by re-extracting parent_session_id from jsonl_path.
    /// Returns count of fixed rows.
    async fn fix_orphan_parent_links(&self, session_ids: &[String]) -> DbResult<usize>;
    /// Link a compaction fragment to its original session.
    /// Returns true if the link was newly created.
    async fn link_compaction_fragment(
        &self,
        fragment_id: &str,
        original_id: &str,
    ) -> DbResult<bool>;

    // -- conversation.rs: deep analysis tracking --
    async fn get_pending_deep_analysis(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<Vec<Conversation>>;
    async fn has_pending_deep_analysis(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<bool>;
    async fn count_pending_deep_analysis(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<i64>;
    async fn count_pending_realtime(&self) -> DbResult<i64>;
    async fn pending_realtime_detail(&self) -> DbResult<Vec<(String, i64, String)>>;
    async fn pending_deep_detail(
        &self,
        current_version: i32,
        max_retries: i32,
    ) -> DbResult<Vec<(String, String, i32)>>;
    async fn mark_analysis_complete(&self, id: &str, version: i32) -> DbResult<()>;
    async fn update_deep_checkpoint(&self, id: &str, message_id: i64) -> DbResult<()>;
    async fn mark_analysis_failed(&self, id: &str) -> DbResult<()>;

    // -- conversation.rs: habit scanning --
    async fn get_unscanned_conversations(&self, limit: usize) -> DbResult<Vec<Conversation>>;
    async fn mark_habit_scanned(&self, id: &str) -> DbResult<()>;
    async fn count_unscanned_conversations(&self) -> DbResult<i64>;
    async fn count_scannable_conversations(&self) -> DbResult<i64>;

    // -- conversation.rs: summary & embeddings --
    async fn set_conversation_summary(&self, id: &str, summary: &str) -> DbResult<()>;
    async fn clear_conversation_summary(&self, id: &str) -> DbResult<()>;
    async fn set_conversation_embedding(
        &self,
        id: &str,
        embedding: &[f32],
        provider: &str,
    ) -> DbResult<()>;
    async fn load_conversation_embeddings(
        &self,
        provider: &str,
    ) -> DbResult<Vec<(String, Vec<f32>)>>;
    async fn conversations_missing_summary(&self, limit: i64) -> DbResult<Vec<String>>;
    async fn conversations_stale_embedding(
        &self,
        current_provider: &str,
        limit: i64,
    ) -> DbResult<Vec<String>>;

    // -- conversation.rs: topic vectors --
    async fn set_conversation_topic_vectors(
        &self,
        session_id: &str,
        topics: &[(String, Vec<f32>)],
        provider: &str,
    ) -> DbResult<()>;
    async fn load_conversation_topic_vectors(
        &self,
        provider: &str,
    ) -> DbResult<Vec<(String, Vec<Vec<f32>>)>>;
    async fn get_conversation_topics(&self, session_id: &str) -> DbResult<Vec<String>>;
    async fn conversations_needing_topic_vectors(
        &self,
        provider: &str,
        limit: i64,
    ) -> DbResult<Vec<String>>;

    // -- conversation.rs: timeline reconstruction --
    async fn conversations_needing_timeline(&self, limit: i64) -> DbResult<Vec<String>>;
    async fn get_compaction_fragments(
        &self,
        parent_id: &str,
    ) -> DbResult<Vec<(String, String, i64)>>;
    async fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>>;
    async fn set_session_timeline(&self, parent_id: &str, timeline_json: &str) -> DbResult<bool>;

    // -- audit.rs: conversation lifecycle --
    async fn mark_conversation_analyzed(&self, id: &str) -> DbResult<()>;
    async fn get_unanalyzed_conversations(&self) -> DbResult<Vec<Conversation>>;
    async fn complete_conversation(&self, id: &str) -> DbResult<()>;
    async fn save_conversation_exit_code(&self, id: &str, exit_code: i32) -> DbResult<()>;
    async fn complete_stale_conversations(&self, cutoff: &str) -> DbResult<usize>;
    async fn mark_conversation_compacted(&self, id: &str) -> DbResult<()>;
    async fn set_conversation_task_id(&self, id: &str, task_id: &str) -> DbResult<()>;
    /// Historical repair surface: update durable conversation classification after
    /// provider-aware audit has high-confidence evidence that a row was misfiled.
    async fn set_conversation_type(&self, id: &str, conversation_type: &str) -> DbResult<usize>;
    /// Historical repair surface: older provider ingesters sometimes left
    /// `raw_role` NULL. Backfill it from the normalized `role` column so
    /// provider-aware audits can distinguish human turns from worker/provider turns.
    async fn backfill_missing_raw_roles_for_session(&self, id: &str) -> DbResult<usize>;
    /// Historical repair surface: find ClaudeCode worker-session messages
    /// whose provider raw_role=user was stored as human role=user before
    /// worker_user normalization was enforced.
    async fn claude_worker_user_role_backfill_candidates(
        &self,
        session_id: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<MessageRoleBackfillCandidate>>;
    /// Reviewed repair surface: rewrite selected historical worker messages
    /// from role=user to role=worker_user. Callers must rebuild turns for
    /// touched sessions after this update.
    async fn backfill_claude_worker_user_message_roles(
        &self,
        message_ids: &[i64],
    ) -> DbResult<usize>;
    async fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>>;
    async fn reactivate_conversation(&self, id: &str) -> DbResult<usize>;

    // -- message embeddings (independent table) --
    async fn insert_message_embedding(
        &self,
        message_id: i64,
        session_id: &str,
        embedding_vec: &[f32],
        model_version: &str,
    ) -> DbResult<()>;
    async fn insert_message_embedding_skip(
        &self,
        message_id: i64,
        skip_reason: &str,
    ) -> DbResult<()>;
    async fn insert_message_embeddings_batch(
        &self,
        entries: &[(i64, &str, Vec<f32>, &str)],
    ) -> DbResult<usize>;
    async fn insert_message_embedding_skips_batch(
        &self,
        entries: &[(i64, &str)],
    ) -> DbResult<usize>;
    async fn messages_pending_embedding(
        &self,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String, String, String)>>;
    async fn message_embedding_stats(&self) -> DbResult<serde_json::Value>;

    // -- audit.rs: extraction watermarks --
    async fn get_pending_memory_messages(
        &self,
        today: &str,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>>;
    async fn update_memory_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()>;
    async fn get_pending_user_voice_messages(
        &self,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>>;
    async fn update_user_voice_forwarded_at(
        &self,
        session_id: &str,
        timestamp: &str,
    ) -> DbResult<()>;
    async fn get_pending_realtime_messages(
        &self,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>>;
    async fn get_pending_realtime_messages_with_limit(
        &self,
        limit: usize,
    ) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>>;
    async fn update_realtime_forwarded_at(&self, session_id: &str, timestamp: &str)
        -> DbResult<()>;

    // -- conversation_turns (S3 Tagger & Chunker) --
    /// Get the end_message_id of the last Turn for incremental processing.
    async fn get_last_turn_end_message_id(&self, session_id: &str) -> DbResult<Option<i64>>;
    /// Get the maximum turn_idx for a session.
    async fn get_max_turn_idx(&self, session_id: &str) -> DbResult<Option<i32>>;
    /// Batch insert conversation turns. Returns number of rows inserted.
    async fn insert_conversation_turns_batch(
        &self,
        session_id: &str,
        base_idx: i32,
        turns: &[RawTurn],
    ) -> DbResult<usize>;
    /// Delete existing turns for a session before an explicit rebuild.
    async fn clear_conversation_turns(&self, session_id: &str) -> DbResult<usize>;
    /// Insert message labels in batch (message_id, label, value). Idempotent via ON CONFLICT DO NOTHING.
    async fn insert_message_labels_batch(
        &self,
        labels: &[(i64, &str, &str, &str)],
    ) -> DbResult<usize>;
    /// Get session IDs that have messages but no turns (for backfill).
    async fn sessions_pending_turn_extraction(&self, limit: i64) -> DbResult<Vec<String>>;
    /// Get session IDs updated within `since_minutes` that have messages but no turns (reconciliation).
    async fn sessions_recently_active_without_turns(
        &self,
        since_minutes: i64,
        limit: i64,
    ) -> DbResult<Vec<String>>;

    // -- conversation_turns: S4 per-turn embedding --
    /// Get turns that have no corresponding topic_vector entry yet (incremental).
    async fn turns_pending_embedding(
        &self,
        session_id: &str,
        provider: &str,
    ) -> DbResult<Vec<ConversationTurn>>;
    /// Batch update conversation_turns.topic column.
    async fn update_turn_topics_batch(&self, updates: &[(i64, &str)]) -> DbResult<usize>;
    /// Upsert per-turn topic vectors. ON CONFLICT DO UPDATE (incremental safe).
    async fn set_conversation_turn_vectors(
        &self,
        session_id: &str,
        vectors: &[(String, i32, Vec<f32>)],
        provider: &str,
    ) -> DbResult<usize>;
    /// Find sessions that have turns but lack per-turn topic_vectors (backfill).
    async fn sessions_with_turns_but_no_vectors(
        &self,
        provider: &str,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<String>>;

    // -- user_intents (Phase 6 Intent Analyst) --
    /// Insert a single intent analysis result. Returns the generated id.
    async fn insert_user_intent(
        &self,
        session_id: &str,
        turn_range_start: i32,
        turn_range_end: i32,
        intent_type: &str,
        confidence: f32,
        summary: Option<&str>,
        context_json: Option<&str>,
        related_goal_id: Option<&str>,
    ) -> DbResult<i64>;
    /// Get the max turn_range_end covered by existing intents for a session (incremental cursor).
    async fn get_intent_coverage(&self, session_id: &str) -> DbResult<Option<i32>>;
    /// Get turns after a given turn_idx for a session.
    async fn get_turns_after(
        &self,
        session_id: &str,
        after_idx: i32,
    ) -> DbResult<Vec<ConversationTurn>>;
    /// Update conversation_turns.intent_group_id for a turn range.
    async fn update_turns_intent_group(
        &self,
        session_id: &str,
        turn_range_start: i32,
        turn_range_end: i32,
        intent_id: i64,
    ) -> DbResult<()>;
    /// Get recent intents across all sessions within a time window.
    async fn get_recent_intents(&self, since_secs: i64) -> DbResult<Vec<UserIntent>>;
    /// Find sessions with turns but no intent analysis (backfill).
    async fn sessions_pending_intent_analysis(&self, limit: i64) -> DbResult<Vec<String>>;

    // -- tool call CRUD, stats (from ToolCallStore v0.4.x) --
    async fn insert_tool_call(&self, tc: &ToolCallRecord) -> DbResult<()>;
    async fn insert_tool_calls_batch(&self, calls: &[ToolCallRecord]) -> DbResult<usize>;
    async fn update_tool_call_output(
        &self,
        tool_use_id: &str,
        output_summary: &str,
        raw_output: &str,
        status: &str,
    ) -> DbResult<bool>;
    async fn get_tool_calls_by_session(
        &self,
        session_id: &str,
        tool_filter: Option<&[String]>,
        limit: i64,
    ) -> DbResult<Vec<ToolCallRecord>>;
    async fn get_tool_call_by_id(&self, tool_use_id: &str) -> DbResult<Option<ToolCallRecord>>;
    async fn get_tool_call_stats(&self, session_id: &str)
        -> DbResult<Vec<(String, i64, i64, i64)>>;
    async fn count_pending_tool_calls(&self) -> DbResult<i64>;
    async fn get_sessions_with_pending_tool_calls(&self) -> DbResult<Vec<String>>;
    async fn get_sessions_with_tool_calls(&self) -> DbResult<std::collections::HashSet<String>>;
    async fn get_messages_for_tool_call_backfill(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, String, String)>>;
    async fn get_conversations_with_jsonl(&self) -> DbResult<Vec<(String, String)>>;

    // -- retrospective tool analysis (from ToolCallStore v0.4.x) --
    async fn get_retrospective_tool_stats(
        &self,
        session_id: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, i64, i64, i64, f64)>>;
    async fn get_retrospective_meta(&self, session_id: &str) -> DbResult<(i64, i64, i64, i64)>;
    async fn get_retrospective_repeat_patterns(
        &self,
        session_id: &str,
        min_streak: i64,
    ) -> DbResult<Vec<(String, i64, String, String)>>;
    async fn get_tool_name_sequence(&self, session_id: &str) -> DbResult<Vec<String>>;
    async fn get_retrospective_high_error_tools(
        &self,
        session_id: &str,
        min_error_rate: f64,
    ) -> DbResult<Vec<(String, f64, i64)>>;
    async fn get_tool_error_samples(
        &self,
        session_id: &str,
        tool_name: &str,
    ) -> DbResult<Vec<(String, String, String)>>;
    async fn get_tool_calls_for_detailed_analysis(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, String, String, String)>>;
    async fn get_tool_calls_with_status_timeline(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(String, String, String, String)>>;
    /// Global aggregation across all sessions: returns
    /// `(tool_name, total_calls, max_timestamp, success_count, error_count)`.
    /// Used by capability-usage-read-model (memory :: system-support, v0.5.4) to
    /// derive snapshots without per-session iteration. `since_iso` filters by
    /// `timestamp >= since_iso` when present (timestamps are stored as ISO text
    /// in `conversation_tool_calls.timestamp`).
    async fn get_tool_call_global_stats(
        &self,
        since_iso: Option<&str>,
    ) -> DbResult<Vec<(String, i64, Option<String>, i64, i64)>>;

    // -- conversation events / JSONL audit (from EventStore v0.4.x) --
    async fn insert_conversation_events_batch(
        &self,
        events: &[ConversationEvent],
    ) -> DbResult<usize>;
    async fn get_conversation_events(
        &self,
        session_id: &str,
        event_type: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<ConversationEvent>>;
    /// Replay interaction-gateway events by durable interaction id.
    /// This is the first interaction ledger read model: Jarvis/iOS/Web/WeChat
    /// visible SSE milestones are mirrored into conversation_events with
    /// `raw_data.interaction_id`, so operators can reconstruct a client-channel
    /// run without relying on a live SSE stream or PTY buffer.
    async fn get_interaction_events(
        &self,
        interaction_id: &str,
        limit: i64,
    ) -> DbResult<Vec<ConversationEvent>>;
    async fn is_compact_boundary_event(&self, session_id: &str, event_uuid: &str)
        -> DbResult<bool>;
    async fn get_agent_trajectory(
        &self,
        tool_use_id: &str,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>>;
    async fn get_event_type_summary(
        &self,
        session_id: Option<&str>,
    ) -> DbResult<Vec<(String, i64)>>;
    async fn cleanup_old_events(&self, cutoff: &str) -> DbResult<usize>;
    async fn get_sessions_with_events(&self) -> DbResult<std::collections::HashSet<String>>;

    // -- retrospective results (from RetrospectiveStore v0.4.x) --
    async fn save_retrospective_result(
        &self,
        session_id: &str,
        trigger_reason: &str,
        quick_stats: &str,
        full_analysis: Option<&str>,
    ) -> DbResult<()>;
    async fn has_retrospective_result(&self, session_id: &str) -> DbResult<bool>;
    async fn get_sessions_needing_retrospective(&self) -> DbResult<Vec<(String, i64, i64, f64)>>;
    async fn get_sessions_for_retro_backfill(
        &self,
        since: &str,
        force: bool,
    ) -> DbResult<Vec<(String, i64, i64, f64)>>;
    async fn list_retrospective_results(
        &self,
        limit: i64,
    ) -> DbResult<Vec<(String, String, String, Option<String>, String)>>;
    async fn get_retrospective_result(
        &self,
        session_id: &str,
    ) -> DbResult<Option<(String, String, Option<String>, String)>>;

    // -- narration (removed v0.4.23 Phase 6): tables message_narrations +
    //    narration_cursors dropped; step_narrator worker deleted together.
}

// ============================================================================
// 2. MessageStore — Message CRUD, search, pagination
// Source: conversation.rs (message methods)
// ============================================================================

#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn insert_conversation_message(&self, msg: &ConversationMessage) -> DbResult<i64>;
    async fn insert_conversation_messages_batch(
        &self,
        messages: &[ConversationMessage],
    ) -> DbResult<Vec<i64>>;
    /// Check which message UUIDs exist in the DB. Returns the set of existing UUIDs.
    async fn check_message_uuids_exist(
        &self,
        uuids: &[&str],
    ) -> DbResult<std::collections::HashSet<String>>;
    async fn get_conversation_message_by_id(
        &self,
        id: i64,
    ) -> DbResult<Option<ConversationMessage>>;
    async fn get_conversation_messages(
        &self,
        session_id: &str,
        since_id: Option<i64>,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>>;
    async fn get_messages_around(
        &self,
        message_id: i64,
        before: i64,
        after: i64,
    ) -> DbResult<Option<(String, Vec<ConversationMessage>)>>;
    async fn search_conversation_messages(
        &self,
        query: &str,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>>;
    async fn search_messages_filtered(
        &self,
        query: &str,
        session_id: Option<&str>,
        role: Option<&str>,
        tool_name: Option<&str>,
        time_after: Option<&str>,
        limit: i64,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<ConversationMessage>>;
    async fn search_conversation_sessions_fts(
        &self,
        query: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, f64)>>;
    async fn get_session_fts_snippets(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> DbResult<Vec<(String, String)>>;
    async fn search_conversation_sessions_fts_filtered(
        &self,
        query: &str,
        limit: i64,
        time_after: Option<&str>,
        project: Option<&str>,
        conversation_type: Option<&str>,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<(String, f64)>>;
    /// Search conversations table by ID prefix, task_id, or llm_summary.
    async fn search_conversations_by_metadata(
        &self,
        query: &str,
        limit: i64,
        conversation_type: Option<&str>,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<(String, f64)>> {
        let _ = (
            query,
            limit,
            conversation_type,
            user_id,
            tenant_id,
            application_id,
            channel,
        );
        Ok(vec![])
    }
    /// Lightweight user message index: (id, timestamp, content_preview) for minimap.
    async fn get_user_message_index(
        &self,
        session_id: &str,
    ) -> DbResult<Vec<(i64, String, String)>> {
        let _ = session_id;
        Ok(vec![])
    }
    async fn get_first_user_message(&self, session_id: &str) -> DbResult<Option<String>>;

    // -- pgvector hybrid search (P3: message-level embedding) --

    /// Hybrid message search: HNSW Top-N + FTS -> RRF merge (two-step CTE).
    /// Returns (message_id, session_id, role, content, timestamp, rrf_score).
    async fn hybrid_message_search(
        &self,
        query_vec: &[f32],
        query_text: &str,
        session_id: Option<&str>,
        limit: i64,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<(i64, String, String, String, String, f64)>> {
        let _ = (
            query_vec,
            query_text,
            session_id,
            limit,
            user_id,
            tenant_id,
            application_id,
            channel,
        );
        Ok(vec![])
    }

    /// Session-internal semantic search: B-Tree filter + exact cosine distance.
    /// Returns (message_id, role, content, timestamp, similarity).
    async fn session_semantic_search(
        &self,
        query_vec: &[f32],
        session_id: &str,
        limit: i64,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<(i64, String, String, String, f64)>> {
        let _ = (
            query_vec,
            session_id,
            limit,
            user_id,
            tenant_id,
            application_id,
            channel,
        );
        Ok(vec![])
    }

    /// Conversation-level semantic search via topic vectors HNSW.
    /// Returns (session_id, max_similarity). No GROUP BY full table — HNSW recall then group.
    async fn semantic_conversation_search(
        &self,
        query_vec: &[f32],
        limit: i64,
        time_after: Option<&str>,
        project: Option<&str>,
        conversation_type: Option<&str>,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
    ) -> DbResult<Vec<(String, f64)>> {
        let _ = (
            query_vec,
            limit,
            time_after,
            project,
            conversation_type,
            user_id,
            tenant_id,
            application_id,
            channel,
        );
        Ok(vec![])
    }
}

// ============================================================================
// 3-5. [MERGED] ToolCallStore, EventStore, RetrospectiveStore → ConversationStore
// Per memory pillar v0.4.23: module conversation-logs unifies all session-scoped
// stores into a single trait. See ConversationStore above for the merged methods.
// ============================================================================

// ============================================================================
// 6. [MERGED] VisionStore → ObservabilityStore (v0.4.23)
// Per memory pillar v0.4.23: image_descriptions belongs to module system-support
// which is covered by ObservabilityStore. VisionStore merged as a section below.
// ============================================================================

// ============================================================================
// 7. KbStore — KB CRUD, FTS, embedding, graph, ops, AST links
// Source: knowledge.rs (53 methods) + migration.rs (kb_rebuild_fts_if_dirty)
// ============================================================================

#[async_trait]
pub trait KbStore: Send + Sync {
    // -- Core CRUD --
    async fn kb_remember(&self, input: &KBRememberInput) -> DbResult<KBRememberResult>;
    async fn kb_get(&self, key: &str) -> DbResult<Option<KnowledgeEntry>>;
    async fn kb_get_by_id(&self, id: &str) -> DbResult<Option<KnowledgeEntry>>;
    async fn kb_get_id_by_key(&self, key: &str) -> DbResult<Option<String>>;
    async fn kb_update(
        &self,
        key: &str,
        new_category: Option<&str>,
        new_summary: Option<&str>,
        new_detail: Option<&serde_json::Value>,
        new_confidence: Option<f64>,
        new_linked_task_id: Option<&str>,
        new_project_id: Option<&str>,
    ) -> DbResult<Option<(KnowledgeEntry, bool)>>;
    async fn kb_set_linked_task_id(&self, key: &str, task_id: Option<&str>) -> DbResult<bool>;
    async fn kb_forget(&self, key: &str) -> DbResult<bool>;
    async fn kb_batch_forget(&self, keys: &[String]) -> DbResult<usize>;
    async fn kb_list(&self, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_list_paginated(
        &self,
        category: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_list_by_scope(&self, task_id: &str) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_clear_scope(&self, id: &str) -> DbResult<()>;
    async fn kb_update_access_stats(&self, entries: &[KnowledgeEntry]) -> DbResult<()>;
    async fn kb_review_upsert(
        &self,
        input: &KnowledgeReviewInput,
    ) -> DbResult<KnowledgeReviewState>;
    async fn kb_review_current_for_ids(
        &self,
        ids: &[String],
    ) -> DbResult<HashMap<String, KnowledgeReviewState>>;
    async fn kb_review_get_by_key(&self, key: &str) -> DbResult<Option<KnowledgeReviewState>>;
    async fn kb_review_stats(&self) -> DbResult<serde_json::Value>;

    // -- Search --
    async fn kb_search(&self, query: &str, category: Option<&str>)
        -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_search_ranked(
        &self,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<(KnowledgeEntry, usize)>>;
    async fn kb_search_fts_ranked(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<(String, usize, Option<String>)>>;
    async fn kb_search_like_ranked(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<(String, usize)>>;

    // -- Search (project-scoped) --
    async fn kb_search_fts_ranked_scoped(
        &self,
        query: &str,
        category: Option<&str>,
        project_id: Option<&str>,
    ) -> DbResult<Vec<(String, usize, Option<String>)>>;
    async fn kb_search_like_ranked_scoped(
        &self,
        query: &str,
        category: Option<&str>,
        project_id: Option<&str>,
    ) -> DbResult<Vec<(String, usize)>>;

    // -- Embeddings --
    async fn kb_set_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()>;
    async fn kb_load_embeddings(&self, category: &str) -> DbResult<Vec<(String, Vec<f32>)>>;
    async fn kb_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>>;
    async fn kb_entries_missing_embedding(
        &self,
        category: Option<&str>,
    ) -> DbResult<Vec<(String, String, String)>>;
    async fn kb_entries_stale_embedding(
        &self,
        current_provider: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, String, String)>>;

    // -- Stats & GC --
    async fn kb_stats(&self) -> DbResult<serde_json::Value>;
    async fn kb_summary(&self) -> DbResult<Vec<(String, i64)>>;
    async fn kb_hot_keys(&self, limit: i64) -> DbResult<Vec<String>>;
    async fn kb_find_stale(&self, days: i64) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_find_duplicates(&self) -> DbResult<Vec<(KnowledgeEntry, KnowledgeEntry, f64)>>;
    async fn embedding_stats(&self) -> DbResult<serde_json::Value>;
    async fn kb_auto_gc(&self) -> DbResult<usize>;
    async fn kb_adjust_confidence(&self, id: &str, delta: f64) -> DbResult<Option<f64>>;
    async fn kb_batch_apply_utility_feedback(
        &self,
        kb_ids: &[String],
        success: bool,
    ) -> DbResult<usize>;
    async fn kb_list_low_confidence(
        &self,
        threshold: f64,
        limit: usize,
    ) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_list_low_utility(
        &self,
        threshold: f64,
        min_access: i64,
        limit: usize,
    ) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_mark_needs_re_extraction(&self, ids: &[String]) -> DbResult<usize>;
    async fn kb_list_stale_state_entries(
        &self,
        stale_days: i64,
        limit: usize,
    ) -> DbResult<Vec<KnowledgeEntry>>;

    // -- FTS management --
    async fn kb_rebuild_fts_if_dirty(&self) -> DbResult<bool>;

    // -- KB Operations queue --
    async fn kb_ops_save_plan(
        &self,
        plan_id: &str,
        task_id: Option<&str>,
        operations: &[KBOperation],
    ) -> DbResult<usize>;
    async fn kb_ops_list(
        &self,
        plan_id: Option<&str>,
        status: Option<&str>,
    ) -> DbResult<Vec<KBOperationRow>>;
    async fn kb_ops_update_status(
        &self,
        op_id: &str,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> DbResult<bool>;
    async fn kb_ops_complete_by_task_id(
        &self,
        task_id: &str,
        new_status: &str,
        result: Option<&str>,
    ) -> DbResult<bool>;
    async fn kb_ops_expire_stale(&self, ttl_secs: i64) -> DbResult<usize>;
    async fn kb_ops_plan_summary(&self, plan_id: &str) -> DbResult<serde_json::Value>;

    // -- Knowledge Graph edges --
    async fn kb_add_edge(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
        weight: f64,
    ) -> DbResult<()>;
    async fn kb_get_edges(&self, id: &str) -> DbResult<Vec<KBEdge>>;
    async fn kb_expand_related(&self, ids: &[String], max_extra: usize) -> DbResult<Vec<String>>;
    async fn kb_delete_edges_for(&self, id: &str) -> DbResult<()>;
    async fn kb_log_co_access(
        &self,
        kb_ids: &[String],
        source: &str,
        session_id: Option<&str>,
    ) -> DbResult<()>;
    async fn get_session_cited_kb_ids(&self, session_id: &str) -> DbResult<Vec<String>>;
    async fn kb_compute_cooccurrence(
        &self,
        since_hours: i64,
        top_n: usize,
    ) -> DbResult<HashMap<String, Vec<String>>>;

    // -- Prompt snapshots --
    async fn save_prompt_snapshot(
        &self,
        task_id: &str,
        prompt: &str,
        cited_kb_ids: &[String],
        category: &str,
    ) -> DbResult<()>;
    async fn update_prompt_snapshot_outcome(&self, task_id: &str, outcome: &str) -> DbResult<()>;
    async fn list_prompt_snapshots(
        &self,
        category: &str,
        limit: usize,
    ) -> DbResult<Vec<(String, String, String)>>;
    async fn list_modified_snapshots(
        &self,
        limit: usize,
    ) -> DbResult<Vec<(String, String, String, String)>>;

    // -- AST links --
    async fn kb_add_ast_link(
        &self,
        kb_id: &str,
        symbol_name: &str,
        file_path: Option<&str>,
        ast_node_id: Option<&str>,
        relation: &str,
        confidence: f64,
    ) -> DbResult<()>;
    async fn kb_get_memories_for_symbol(&self, symbol_name: &str) -> DbResult<Vec<KnowledgeEntry>>;
    async fn kb_get_memories_for_file(
        &self,
        file_path: &str,
    ) -> DbResult<Vec<(String, KnowledgeEntry)>>;
    async fn kb_delete_ast_links_for(&self, kb_id: &str) -> DbResult<()>;
    async fn kb_ast_lazy_resolve(
        &self,
        link_id: i64,
        symbol_name: &str,
    ) -> DbResult<Option<String>>;
}

// ============================================================================
// 8. BoardStore — Tasks, claiming, flow, dependencies, notes, questions
// Source: board.rs + question.rs
// ============================================================================

#[async_trait]
pub trait BoardStore: Send + Sync {
    // -- board.rs: task CRUD --
    async fn insert_board_task(&self, task: &BoardTask) -> DbResult<()>;
    async fn create_board_task(&self, input: &CreateBoardTaskInput) -> DbResult<BoardTask>;
    async fn get_board_task(&self, id: &str) -> DbResult<Option<BoardTask>>;
    async fn get_board_task_projection_artifact_hash(&self, id: &str) -> DbResult<Option<String>>;
    async fn list_board_tasks(
        &self,
        status: Option<&str>,
        include_hidden: bool,
    ) -> DbResult<Vec<BoardTask>>;
    /// v0.5.0: List board tasks filtered by trigger_source + status.
    /// Used by memory_scheduler to poll `trigger_source='memory_hook'` tasks after the
    /// legacy `tasks`-table → `board_tasks` migration.
    async fn list_board_tasks_by_trigger(
        &self,
        trigger_source: &str,
        status: &str,
        limit: i64,
    ) -> DbResult<Vec<BoardTask>>;
    async fn update_board_task(
        &self,
        id: &str,
        update: &UpdateBoardTaskInput,
    ) -> DbResult<Option<BoardTask>>;
    async fn delete_board_task(&self, id: &str) -> DbResult<i64>;
    async fn toggle_board_task(&self, id: &str) -> DbResult<Option<BoardTask>>;
    async fn resolve_board_task_id(&self, id: &str) -> DbResult<Option<String>>;
    async fn find_open_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>>;
    async fn close_task_by_dedupe_key(&self, dedupe_key: &str) -> DbResult<Option<BoardTask>>;
    async fn search_board_tasks(&self, input: &BoardSearchInput) -> DbResult<BoardSearchResult>;
    async fn board_summary(&self, since: Option<&str>) -> DbResult<serde_json::Value>;

    // -- board.rs: claiming & autopilot --
    async fn claim_board_task(
        &self,
        id: &str,
        executor_id: &str,
        executor_type: &str,
    ) -> DbResult<Option<BoardTask>>;
    async fn clear_board_task_assignee(
        &self,
        task_id: &str,
        expected_assignee: &str,
    ) -> DbResult<usize>;
    async fn fail_board_task_and_release_claim(&self, task_id: &str)
        -> DbResult<Option<BoardTask>>;
    async fn release_board_claims_by_executor(&self, executor_id: &str) -> DbResult<usize>;
    async fn recover_stale_running_tasks(&self, fallback_stale_minutes: i64) -> DbResult<usize>;
    /// Null out `assignee` on board_tasks whose assignee references a
    /// `slot-dyn-*` slot that is no longer `active` (terminated, or whose
    /// dynamic_slots row has been removed). Used at daemon startup as a
    /// companion to `recover_stale_running_tasks`: that function resets the
    /// status / claim fields but intentionally preserves `assignee`, leaving
    /// a dangling pointer that makes autopilot's per-slot dispatch path keep
    /// trying to send to a slot that no longer exists. Returns the number of
    /// rows whose assignee was cleared.
    async fn clear_dangling_dynamic_slot_assignees(&self) -> DbResult<usize>;
    async fn set_board_task_lease(&self, task_id: &str, lease_expires_at: &str) -> DbResult<usize>;
    async fn list_running_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>>;
    async fn list_autopilot_tasks(&self) -> DbResult<Vec<BoardTask>>;
    async fn count_open_tasks_by_priority(&self, priorities: &[&str]) -> DbResult<i64>;
    async fn count_tasks_by_category(&self, category: &str) -> DbResult<i64>;
    async fn clear_done_board_tasks(&self) -> DbResult<i64>;

    // -- board.rs: dependencies (DAG) --
    async fn check_dependencies(&self, depends_on: &[TaskId]) -> DbResult<DependencyStatus>;
    async fn find_downstream_tasks(&self, task_id: &str) -> DbResult<Vec<String>>;
    async fn retry_board_task(
        &self,
        task_id: &str,
        reset_downstream: bool,
    ) -> DbResult<Vec<String>>;

    // -- board.rs: context & notes --
    async fn get_board_tasks_with_context(
        &self,
        ids: &[String],
        include_children: bool,
    ) -> DbResult<Vec<BoardTaskWithContext>>;
    async fn add_board_task_note(&self, input: &AddBoardTaskNoteInput) -> DbResult<BoardTaskNote>;
    async fn get_board_task_notes(&self, task_id: &str) -> DbResult<Vec<BoardTaskNote>>;
    async fn get_board_task_with_notes(&self, id: &str) -> DbResult<Option<BoardTaskWithNotes>>;
    async fn query_board_tasks_in_range(
        &self,
        time_col: &str,
        since: &str,
        until: &str,
    ) -> DbResult<Vec<serde_json::Value>>;
    async fn query_board_tasks_in_range_with_status(
        &self,
        status: &str,
        since: &str,
        until: &str,
    ) -> DbResult<Vec<serde_json::Value>>;

    // -- question.rs: agent decision requests --
    async fn create_agent_question(
        &self,
        input: &CreateAgentQuestionInput,
    ) -> DbResult<AgentQuestion>;
    async fn get_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>>;
    async fn list_agent_questions(
        &self,
        status: Option<&str>,
        target: Option<&str>,
        limit: Option<usize>,
    ) -> DbResult<Vec<AgentQuestion>>;
    async fn list_questions_for_task(&self, task_id: &str) -> DbResult<Vec<AgentQuestion>>;
    async fn answer_agent_question(
        &self,
        id: &str,
        answer: &str,
    ) -> DbResult<Option<AgentQuestion>>;
    async fn dismiss_agent_question(&self, id: &str) -> DbResult<Option<AgentQuestion>>;
    async fn set_question_routing_trace(&self, id: &str, trace_json: &str) -> DbResult<()>;
    async fn downgrade_question_to_user(&self, id: &str) -> DbResult<()>;
    async fn increment_question_retry(&self, id: &str) -> DbResult<()>;
    async fn find_stale_master_questions(&self, max_age_secs: i64) -> DbResult<Vec<AgentQuestion>>;
    async fn find_tasks_with_unharvested_decisions(
        &self,
        min_count: usize,
    ) -> DbResult<Vec<(String, String, usize)>>;
    async fn decision_stats(&self, hours: i64) -> DbResult<serde_json::Value>;
    async fn increment_board_task_retry(&self, task_id: &str, new_retry: i64) -> DbResult<()>;
    async fn unclaim_board_task(&self, task_id: &str) -> DbResult<()>;
}

// ============================================================================
// 9. TimelineStore — timeline read projection over event_log (SSOT)
// v1.3.0 SSOT cutover: event_log is the timeline truth source (frozen lisp §4.6).
// All methods are pure reads, shaped via `event::projection`. Writes no longer
// exist — insert/update/cleanup paths went away with the legacy
// `system_timeline` table (drop migration 20260420200000).
// Source: db/pg/timeline.rs → missiond_core::event::projection
// ============================================================================

#[async_trait]
pub trait TimelineStore: Send + Sync {
    async fn query_timeline_since(
        &self,
        since_seq: i64,
        limit: usize,
    ) -> DbResult<Vec<TimelineRow>>;
    async fn timeline_latest_seq(&self) -> DbResult<i64>;
    async fn query_timeline_filtered(
        &self,
        event_type: Option<&str>,
        trace_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<TimelineRow>>;
    async fn query_timeline_stratified(
        &self,
        since: &str,
        until: &str,
        per_type_limit: i64,
        type_limits: &HashMap<String, i64>,
    ) -> DbResult<Vec<TimelineRow>>;
    async fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>>;
    async fn query_timeline_stats(
        &self,
        since: Option<&str>,
        until: Option<&str>,
    ) -> DbResult<TimelineStats>;
    async fn query_timeline_search(
        &self,
        keyword: &str,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<TimelineRow>>;
}

// ============================================================================
// 10. SlotStore — Slot sessions, slot tasks, dynamic slots, daemon state
// Source: slot.rs + dynamic_slot.rs + task.rs
// ============================================================================

#[async_trait]
pub trait SlotStore: Send + Sync {
    // -- slot.rs: session mapping --
    async fn get_slot_session(&self, slot_id: &str) -> DbResult<Option<String>>;
    async fn set_slot_session(&self, slot_id: &str, session_id: &str) -> DbResult<()>;
    async fn cleanup_pty_placeholder(&self, slot_id: &str) -> DbResult<()>;
    async fn delete_slot_session(&self, slot_id: &str) -> DbResult<()>;
    async fn get_all_slot_sessions(&self) -> DbResult<Vec<(String, String)>>;
    async fn get_slot_for_session(&self, session_id: &str) -> DbResult<Option<String>>;

    // -- slot.rs: slot tasks --
    async fn insert_slot_task(&self, task: &SlotTask) -> DbResult<()>;
    async fn slot_task_set_running(&self, id: &str) -> DbResult<()>;
    async fn slot_task_set_completed(&self, id: &str, output_count: i64) -> DbResult<()>;
    async fn slot_task_set_failed(&self, id: &str, error: &str) -> DbResult<()>;
    async fn list_slot_tasks(
        &self,
        slot_id: Option<&str>,
        task_type: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<SlotTask>>;
    async fn slot_task_stats(&self, slot_id: Option<&str>) -> DbResult<serde_json::Value>;
    async fn reap_stale_slot_tasks(&self, max_age_secs: i64) -> DbResult<usize>;
    async fn find_stale_decision_tasks(&self, max_age_secs: i64) -> DbResult<Vec<SlotTask>>;
    async fn cleanup_orphan_slot_tasks(&self) -> DbResult<usize>;
    async fn last_completed_slot_task_at(&self, task_type: &str) -> DbResult<Option<i64>>;
    async fn get_running_slot_task(&self, slot_id: &str) -> DbResult<Option<String>>;

    // -- task.rs: generic tasks --
    async fn insert_task(&self, task: &Task) -> DbResult<()>;
    async fn update_task(&self, id: &str, update: &TaskUpdate) -> DbResult<()>;
    async fn get_task(&self, id: &str) -> DbResult<Option<Task>>;
    async fn get_tasks_by_status(&self, status: TaskStatus) -> DbResult<Vec<Task>>;
    async fn get_queued_tasks_by_role(&self, role: &str) -> DbResult<Vec<Task>>;
    async fn requeue_running_tasks_for_slot(&self, slot_id: &str) -> DbResult<usize>;
    async fn get_all_tasks(&self, limit: i64) -> DbResult<Vec<Task>>;
    async fn ack_completed_tasks(&self, since: Option<i64>) -> DbResult<Vec<Task>>;

    // -- task.rs: inbox --
    async fn insert_inbox_message(&self, msg: &InboxMessage) -> DbResult<()>;
    async fn get_inbox_messages(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> DbResult<Vec<InboxMessage>>;
    async fn mark_inbox_read(&self, id: &str) -> DbResult<()>;

    // -- task.rs: events (legacy) — removed v0.4.23 Phase 6:
    //    table `events` dropped; pillar 四 event_log is the SSOT.

    // -- dynamic_slot.rs --
    async fn create_dynamic_slot(&self, slot: &DynamicSlot) -> DbResult<()>;
    async fn get_dynamic_slot(&self, id: &str) -> DbResult<Option<DynamicSlot>>;
    async fn list_dynamic_slots(&self, status: Option<&str>) -> DbResult<Vec<DynamicSlot>>;
    async fn count_active_dynamic_slots(&self) -> DbResult<i64>;
    async fn terminate_dynamic_slot(&self, id: &str, reason: &str) -> DbResult<bool>;
    async fn extend_dynamic_slot(
        &self,
        id: &str,
        additional_seconds: i64,
    ) -> DbResult<Option<String>>;
    async fn find_expired_dynamic_slots(&self) -> DbResult<Vec<DynamicSlot>>;
    async fn find_expiring_dynamic_slots(&self, within_seconds: i64) -> DbResult<Vec<DynamicSlot>>;
}

// ============================================================================
// 12. ObservabilityStore — Gemini log, incidents, tokens, watermarks, backfill, router chat
// Source: gemini_log.rs + incident.rs + watermark.rs + backfill.rs + router_chat.rs
// ============================================================================

#[async_trait]
pub trait ObservabilityStore: Send + Sync {
    // -- gemini_log.rs --
    async fn gemini_log_insert_started(
        &self,
        id: &str,
        caller: &str,
        session_id: Option<&str>,
        model: &str,
        prompt_chars: i64,
        prompt_text: Option<&str>,
    ) -> DbResult<()>;
    async fn gemini_log_update_completed(
        &self,
        id: &str,
        api_mode: &str,
        response_chars: i64,
        queue_wait_ms: i64,
        duration_ms: i64,
        retry_count: i64,
        status: &str,
        error_msg: Option<&str>,
        response_text: Option<&str>,
    ) -> DbResult<()>;
    async fn gemini_log_get_content(&self, request_id: &str)
        -> DbResult<Option<serde_json::Value>>;
    async fn gemini_log_query(
        &self,
        caller: Option<&str>,
        session_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>>;
    async fn gemini_log_stats(&self) -> DbResult<serde_json::Value>;
    async fn gemini_log_cleanup(&self, retention_days: i64) -> DbResult<i64>;
    async fn gemini_file_cache_get(&self, file_hash: &str) -> DbResult<Option<String>>;
    async fn gemini_file_cache_put(
        &self,
        file_hash: &str,
        file_uri: &str,
        mime_type: &str,
        expires_at: i64,
    ) -> DbResult<()>;
    async fn gemini_file_cache_gc(&self) -> DbResult<i64>;

    // -- incident.rs --
    async fn insert_incident(
        &self,
        id: &str,
        severity: &str,
        source: &str,
        title: &str,
        description: &str,
        server_id: Option<&str>,
        raw_payload: Option<&str>,
        board_task_id: Option<&str>,
        dedupe_key: &str,
    ) -> DbResult<()>;
    async fn has_recent_incident(&self, dedupe_key: &str, window_secs: i64) -> DbResult<bool>;
    async fn list_incidents(&self, limit: i64) -> DbResult<Vec<IncidentRow>>;
    async fn get_incident_by_id(&self, id: &str) -> DbResult<Option<IncidentRow>>;
    async fn update_incident_board_task_id(&self, id: &str, board_task_id: &str) -> DbResult<bool>;

    // -- incident.rs: token ledger --
    async fn insert_token_usage(
        &self,
        conversation_id: &str,
        slot_id: Option<&str>,
        slot_task_id: Option<&str>,
        model: Option<&str>,
        input_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
        output_tokens: i64,
        message_id: Option<i64>,
    ) -> DbResult<()>;
    async fn token_stats(
        &self,
        conversation_id: Option<&str>,
        slot_id: Option<&str>,
        since: Option<&str>,
        group_by: Option<&str>,
    ) -> DbResult<Vec<HashMap<String, serde_json::Value>>>;

    // -- watermark.rs: labels --
    async fn label_set(
        &self,
        message_id: i64,
        label: &str,
        value: &str,
        source: &str,
    ) -> DbResult<()>;
    async fn label_set_batch(&self, labels: &[(i64, &str, &str, &str)]) -> DbResult<usize>;
    async fn label_get(&self, message_id: i64) -> DbResult<Vec<(String, String, String)>>;
    async fn label_get_batch(
        &self,
        message_ids: &[i64],
    ) -> DbResult<HashMap<i64, Vec<(String, String)>>>;
    async fn label_find_messages(
        &self,
        label: &str,
        value: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<i64>>;
    async fn message_label_evidence_upsert_batch(
        &self,
        evidence: &[MessageLabelEvidenceInput],
    ) -> DbResult<usize>;
    async fn message_label_projection_refresh(&self, message_ids: &[i64]) -> DbResult<usize>;
    async fn message_labeler_pending_sessions(
        &self,
        consumer: &str,
        source: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<String>>;
    async fn message_labeler_audit(
        &self,
        consumer: &str,
        source: Option<&str>,
    ) -> DbResult<serde_json::Value>;

    // -- conversation labels (EAV, same pattern as message_labels) --
    async fn conversation_label_set(
        &self,
        session_id: &str,
        label: &str,
        value: &str,
        source: &str,
    ) -> DbResult<()> {
        let _ = (session_id, label, value, source);
        Ok(())
    }
    async fn conversation_label_delete(&self, session_id: &str, label: &str) -> DbResult<()> {
        let _ = (session_id, label);
        Ok(())
    }
    async fn conversation_label_get(&self, session_id: &str) -> DbResult<Vec<(String, String)>> {
        let _ = session_id;
        Ok(vec![])
    }
    async fn conversation_label_get_batch(
        &self,
        session_ids: &[&str],
    ) -> DbResult<HashMap<String, Vec<(String, String)>>> {
        let _ = session_ids;
        Ok(HashMap::new())
    }
    async fn conversation_label_find(
        &self,
        label: &str,
        value: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let _ = (label, value, limit);
        Ok(vec![])
    }

    // -- backfill.rs: conversation-specific cursors (stay here, tied to conversation analysis) --
    async fn conversations_missing_summary_cursor(
        &self,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String)>>;
    async fn conversations_needing_topic_vectors_cursor(
        &self,
        provider: &str,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String)>>;
    async fn conversations_missing_summary_count(&self) -> DbResult<i64>;
    async fn conversations_needing_topic_vectors_count(&self, provider: &str) -> DbResult<i64>;

    // -- router_chat.rs --
    async fn router_chat_get_or_create(&self, task_id: &str, model: &str) -> DbResult<String>;
    async fn router_chat_load_history(&self, conv_id: &str) -> DbResult<Vec<serde_json::Value>>;
    async fn router_chat_get_summary(&self, conv_id: &str) -> DbResult<(Option<String>, i64)>;
    async fn router_chat_load_active_history(
        &self,
        conv_id: &str,
        after_id: i64,
    ) -> DbResult<Vec<serde_json::Value>>;
    async fn router_chat_unsummarized_count(&self, conv_id: &str, after_id: i64) -> DbResult<i64>;
    async fn router_chat_load_compressible(
        &self,
        conv_id: &str,
        after_id: i64,
        batch_size: i64,
    ) -> DbResult<Vec<(i64, String, String)>>;
    async fn router_chat_update_summary(
        &self,
        conv_id: &str,
        new_summary: &str,
        new_cursor: i64,
        expected_old_cursor: i64,
    ) -> DbResult<bool>;
    async fn router_chat_append_messages(
        &self,
        conv_id: &str,
        messages: &[(String, String)],
    ) -> DbResult<()>;
    async fn jarvis_get_or_create(&self, conversation_id: Option<&str>) -> DbResult<String>;
    async fn jarvis_get_or_create_scoped(
        &self,
        conversation_id: Option<&str>,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
    ) -> DbResult<String>;
    async fn jarvis_list_scoped(
        &self,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        include_legacy_unscoped: bool,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>>;
    async fn jarvis_history_scoped(
        &self,
        conversation_id: &str,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        include_legacy_unscoped: bool,
        tail: i64,
    ) -> DbResult<Option<serde_json::Value>>;
    async fn jarvis_save_exchange(
        &self,
        conv_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> DbResult<()>;
    async fn jarvis_update_title(&self, conv_id: &str, title: &str) -> DbResult<()>;
    /// Phase 7: Find the most recent active Jarvis UI conversation.
    async fn find_latest_jarvis_conversation(&self) -> DbResult<Option<String>>;
    async fn router_chat_list(&self, limit: i64) -> DbResult<Vec<serde_json::Value>>;
    async fn router_chat_stats(&self) -> DbResult<serde_json::Value>;
    async fn router_chat_clear(
        &self,
        conversation_id: &str,
        count: Option<i64>,
    ) -> DbResult<(i64, i64)>;
    async fn router_chat_clear_by_task(&self, task_id: &str, count: Option<i64>) -> DbResult<i64>;
    async fn router_chat_delete_message(&self, message_id: i64) -> DbResult<Option<String>>;
    async fn router_chat_delete(&self, conversation_id: &str) -> DbResult<(i64, i64)>;
    async fn router_chat_delete_by_task(&self, task_id: &str) -> DbResult<(i64, i64)>;
    async fn router_chat_restore(&self, conversation_id: &str) -> DbResult<i64>;

    // -- from VisionStore v0.4.23 --
    // Source: vision.rs (image_descriptions, system-support module) + translation.rs (message_translations, conversation-logs)
    async fn get_image_description(&self, image_hash: &str) -> DbResult<Option<String>>;
    async fn save_image_description(
        &self,
        image_hash: &str,
        media_type: &str,
        description: &str,
    ) -> DbResult<()>;
    async fn update_message_content(&self, message_id: i64, new_content: &str) -> DbResult<()>;
    async fn get_message_raw_content(&self, message_id: i64) -> DbResult<Option<String>>;
    async fn find_unprocessed_image_messages(&self, limit: usize) -> DbResult<Vec<(i64, String)>>;
    async fn mark_vision_permanently_failed(&self, message_id: i64) -> DbResult<bool>;
    async fn image_description_count(&self) -> DbResult<i64>;
    async fn insert_translation(
        &self,
        message_id: i64,
        translation: &str,
        model: &str,
        duration_ms: u64,
    ) -> DbResult<()>;
    async fn get_translation(&self, message_id: i64) -> DbResult<Option<(String, String)>>;
    async fn has_translation(&self, message_id: i64) -> DbResult<bool>;
}

// ============================================================================
// 13. InfraStore — Infrastructure state management
// Source: db/... (Stage 2D: 从 ObservabilityStore + SlotStore 拆分方法进来)
// Scope: watermarks (reconcile/gemini_cli/consumer/watcher) + backfill (progress/failures) + daemon_state
// ============================================================================

#[async_trait]
pub trait InfraStore: Send + Sync {
    // -- watermarks (from ObservabilityStore v0.4.x) --
    // consumer_watermarks (generic per-consumer per-session cursor)
    async fn watermark_get(
        &self,
        consumer: &str,
        session_id: &str,
    ) -> DbResult<Option<(Option<i64>, Option<String>)>>;
    async fn watermark_advance_time(
        &self,
        consumer: &str,
        session_id: &str,
        timestamp: &str,
    ) -> DbResult<()>;
    async fn watermark_advance_msg_id(
        &self,
        consumer: &str,
        session_id: &str,
        msg_id: i64,
    ) -> DbResult<()>;
    async fn watermark_advance_full(
        &self,
        consumer: &str,
        session_id: &str,
        msg_id: Option<i64>,
        timestamp: Option<&str>,
        extra: Option<&str>,
    ) -> DbResult<()>;
    async fn watermark_list(
        &self,
        consumer: &str,
    ) -> DbResult<Vec<(String, Option<i64>, Option<String>)>>;

    // watcher_cursors (JSONL byte-offset tracking)
    async fn load_watcher_cursors(&self) -> DbResult<HashMap<String, u64>>;
    async fn upsert_watcher_cursors_batch(&self, cursors: &HashMap<String, u64>) -> DbResult<()>;
    async fn delete_watcher_cursor(&self, file_path: &str) -> DbResult<()>;

    // reconcile_watermarks (last-reconciled file size)
    async fn get_reconcile_watermark(&self, path: &str) -> DbResult<Option<i64>>;
    async fn upsert_reconcile_watermark(&self, path: &str, size: i64) -> DbResult<()>;
    async fn get_all_reconcile_watermarks(&self) -> DbResult<HashMap<String, i64>>;

    // gemini_cli_watermarks
    async fn load_gemini_cursors(&self) -> DbResult<HashMap<String, i64>>;
    async fn save_gemini_cursor(
        &self,
        file_path: &str,
        session_id: &str,
        msg_count: i64,
    ) -> DbResult<()>;

    // -- backfill (from ObservabilityStore v0.4.x) --
    async fn backfill_get_phase(&self, phase: &str) -> DbResult<Option<BackfillPhaseStatus>>;
    async fn backfill_list_phases(&self) -> DbResult<Vec<BackfillPhaseStatus>>;
    async fn backfill_start_phase(&self, phase: &str, total_estimated: i64) -> DbResult<()>;
    async fn backfill_update_progress(
        &self,
        phase: &str,
        new_cursor: i64,
        batch_success: i64,
        batch_failed: i64,
    ) -> DbResult<()>;
    async fn backfill_complete_phase(&self, phase: &str) -> DbResult<()>;
    async fn backfill_record_failure(
        &self,
        session_id: &str,
        phase: &str,
        error: &str,
    ) -> DbResult<()>;
    async fn backfill_retryable_failures(
        &self,
        phase: &str,
        max_retries: i64,
        limit: i64,
    ) -> DbResult<Vec<String>>;
    async fn backfill_retryable_failures_no_cooldown(
        &self,
        phase: &str,
        max_retries: i64,
    ) -> DbResult<i64>;
    async fn backfill_clear_failure(&self, session_id: &str, phase: &str) -> DbResult<()>;

    // -- daemon_state (from SlotStore v0.4.x) --
    async fn daemon_state_get(&self, key: &str) -> DbResult<Option<i64>>;
    async fn daemon_state_set(&self, key: &str, value: i64) -> DbResult<()>;
}

// ============================================================================
// 14. DirectiveLayerStore — User directive compilation pipeline (directive → plan → workflow)
// Source: module directive-layer, 管 3 表 directive/plan/workflow
// Status: schema-ready-pending-implementation (writer TBD: pillar 五 actor)
// ============================================================================

#[async_trait]
pub trait DirectiveLayerStore: Send + Sync {
    // -- directive 表 (6 方法) --
    async fn directive_insert(
        &self,
        utterance_text: &str,
        sexp_text: &str,
        version: i32,
        status: DirectiveStatus,
        compiler_model: Option<&str>,
        references_json: Option<&serde_json::Value>,
    ) -> DbResult<uuid::Uuid>;
    async fn directive_get(&self, id: uuid::Uuid, version: i32) -> DbResult<Option<Directive>>;
    async fn directive_update_status(
        &self,
        id: uuid::Uuid,
        version: i32,
        new_status: DirectiveStatus,
    ) -> DbResult<()>;
    async fn directive_approve(&self, id: uuid::Uuid, version: i32) -> DbResult<()>;
    async fn directive_list_by_status(
        &self,
        status: DirectiveStatus,
        limit: i64,
    ) -> DbResult<Vec<Directive>>;
    async fn directive_get_version_chain(&self, id: uuid::Uuid) -> DbResult<Vec<Directive>>;
    /// Recent directives, optionally filtered by status. `limit` capped by caller.
    async fn directive_list_recent(
        &self,
        status: Option<DirectiveStatus>,
        limit: i64,
    ) -> DbResult<Vec<Directive>>;

    // -- plan 表 (6 方法) --
    async fn plan_insert(
        &self,
        board_task_id: &str,
        source_directive_id: Option<uuid::Uuid>,
        version: i32,
        sexp_text: &str,
        sexp_hash: &str,
        status: PlanStatus,
        compiler_model: Option<&str>,
        compiled_from: Option<&str>,
    ) -> DbResult<uuid::Uuid>;
    async fn plan_get(&self, id: uuid::Uuid) -> DbResult<Option<Plan>>;
    async fn plan_update_contract_json(
        &self,
        id: uuid::Uuid,
        contract_json: &serde_json::Value,
    ) -> DbResult<()>;
    async fn plan_update_status(&self, id: uuid::Uuid, new_status: PlanStatus) -> DbResult<()>;
    async fn plan_supersede(&self, old_id: uuid::Uuid, new_id: uuid::Uuid) -> DbResult<()>;
    async fn plan_list_by_task(&self, board_task_id: &str) -> DbResult<Vec<Plan>>;
    async fn plan_get_latest(&self, board_task_id: &str) -> DbResult<Option<Plan>>;
    /// Recent plans, optionally filtered by status (cross-task, manager-surface only).
    async fn plan_list_recent(&self, status: Option<PlanStatus>, limit: i64)
        -> DbResult<Vec<Plan>>;
    async fn plan_list_contract_backfill_candidates(
        &self,
        status: Option<PlanStatus>,
        include_terminal: bool,
        limit: i64,
    ) -> DbResult<Vec<Plan>>;

    async fn lisp_code_sync_enqueue_job(
        &self,
        input: &EnqueueLispCodeSyncJob,
    ) -> DbResult<uuid::Uuid>;
    async fn lisp_code_sync_claim_due_jobs(
        &self,
        lease_owner: &str,
        limit: i64,
        lease_secs: i64,
    ) -> DbResult<Vec<LispCodeSyncJob>>;
    async fn lisp_code_sync_complete_job(
        &self,
        id: uuid::Uuid,
        status: &str,
        checker_ok: Option<bool>,
        checker_command: Option<&str>,
        checker_tail: Option<&str>,
        sync_task_id: Option<&str>,
        last_error: Option<&str>,
        retry_after_secs: Option<i64>,
    ) -> DbResult<()>;
    async fn lisp_code_sync_queue_stats(&self) -> DbResult<LispCodeSyncQueueStats>;

    // -- workflow 表 (5 方法) --
    async fn workflow_insert(
        &self,
        name: &str,
        sexp_text: &str,
        match_rules: &serde_json::Value,
        learned_from: Option<uuid::Uuid>,
    ) -> DbResult<uuid::Uuid>;
    async fn workflow_get_by_name(&self, name: &str) -> DbResult<Option<Workflow>>;
    async fn workflow_get_by_id(&self, id: uuid::Uuid) -> DbResult<Option<Workflow>>;
    /// Find workflows whose `match_rules` JSONB contains any token of `query_utterance`.
    /// Simplified: substring match via JSONB `@>`-like text search.
    async fn workflow_find_by_match(&self, query_utterance: &str) -> DbResult<Vec<Workflow>>;
    async fn workflow_record_execution(
        &self,
        id: uuid::Uuid,
        success: bool,
        cost_usd: Option<f64>,
    ) -> DbResult<()>;
    async fn workflow_list_top_n(&self, n: i64) -> DbResult<Vec<Workflow>>;
}

// ============================================================================
// ProjectStore — module project-management
// Source: db/project.rs + db/skill.rs + db/beacon.rs + db/ast.rs
// Scope: projects + skill_topics/blocks/versions/executions (5 tables per v0.4.23)
// ============================================================================

#[async_trait]
pub trait ProjectStore: Send + Sync {
    async fn list_projects(&self) -> DbResult<Vec<crate::types::ProjectConfig>>;
    async fn get_project(&self, id: &str) -> DbResult<Option<crate::types::ProjectConfig>>;
    async fn upsert_project(&self, config: &crate::types::ProjectConfig) -> DbResult<()>;
    async fn set_project_active(&self, id: &str, active: bool) -> DbResult<bool>;
    /// Backfill project_id on conversations matching the given path prefix pattern.
    async fn backfill_project_id(&self, project_id: &str, path_pattern: &str) -> DbResult<u64>;

    /// Get conversation statistics for a project.
    async fn conversation_stats_by_project(&self, project_id: &str) -> DbResult<serde_json::Value>;

    /// Get recent conversations for a project (most recent first).
    async fn recent_conversations_by_project(
        &self,
        project_id: &str,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>>;

    /// Get KB entry count grouped by category for a project.
    async fn kb_stats_by_project(&self, project_id: &str) -> DbResult<serde_json::Value>;

    // -- skill_topics / blocks / versions / executions (from SkillStore v0.4.x) --

    // -- skill.rs: topics --
    async fn skill_topic_upsert(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
    ) -> DbResult<()>;
    async fn skill_topic_upsert_full(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
        context_hooks_json: Option<&str>,
    ) -> DbResult<()>;
    async fn skill_topic_get(&self, topic: &str) -> DbResult<Option<SkillTopic>>;
    async fn skill_topic_list(&self) -> DbResult<Vec<SkillTopic>>;
    async fn skill_topic_delete(&self, topic: &str) -> DbResult<bool>;
    async fn skill_topic_hit(&self, topic: &str) -> DbResult<()>;
    async fn skill_topic_update_stats(
        &self,
        topic: &str,
        total_lines: i32,
        checksum: &str,
    ) -> DbResult<()>;

    // -- skill.rs: blocks --
    async fn skill_block_insert(
        &self,
        topic: &str,
        block_type: &str,
        title: Option<&str>,
        content: &str,
        sort_order: i32,
    ) -> DbResult<String>;
    async fn skill_block_update(&self, id: &str, content: &str) -> DbResult<bool>;
    async fn skill_blocks_for_topic(&self, topic: &str) -> DbResult<Vec<SkillBlock>>;
    async fn skill_block_set_status(&self, id: &str, status: &str) -> DbResult<bool>;
    async fn skill_blocks_delete_topic(&self, topic: &str) -> DbResult<usize>;

    // -- skill.rs: search --
    async fn skill_search_fts(&self, query: &str) -> DbResult<Vec<SkillSearchResult>>;
    async fn skill_search_topics(&self, query: &str) -> DbResult<Vec<SkillTopic>>;
    async fn skill_rebuild_fts(&self) -> DbResult<()>;

    // -- skill.rs: embeddings --
    async fn skill_set_topic_embedding(
        &self,
        topic: &str,
        embedding: &[f32],
        provider: &str,
    ) -> DbResult<()>;
    async fn skill_load_topic_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>>;
    async fn skill_topics_missing_embedding(&self, limit: i64) -> DbResult<Vec<(String, String)>>;
    async fn skill_topics_stale_embedding(
        &self,
        current_provider: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, String)>>;

    // -- skill.rs: executions --
    async fn skill_execution_insert(
        &self,
        id: &str,
        skill_topic: &str,
        action_id: &str,
        steps_total: i32,
        triggered_by: &str,
    ) -> DbResult<()>;
    async fn skill_execution_update(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
    ) -> DbResult<()>;
    async fn skill_execution_update_with_duration(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
        duration_ms: Option<i64>,
    ) -> DbResult<()>;
    async fn skill_execution_stats(&self, topic: Option<&str>)
        -> DbResult<Vec<SkillExecutionStat>>;
    async fn skill_execution_is_running(
        &self,
        skill_topic: &str,
        action_id: &str,
    ) -> DbResult<bool>;

    // -- skill.rs: versions --
    async fn skill_version_save(&self, topic: &str, content: &str, checksum: &str) -> DbResult<()>;
    async fn skill_version_list(&self, topic: &str, limit: i64) -> DbResult<Vec<SkillVersion>>;
    async fn skill_version_get(&self, id: i64) -> DbResult<Option<SkillVersion>>;

    // -- beacon.rs --
    async fn beacon_list(&self) -> DbResult<Vec<BeaconInfo>>;
    async fn beacon_ensure(&self, name: &str) -> DbResult<String>;
    async fn beacon_increment_harvest(&self, name: &str) -> DbResult<i64>;
    async fn beacon_set_description(&self, name: &str, description: &str) -> DbResult<bool>;
    async fn beacon_node_upsert(
        &self,
        beacon_id: &str,
        repo: &str,
        file_path: &str,
        symbol_name: &str,
        annotation: Option<&str>,
    ) -> DbResult<()>;
    async fn beacon_map(&self, beacon_name: &str) -> DbResult<Vec<BeaconNode>>;
    async fn beacon_nodes_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize>;
    async fn beacon_node_annotate(
        &self,
        beacon_name: &str,
        repo: &str,
        file_path: &str,
        symbol_name: &str,
        annotation: &str,
    ) -> DbResult<bool>;
    async fn beacon_search(&self, query: &str) -> DbResult<Vec<BeaconInfo>>;
    async fn beacon_cleanup_empty(&self) -> DbResult<usize>;

    // -- ast.rs --
    async fn ast_sync_file(
        &self,
        repo: &str,
        file_path: &str,
        commit_hash: &str,
        nodes: &[CodeNode],
    ) -> DbResult<AstSyncResult>;
    async fn ast_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize>;
    async fn ast_search(&self, query: &str, limit: usize) -> DbResult<Vec<AstSearchHit>>;
    async fn ast_get_file_nodes(&self, repo: &str, file_path: &str) -> DbResult<Vec<AstNodeRow>>;
    async fn ast_find_related(&self, name: &str, limit: usize) -> DbResult<Vec<AstSearchHit>>;
    async fn ast_get_file_commit(&self, repo: &str, file_path: &str) -> DbResult<Option<String>>;
    async fn ast_stats(&self) -> DbResult<AstStats>;
    async fn ast_set_embedding(
        &self,
        node_id: &str,
        embedding: &[u8],
        provider: &str,
    ) -> DbResult<()>;
    async fn ast_find_unembedded(&self, limit: usize) -> DbResult<Vec<(String, String, String)>>;
    async fn ast_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>>;
    async fn ast_search_ranked(&self, query: &str, limit: usize) -> DbResult<Vec<(String, usize)>>;
    async fn ast_get_search_hit(&self, node_id: &str) -> DbResult<Option<AstSearchHit>>;
    async fn ast_get_node(&self, node_id: &str) -> DbResult<Option<AstNodeRow>>;
    async fn ast_module_summaries(&self, repo: &str) -> DbResult<Vec<ModuleAstSummary>>;
}

// ============================================================================
// 15. ArtifactCommitStore — artifact/DB/event commit outbox
// ============================================================================

#[async_trait]
pub trait ArtifactCommitStore: Send + Sync {
    async fn artifact_commit_outbox_upsert_pending(
        &self,
        input: &ArtifactCommitOutboxInput,
    ) -> DbResult<ArtifactCommitOutboxRecord>;

    async fn artifact_commit_outbox_mark_complete(
        &self,
        operation_key: &str,
        artifact_sha256: &str,
        payload: &serde_json::Value,
    ) -> DbResult<ArtifactCommitOutboxRecord>;

    async fn artifact_commit_outbox_mark_failed(
        &self,
        operation_key: &str,
        error: &str,
    ) -> DbResult<ArtifactCommitOutboxRecord>;

    async fn artifact_commit_outbox_get(
        &self,
        operation_key: &str,
    ) -> DbResult<Option<ArtifactCommitOutboxRecord>>;

    async fn artifact_commit_outbox_claim_recoverable(
        &self,
        limit: i64,
    ) -> DbResult<Vec<ArtifactCommitOutboxRecord>>;
}

// ============================================================================
// MissionStore — Super-trait aggregating all domain stores
// ============================================================================

/// Unified store trait. Use `Arc<dyn MissionStore>` for dependency injection.
/// Backends must explicitly implement this trait (no blanket impl).
#[async_trait]
pub trait MissionStore:
    ConversationStore
    + MessageStore
    + KbStore
    + BoardStore
    + TimelineStore
    + SlotStore
    + ObservabilityStore
    + ProjectStore
    + InfraStore
    + DirectiveLayerStore
    + ArtifactCommitStore
    + Send
    + Sync
{
    /// Schema initialization (called once at startup).
    /// PostgreSQL runs sqlx migrations.
    async fn init(&self) -> DbResult<()>;
}
