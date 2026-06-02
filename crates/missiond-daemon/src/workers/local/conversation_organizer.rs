//! Conversation Organizer Worker — Stage 2 of the Cognitive Pipeline.
//!
//! Post-ingestion processor that repairs structural relationships:
//! - P0: Compaction fragment linking (agent-acompact-* → original session)
//! - P1: Orphan parent link repair (subagent → parent via jsonl_path)
//! - Emits SessionOrganized event to trigger downstream S3 Chunker.
//!
//! Design doc: `docs/designs/conversation-organizer.md`

use std::collections::HashSet;
use std::sync::Arc;

use tracing::{debug, info, warn};

use super::{BackgroundWorker, WorkerContext, WorkerKind};
use crate::state::AppState;
use missiond_core::event::events::{MessageEvent, SessionEvent};
use missiond_core::event::subscription::SubscriptionOpts;

/// Debounce interval: wait for 5s of silence before processing batch.
const DEBOUNCE_SECS: u64 = 5;

pub struct ConversationOrganizerWorker;

impl BackgroundWorker for ConversationOrganizerWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "conversation_organizer"
    }

    fn run(
        self,
        state: Arc<AppState>,
        ctx: WorkerContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        run_organizer(state, ctx)
    }
}

async fn run_organizer(state: Arc<AppState>, mut ctx: WorkerContext) {
    let mut sub = match state
        .bus
        .subscribe::<MessageEvent>(
            "conversation_organizer",
            SubscriptionOpts::named("conversation_organizer"),
        )
        .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "conversation_organizer: bus subscribe failed, worker exiting");
            return;
        }
    };

    let mut dirty: HashSet<String> = HashSet::new();
    let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(DEBOUNCE_SECS));
    tick.tick().await;

    loop {
        ctx.wait_if_paused().await;
        tokio::select! {
            ack_opt = sub.next() => {
                let Some(ack) = ack_opt else { break; };
                if let MessageEvent::Logged { session_id, .. } = ack.event() {
                    ctx.begin_event("message", ack.seq().0, None);
                    debug!(session_id = %session_id, "Organizer: received MessageEvent::Logged");
                    ctx.progress(format!("queued message session {session_id}"));
                    dirty.insert(session_id.clone());
                    ctx.complete("session queued for organization");
                }
                ack.ack().await;
            }
            _ = tick.tick(), if !dirty.is_empty() => {
                let batch: Vec<String> = dirty.drain().collect();
                ctx.begin_poll(Some(300));
                ctx.progress(format!("organizing {} sessions", batch.len()));
                organize(&state, &batch).await;
                ctx.record_success();
            }
        }
    }
}

/// Core organize logic: fix links + emit events.
async fn organize(state: &AppState, session_ids: &[String]) {
    let mut compaction_linked = 0usize;
    let mut parent_fixed = 0usize;

    // P0 (highest priority): Link compaction fragments
    for sid in session_ids {
        if sid.contains("-acompact-") {
            if let Some(original_id) = extract_compaction_parent(sid) {
                match state
                    .store
                    .link_compaction_fragment(sid, &original_id)
                    .await
                {
                    Ok(true) => compaction_linked += 1,
                    Ok(false) => {} // already linked or no-op
                    Err(e) => {
                        warn!(session = %sid, error = %e, "Organizer: compaction link failed")
                    }
                }
            }
        }
    }

    // P1: Fix orphan parent links (safety net — Phase 1 root-cause fix handles most cases)
    let subagent_ids: Vec<String> = session_ids
        .iter()
        .filter(|s| s.starts_with("agent-") && !s.contains("-acompact-"))
        .cloned()
        .collect();
    if !subagent_ids.is_empty() {
        match state.store.fix_orphan_parent_links(&subagent_ids).await {
            Ok(n) => parent_fixed = n,
            Err(e) => warn!(error = %e, "Organizer: fix_orphan_parent_links failed"),
        }
    }

    // Emit SessionOrganized for all processed sessions
    for sid in session_ids {
        info!(session_id = %sid, "Organizer: emitting SessionOrganized");
        let _ = state
            .bus
            .publish_session(SessionEvent::Organized {
                session_id: sid.clone(),
            })
            .await;
    }

    info!(
        compaction_linked,
        parent_fixed,
        sessions = session_ids.len(),
        "Organizer: batch complete"
    );
}

/// Extract original session ID from compaction fragment.
/// Format: `agent-acompact-ORIGINAL_SESSION_ID-TIMESTAMP`
/// Returns the ORIGINAL_SESSION_ID portion.
fn extract_compaction_parent(session_id: &str) -> Option<String> {
    let rest = session_id.strip_prefix("agent-acompact-")?;
    // The original session_id is a UUID like "abc123-def456-..."
    // The timestamp suffix is typically the last hyphen-separated segment.
    let parts: Vec<&str> = rest.rsplitn(2, '-').collect();
    if parts.len() == 2 {
        Some(parts[1].to_string())
    } else {
        Some(rest.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_compaction_parent() {
        assert_eq!(
            extract_compaction_parent("agent-acompact-abc123-def456-1234567890"),
            Some("abc123-def456".to_string())
        );
        assert_eq!(
            extract_compaction_parent("agent-acompact-some-uuid-here-ts"),
            Some("some-uuid-here".to_string())
        );
        assert_eq!(extract_compaction_parent("agent-regular"), None);
        assert_eq!(extract_compaction_parent("not-agent"), None);
    }
}
