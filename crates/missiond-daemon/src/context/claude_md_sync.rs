use tracing::{info, warn};

use crate::state::AppState;

const MANAGED_START: &str = "<!-- missiond:managed:start -->";
const MANAGED_END: &str = "<!-- missiond:managed:end -->";

/// Sync KB preferences + strategic context + hot topics into ~/.claude/CLAUDE.md managed section.
/// Only writes when content actually changes (hash-based detection).
pub(crate) async fn sync_claude_md(state: &AppState) {
    let preferences = state
        .store
        .kb_list(Some("preference"))
        .await
        .unwrap_or_default();
    let hot_keys = state.store.kb_hot_keys(20).await.unwrap_or_default();

    // Load strategic state from KB (Phase 2b: context feedback)
    let strategic_state = state
        .store
        .kb_get("strategic-state")
        .await
        .ok()
        .flatten()
        .and_then(|e| e.detail);

    // Running board tasks for Active Tasks anchor
    let running_tasks = state
        .store
        .list_board_tasks(Some("running"), false)
        .await
        .unwrap_or_default();

    // Nothing to sync
    if preferences.is_empty()
        && hot_keys.is_empty()
        && strategic_state.is_none()
        && running_tasks.is_empty()
    {
        return;
    }

    // Hash-based change detection
    let new_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for p in &preferences {
            p.summary.hash(&mut hasher);
        }
        for k in &hot_keys {
            k.hash(&mut hasher);
        }
        if let Some(ref s) = strategic_state {
            s.to_string().hash(&mut hasher);
        }
        for t in &running_tasks {
            t.id.hash(&mut hasher);
            t.title.hash(&mut hasher);
        }
        hasher.finish()
    };
    let last_hash = state
        .claude_md_hash
        .load(std::sync::atomic::Ordering::Relaxed);
    if new_hash == last_hash && last_hash != 0 {
        return;
    }

    // Build managed section
    let mut managed = String::new();
    managed.push_str(MANAGED_START);
    managed.push_str("\n# MissionD Managed\n");

    // Phase 2b: Inject strategic context (dev trajectory, anti-patterns, collaboration patterns)
    if let Some(ref state_json) = strategic_state {
        build_strategic_section(&mut managed, state_json);
    }

    if !preferences.is_empty() {
        managed.push_str("\n## Preferences\n");
        for p in &preferences {
            managed.push_str(&format!("- {}\n", p.summary));
        }
    }

    // Active running tasks — persistent background anchor (prevents role drift after context compaction)
    if !running_tasks.is_empty() {
        managed.push_str("\n## Active Tasks\n");
        for t in running_tasks.iter().take(5) {
            let age = chrono::DateTime::parse_from_rfc3339(&t.updated_at)
                .map(|dt| {
                    let mins = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_minutes();
                    if mins < 60 {
                        format!(", {}min", mins)
                    } else {
                        format!(", {}h", mins / 60)
                    }
                })
                .unwrap_or_default();
            managed.push_str(&format!(
                "- [{}] {} (running{})\n",
                &t.id.as_str()[..8.min(t.id.as_str().len())],
                t.title,
                age,
            ));
        }
    }

    if !hot_keys.is_empty() {
        managed.push_str(&format!("\n## Hot Topics\n{}\n", hot_keys.join(", ")));
    }

    managed.push_str(MANAGED_END);

    // Read existing file
    let claude_md_path = match dirs::home_dir() {
        Some(home) => home.join(".claude/CLAUDE.md"),
        None => {
            warn!("Cannot determine home directory for CLAUDE.md sync");
            return;
        }
    };

    let existing = std::fs::read_to_string(&claude_md_path).unwrap_or_default();

    // Replace or append managed section
    let new_content = if let (Some(start), Some(end_pos)) =
        (existing.find(MANAGED_START), existing.find(MANAGED_END))
    {
        let before = &existing[..start];
        let after_marker = end_pos + MANAGED_END.len();
        let after = &existing[after_marker..];
        format!("{}{}{}", before, managed, after)
    } else {
        // Append to end
        if existing.trim().is_empty() {
            managed
        } else {
            format!("{}\n\n{}\n", existing.trim_end(), managed)
        }
    };

    // Only write if content actually differs
    if new_content == existing {
        state
            .claude_md_hash
            .store(new_hash, std::sync::atomic::Ordering::Relaxed);
        return;
    }

    match std::fs::write(&claude_md_path, &new_content) {
        Ok(_) => {
            info!(
                prefs = preferences.len(),
                topics = hot_keys.len(),
                "CLAUDE.md managed section synced"
            );
            state
                .claude_md_hash
                .store(new_hash, std::sync::atomic::Ordering::Relaxed);
        }
        Err(e) => warn!(error = %e, "Failed to write CLAUDE.md"),
    }
}

/// Build strategic context section from Strategic State JSON.
/// Injects development trajectory, anti-patterns, and negative collaboration patterns
/// so every new Claude Code session has strategic awareness.
///
/// **Capped** to prevent CLAUDE.md bloat (Gemini ARB recommendation):
/// - Focus: 1 entry, Goals: max 2, Anti-patterns: max 3, Negative patterns: max 3
fn build_strategic_section(managed: &mut String, state_json: &serde_json::Value) {
    let mut has_content = false;

    // Development trajectory — current focus (1) & goals (max 2)
    if let Some(traj) = state_json.get("development_trajectory") {
        let focus = traj
            .get("current_focus")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let goals: Vec<&str> = traj
            .get("inferred_goals")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).take(2).collect())
            .unwrap_or_default();

        if !focus.is_empty() || !goals.is_empty() {
            if !has_content {
                managed.push_str("\n## Strategic Context\n");
                has_content = true;
            }
            if !focus.is_empty() {
                managed.push_str(&format!("- 当前焦点: {}\n", focus));
            }
            for goal in &goals {
                managed.push_str(&format!("- 推测目标: {}\n", goal));
            }
        }
    }

    // Anti-patterns — high-severity friction points as warnings (max 3)
    if let Some(anti) = state_json.get("anti_patterns").and_then(|v| v.as_array()) {
        let rules: Vec<&str> = anti
            .iter()
            .filter_map(|v| v.get("rule").and_then(|r| r.as_str()))
            .take(3)
            .collect();
        if !rules.is_empty() {
            if !has_content {
                managed.push_str("\n## Strategic Context\n");
                has_content = true;
            }
            for rule in &rules {
                managed.push_str(&format!("- 反面模式: {}\n", rule));
            }
        }
    }

    // Negative collaboration patterns — things to avoid (max 3)
    if let Some(patterns) = state_json
        .get("collaboration_patterns")
        .and_then(|v| v.as_array())
    {
        let negatives: Vec<&str> = patterns
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("negative"))
            .filter_map(|v| v.get("pattern").and_then(|p| p.as_str()))
            .take(3)
            .collect();
        if !negatives.is_empty() {
            if !has_content {
                managed.push_str("\n## Strategic Context\n");
            }
            for neg in &negatives {
                managed.push_str(&format!("- 避免: {}\n", neg));
            }
        }
    }
}
