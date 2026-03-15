//! Idle Exploration — autonomous maintenance when system has spare capacity.
//!
//! Phase 4 of Jarvis evolution: when no user work is active, the system
//! generates read-only analysis tasks that run on idle slots.
//!
//! Safety: exploration tasks are auto_execute=true but READ-ONLY analysis.
//! Any modification suggestions become auto_execute=false follow-up tasks.

use tracing::{info, debug, warn};
use crate::state::AppState;

/// Cadence: minimum interval between exploration runs (2 hours).
const EXPLORE_INTERVAL_SECS: i64 = 2 * 3600;

/// Check if conditions are right for idle exploration, and if so,
/// create a single exploration Board task.
///
/// Called from `learning_tick()` on every autopilot tick (60s).
/// Internal cadence guard prevents over-triggering.
pub(crate) async fn check_idle_exploration(state: &AppState) {
    // Gate 1: cadence — at least 2h since last exploration
    let now = chrono::Utc::now().timestamp();
    let last = state.mission.db()
        .daemon_state_get("last_idle_explore_at")
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last < EXPLORE_INTERVAL_SECS {
        return;
    }

    // Gate 2: not paused
    if state.global_paused.load(std::sync::atomic::Ordering::Relaxed)
        || state.memory_paused.load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }

    // Gate 3: no pending explore tasks already (prevent flooding)
    match state.mission.db().count_tasks_by_category("explore") {
        Ok(n) if n > 0 => {
            debug!(pending = n, "Idle explorer: existing explore task pending, skipping");
            return;
        }
        Err(e) => {
            warn!(error = %e, "Idle explorer: failed to count explore tasks");
            return;
        }
        _ => {}
    }

    // Gate 4: no high/medium priority work in progress
    match state.mission.db().count_open_tasks_by_priority(&["high", "medium"]) {
        Ok(n) if n > 0 => {
            debug!(active_work = n, "Idle explorer: active work tasks exist, skipping");
            return;
        }
        Err(e) => {
            warn!(error = %e, "Idle explorer: failed to count active tasks");
            return;
        }
        _ => {}
    }

    // Gate 5: find an idle autopilot slot to assign the task to
    let assignee = find_idle_autopilot_slot(state).await;
    if assignee.is_none() {
        debug!("Idle explorer: no idle autopilot slot available");
        return;
    }
    let assignee = assignee.unwrap();

    // Round-robin through exploration types
    let explore_idx = state.mission.db()
        .daemon_state_get("idle_explore_idx")
        .unwrap_or(None)
        .unwrap_or(0);

    let task_created = match explore_idx % 5 {
        0 => explore_kb_consistency(state, &assignee).await,
        1 => explore_stale_dependencies(state, &assignee).await,
        2 => explore_unharvested_beacons(state, &assignee).await,
        3 => explore_kb_duplicates(state, &assignee).await,
        4 => explore_skill_synthesis(state, &assignee).await,
        _ => false,
    };

    if task_created {
        let _ = state.mission.db().daemon_state_set("last_idle_explore_at", now);
        let _ = state.mission.db().daemon_state_set("idle_explore_idx", explore_idx + 1);
        info!(explore_type = explore_idx % 4, assignee = %assignee, "Idle explorer: created exploration task");
    } else {
        // Even if no task was needed, advance the index to try next type on next run.
        // But don't update the timestamp — try again sooner.
        let _ = state.mission.db().daemon_state_set("idle_explore_idx", explore_idx + 1);
        debug!(explore_type = explore_idx % 4, "Idle explorer: no issues found for this type, advancing");
    }
}

/// Find an idle slot with `persistent` lifecycle that isn't a memory/supervisor slot.
async fn find_idle_autopilot_slot(state: &AppState) -> Option<String> {
    let slots = state.mission.list_slots();
    for slot in &slots {
        let id = &slot.config.id;
        // Skip system slots
        if id == "slot-memory" || id == "slot-memory-slow" || id == "slot-supervisor" {
            continue;
        }
        // Must be a persistent slot (those are the ones that accept board tasks)
        if !slot.config.is_persistent() {
            continue;
        }
        // Check if idle
        if let Some(info) = state.pty.get_status(id).await {
            if info.state == missiond_core::SessionState::Idle {
                return Some(id.clone());
            }
        }
    }
    None
}

// ============ Exploration Tasks ============

/// Explore 0: KB consistency — find low-confidence entries that need review.
async fn explore_kb_consistency(state: &AppState, assignee: &str) -> bool {
    let entries = match state.mission.db().kb_list_low_confidence(0.5, 10) {
        Ok(e) => e,
        Err(_) => return false,
    };
    if entries.is_empty() {
        return false;
    }

    let keys: Vec<String> = entries.iter().map(|e| {
        format!("- `{}` (confidence: {:.2}, category: {})", e.key, e.confidence, e.category)
    }).collect();
    let keys_list = keys.join("\n");

    let description = format!(
        "## READ-ONLY Analysis Task\n\n\
        Found {} KB entries with confidence < 0.5. Review each entry:\n\n\
        {}\n\n\
        For each entry, use `mission_kb_get` to read the full content, then decide:\n\
        1. If the information is correct and useful → use `mission_kb_update` to boost confidence to 0.7\n\
        2. If the information is outdated/wrong → use `mission_kb_forget` to remove it\n\
        3. If uncertain → leave as-is\n\n\
        Report your findings as a Board note on this task when done.",
        entries.len(), keys_list,
    );

    create_explore_task(state, "Explore: KB Consistency Review", &description, assignee).await
}

/// Explore 1: stale dependencies — check Cargo.toml for outdated crates.
async fn explore_stale_dependencies(state: &AppState, assignee: &str) -> bool {
    // Only trigger if Cargo.toml exists in cwd
    let cargo_path = std::path::Path::new("<REPO_ROOT>/Cargo.toml");
    if !cargo_path.exists() {
        return false;
    }

    let description = "\
        ## READ-ONLY Analysis Task\n\n\
        Check the project's Cargo.toml for outdated dependencies:\n\n\
        1. Read the workspace `Cargo.toml` and list major dependencies\n\
        2. For each significant dependency, check the current version vs latest available\n\
        3. Flag any dependencies more than 2 major versions behind\n\
        4. Report findings as a Board note on this task\n\n\
        **DO NOT modify any files.** Only analyze and report.\n\
        If updates are needed, create a separate Board task with `mission_board_create` \
        (auto_execute=false, priority=low) listing the recommended updates."
        .to_string();

    create_explore_task(state, "Explore: Dependency Staleness Check", &description, assignee).await
}

/// Explore 2: unharvested beacons — find feature beacons that haven't been analyzed.
async fn explore_unharvested_beacons(state: &AppState, assignee: &str) -> bool {
    let beacons = match state.mission.db().beacon_list() {
        Ok(b) => b,
        Err(_) => return false,
    };

    // Filter to beacons with nodes but no description (never reviewed)
    let unreviewed: Vec<_> = beacons.iter()
        .filter(|b| b.node_count > 0 && b.description.is_none())
        .take(5)
        .collect();

    if unreviewed.is_empty() {
        return false;
    }

    let beacon_list: Vec<String> = unreviewed.iter().map(|b| {
        format!("- `{}` ({} nodes)", b.name, b.node_count)
    }).collect();
    let beacons_str = beacon_list.join("\n");

    let description = format!(
        "## READ-ONLY Analysis Task\n\n\
        Found {} feature beacons without descriptions. Review each:\n\n\
        {}\n\n\
        For each beacon:\n\
        1. Use `mission_beacon_map` to see the tagged code symbols\n\
        2. Read the relevant source files to understand the feature\n\
        3. Use `mission_beacon_annotate` to add a concise description\n\n\
        Report your findings as a Board note on this task when done.",
        unreviewed.len(), beacons_str,
    );

    create_explore_task(state, "Explore: Beacon Feature Review", &description, assignee).await
}

/// Explore 3: KB duplicate detection — find KB entries with similar keys.
async fn explore_kb_duplicates(state: &AppState, assignee: &str) -> bool {
    // Use kb_list to get all entries, then find potential duplicates by key similarity
    let entries = match state.mission.db().kb_list(None) {
        Ok(e) => e,
        Err(_) => return false,
    };

    if entries.len() < 10 {
        return false; // Too few entries to have meaningful duplicates
    }

    let total = entries.len();
    let categories: std::collections::HashMap<String, usize> = {
        let mut m = std::collections::HashMap::new();
        for e in &entries {
            *m.entry(e.category.clone()).or_insert(0) += 1;
        }
        m
    };

    // Find categories with high density (>10 entries) — most likely to have duplicates
    let dense_cats: Vec<_> = categories.iter()
        .filter(|(_, &count)| count > 10)
        .map(|(cat, count)| format!("- `{}`: {} entries", cat, count))
        .collect();

    if dense_cats.is_empty() {
        return false;
    }

    let cats_str = dense_cats.join("\n");

    let description = format!(
        "## READ-ONLY Analysis Task\n\n\
        KB has {} total entries. Categories with high density (potential duplicates):\n\n\
        {}\n\n\
        For each dense category:\n\
        1. Use `mission_kb_search` with the category filter to list entries\n\
        2. Identify entries with similar keys, summaries, or overlapping content\n\
        3. For confirmed duplicates, use `mission_kb_forget` to remove the lower-quality one\n\
        4. For entries that should be merged, update the better one with combined content\n\n\
        Report your findings as a Board note on this task when done.",
        total, cats_str,
    );

    create_explore_task(state, "Explore: KB Duplicate Detection", &description, assignee).await
}

/// Explore 4: Skill Synthesis — cluster high-confidence, high-access KB entries
/// and generate Skill drafts that codify recurring patterns into reusable SOPs.
async fn explore_skill_synthesis(state: &AppState, assignee: &str) -> bool {
    let entries = match state.mission.db().kb_list(None) {
        Ok(e) => e,
        Err(_) => return false,
    };

    // Find high-quality entries: confidence >= 0.9, access_count >= 5, type = rule or fact
    let candidates: Vec<_> = entries.iter()
        .filter(|e| e.confidence >= 0.9 && e.access_count >= 5)
        .filter(|e| e.kb_type == "rule" || e.kb_type == "fact")
        .collect();

    if candidates.len() < 5 {
        return false; // Not enough mature entries to synthesize
    }

    // Group by category prefix to find clusters
    let mut clusters: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for e in &candidates {
        let prefix = e.category.split(':').next().unwrap_or(&e.category).to_string();
        clusters.entry(prefix).or_default().push(
            format!("- `{}` [{}] (conf={:.2}, access={}): {}", e.key, e.category, e.confidence, e.access_count, e.summary)
        );
    }

    // Find the largest cluster with >= 5 entries (most promising for synthesis)
    let best_cluster = clusters.iter()
        .filter(|(_, entries)| entries.len() >= 5)
        .max_by_key(|(_, entries)| entries.len());

    let (cluster_name, cluster_entries) = match best_cluster {
        Some((name, entries)) => (name.clone(), entries.clone()),
        None => return false,
    };

    let entries_str = cluster_entries.iter().take(20).cloned().collect::<Vec<_>>().join("\n");

    let description = format!(
        "## Skill Synthesis Task\n\n\
        Found {} high-confidence, high-access KB entries in the `{}` cluster.\n\
        These represent recurring knowledge that should be codified into a reusable Skill.\n\n\
        ### Candidate KB entries:\n{}\n\n\
        ### Instructions:\n\
        1. Read each KB entry with `mission_kb_search` to understand the full context\n\
        2. Identify the common theme/pattern across these entries\n\
        3. Generate a Skill draft following the SKILL.md format:\n\
           - frontmatter: name, description, allowed-tools\n\
           - INDEX table: intent → section\n\
           - Core sections with actionable instructions\n\
        4. Write the draft to `~/.claude/skills/auto-generated/{cluster_name}.md`\n\
        5. Report the synthesis as a Board note with:\n\
           - Which KB entries were consolidated\n\
           - The generated Skill path\n\
           - Confidence assessment of the synthesis quality\n\n\
        **Important**: The generated Skill should capture the *why* behind the rules, not just the *what*.\n\
        If entries contradict each other, flag the conflict rather than arbitrarily picking one.",
        cluster_entries.len(), cluster_name, entries_str
    );

    create_explore_task(state, &format!("Explore: Skill Synthesis — {}", cluster_name), &description, assignee).await
}

/// Helper: create an exploration Board task.
async fn create_explore_task(
    state: &AppState,
    title: &str,
    description: &str,
    assignee: &str,
) -> bool {
    let input = missiond_core::types::CreateBoardTaskInput {
        title: title.to_string(),
        description: Some(description.to_string()),
        priority: Some("low".to_string()),
        category: Some("explore".to_string()),
        project: Some("missiond".to_string()),
        server: None,
        due_date: None,
        parent_id: None,
        assignee: Some(assignee.to_string()),
        auto_execute: Some(true),
        prompt_template: None,
        hidden: None,
        flow_template: None,
        depends_on: None,
        dedupe_key: None,
    };

    match state.mission.db().create_board_task(&input) {
        Ok(task) => {
            state.event_bus.publish(
                crate::event_bus::DaemonEvent::BoardTaskStatusChanged {
                    task_id: task.id.clone(),
                    old_status: String::new(),
                    new_status: "open".to_string(),
                },
            );
            info!(task_id = %task.id, title = %title, "Idle explorer: created exploration task");
            true
        }
        Err(e) => {
            warn!(error = %e, title = %title, "Idle explorer: failed to create task");
            false
        }
    }
}
