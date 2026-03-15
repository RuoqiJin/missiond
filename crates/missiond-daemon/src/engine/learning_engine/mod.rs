//! Learning Engine — 事件驱动的认知与进化中枢。
//!
//! 合并: decision_engine (tier routing) + extraction (memory pipeline)
//!       + decision_harvest (policy generalization) + timeline_analyst (pattern detection)

pub mod decision_engine;
pub mod extraction;
pub mod decision_harvest;
pub mod timeline_analyst;
pub mod idle_explorer;
pub mod historical_scanner;

use tracing::info;
use crate::state::AppState;

/// Learning Engine periodic tick — called from autopilot_tick.
///
/// Runs all learning-side maintenance:
/// - KB auto-GC (hourly)
/// - Decision task reaper (15min timeout)
/// - Decision harvester checkpoint (24h)
/// - Timeline analyst (12h cadence)
pub(crate) async fn learning_tick(state: &AppState) {
    // KB auto-GC: every hour (internal cadence guard)
    extraction::check_kb_auto_gc(state);

    // Decision Engine reaper: 15min timeout for master questions
    decision_engine::reap_stale_decision_tasks(state).await;

    // Decision Engine: checkpoint harvester (every 24h, tasks with ≥3 unharvested decisions)
    {
        let now = chrono::Utc::now().timestamp();
        let last = state.mission.db().daemon_state_get("last_decision_harvest_at").unwrap_or(None).unwrap_or(0);
        if now - last > 86400 {
            let _ = state.mission.db().daemon_state_set("last_decision_harvest_at", now);
            if let Ok(tasks) = state.mission.db().find_tasks_with_unharvested_decisions(3) {
                for (task_id, task_title, count) in &tasks {
                    info!(task_id, count, "Decision harvester checkpoint: incremental harvest");
                    let state_clone = state.clone();
                    let tid = task_id.clone();
                    let tt = task_title.clone();
                    tokio::spawn(async move {
                        decision_harvest::harvest_decisions_for_task(&state_clone, &tid, &tt).await;
                    });
                }
            }
        }
    }

    // L4: Timeline Analyst (12h cadence, internal guard)
    timeline_analyst::check_timeline_analysis(state).await;

    // Co-occurrence cache refresh (every 6h) for preemptive context prefetching
    {
        let now = chrono::Utc::now().timestamp();
        let last = state.mission.db().daemon_state_get("last_cooccurrence_refresh").unwrap_or(None).unwrap_or(0);
        if now - last > 6 * 3600 {
            let _ = state.mission.db().daemon_state_set("last_cooccurrence_refresh", now);
            if let Ok(matrix) = state.mission.db().kb_compute_cooccurrence(168, 3) {
                let count = matrix.len();
                *state.kb_cooccurrence_cache.write().await = matrix;
                info!(entries = count, "Co-occurrence cache refreshed");
            }
        }
    }

    // L5: Idle Exploration (2h cadence, only when system has spare capacity)
    idle_explorer::check_idle_exploration(state).await;

    // L6: Historical Habit Scan (4h cadence, lowest priority, only when idle)
    historical_scanner::check_historical_scan(state).await;
}
