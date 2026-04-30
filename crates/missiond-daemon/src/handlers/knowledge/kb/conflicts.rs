use crate::state::AppState;

/// Detect semantically similar entries that may conflict with a newly created KB entry.
/// Uses embedding cosine similarity within the same category prefix.
pub(super) async fn detect_kb_conflicts(
    state: &AppState,
    new_entry: &missiond_core::types::KnowledgeEntry,
) -> Vec<serde_json::Value> {
    const CONFLICT_SIM_THRESHOLD: f32 = 0.82;

    let svc = match state.embedding_service.as_ref() {
        Some(s) => s,
        None => return vec![],
    };

    // Embed the new entry's summary
    let new_text = format!("{} {}", new_entry.key, new_entry.summary);
    let new_vec = match svc.embed(&new_text) {
        Some(v) => v,
        None => return vec![],
    };

    // Compare against cached KB embeddings
    let cache = state.kb_search_cache.read().await;
    let category_prefix = new_entry
        .category
        .split(':')
        .next()
        .unwrap_or(&new_entry.category);

    let mut conflicts = Vec::new();
    for (id, vec) in cache.iter() {
        if id == &new_entry.id {
            continue; // skip self
        }
        let cosine = missiond_core::embedding::cosine_similarity(&new_vec, vec);
        // Hybrid conflict detection: pure cosine OR (moderate cosine + Jaccard overlap)
        // Addresses semantic dilution for long vs short entries
        let is_conflict = if cosine >= CONFLICT_SIM_THRESHOLD {
            true
        } else if cosine >= 0.6 {
            // Fetch entry to compute Jaccard on summary text
            if let Ok(Some(existing)) = state.store.kb_get_by_id(id).await {
                let existing_prefix = existing
                    .category
                    .split(':')
                    .next()
                    .unwrap_or(&existing.category);
                if existing_prefix == category_prefix {
                    let jaccard =
                        text_jaccard(&new_text, &format!("{} {}", existing.key, existing.summary));
                    jaccard >= 0.5
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        if is_conflict {
            if let Ok(Some(existing)) = state.store.kb_get_by_id(id).await {
                let existing_prefix = existing
                    .category
                    .split(':')
                    .next()
                    .unwrap_or(&existing.category);
                if existing_prefix == category_prefix {
                    conflicts.push(serde_json::json!({
                        "id": existing.id,
                        "category": existing.category,
                        "key": existing.key,
                        "summary": existing.summary,
                        "confidence": existing.confidence,
                        "similarity": format!("{:.3}", cosine),
                    }));
                }
            }
        }
    }

    // Sort by similarity descending, limit to top 5
    conflicts.sort_by(|a, b| {
        let sa = a["similarity"].as_str().unwrap_or("0");
        let sb = b["similarity"].as_str().unwrap_or("0");
        sb.partial_cmp(sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    conflicts.truncate(5);
    conflicts
}

/// Lightweight Jaccard similarity on tokenized text (CJK unigrams + ASCII words).
fn text_jaccard(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let tokenize = |text: &str| -> HashSet<String> {
        let mut tokens = HashSet::new();
        let mut word = String::new();
        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                word.push(ch.to_ascii_lowercase());
            } else {
                if word.len() >= 2 {
                    tokens.insert(word.clone());
                }
                word.clear();
                if ch as u32 > 0x2E80 {
                    tokens.insert(ch.to_string());
                }
            }
        }
        if word.len() >= 2 {
            tokens.insert(word);
        }
        tokens
    };
    let ta = tokenize(a);
    let tb = tokenize(b);
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}
