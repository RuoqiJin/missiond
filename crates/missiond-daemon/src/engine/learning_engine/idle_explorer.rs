//! Idle Exploration — autonomous maintenance when system has spare capacity.
//!
//! Phase 4 of Jarvis evolution: when no user work is active, the system
//! generates read-only analysis tasks for operator review.
//!
//! Safety: exploration tasks are review-only projections. Any modification
//! suggestions require a separate delegated task with explicit grants.

use crate::context::v3_blueprint_runtime::LearningEngineRuntimeConfig;
use crate::engine::control_plane_kernel::{ControlPlaneKernel, UpsertTaskContractCommand};
use crate::state::AppState;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

/// Check if conditions are right for idle exploration, and if so,
/// create a single exploration Board task.
///
/// Called from `learning_tick()` on every autopilot tick (60s).
/// Internal cadence guard prevents over-triggering.
pub(crate) async fn check_idle_exploration(state: &AppState) {
    let config = match LearningEngineRuntimeConfig::load_for_current_dir() {
        Ok(config) => config,
        Err(err) => {
            warn!(error = %err, "Idle explorer: V3 learning-engine-policy unavailable");
            return;
        }
    };

    // Gate 1: cadence — at least 2h since last exploration
    let now = chrono::Utc::now().timestamp();
    let last = state
        .store
        .daemon_state_get("last_idle_explore_at")
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last < config.idle_explore_interval_secs {
        return;
    }

    // Gate 2: not paused (read from ControlTree — single source of truth)
    {
        let tree = state.control_manager.current();
        if tree.global_paused || tree.is_domain_paused(crate::control_tree::CtlDomain::Memory) {
            return;
        }
    }

    // Gate 3: no pending explore tasks already (prevent flooding)
    match state.store.count_tasks_by_category("explore").await {
        Ok(n) if n > 0 => {
            debug!(
                pending = n,
                "Idle explorer: existing explore task pending, skipping"
            );
            return;
        }
        Err(e) => {
            warn!(error = %e, "Idle explorer: failed to count explore tasks");
            return;
        }
        _ => {}
    }

    // Gate 4: no high/medium priority work in progress
    match state
        .store
        .count_open_tasks_by_priority(&["high", "medium"])
        .await
    {
        Ok(n) if n > 0 => {
            debug!(
                active_work = n,
                "Idle explorer: active work tasks exist, skipping"
            );
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
    let explore_idx = state
        .store
        .daemon_state_get("idle_explore_idx")
        .await
        .unwrap_or(None)
        .unwrap_or(0);

    let task_created = match explore_idx % 8 {
        0 => explore_kb_consistency(state, &assignee).await,
        1 => explore_stale_dependencies(state, &assignee).await,
        2 => explore_unharvested_beacons(state, &assignee).await,
        3 => explore_kb_duplicates(state, &assignee).await,
        4 => explore_skill_synthesis(state, &assignee).await,
        5 => explore_memory_consolidation(state, &assignee).await,
        6 => explore_stale_state_verification(state, &assignee).await,
        7 => explore_shadow_replay(state, &assignee).await,
        _ => false,
    };

    if task_created {
        let _ = state
            .store
            .daemon_state_set("last_idle_explore_at", now)
            .await;
        let _ = state
            .store
            .daemon_state_set("idle_explore_idx", explore_idx + 1)
            .await;
        info!(explore_type = explore_idx % 6, assignee = %assignee, "Idle explorer: created exploration task");
    } else {
        let _ = state
            .store
            .daemon_state_set("idle_explore_idx", explore_idx + 1)
            .await;
        debug!(
            explore_type = explore_idx % 6,
            "Idle explorer: no issues found for this type, advancing"
        );
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
    let entries = match state.store.kb_list_low_confidence(0.5, 10).await {
        Ok(e) => e,
        Err(_) => return false,
    };
    if entries.is_empty() {
        return false;
    }

    let keys: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                "- `{}` (confidence: {:.2}, category: {})",
                e.key, e.confidence, e.category
            )
        })
        .collect();
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
        entries.len(),
        keys_list,
    );

    create_explore_task(
        state,
        "Explore: KB Consistency Review",
        &description,
        assignee,
    )
    .await
}

/// Explore 1: stale dependencies — check Cargo.toml for outdated crates.
async fn explore_stale_dependencies(state: &AppState, assignee: &str) -> bool {
    // Only trigger if Cargo.toml exists in cwd
    let project_root = crate::helpers::missiond_project_root();
    let cargo_path = project_root.join("Cargo.toml");
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

    create_explore_task(
        state,
        "Explore: Dependency Staleness Check",
        &description,
        assignee,
    )
    .await
}

/// Explore 2: unharvested beacons — find feature beacons that haven't been analyzed.
async fn explore_unharvested_beacons(state: &AppState, assignee: &str) -> bool {
    let beacons = match state.store.beacon_list().await {
        Ok(b) => b,
        Err(_) => return false,
    };

    // Filter to beacons with nodes but no description (never reviewed)
    let unreviewed: Vec<_> = beacons
        .iter()
        .filter(|b| b.node_count > 0 && b.description.is_none())
        .take(5)
        .collect();

    if unreviewed.is_empty() {
        return false;
    }

    let beacon_list: Vec<String> = unreviewed
        .iter()
        .map(|b| format!("- `{}` ({} nodes)", b.name, b.node_count))
        .collect();
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
        unreviewed.len(),
        beacons_str,
    );

    create_explore_task(
        state,
        "Explore: Beacon Feature Review",
        &description,
        assignee,
    )
    .await
}

/// Explore 3: KB duplicate detection — find KB entries with similar keys.
async fn explore_kb_duplicates(state: &AppState, assignee: &str) -> bool {
    // Use kb_list to get all entries, then find potential duplicates by key similarity
    let entries = match state.store.kb_list(None).await {
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
    let dense_cats: Vec<_> = categories
        .iter()
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

    create_explore_task(
        state,
        "Explore: KB Duplicate Detection",
        &description,
        assignee,
    )
    .await
}

/// Explore 4: Skill Synthesis — cluster high-confidence, high-access KB entries
/// and generate Skill drafts that codify recurring patterns into reusable SOPs.
async fn explore_skill_synthesis(state: &AppState, assignee: &str) -> bool {
    let now = chrono::Utc::now().timestamp();
    let entries = match state.store.kb_list(None).await {
        Ok(e) => e,
        Err(_) => return false,
    };

    // Find high-quality entries with composite trigger:
    // confidence >= 0.85 AND confidence * ln(access_count + 1) >= 1.15
    // High-weight categories (architecture/policy) only need cluster >= 3
    let candidates: Vec<_> = entries
        .iter()
        .filter(|e| e.kb_type == "rule" || e.kb_type == "fact")
        .filter(|e| {
            e.confidence >= 0.85 && e.confidence * ((e.access_count as f64) + 1.0).ln() >= 1.15
        })
        // Time window: only recently active entries (accessed in last 30 days)
        .filter(|e| {
            e.last_accessed_at
                .as_ref()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_days() <= 30)
                .unwrap_or(false)
        })
        .collect();

    if candidates.len() < 3 {
        return false;
    }

    // Group by category prefix to find clusters
    let mut clusters: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for e in &candidates {
        let prefix = e
            .category
            .split(':')
            .next()
            .unwrap_or(&e.category)
            .to_string();
        clusters.entry(prefix).or_default().push(format!(
            "- `{}` [{}] (conf={:.2}, access={}): {}",
            e.key, e.category, e.confidence, e.access_count, e.summary
        ));
    }

    // Dynamic cluster size: architecture/policy need >= 3, others >= 5
    let best_cluster = clusters
        .iter()
        .filter(|(prefix, entries)| {
            let min_size = match prefix.as_str() {
                "architecture" | "policy" | "preference" => 3,
                _ => 5,
            };
            entries.len() >= min_size
        })
        .max_by_key(|(_, entries)| entries.len());

    let (cluster_name, cluster_entries) = match best_cluster {
        Some((name, entries)) => (name.clone(), entries.clone()),
        None => return false,
    };

    // Reentry guard: skip clusters synthesized within last 7 days
    let lock_key = format!("skill_synth_{}", cluster_name);
    let last_synth = state
        .store
        .daemon_state_get(&lock_key)
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last_synth < 7 * 86400 {
        debug!(cluster = %cluster_name, days_ago = (now - last_synth) / 86400, "Skill synthesis: cluster recently synthesized, skipping");
        return false;
    }

    let entries_str = cluster_entries
        .iter()
        .take(20)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

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
        cluster_entries.len(),
        cluster_name,
        entries_str
    );

    // Set reentry lock BEFORE creating task (cleared on failure by autopilot event listener)
    let _ = state.store.daemon_state_set(&lock_key, now).await;

    create_explore_task(
        state,
        &format!("Explore: Skill Synthesis — {}", cluster_name),
        &description,
        assignee,
    )
    .await
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
        auto_execute: Some(false),
        prompt_template: None,
        hidden: None,
        flow_template: None,
        depends_on: None,
        dedupe_key: None,
        timeout_secs: None,
        context_intent: None,
        runtime_metadata: Some(idle_exploration_runtime_metadata(
            title,
            description,
            assignee,
        )),
    };

    match state.store.create_board_task(&input).await {
        Ok(task) => {
            upsert_idle_exploration_task_contract(state, &task).await;
            let _ = state
                .bus
                .publish_board(missiond_core::event::events::BoardEvent::StatusChanged {
                    task_id: task.id.to_string(),
                    old_status: String::new(),
                    new_status: "open".to_string(),
                })
                .await;
            info!(task_id = %task.id, title = %title, "Idle explorer: created exploration task");
            true
        }
        Err(e) => {
            warn!(error = %e, title = %title, "Idle explorer: failed to create task");
            false
        }
    }
}

async fn upsert_idle_exploration_task_contract(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
) {
    if let Err(err) = ControlPlaneKernel::new(state)
        .upsert_task_contract_command(UpsertTaskContractCommand {
            task_id: task.id.to_string(),
            project_id: task.project.clone(),
            runtime_metadata: task.runtime_metadata.clone(),
        })
        .await
    {
        warn!(
            error = %err,
            task_id = %task.id,
            "Idle explorer: failed to upsert task_contracts for exploration BoardTask"
        );
    }
}

fn idle_exploration_runtime_metadata(
    title: &str,
    description: &str,
    candidate_slot: &str,
) -> Value {
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "idle_explorer",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "idle-exploration-review",
            "title": title,
            "candidate_slot": candidate_slot,
            "description_preview": description.chars().take(240).collect::<String>(),
            "completion_protocol": "review-only task; learning prose is projection and cannot close tasks"
        },
        "read_scope": [
            "learning-engine:idle-explorer",
            "knowledge-base:read"
        ],
        "write_scope": [],
        "must_not_touch": [],
        "capability_grant_ids": [],
        "sandbox_profile": "system-learning-review",
        "projection_policy": "description_notes_are_projection_only"
    })
}

/// Explore 5: Memory Consolidation — find clusters of similar KB entries within the same
/// category that can be merged into a single Master Rule.
async fn explore_memory_consolidation(state: &AppState, assignee: &str) -> bool {
    let now = chrono::Utc::now().timestamp();

    // Reentry guard: 7-day cooldown
    let last_run = state
        .store
        .daemon_state_get("last_consolidation_at")
        .await
        .unwrap_or(None)
        .unwrap_or(0);
    if now - last_run < 7 * 86400 {
        debug!(
            days_ago = (now - last_run) / 86400,
            "Memory consolidation: recently run, skipping"
        );
        return false;
    }

    let entries = match state.store.kb_list(None).await {
        Ok(e) => e,
        Err(_) => return false,
    };

    // Group by category, find categories with many similar entries
    let mut by_category: std::collections::HashMap<
        String,
        Vec<&missiond_core::types::KnowledgeEntry>,
    > = std::collections::HashMap::new();
    for e in &entries {
        by_category.entry(e.category.clone()).or_default().push(e);
    }

    // Find consolidation candidates: categories with ≥8 entries
    let mut candidates: Vec<(String, usize)> = by_category
        .iter()
        .filter(|(cat, items)| {
            // Skip categories that should not be consolidated
            !cat.starts_with("infra") && !cat.starts_with("credential") && items.len() >= 8
        })
        .map(|(cat, items)| (cat.clone(), items.len()))
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    if candidates.is_empty() {
        return false;
    }

    // Pick the densest category for consolidation
    let (target_cat, count) = &candidates[0];
    let sample_entries: Vec<String> = by_category[target_cat]
        .iter()
        .take(15)
        .map(|e| format!("- `{}` (conf={:.2}): {}", e.key, e.confidence, e.summary))
        .collect();
    let sample_str = sample_entries.join("\n");

    let description = format!(
        "## Memory Consolidation Task\n\n\
        Category `{}` has {} entries — likely contains redundant or overlapping knowledge.\n\n\
        ### Sample entries:\n{}\n\n\
        ### Instructions:\n\
        1. Use `mission_kb_search(category=\"{}\")` to review ALL entries in this category\n\
        2. Identify clusters of entries covering the same topic/concept\n\
        3. For each cluster (≥3 similar entries):\n\
           a. Synthesize a single **Master Rule** that captures the consolidated knowledge\n\
           b. Use `mission_kb_remember` to create the master entry with:\n\
              - key: `master-<topic>` prefix\n\
              - confidence: MAX of the original entries\n\
              - detail: include `{{\"consolidated_from\": [\"key1\", \"key2\", ...]}}` for traceability\n\
           c. Use `mission_kb_forget` to remove the original fragmented entries\n\
        4. Report consolidation results as a Board note:\n\
           - How many clusters found\n\
           - How many entries merged (N→1 for each cluster)\n\
           - Net reduction in entry count\n\n\
        **Important**: Preserve the *why* behind each piece of knowledge. If entries conflict, keep the higher-confidence one and flag the conflict.",
        target_cat, count, sample_str, target_cat
    );

    let _ = state
        .store
        .daemon_state_set("last_consolidation_at", now)
        .await;
    create_explore_task(
        state,
        &format!("Explore: Memory Consolidation — {}", target_cat),
        &description,
        assignee,
    )
    .await
}

/// Explore 6: Stale State Verification — verify state-type KB entries that
/// haven't been accessed recently are still accurate.
async fn explore_stale_state_verification(state: &AppState, assignee: &str) -> bool {
    let entries = match state.store.kb_list_stale_state_entries(14, 5).await {
        Ok(e) => e,
        Err(_) => return false,
    };
    if entries.is_empty() {
        return false;
    }

    let keys_list: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                "- `{}` [{}] (conf={:.2}, last_access={})",
                e.key,
                e.category,
                e.confidence,
                e.last_accessed_at.as_deref().unwrap_or("never")
            )
        })
        .collect();
    let keys_str = keys_list.join("\n");

    let description = format!(
        "## State Verification Task\n\n\
        Found {} state-type KB entries not accessed in 14+ days.\n\
        These may describe outdated operational state.\n\n\
        {}\n\n\
        ### Instructions:\n\
        For each entry:\n\
        1. Use `mission_kb_get` to read the full content\n\
        2. Verify the stated fact using appropriate tools:\n\
           - `mission_infra_query` for server/service state\n\
           - `mission_reachability` for connectivity checks\n\
           - `mission_os_diagnose` for system diagnostics\n\
        3. If CONFIRMED still true → `mission_kb_remember` with same key, confidence boosted to 0.9\n\
        4. If STALE/WRONG → `mission_kb_forget` to remove outdated state\n\
        5. If CANNOT VERIFY → leave unchanged\n\n\
        Report findings as a Board note.",
        entries.len(),
        keys_str,
    );

    create_explore_task(
        state,
        "Explore: Stale State Verification",
        &description,
        assignee,
    )
    .await
}

/// Explore 7: Shadow Replay — verify that KB mutations haven't broken
/// previously-successful task dispatch by comparing prompt snapshots
/// against current KB state.
async fn explore_shadow_replay(state: &AppState, assignee: &str) -> bool {
    let snapshots = match state.store.list_modified_snapshots(3).await {
        Ok(s) => s,
        Err(_) => return false,
    };
    if snapshots.is_empty() {
        return false;
    }

    let snapshot_list: Vec<String> = snapshots
        .iter()
        .map(|(task_id, _prompt, kb_ids, created)| {
            format!(
                "- task `{}` (created: {}, cited KBs: {})",
                &task_id[..8.min(task_id.len())],
                created,
                kb_ids
            )
        })
        .collect();
    let list_str = snapshot_list.join("\n");

    let first_prompt_preview: String = snapshots[0].1.chars().take(500).collect();

    let description = format!(
        "## Shadow Replay Verification Task\n\n\
        Found {} successful prompt snapshots whose cited KB entries have been modified.\n\
        This could indicate that KB mutations have invalidated previously-working dispatch.\n\n\
        ### Snapshots to verify:\n{}\n\n\
        ### First snapshot prompt preview:\n```\n{}\n```\n\n\
        ### Instructions:\n\
        For each snapshot:\n\
        1. Read the cited KB entries using `mission_kb_get` (by key)\n\
        2. Compare the current KB content with what the prompt expected\n\
        3. Evaluate whether the prompt would still succeed with the current KB state\n\
        4. If STILL VALID → no action needed\n\
        5. If BROKEN → lower confidence of the modified KB entries via `mission_kb_remember`\n\
        6. If entry was DELETED → note it but take no action\n\n\
        **This is a READ-ONLY analysis task.** Do not re-execute any tasks.\n\
        Report findings as a Board note.",
        snapshots.len(),
        list_str,
        first_prompt_preview
    );

    create_explore_task(
        state,
        "Explore: Shadow Replay Verification",
        &description,
        assignee,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_exploration_metadata_declares_task_contract_authority() {
        let metadata =
            idle_exploration_runtime_metadata("Explore: KB", "review knowledge base", "slot-a");
        assert_eq!(metadata["source"], "idle_explorer");
        assert_eq!(metadata["control_state"], "task_contracts");
        assert_eq!(
            metadata["dispatch_metadata"]["task_class"],
            "idle-exploration-review"
        );
        assert_eq!(metadata["dispatch_metadata"]["candidate_slot"], "slot-a");
        assert_eq!(metadata["write_scope"].as_array().unwrap().len(), 0);
        assert_eq!(metadata["sandbox_profile"], "system-learning-review");
    }
}
