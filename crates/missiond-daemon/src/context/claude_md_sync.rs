use tracing::{info, warn};

use crate::state::AppState;

const MANAGED_START: &str = "<!-- missiond:managed:start -->";
const MANAGED_END: &str = "<!-- missiond:managed:end -->";

/// Sync KB preferences + hot topics into ~/.claude/CLAUDE.md managed section.
/// Only writes when content actually changes (hash-based detection).
pub(crate) fn sync_claude_md(state: &AppState) {
    let db = state.mission.db();

    let preferences = db.kb_list(Some("preference")).unwrap_or_default();
    let hot_keys = db.kb_hot_keys(20).unwrap_or_default();

    // Nothing to sync
    if preferences.is_empty() && hot_keys.is_empty() {
        return;
    }

    // Hash-based change detection
    let new_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for p in &preferences { p.summary.hash(&mut hasher); }
        for k in &hot_keys { k.hash(&mut hasher); }
        hasher.finish()
    };
    let last_hash = state.claude_md_hash.load(std::sync::atomic::Ordering::Relaxed);
    if new_hash == last_hash && last_hash != 0 {
        return;
    }

    // Build managed section
    let mut managed = String::new();
    managed.push_str(MANAGED_START);
    managed.push_str("\n# MissionD Managed\n");

    if !preferences.is_empty() {
        managed.push_str("\n## Preferences\n");
        for p in &preferences {
            managed.push_str(&format!("- {}\n", p.summary));
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
    let new_content = if let (Some(start), Some(end_pos)) = (
        existing.find(MANAGED_START),
        existing.find(MANAGED_END),
    ) {
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
        state.claude_md_hash.store(new_hash, std::sync::atomic::Ordering::Relaxed);
        return;
    }

    match std::fs::write(&claude_md_path, &new_content) {
        Ok(_) => {
            info!(
                prefs = preferences.len(),
                topics = hot_keys.len(),
                "CLAUDE.md managed section synced"
            );
            state.claude_md_hash.store(new_hash, std::sync::atomic::Ordering::Relaxed);
        }
        Err(e) => warn!(error = %e, "Failed to write CLAUDE.md"),
    }
}
