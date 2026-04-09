use crate::context::slot_env::{build_slot_tracking_env, capture_slot_session_uuid};
use anyhow::Result;
use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{PTYAgentInfo, PTYManager};
use missiond_core::{PTYSlot, PTYSpawnOptions};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// A unified spawner for tracked slot PTY processes.
/// This ensures that the session tracking environment and UUID capture
/// are always correctly applied, preventing orphaned sessions.
pub async fn spawn_tracked_slot(
    pty: &Arc<PTYManager>,
    store: &Arc<dyn MissionStore>,
    pty_session_uuids: &Arc<RwLock<HashSet<String>>>,
    pty_slot: &PTYSlot,
    mut options: PTYSpawnOptions,
    original_slot_env: Option<&HashMap<String, String>>,
) -> Result<PTYAgentInfo> {
    // 1. Automatically build tracking environment and session file path
    let (tracking_env, session_file) =
        build_slot_tracking_env(&pty_slot.id, original_slot_env).await;

    // 2. Merge tracking variables into options
    options.extra_env.extend(tracking_env);

    let wait_for_idle = options.wait_for_idle;
    let initial_prompt = options.initial_prompt.take();

    // 3. Execute underlying PTY spawn
    let spawn_result = pty.spawn(pty_slot, options).await?;

    // 4. Handle UUID capture based on whether we waited for idle
    if wait_for_idle {
        // The process has reached idle, so the hook should have written the file
        capture_slot_session_uuid(store, pty_session_uuids, &pty_slot.id, &session_file).await;

        // 5. Send initial prompt if configured (session is already idle)
        if let Some(prompt) = initial_prompt {
            info!(slot_id = %pty_slot.id, "Sending initial prompt after idle");
            if let Err(e) = pty.send_fire_and_forget(&pty_slot.id, &prompt).await {
                warn!(slot_id = %pty_slot.id, error = %e, "Failed to send initial prompt");
            }
        }
    } else {
        // Spawning asynchronously, poll for the UUID in a background task
        let store_clone = Arc::clone(store);
        let uuids_clone = Arc::clone(pty_session_uuids);
        let slot_id = pty_slot.id.clone();
        let pty_clone = Arc::clone(pty);
        tokio::spawn(async move {
            capture_slot_session_uuid(&store_clone, &uuids_clone, &slot_id, &session_file).await;

            // Send initial prompt after UUID capture (session should be idle by now)
            if let Some(prompt) = initial_prompt {
                info!(slot_id = %slot_id, "Sending initial prompt after async idle");
                if let Err(e) = pty_clone.send_fire_and_forget(&slot_id, &prompt).await {
                    warn!(slot_id = %slot_id, error = %e, "Failed to send initial prompt");
                }
            }
        });
    }

    Ok(spawn_result)
}
