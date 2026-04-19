//! v1 WebSocket wire format mapping (SSOT for both live + catch-up paths).
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.6 event_log declares
//! `read-ui-projection` as the access pattern for the browser timeline. The
//! mapping here is the **canonical** `(domain, kind, payload)` →
//! `(wire_type, v1_payload)` transform, consumed by both:
//!
//! * **Live stream** (`missiond-daemon/src/bus/ws_bridge.rs`) — tails
//!   `event_log` and broadcasts JSON envelopes on the `/events` WS topic.
//! * **Catch-up replay** (`missiond-core/src/ws/server.rs::handle_catch_up` +
//!   `event::projection`) — on client `sync` request, replays historical
//!   `event_log` rows from `since_seq`.
//!
//! Previously the live path owned the 52-arm match while catch-up emitted the
//! raw `"domain::kind"` projection format. That drift broke the frontend when
//! clients reconnected. This module centralizes the mapping so **both paths
//! produce byte-identical wire output**.
//!
//! The tables here are the **inverse** of `bus::compat::v2_from_v1` plus
//! `event_bus::DaemonEvent::wire_type` / `to_frontend_payload`. Each arm is
//! deliberately verbose to mirror the v1 output byte-for-byte.

use chrono::Utc;
use serde_json::{json, Value};

use crate::event::log::reader::LoggedEvent;

/// Convert a v2 [`LoggedEvent`] into the bit-identical v1 JSON wire envelope
/// consumed by the browser:
///
/// ```json
/// {
///   "type": "<wire_type>",
///   "ts":   <unix_ms>,
///   "seq":  <i64 | 0 for ephemeral>,
///   "trace_id": "<uuid> | null",
///   "span_id":  "<uuid>",
///   "parent_span_id": "<uuid> | null",
///   "payload": { … variant-specific … }
/// }
/// ```
pub fn v2_logged_to_v1_wire_format(logged: &LoggedEvent) -> String {
    let ts_ms = logged.ts.timestamp_millis();
    let seq = if logged.ephemeral { 0 } else { logged.seq.0 };

    let (wire_type, payload) =
        v2_payload_to_v1_shape(logged.domain.as_str(), &logged.kind, &logged.payload);

    let envelope = json!({
        "type": wire_type,
        "ts": ts_ms,
        "seq": seq,
        "trace_id": logged.trace_id.map(|u| u.to_string()),
        "span_id": logged.span_id.map(|u| u.to_string()).unwrap_or_default(),
        "parent_span_id": logged.parent_span_id.map(|u| u.to_string()),
        "payload": payload,
    });
    envelope.to_string()
}

/// Maps the v2 `(domain, kind, payload)` triple to the legacy v1
/// `(wire_type, payload)` tuple.
///
/// Exposed (`pub`) so [`crate::event::projection`] can reuse it in the
/// catch-up path.
pub fn v2_payload_to_v1_shape(
    domain: &str,
    kind: &str,
    payload: &Value,
) -> (&'static str, Value) {
    match (domain, kind) {
        // ── Slot ──
        ("slot", "became_idle") => {
            let slot_id = payload_of(payload, "BecameIdle").get("slot_id").cloned().unwrap_or(Value::Null);
            (
                "slot_state_changed",
                json!({ "slot_id": slot_id, "new_state": "Idle" }),
            )
        }
        ("slot", "state_changed") => {
            let p = payload_of(payload, "StateChanged");
            (
                "slot_state_changed",
                json!({
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "new_state": p.get("new_state").cloned().unwrap_or(Value::Null),
                    "prev_state": p.get("prev_state").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("slot", "task_dispatched") => {
            let p = payload_of(payload, "TaskDispatched");
            (
                "slot_task_dispatched",
                json!({
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "purpose": p.get("purpose").cloned().unwrap_or(Value::Null),
                    "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                    "preview": p.get("preview").cloned().unwrap_or(Value::Null),
                    "cited_kb_ids": p.get("cited_kb_ids").cloned().unwrap_or_else(|| json!([])),
                }),
            )
        }
        ("slot", "stuck") => (
            "slot_state_changed",
            payload_of(payload, "Stuck").clone(),
        ),

        // ── Board ──
        ("board", "task_created") => {
            let p = payload_of(payload, "TaskCreated");
            (
                "board_task_created",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "title": p.get("title").cloned().unwrap_or(Value::Null),
                    "category": p.get("category").cloned().unwrap_or(Value::Null),
                    "action": "created",
                }),
            )
        }
        ("board", "status_changed") => {
            let p = payload_of(payload, "StatusChanged");
            (
                "board_task_status_changed",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "old_status": p.get("old_status").cloned().unwrap_or(Value::Null),
                    "new_status": p.get("new_status").cloned().unwrap_or(Value::Null),
                    "action": "status_changed",
                }),
            )
        }
        ("board", "note_added") => {
            let p = payload_of(payload, "NoteAdded");
            (
                "board_task_note_added",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "note_id": p.get("note_id").cloned().unwrap_or(Value::Null),
                    "content_preview": p.get("content_preview").cloned().unwrap_or(Value::Null),
                    "action": "note_added",
                }),
            )
        }
        ("board", "claimed") => {
            let p = payload_of(payload, "Claimed");
            (
                "board_task_claimed",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "action": "claimed",
                }),
            )
        }
        ("board", "deleted") => {
            let p = payload_of(payload, "Deleted");
            (
                "board_task_deleted",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "title": p.get("title").cloned().unwrap_or(Value::Null),
                    "action": "deleted",
                }),
            )
        }
        ("board", "updated") => {
            let p = payload_of(payload, "Updated");
            (
                "board_task_updated",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "status": p.get("status").cloned().unwrap_or(Value::Null),
                    "category": p.get("category").cloned().unwrap_or(Value::Null),
                    "action": "updated",
                }),
            )
        }

        // ── Task ──
        ("task", "created") => {
            let p = payload_of(payload, "Created");
            (
                "task_lifecycle",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "action": "created",
                }),
            )
        }
        ("task", "completed") => {
            let p = payload_of(payload, "Completed");
            (
                "task_lifecycle",
                json!({
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "action": "completed",
                }),
            )
        }
        ("task", "cascade_triggered") => {
            let p = payload_of(payload, "CascadeTriggered");
            (
                "cascade_triggered",
                json!({
                    "service": p.get("service").cloned().unwrap_or(Value::Null),
                    "changed": p.get("changed").cloned().unwrap_or(Value::Null),
                    "action": "triggered",
                }),
            )
        }
        ("task", "cascade_completed") => {
            let p = payload_of(payload, "CascadeCompleted");
            (
                "cascade_completed",
                json!({
                    "service": p.get("service").cloned().unwrap_or(Value::Null),
                    "services_repaired": p.get("services_repaired").cloned().unwrap_or(Value::Null),
                    "services_failed": p.get("services_failed").cloned().unwrap_or(Value::Null),
                    "hard_halted": p.get("hard_halted").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "action": "completed",
                }),
            )
        }

        // ── Question ──
        ("question", "created") => {
            let p = payload_of(payload, "Created");
            (
                "question_created",
                json!({ "question_id": p.get("question_id").cloned().unwrap_or(Value::Null) }),
            )
        }
        ("question", "resolved") => {
            let p = payload_of(payload, "Resolved");
            (
                "question_resolved",
                json!({
                    "question_id": p.get("question_id").cloned().unwrap_or(Value::Null),
                    "resolution": p.get("resolution").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("question", "decision_resolved") => {
            let p = payload_of(payload, "DecisionResolved");
            (
                "decision_made",
                json!({
                    "question_id": p.get("question_id").cloned().unwrap_or(Value::Null),
                    "tier": p.get("tier").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Llm (unified) ──
        ("llm", "request_started") => {
            let p = payload_of(payload, "RequestStarted");
            let mut obj = json!({
                "engine": provider_as_engine_string(p.get("provider")),
                "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                "model": p.get("model").cloned().unwrap_or(Value::Null),
                "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
            });
            if let Some(extra_obj) = p.get("extra").and_then(|v| v.as_object()) {
                for (k, v) in extra_obj {
                    obj[k] = v.clone();
                }
            }
            ("cli_request_started", obj)
        }
        ("llm", "request_completed") => {
            let p = payload_of(payload, "RequestCompleted");
            let mut obj = json!({
                "engine": provider_as_engine_string(p.get("provider")),
                "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                "model": p.get("model").cloned().unwrap_or(Value::Null),
                "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                "response_chars": p.get("response_chars").cloned().unwrap_or(Value::Null),
                "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                "status": p.get("status").cloned().unwrap_or(Value::Null),
                "error": p.get("error_msg").cloned().unwrap_or(Value::Null),
            });
            if let Some(extra_obj) = p.get("extra").and_then(|v| v.as_object()) {
                for (k, v) in extra_obj {
                    obj[k] = v.clone();
                }
            }
            ("cli_request_completed", obj)
        }
        ("llm", "tool_activity") => {
            let p = payload_of(payload, "ToolActivity");
            (
                "cli_tool_activity",
                json!({
                    "engine": provider_as_engine_string(p.get("provider")),
                    "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                    "tool_seq": p.get("tool_seq").cloned().unwrap_or(Value::Null),
                    "activity": p.get("activity").cloned().unwrap_or(Value::Null),
                    "tool_name": p.get("tool_name").cloned().unwrap_or(Value::Null),
                    "input_preview": p.get("input_preview").cloned().unwrap_or(Value::Null),
                    "result_preview": p.get("result_preview").cloned().unwrap_or(Value::Null),
                    "is_error": p.get("is_error").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Llm (legacy engine-specific) ──
        ("llm", "legacy_gemini_request_started") => {
            let p = payload_of(payload, "LegacyGeminiRequestStarted");
            (
                "gemini_request_started",
                json!({
                    "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                    "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "model": p.get("model").cloned().unwrap_or(Value::Null),
                    "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("llm", "legacy_gemini_request_completed") => {
            let p = payload_of(payload, "LegacyGeminiRequestCompleted");
            (
                "gemini_request_completed",
                json!({
                    "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                    "model": p.get("model").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "status": p.get("status").cloned().unwrap_or(Value::Null),
                    "error": p.get("error_msg").cloned().unwrap_or(Value::Null),
                    "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "api_mode": p.get("api_mode").cloned().unwrap_or(Value::Null),
                    "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                    "response_chars": p.get("response_chars").cloned().unwrap_or(Value::Null),
                    "queue_wait_ms": p.get("queue_wait_ms").cloned().unwrap_or(Value::Null),
                    "retry_count": p.get("retry_count").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("llm", "legacy_gemini_tool_activity") => {
            let p = payload_of(payload, "LegacyGeminiToolActivity");
            (
                "gemini_tool_activity",
                json!({
                    "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                    "tool_seq": p.get("tool_seq").cloned().unwrap_or(Value::Null),
                    "activity": p.get("activity").cloned().unwrap_or(Value::Null),
                    "tool_name": p.get("tool_name").cloned().unwrap_or(Value::Null),
                    "input_preview": p.get("input_preview").cloned().unwrap_or(Value::Null),
                    "result_preview": p.get("result_preview").cloned().unwrap_or(Value::Null),
                    "is_error": p.get("is_error").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("llm", "legacy_codex_request_started") => {
            let p = payload_of(payload, "LegacyCodexRequestStarted");
            (
                "codex_request_started",
                json!({
                    "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                    "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                    "model": p.get("model").cloned().unwrap_or(Value::Null),
                    "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                    "has_image": p.get("has_image").cloned().unwrap_or(Value::Null),
                    "image_hash": p.get("image_hash").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("llm", "legacy_codex_request_completed") => {
            let p = payload_of(payload, "LegacyCodexRequestCompleted");
            (
                "codex_request_completed",
                json!({
                    "request_id": p.get("request_id").cloned().unwrap_or(Value::Null),
                    "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                    "model": p.get("model").cloned().unwrap_or(Value::Null),
                    "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                    "response_chars": p.get("response_chars").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "status": p.get("status").cloned().unwrap_or(Value::Null),
                    "error": p.get("error_msg").cloned().unwrap_or(Value::Null),
                    "input_tokens": p.get("input_tokens").cloned().unwrap_or(Value::Null),
                    "output_tokens": p.get("output_tokens").cloned().unwrap_or(Value::Null),
                    "image_hash": p.get("image_hash").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Worker ──
        ("worker", "llm_call") => {
            let p = payload_of(payload, "LlmCall");
            (
                "worker_llm_call",
                json!({
                    "caller": p.get("caller").cloned().unwrap_or(Value::Null),
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                    "status": p.get("status").cloned().unwrap_or(Value::Null),
                    "prompt_chars": p.get("prompt_chars").cloned().unwrap_or(Value::Null),
                    "response_chars": p.get("response_chars").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "queue_wait_ms": p.get("queue_wait_ms").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "translation_started") => {
            let p = payload_of(payload, "TranslationStarted");
            (
                "translation_started",
                json!({
                    "message_id": p.get("message_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "content_chars": p.get("content_chars").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "translation_completed") => {
            let p = payload_of(payload, "TranslationCompleted");
            (
                "translation_completed",
                json!({
                    "message_id": p.get("message_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "preview": p.get("preview").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "translation_failed") => {
            let p = payload_of(payload, "TranslationFailed");
            (
                "translation_failed",
                json!({
                    "message_id": p.get("message_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "error": p.get("error").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "narration_session_started") => {
            let p = payload_of(payload, "NarrationSessionStarted");
            (
                "narration_session_started",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "total_messages": p.get("total_messages").cloned().unwrap_or(Value::Null),
                    "already_narrated": p.get("already_narrated").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "narration_batch_completed") => {
            let p = payload_of(payload, "NarrationBatchCompleted");
            (
                "narration_batch_completed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "batch_index": p.get("batch_index").cloned().unwrap_or(Value::Null),
                    "processed_count": p.get("processed_count").cloned().unwrap_or(Value::Null),
                    "total_messages": p.get("total_messages").cloned().unwrap_or(Value::Null),
                    "duration_ms": p.get("duration_ms").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "narration_session_completed") => {
            let p = payload_of(payload, "NarrationSessionCompleted");
            (
                "narration_session_completed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "total_narrated": p.get("total_narrated").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "narration_failed") => {
            let p = payload_of(payload, "NarrationFailed");
            (
                "narration_failed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "batch_index": p.get("batch_index").cloned().unwrap_or(Value::Null),
                    "error": p.get("error").cloned().unwrap_or(Value::Null),
                    "will_retry": p.get("will_retry").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("worker", "briefing_batch_started") => {
            let p = payload_of(payload, "BriefingBatchStarted");
            (
                "briefing_batch_started",
                json!({ "pending_count": p.get("pending_count").cloned().unwrap_or(Value::Null) }),
            )
        }
        ("worker", "briefing_summary_generated") => {
            let p = payload_of(payload, "BriefingSummaryGenerated");
            (
                "briefing_summary_generated",
                json!({
                    "target_seq": p.get("target_seq").cloned().unwrap_or(Value::Null),
                    "summary": p.get("summary").cloned().unwrap_or(Value::Null),
                    "method": p.get("method").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Memory ──
        ("memory", "phase_changed") => {
            let p = payload_of(payload, "PhaseChanged");
            (
                "memory_phase_changed",
                json!({
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "phase": p.get("phase").cloned().unwrap_or(Value::Null),
                    "active_type": p.get("active_type").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("memory", "deep_analysis_completed") => {
            let p = payload_of(payload, "DeepAnalysisCompleted");
            (
                "deep_analysis_completed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "kb_entries_created": p.get("kb_entries_created").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("memory", "kb_batch_mutated") => {
            let p = payload_of(payload, "KBBatchMutated");
            (
                "kb_batch_mutated",
                json!({
                    "count": p.get("count").cloned().unwrap_or(Value::Null),
                    "categories": p.get("categories").cloned().unwrap_or(Value::Null),
                    "action": p.get("action").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("memory", "turn_extracted") => {
            let p = payload_of(payload, "TurnExtracted");
            (
                "turn_extracted",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "turn_count": p.get("turn_count").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("memory", "intent_analyzed") => {
            let p = payload_of(payload, "IntentAnalyzed");
            (
                "intent_analyzed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "intent_type": p.get("intent_type").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Message ──
        ("message", "logged") => {
            let p = payload_of(payload, "Logged");
            let role = p
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("assistant")
                .to_string();
            let wire = match role.as_str() {
                "user" => "user_message",
                "system" => "system_message",
                "thinking" => "thinking_message",
                _ => "assistant_message",
            };
            (
                wire,
                json!({
                    "message_id": p.get("message_id").cloned().unwrap_or(Value::Null),
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "parent_session_id": p.get("parent_session_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "role": p.get("role").cloned().unwrap_or(Value::Null),
                    "content_chars": p.get("content_chars").cloned().unwrap_or(Value::Null),
                    "preview": p.get("preview").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("message", "image_inserted") => {
            let p = payload_of(payload, "ImageInserted");
            (
                "image_message_inserted",
                json!({
                    "message_id": p.get("message_id").cloned().unwrap_or(Value::Null),
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Session ──
        ("session", "completed") => {
            let p = payload_of(payload, "Completed");
            (
                "session_completed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "message_count": p.get("message_count").cloned().unwrap_or(Value::Null),
                    "duration_secs": p.get("duration_secs").cloned().unwrap_or(Value::Null),
                    "status": p.get("status").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("session", "jarvis_task_completed") => {
            let p = payload_of(payload, "JarvisTaskCompleted");
            (
                "jarvis_task_completed",
                json!({
                    "conversation_id": p.get("conversation_id").cloned().unwrap_or(Value::Null),
                    "task_id": p.get("task_id").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("session", "organized") => {
            let p = payload_of(payload, "Organized");
            (
                "session_organized",
                json!({ "session_id": p.get("session_id").cloned().unwrap_or(Value::Null) }),
            )
        }

        // ── System ──
        ("system", "config_changed") => {
            let p = payload_of(payload, "ConfigChanged");
            (
                "config_file_changed",
                json!({
                    "path": p.get("path").cloned().unwrap_or(Value::Null),
                    "kind": p.get("kind").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("system", "tool_completed") => {
            let p = payload_of(payload, "ToolCompleted");
            (
                "tool_completed",
                json!({
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                    "tool_name": p.get("tool_name").cloned().unwrap_or(Value::Null),
                    "status": p.get("status").cloned().unwrap_or(Value::Null),
                    "is_error": p.get("is_error").cloned().unwrap_or(Value::Null),
                    "input_summary": p.get("input_summary").cloned().unwrap_or(Value::Null),
                    "output_summary": p.get("output_summary").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("system", "insight_generated") => {
            let p = payload_of(payload, "InsightGenerated");
            (
                "insight_generated",
                json!({
                    "category": p.get("category").cloned().unwrap_or(Value::Null),
                    "priority": p.get("priority").cloned().unwrap_or(Value::Null),
                    "title": p.get("title").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("system", "jarvis_proactive_push") => {
            let p = payload_of(payload, "JarvisProactivePush");
            (
                "jarvis_proactive_push",
                json!({
                    "conversation_id": p.get("conversation_id").cloned().unwrap_or(Value::Null),
                    "trigger_reason": p.get("trigger_reason").cloned().unwrap_or(Value::Null),
                    "summary": p.get("summary").cloned().unwrap_or(Value::Null),
                }),
            )
        }
        ("system", "contextual_commit_detected") => {
            let p = payload_of(payload, "ContextualCommitDetected");
            (
                "contextual_commit_detected",
                json!({
                    "commit_hash": p.get("commit_hash").cloned().unwrap_or(Value::Null),
                    "branch": p.get("branch").cloned().unwrap_or(Value::Null),
                    "summary": p.get("summary").cloned().unwrap_or(Value::Null),
                    "conversation_id": p.get("conversation_id").cloned().unwrap_or(Value::Null),
                    "message_id": p.get("message_id").cloned().unwrap_or(Value::Null),
                    "session_id": p.get("session_id").cloned().unwrap_or(Value::Null),
                    "slot_id": p.get("slot_id").cloned().unwrap_or(Value::Null),
                }),
            )
        }

        // ── Observability / Incident (no v1 counterpart; pass-through with
        //     synthetic wire types so the browser can ignore or display). ──
        ("observability", _) => ("observability", payload.clone()),
        ("incident", _) => ("incident", payload.clone()),

        // Fallback — unknown (domain, kind). Preserve enough to debug, but
        // never crash.
        (d, k) => (
            "unknown_event",
            json!({ "domain": d, "kind": k, "payload": payload }),
        ),
    }
}

/// The v2 `payload` JSON is `serde_json` externally-tagged (DC002):
/// `{"VariantName": { … fields … }}`. Unwrap the named variant object.
fn payload_of<'a>(payload: &'a Value, variant: &str) -> &'a Value {
    payload.get(variant).unwrap_or(payload)
}

/// Map v2 `LlmEvent::Provider` enum serialization to the v1
/// `CliEngine::to_string` output.
fn provider_as_engine_string(v: Option<&Value>) -> Value {
    let s = v
        .and_then(|v| v.as_str())
        .unwrap_or("Claude")
        .to_lowercase();
    match s.as_str() {
        "sonnet" => json!("ClaudeCode"),
        "codex" => json!("Codex"),
        "gemini" => json!("Gemini"),
        _ => json!("ClaudeCode"),
    }
}

/// Reverse lookup: v1 `wire_type` → possible `(domain, kind)` pairs.
///
/// Used by the catch-up filter path: when a client queries events by
/// `event_type = "board_task_created"` (v1 wire_type), we need to translate
/// that back into SQL conditions on `event_log.domain` and
/// `event_log.kind`.
///
/// Returns `None` if the `wire_type` is unknown (e.g. `"unknown_event"` or
/// an ad-hoc synthetic type from `observability`/`incident`).
///
/// Note: a few v1 wire_types are **many-to-one** on the (domain, kind) side
/// (e.g. `slot_state_changed` is emitted by `slot::became_idle`,
/// `slot::state_changed`, `slot::stuck`). In that case we return the
/// **domain-only** match (kind=None), which the caller translates into
/// `WHERE domain = $1` — a slightly broader match that still covers every
/// event for that wire_type.
pub fn v1_wire_type_to_v2_parts(wire_type: &str) -> Option<(&'static str, Option<&'static str>)> {
    match wire_type {
        // Slot — 4 kinds collapse into one wire_type; filter by domain.
        "slot_state_changed" => Some(("slot", None)),
        "slot_task_dispatched" => Some(("slot", Some("task_dispatched"))),

        // Board — each kind has its own wire_type.
        "board_task_created" => Some(("board", Some("task_created"))),
        "board_task_status_changed" => Some(("board", Some("status_changed"))),
        "board_task_note_added" => Some(("board", Some("note_added"))),
        "board_task_claimed" => Some(("board", Some("claimed"))),
        "board_task_deleted" => Some(("board", Some("deleted"))),
        "board_task_updated" => Some(("board", Some("updated"))),

        // Task — created + completed share `task_lifecycle` wire_type; domain
        // match is sufficient and omits the unrelated `cascade_*` kinds.
        "task_lifecycle" => Some(("task", None)),
        "cascade_triggered" => Some(("task", Some("cascade_triggered"))),
        "cascade_completed" => Some(("task", Some("cascade_completed"))),

        // Question
        "question_created" => Some(("question", Some("created"))),
        "question_resolved" => Some(("question", Some("resolved"))),
        "decision_made" => Some(("question", Some("decision_resolved"))),

        // Llm unified
        "cli_request_started" => Some(("llm", Some("request_started"))),
        "cli_request_completed" => Some(("llm", Some("request_completed"))),
        "cli_tool_activity" => Some(("llm", Some("tool_activity"))),

        // Llm legacy engine-specific
        "gemini_request_started" => Some(("llm", Some("legacy_gemini_request_started"))),
        "gemini_request_completed" => Some(("llm", Some("legacy_gemini_request_completed"))),
        "gemini_tool_activity" => Some(("llm", Some("legacy_gemini_tool_activity"))),
        "codex_request_started" => Some(("llm", Some("legacy_codex_request_started"))),
        "codex_request_completed" => Some(("llm", Some("legacy_codex_request_completed"))),

        // Worker
        "worker_llm_call" => Some(("worker", Some("llm_call"))),
        "translation_started" => Some(("worker", Some("translation_started"))),
        "translation_completed" => Some(("worker", Some("translation_completed"))),
        "translation_failed" => Some(("worker", Some("translation_failed"))),
        "narration_session_started" => Some(("worker", Some("narration_session_started"))),
        "narration_batch_completed" => Some(("worker", Some("narration_batch_completed"))),
        "narration_session_completed" => Some(("worker", Some("narration_session_completed"))),
        "narration_failed" => Some(("worker", Some("narration_failed"))),
        "briefing_batch_started" => Some(("worker", Some("briefing_batch_started"))),
        "briefing_summary_generated" => Some(("worker", Some("briefing_summary_generated"))),

        // Memory
        "memory_phase_changed" => Some(("memory", Some("phase_changed"))),
        "deep_analysis_completed" => Some(("memory", Some("deep_analysis_completed"))),
        "kb_batch_mutated" => Some(("memory", Some("kb_batch_mutated"))),
        "turn_extracted" => Some(("memory", Some("turn_extracted"))),
        "intent_analyzed" => Some(("memory", Some("intent_analyzed"))),

        // Message — {user,assistant,system,thinking}_message all map back to
        // `message::logged` with a role discriminator. Domain-only match is
        // the broadest safe choice; role filtering requires a separate arg.
        "user_message" | "assistant_message" | "system_message" | "thinking_message" => {
            Some(("message", Some("logged")))
        }
        "image_message_inserted" => Some(("message", Some("image_inserted"))),

        // Session
        "session_completed" => Some(("session", Some("completed"))),
        "jarvis_task_completed" => Some(("session", Some("jarvis_task_completed"))),
        "session_organized" => Some(("session", Some("organized"))),

        // System
        "config_file_changed" => Some(("system", Some("config_changed"))),
        "tool_completed" => Some(("system", Some("tool_completed"))),
        "insight_generated" => Some(("system", Some("insight_generated"))),
        "jarvis_proactive_push" => Some(("system", Some("jarvis_proactive_push"))),
        "contextual_commit_detected" => Some(("system", Some("contextual_commit_detected"))),

        // Synthetic / pass-through types — domain match only.
        "observability" => Some(("observability", None)),
        "incident" => Some(("incident", None)),

        _ => None,
    }
}

/// Synthetic `ts` constructor used by tests — matches v1 which used
/// `chrono::Utc::now().timestamp_millis()`.
#[allow(dead_code)]
pub fn fresh_ts_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::domain::Domain;
    use crate::event::log::Seq;
    use chrono::{TimeZone, Utc};

    fn make_logged(domain: Domain, kind: &str, payload: Value) -> LoggedEvent {
        LoggedEvent {
            seq: Seq(42),
            domain,
            kind: kind.into(),
            payload,
            producer_id: "test".into(),
            dedupe_key: None,
            causation_depth: 0,
            trace_id: None,
            span_id: Some(uuid::Uuid::nil()),
            parent_span_id: None,
            ts: Utc.with_ymd_and_hms(2026, 4, 19, 0, 0, 0).unwrap(),
            ephemeral: false,
        }
    }

    /// Cross-check: v1 `TimelineEvent::to_frontend_json` vs v2
    /// `v2_logged_to_v1_wire_format`. We construct the v1 and v2 sides
    /// manually and compare each top-level key.
    fn assert_equiv(v1_wire_type: &str, v1_payload: Value, v2_logged: &LoggedEvent) {
        let v2_json_str = v2_logged_to_v1_wire_format(v2_logged);
        let v2_json: Value = serde_json::from_str(&v2_json_str).expect("v2 json parses");

        assert_eq!(
            v2_json.get("type").and_then(|v| v.as_str()),
            Some(v1_wire_type),
            "wire type mismatch"
        );
        assert_eq!(
            v2_json.get("seq").and_then(|v| v.as_i64()),
            Some(v2_logged.seq.0),
            "seq mismatch"
        );
        assert_eq!(
            v2_json.get("payload"),
            Some(&v1_payload),
            "payload mismatch:\n  v1={}\n  v2={}",
            v1_payload,
            v2_json.get("payload").cloned().unwrap_or(Value::Null)
        );
    }

    #[test]
    fn slot_became_idle_equiv() {
        let logged = make_logged(
            Domain::Slot,
            "became_idle",
            json!({ "BecameIdle": { "slot_id": "slot-a" } }),
        );
        assert_equiv(
            "slot_state_changed",
            json!({ "slot_id": "slot-a", "new_state": "Idle" }),
            &logged,
        );
    }

    #[test]
    fn board_task_created_equiv() {
        let logged = make_logged(
            Domain::Board,
            "task_created",
            json!({
                "TaskCreated": {
                    "task_id": "t-1",
                    "title": "hello",
                    "category": "code"
                }
            }),
        );
        assert_equiv(
            "board_task_created",
            json!({
                "task_id": "t-1",
                "title": "hello",
                "category": "code",
                "action": "created"
            }),
            &logged,
        );
    }

    #[test]
    fn board_task_status_changed_equiv() {
        let logged = make_logged(
            Domain::Board,
            "status_changed",
            json!({
                "StatusChanged": {
                    "task_id": "t-1",
                    "old_status": "queued",
                    "new_status": "running"
                }
            }),
        );
        assert_equiv(
            "board_task_status_changed",
            json!({
                "task_id": "t-1",
                "old_status": "queued",
                "new_status": "running",
                "action": "status_changed"
            }),
            &logged,
        );
    }

    #[test]
    fn task_created_equiv() {
        let logged = make_logged(
            Domain::Task,
            "created",
            json!({ "Created": { "task_id": "t-42" } }),
        );
        assert_equiv(
            "task_lifecycle",
            json!({ "task_id": "t-42", "action": "created" }),
            &logged,
        );
    }

    #[test]
    fn question_created_equiv() {
        let logged = make_logged(
            Domain::Question,
            "created",
            json!({ "Created": { "question_id": "q-9" } }),
        );
        assert_equiv("question_created", json!({ "question_id": "q-9" }), &logged);
    }

    #[test]
    fn llm_request_started_equiv() {
        let logged = make_logged(
            Domain::Llm,
            "request_started",
            json!({
                "RequestStarted": {
                    "provider": "Gemini",
                    "request_id": "r-1",
                    "caller": "worker",
                    "session_id": "s-1",
                    "model": "gemini-2",
                    "prompt_chars": 42,
                    "prompt_text": null,
                    "extra": {}
                }
            }),
        );
        let s = v2_logged_to_v1_wire_format(&logged);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("type").unwrap(), "cli_request_started");
        let p = v.get("payload").unwrap();
        assert_eq!(p.get("engine").unwrap(), "Gemini");
        assert_eq!(p.get("model").unwrap(), "gemini-2");
    }

    #[test]
    fn worker_llm_call_equiv() {
        let logged = make_logged(
            Domain::Worker,
            "llm_call",
            json!({
                "LlmCall": {
                    "caller": "briefing",
                    "task_id": "t-1",
                    "status": "success",
                    "prompt_chars": 100,
                    "response_chars": 50,
                    "duration_ms": 250,
                    "queue_wait_ms": 10
                }
            }),
        );
        assert_equiv(
            "worker_llm_call",
            json!({
                "caller": "briefing",
                "task_id": "t-1",
                "status": "success",
                "prompt_chars": 100,
                "response_chars": 50,
                "duration_ms": 250,
                "queue_wait_ms": 10
            }),
            &logged,
        );
    }

    #[test]
    fn memory_phase_changed_equiv() {
        let logged = make_logged(
            Domain::Memory,
            "phase_changed",
            json!({
                "PhaseChanged": {
                    "slot_id": "slot-memory",
                    "phase": "Busy",
                    "active_type": "realtime"
                }
            }),
        );
        assert_equiv(
            "memory_phase_changed",
            json!({
                "slot_id": "slot-memory",
                "phase": "Busy",
                "active_type": "realtime"
            }),
            &logged,
        );
    }

    #[test]
    fn message_logged_user_equiv() {
        let logged = make_logged(
            Domain::Message,
            "logged",
            json!({
                "Logged": {
                    "message_id": 7,
                    "session_id": "s-1",
                    "parent_session_id": null,
                    "slot_id": null,
                    "role": "user",
                    "content_chars": 12,
                    "preview": "hi"
                }
            }),
        );
        assert_equiv(
            "user_message",
            json!({
                "message_id": 7,
                "session_id": "s-1",
                "parent_session_id": null,
                "slot_id": null,
                "role": "user",
                "content_chars": 12,
                "preview": "hi"
            }),
            &logged,
        );
    }

    #[test]
    fn session_completed_equiv() {
        let logged = make_logged(
            Domain::Session,
            "completed",
            json!({
                "Completed": {
                    "session_id": "s-1",
                    "slot_id": "slot-a",
                    "message_count": 10,
                    "duration_secs": 60,
                    "status": "Success"
                }
            }),
        );
        assert_equiv(
            "session_completed",
            json!({
                "session_id": "s-1",
                "slot_id": "slot-a",
                "message_count": 10,
                "duration_secs": 60,
                "status": "Success"
            }),
            &logged,
        );
    }

    #[test]
    fn system_config_changed_equiv() {
        let logged = make_logged(
            Domain::System,
            "config_changed",
            json!({
                "ConfigChanged": {
                    "path": "/x/slots.yaml",
                    "kind": "modified"
                }
            }),
        );
        assert_equiv(
            "config_file_changed",
            json!({
                "path": "/x/slots.yaml",
                "kind": "modified"
            }),
            &logged,
        );
    }

    #[test]
    fn ephemeral_yields_seq_zero() {
        let mut logged = make_logged(
            Domain::Worker,
            "briefing_batch_started",
            json!({ "BriefingBatchStarted": { "pending_count": 3 } }),
        );
        logged.ephemeral = true;
        let s = v2_logged_to_v1_wire_format(&logged);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("seq").unwrap().as_i64(), Some(0));
    }

    // ─── catch-up path test (new, ensures SSOT) ──────────────────────────
    // Projection-layer integration tests live in
    // `crate::event::projection::tests` so they exercise the full chain.
}
