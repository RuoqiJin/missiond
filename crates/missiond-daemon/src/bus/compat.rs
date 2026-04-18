//! v1 → v2 compat shim.
//!
//! Phase 6 keeps v1 `DaemonEvent` publishing alive (Phase 8 will delete the
//! enum entirely). Producer sites "dual-emit": the existing `event_bus.publish`
//! call stays, and a single new line appends the matching v2 `DomainEvent` to
//! the log:
//!
//! ```ignore
//! event_bus.publish(DaemonEvent::BoardTaskCreated { .. });
//! let _ = compat::publish_v1_shim(&state.bus, &DaemonEvent::BoardTaskCreated { .. }).await;
//! ```
//!
//! To avoid cloning giant payloads twice, producer sites build the v1 event,
//! publish it, then hand `&event` to this shim. The shim maps the variant
//! into the appropriate v2 domain enum and appends with
//! `AppendOpts { ephemeral: <v1.is_ephemeral()>, producer_id: ... }`.
//!
//! I007 note (Phase 6 does NOT re-audit ephemeral semantics): we keep the
//! v1 `is_ephemeral()` contract so behavior is bit-for-bit identical on the
//! new log. Phase 7 or later can flip individual variants.
//!
//! Some v1 variants have no direct v2 mapping today — the shim returns
//! `Ok(None)` in that case. That's deliberate: the v1 publish has already
//! fanned out to subscribers, so a missing v2 path just means there's
//! nothing to replay on the new log (yet).

use missiond_core::event::{
    events::{
        BoardEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent, Provider, QuestionEvent,
        SessionEndStatus as V2EndStatus, SessionEvent, SlotEvent, SystemEvent, TaskEvent,
        WorkerEvent,
    },
    log::{AppendAck, AppendError, AppendOpts},
    DomainEvent,
};
use missiond_core::CliEngine;

use crate::bus::bootstrap::BusServices;
use crate::event_bus::{DaemonEvent, SessionEndStatus as V1EndStatus};

/// Append the v2 mirror of `ev` to the log. Returns `Ok(None)` when the
/// variant does not yet have a v2 counterpart (forward-compatible no-op).
///
/// Producer sites typically call this without caring about the return value:
///
/// ```ignore
/// let _ = compat::publish_v1_shim(&state.bus, &ev).await;
/// ```
pub async fn publish_v1_shim(
    bus: &BusServices,
    ev: &DaemonEvent,
) -> Result<Option<AppendAck>, AppendError> {
    let producer = producer_id_for(ev);
    let ephemeral = ev.is_ephemeral();
    match v2_from_v1(ev) {
        V2Emission::Slot(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Board(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Task(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Question(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Llm(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Worker(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Memory(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Message(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Session(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::System(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::Incident(e) => {
            bus.publish(e, opts(producer, ephemeral)).await.map(Some)
        }
        V2Emission::None => Ok(None),
    }
}

fn opts(producer_id: String, ephemeral: bool) -> AppendOpts {
    AppendOpts {
        producer_id,
        ephemeral,
        ..Default::default()
    }
}

/// Stable-ish producer id for metrics / audit.
fn producer_id_for(ev: &DaemonEvent) -> String {
    // We don't have call-site info here; keep the wire_type so ops can at
    // least filter by event family.
    format!("v1_shim/{}", ev.wire_type())
}

/// Internal sum type used to erase the concrete domain enum. Each arm
/// already implements [`DomainEvent`] so the caller can just forward.
#[allow(clippy::large_enum_variant)]
enum V2Emission {
    Slot(SlotEvent),
    Board(BoardEvent),
    Task(TaskEvent),
    Question(QuestionEvent),
    Llm(LlmEvent),
    Worker(WorkerEvent),
    Memory(MemoryEvent),
    Message(MessageEvent),
    Session(SessionEvent),
    System(SystemEvent),
    Incident(IncidentEvent),
    /// No v2 mapping yet (expected to be empty once Phase 8 cleans v1 up).
    None,
}

fn end_status(v: &V1EndStatus) -> V2EndStatus {
    match v {
        V1EndStatus::Success => V2EndStatus::Success,
        V1EndStatus::Aborted => V2EndStatus::Aborted,
        V1EndStatus::Error => V2EndStatus::Error,
    }
}

fn provider_from_engine(engine: &CliEngine) -> Provider {
    match engine {
        CliEngine::ClaudeCode => Provider::Claude,
        CliEngine::Gemini => Provider::Gemini,
        CliEngine::Codex => Provider::Codex,
    }
}

/// The big match. One arm per v1 variant. Keep the produced v2 variants
/// bit-compatible with the field names on the core side so future wire
/// format work doesn't need to touch this file.
fn v2_from_v1(ev: &DaemonEvent) -> V2Emission {
    match ev {
        // ── Slot ──
        DaemonEvent::SlotBecameIdle { slot_id } => V2Emission::Slot(SlotEvent::BecameIdle {
            slot_id: slot_id.clone(),
        }),
        DaemonEvent::SlotStateChanged {
            slot_id,
            new_state,
            prev_state,
        } => V2Emission::Slot(SlotEvent::StateChanged {
            slot_id: slot_id.clone(),
            new_state: new_state.clone(),
            prev_state: prev_state.clone(),
        }),
        DaemonEvent::SlotTaskDispatched {
            slot_id,
            task_id,
            purpose,
            prompt_chars,
            preview,
            cited_kb_ids,
        } => V2Emission::Slot(SlotEvent::TaskDispatched {
            slot_id: slot_id.clone(),
            task_id: task_id.clone(),
            purpose: purpose.clone(),
            prompt_chars: *prompt_chars,
            preview: preview.clone(),
            cited_kb_ids: cited_kb_ids.clone(),
        }),

        // ── Board ──
        DaemonEvent::BoardTaskCreated {
            task_id,
            title,
            category,
        } => V2Emission::Board(BoardEvent::TaskCreated {
            task_id: task_id.clone(),
            title: title.clone(),
            category: category.clone(),
        }),
        DaemonEvent::BoardTaskStatusChanged {
            task_id,
            old_status,
            new_status,
        } => V2Emission::Board(BoardEvent::StatusChanged {
            task_id: task_id.clone(),
            old_status: old_status.clone(),
            new_status: new_status.clone(),
        }),
        DaemonEvent::BoardTaskNoteAdded {
            task_id,
            note_id,
            content_preview,
        } => V2Emission::Board(BoardEvent::NoteAdded {
            task_id: task_id.clone(),
            note_id: note_id.clone(),
            content_preview: content_preview.clone(),
        }),
        DaemonEvent::BoardTaskClaimed { task_id, slot_id } => {
            V2Emission::Board(BoardEvent::Claimed {
                task_id: task_id.clone(),
                slot_id: slot_id.clone(),
            })
        }
        DaemonEvent::BoardTaskDeleted { task_id, title } => {
            V2Emission::Board(BoardEvent::Deleted {
                task_id: task_id.clone(),
                title: title.clone(),
            })
        }
        DaemonEvent::BoardTaskUpdated {
            task_id,
            status,
            category,
        } => V2Emission::Board(BoardEvent::Updated {
            task_id: task_id.clone(),
            status: status.clone(),
            category: category.clone(),
        }),

        // ── Task ──
        DaemonEvent::TaskCreated { task_id } => V2Emission::Task(TaskEvent::Created {
            task_id: task_id.clone(),
        }),
        DaemonEvent::TaskCompleted { task_id } => V2Emission::Task(TaskEvent::Completed {
            task_id: task_id.clone(),
        }),
        DaemonEvent::CascadeTriggered { service, changed } => {
            V2Emission::Task(TaskEvent::CascadeTriggered {
                service: service.clone(),
                changed: changed.clone(),
            })
        }
        DaemonEvent::CascadeCompleted {
            service,
            services_repaired,
            services_failed,
            hard_halted,
            duration_ms,
        } => V2Emission::Task(TaskEvent::CascadeCompleted {
            service: service.clone(),
            services_repaired: *services_repaired,
            services_failed: *services_failed,
            hard_halted: *hard_halted,
            duration_ms: *duration_ms,
        }),

        // ── Question ──
        DaemonEvent::QuestionCreated { question_id } => {
            V2Emission::Question(QuestionEvent::Created {
                question_id: question_id.clone(),
            })
        }
        DaemonEvent::QuestionResolved {
            question_id,
            resolution,
        } => V2Emission::Question(QuestionEvent::Resolved {
            question_id: question_id.clone(),
            resolution: resolution.clone(),
        }),
        DaemonEvent::DecisionResolved {
            question_id,
            tier,
            duration_ms,
        } => V2Emission::Question(QuestionEvent::DecisionResolved {
            question_id: question_id.clone(),
            tier: tier.clone(),
            duration_ms: *duration_ms,
        }),

        // ── LLM: unified (preferred shape) ──
        DaemonEvent::CliRequestStarted {
            engine,
            request_id,
            caller,
            session_id,
            model,
            prompt_chars,
            prompt_text,
            extra,
        } => V2Emission::Llm(LlmEvent::RequestStarted {
            provider: provider_from_engine(engine),
            request_id: request_id.clone(),
            caller: caller.clone(),
            session_id: session_id.clone(),
            model: model.clone(),
            prompt_chars: *prompt_chars,
            prompt_text: prompt_text.clone(),
            extra: extra.clone(),
        }),
        DaemonEvent::CliRequestCompleted {
            engine,
            request_id,
            caller,
            session_id,
            model,
            prompt_chars,
            response_chars,
            duration_ms,
            status,
            error_msg,
            response_text,
            extra,
        } => V2Emission::Llm(LlmEvent::RequestCompleted {
            provider: provider_from_engine(engine),
            request_id: request_id.clone(),
            caller: caller.clone(),
            session_id: session_id.clone(),
            model: model.clone(),
            prompt_chars: *prompt_chars,
            response_chars: *response_chars,
            duration_ms: *duration_ms,
            status: status.clone(),
            error_msg: error_msg.clone(),
            response_text: response_text.clone(),
            extra: extra.clone(),
        }),
        DaemonEvent::CliToolActivity {
            engine,
            request_id,
            tool_seq,
            activity,
            tool_name,
            input_preview,
            result_preview,
            is_error,
        } => V2Emission::Llm(LlmEvent::ToolActivity {
            provider: provider_from_engine(engine),
            request_id: request_id.clone(),
            tool_seq: *tool_seq,
            activity: activity.clone(),
            tool_name: tool_name.clone(),
            input_preview: input_preview.clone(),
            result_preview: result_preview.clone(),
            is_error: *is_error,
        }),

        // ── LLM: legacy variants (kept verbatim per DC004) ──
        DaemonEvent::GeminiRequestStarted {
            request_id,
            caller,
            session_id,
            model,
            prompt_chars,
            prompt_text,
        } => V2Emission::Llm(LlmEvent::LegacyGeminiRequestStarted {
            request_id: request_id.clone(),
            caller: caller.clone(),
            session_id: session_id.clone(),
            model: model.clone(),
            prompt_chars: *prompt_chars,
            prompt_text: prompt_text.clone(),
        }),
        DaemonEvent::GeminiRequestCompleted {
            request_id,
            caller,
            session_id,
            api_mode,
            model,
            prompt_chars,
            response_chars,
            queue_wait_ms,
            duration_ms,
            retry_count,
            status,
            error_msg,
            response_text,
        } => V2Emission::Llm(LlmEvent::LegacyGeminiRequestCompleted {
            request_id: request_id.clone(),
            caller: caller.clone(),
            session_id: session_id.clone(),
            api_mode: api_mode.clone(),
            model: model.clone(),
            prompt_chars: *prompt_chars,
            response_chars: *response_chars,
            queue_wait_ms: *queue_wait_ms,
            duration_ms: *duration_ms,
            retry_count: *retry_count,
            status: status.clone(),
            error_msg: error_msg.clone(),
            response_text: response_text.clone(),
        }),
        DaemonEvent::GeminiToolActivity {
            request_id,
            tool_seq,
            activity,
            tool_name,
            input_preview,
            result_preview,
            is_error,
        } => V2Emission::Llm(LlmEvent::LegacyGeminiToolActivity {
            request_id: request_id.clone(),
            tool_seq: *tool_seq,
            activity: activity.clone(),
            tool_name: tool_name.clone(),
            input_preview: input_preview.clone(),
            result_preview: result_preview.clone(),
            is_error: *is_error,
        }),
        DaemonEvent::CodexRequestStarted {
            request_id,
            caller,
            model,
            prompt_chars,
            has_image,
            prompt_text,
            image_hash,
        } => V2Emission::Llm(LlmEvent::LegacyCodexRequestStarted {
            request_id: request_id.clone(),
            caller: caller.clone(),
            model: model.clone(),
            prompt_chars: *prompt_chars,
            has_image: *has_image,
            prompt_text: prompt_text.clone(),
            image_hash: image_hash.clone(),
        }),
        DaemonEvent::CodexRequestCompleted {
            request_id,
            caller,
            model,
            prompt_chars,
            response_chars,
            duration_ms,
            status,
            error_msg,
            response_text,
            input_tokens,
            output_tokens,
            image_hash,
        } => V2Emission::Llm(LlmEvent::LegacyCodexRequestCompleted {
            request_id: request_id.clone(),
            caller: caller.clone(),
            model: model.clone(),
            prompt_chars: *prompt_chars,
            response_chars: *response_chars,
            duration_ms: *duration_ms,
            status: status.clone(),
            error_msg: error_msg.clone(),
            response_text: response_text.clone(),
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            image_hash: image_hash.clone(),
        }),

        // ── Worker ──
        DaemonEvent::WorkerLlmCall {
            caller,
            task_id,
            status,
            prompt_chars,
            response_chars,
            duration_ms,
            queue_wait_ms,
        } => V2Emission::Worker(WorkerEvent::LlmCall {
            caller: caller.clone(),
            task_id: task_id.clone(),
            status: status.clone(),
            prompt_chars: *prompt_chars,
            response_chars: *response_chars,
            duration_ms: *duration_ms,
            queue_wait_ms: *queue_wait_ms,
        }),
        DaemonEvent::TranslationStarted {
            message_id,
            slot_id,
            content_chars,
        } => V2Emission::Worker(WorkerEvent::TranslationStarted {
            message_id: *message_id,
            slot_id: slot_id.clone(),
            content_chars: *content_chars,
        }),
        DaemonEvent::TranslationCompleted {
            message_id,
            slot_id,
            preview,
            duration_ms,
        } => V2Emission::Worker(WorkerEvent::TranslationCompleted {
            message_id: *message_id,
            slot_id: slot_id.clone(),
            preview: preview.clone(),
            duration_ms: *duration_ms,
        }),
        DaemonEvent::TranslationFailed {
            message_id,
            slot_id,
            error,
        } => V2Emission::Worker(WorkerEvent::TranslationFailed {
            message_id: *message_id,
            slot_id: slot_id.clone(),
            error: error.clone(),
        }),
        DaemonEvent::NarrationSessionStarted {
            session_id,
            total_messages,
            already_narrated,
        } => V2Emission::Worker(WorkerEvent::NarrationSessionStarted {
            session_id: session_id.clone(),
            total_messages: *total_messages,
            already_narrated: *already_narrated,
        }),
        DaemonEvent::NarrationBatchCompleted {
            session_id,
            batch_index,
            processed_count,
            total_messages,
            duration_ms,
        } => V2Emission::Worker(WorkerEvent::NarrationBatchCompleted {
            session_id: session_id.clone(),
            batch_index: *batch_index,
            processed_count: *processed_count,
            total_messages: *total_messages,
            duration_ms: *duration_ms,
        }),
        DaemonEvent::NarrationSessionCompleted {
            session_id,
            total_narrated,
        } => V2Emission::Worker(WorkerEvent::NarrationSessionCompleted {
            session_id: session_id.clone(),
            total_narrated: *total_narrated,
        }),
        DaemonEvent::NarrationFailed {
            session_id,
            batch_index,
            error,
            will_retry,
        } => V2Emission::Worker(WorkerEvent::NarrationFailed {
            session_id: session_id.clone(),
            batch_index: *batch_index,
            error: error.clone(),
            will_retry: *will_retry,
        }),
        DaemonEvent::BriefingBatchStarted { pending_count } => {
            V2Emission::Worker(WorkerEvent::BriefingBatchStarted {
                pending_count: *pending_count,
            })
        }
        DaemonEvent::BriefingSummaryGenerated {
            target_seq,
            summary,
            method,
        } => V2Emission::Worker(WorkerEvent::BriefingSummaryGenerated {
            target_seq: *target_seq,
            summary: summary.clone(),
            method: method.clone(),
        }),

        // ── Memory ──
        DaemonEvent::MemoryPhaseChanged {
            slot_id,
            phase,
            active_type,
        } => V2Emission::Memory(MemoryEvent::PhaseChanged {
            slot_id: slot_id.clone(),
            phase: phase.clone(),
            active_type: active_type.clone(),
        }),
        DaemonEvent::DeepAnalysisCompleted {
            session_id,
            kb_entries_created,
        } => V2Emission::Memory(MemoryEvent::DeepAnalysisCompleted {
            session_id: session_id.clone(),
            kb_entries_created: *kb_entries_created,
        }),
        DaemonEvent::KBBatchMutated {
            count,
            categories,
            action,
        } => V2Emission::Memory(MemoryEvent::KBBatchMutated {
            count: *count,
            categories: categories.clone(),
            action: action.clone(),
        }),
        DaemonEvent::TurnExtracted {
            session_id,
            turn_count,
        } => V2Emission::Memory(MemoryEvent::TurnExtracted {
            session_id: session_id.clone(),
            turn_count: *turn_count,
        }),
        DaemonEvent::IntentAnalyzed {
            session_id,
            intent_type,
        } => V2Emission::Memory(MemoryEvent::IntentAnalyzed {
            session_id: session_id.clone(),
            intent_type: intent_type.clone(),
        }),

        // ── Message ──
        DaemonEvent::ConversationMessageLogged {
            message_id,
            session_id,
            parent_session_id,
            slot_id,
            role,
            content_chars,
            preview,
        } => V2Emission::Message(MessageEvent::Logged {
            message_id: *message_id,
            session_id: session_id.clone(),
            parent_session_id: parent_session_id.clone(),
            slot_id: slot_id.clone(),
            role: role.clone(),
            content_chars: *content_chars,
            preview: preview.clone(),
        }),
        DaemonEvent::ImageMessageInserted {
            message_id,
            session_id,
        } => V2Emission::Message(MessageEvent::ImageInserted {
            message_id: *message_id,
            session_id: session_id.clone(),
        }),

        // ── Session ──
        DaemonEvent::SessionCompleted {
            session_id,
            slot_id,
            message_count,
            duration_secs,
            status,
        } => V2Emission::Session(SessionEvent::Completed {
            session_id: session_id.clone(),
            slot_id: slot_id.clone(),
            message_count: *message_count,
            duration_secs: *duration_secs,
            status: end_status(status),
        }),
        DaemonEvent::JarvisTaskCompleted {
            conversation_id,
            task_id,
        } => V2Emission::Session(SessionEvent::JarvisTaskCompleted {
            conversation_id: conversation_id.clone(),
            task_id: task_id.clone(),
        }),
        DaemonEvent::SessionOrganized { session_id } => {
            V2Emission::Session(SessionEvent::Organized {
                session_id: session_id.clone(),
            })
        }

        // ── System ──
        DaemonEvent::ConfigFileChanged { path, kind } => {
            V2Emission::System(SystemEvent::ConfigChanged {
                path: path.clone(),
                kind: kind.clone(),
            })
        }
        DaemonEvent::ToolCompleted {
            session_id,
            slot_id,
            tool_name,
            status,
            is_error,
            input_summary,
            output_summary,
        } => V2Emission::System(SystemEvent::ToolCompleted {
            session_id: session_id.clone(),
            slot_id: slot_id.clone(),
            tool_name: tool_name.clone(),
            status: status.clone(),
            is_error: *is_error,
            input_summary: input_summary.clone(),
            output_summary: output_summary.clone(),
        }),
        DaemonEvent::InsightGenerated {
            category,
            priority,
            title,
        } => V2Emission::System(SystemEvent::InsightGenerated {
            category: category.clone(),
            priority: priority.clone(),
            title: title.clone(),
        }),
        DaemonEvent::JarvisProactivePush {
            conversation_id,
            trigger_reason,
            summary,
        } => V2Emission::System(SystemEvent::JarvisProactivePush {
            conversation_id: conversation_id.clone(),
            trigger_reason: trigger_reason.clone(),
            summary: summary.clone(),
        }),
        DaemonEvent::ContextualCommitDetected {
            commit_hash,
            branch,
            summary,
            conversation_id,
            message_id,
            session_id,
            slot_id,
        } => V2Emission::System(SystemEvent::ContextualCommitDetected {
            commit_hash: commit_hash.clone(),
            branch: branch.clone(),
            summary: summary.clone(),
            conversation_id: conversation_id.clone(),
            message_id: *message_id,
            session_id: session_id.clone(),
            slot_id: slot_id.clone(),
        }),

        // ── Incident (v1 has no direct variant; the MPSC bypass gets its
        //    own shim in each producer site during Phase 6) ──
    }
}

/// Convenience: build an `IncidentEvent::Reported` from a v1
/// `MissionIncident` (since there's no v1 `DaemonEvent` variant for it).
/// Producer sites can call this directly when they previously pushed to
/// `state.incident_tx`.
pub fn incident_reported(
    incident: missiond_core::types::MissionIncident,
) -> IncidentEvent {
    IncidentEvent::Reported { incident }
}

/// Spawn a detached task that mirrors `ev` onto the v2 bus. Used by
/// synchronous producers (notably LLM clients that construct `TimelineEntry`
/// directly and send via a raw MPSC) so they don't need to be made async
/// just to satisfy `publish_v1_shim`.
pub fn spawn_v1_shim(
    bus: std::sync::Arc<crate::bus::BusServices>,
    ev: DaemonEvent,
) {
    tokio::spawn(async move {
        let _ = publish_v1_shim(&bus, &ev).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::DaemonEvent as V1;
    use missiond_core::types::IncidentSeverity;
    use missiond_core::types::IncidentSource;

    fn sample_incident() -> missiond_core::types::MissionIncident {
        missiond_core::types::MissionIncident {
            id: "inc-1".into(),
            severity: IncidentSeverity::Warning,
            source: IncidentSource::HealthCheck,
            title: "test".into(),
            description: "d".into(),
            server_id: None,
            raw_payload: serde_json::Value::Null,
            created_at: "2026-04-19T00:00:00Z".into(),
        }
    }

    #[test]
    fn slot_became_idle_maps_to_slot_event() {
        let v1 = V1::SlotBecameIdle {
            slot_id: "slot-a".into(),
        };
        match v2_from_v1(&v1) {
            V2Emission::Slot(SlotEvent::BecameIdle { slot_id }) => {
                assert_eq!(slot_id, "slot-a");
            }
            _ => panic!("expected SlotEvent::BecameIdle"),
        }
    }

    #[test]
    fn cli_request_started_maps_to_llm_with_provider() {
        let v1 = V1::CliRequestStarted {
            engine: CliEngine::Gemini,
            request_id: "r-1".into(),
            caller: "c".into(),
            session_id: Some("s".into()),
            model: "m".into(),
            prompt_chars: 42,
            prompt_text: None,
            extra: serde_json::Value::Null,
        };
        match v2_from_v1(&v1) {
            V2Emission::Llm(LlmEvent::RequestStarted { provider, .. }) => {
                assert_eq!(provider, Provider::Gemini);
            }
            _ => panic!("expected LlmEvent::RequestStarted"),
        }
    }

    #[test]
    fn gemini_legacy_request_preserved() {
        let v1 = V1::GeminiRequestStarted {
            request_id: "r".into(),
            caller: "c".into(),
            session_id: None,
            model: "gemini-2".into(),
            prompt_chars: 10,
            prompt_text: None,
        };
        match v2_from_v1(&v1) {
            V2Emission::Llm(LlmEvent::LegacyGeminiRequestStarted { .. }) => {}
            _ => panic!("expected legacy gemini request"),
        }
    }

    #[test]
    fn incident_reported_helper_builds_event() {
        let ev = incident_reported(sample_incident());
        match ev {
            IncidentEvent::Reported { .. } => {}
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn end_status_round_trips_all_variants() {
        assert!(matches!(
            end_status(&V1EndStatus::Success),
            V2EndStatus::Success
        ));
        assert!(matches!(
            end_status(&V1EndStatus::Aborted),
            V2EndStatus::Aborted
        ));
        assert!(matches!(
            end_status(&V1EndStatus::Error),
            V2EndStatus::Error
        ));
    }
}
