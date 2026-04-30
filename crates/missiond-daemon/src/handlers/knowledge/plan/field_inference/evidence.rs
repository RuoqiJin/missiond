use super::*;

// ── evidence-sidecar scanners ─────────────────────────────────────────

/// Look for the most-recent string value of any matching key. Searches
/// each entry top-level + the well-known nested holders that the wave-12
/// evidence collector emits (`evidence`, `inner_dispatch`, `inner_result`,
/// `typed_evidence`). Newest-first match wins.
pub(super) fn scan_evidence_string_field(entries: &[Value], keys: &[&str]) -> Option<String> {
    for entry in entries.iter().rev() {
        if let Some(v) = pluck_string(entry, keys) {
            return Some(v);
        }
        for nested in &[
            "evidence",
            "inner_dispatch",
            "inner_result",
            "typed_evidence",
        ] {
            if let Some(child) = entry.get(*nested) {
                if let Some(v) = pluck_string(child, keys) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Count distinct string values of a field across entries. Returns
/// `[(value, count), ...]` sorted by descending count then by recency.
pub(super) fn scan_evidence_string_counts(
    entries: &[Value],
    keys: &[&str],
) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for entry in entries {
        let mut found: Option<String> = None;
        if let Some(v) = pluck_string(entry, keys) {
            found = Some(v);
        } else {
            for nested in &[
                "evidence",
                "inner_dispatch",
                "inner_result",
                "typed_evidence",
            ] {
                if let Some(child) = entry.get(*nested) {
                    if let Some(v) = pluck_string(child, keys) {
                        found = Some(v);
                        break;
                    }
                }
            }
        }
        if let Some(v) = found {
            if !counts.contains_key(&v) {
                order.push(v.clone());
            }
            *counts.entry(v).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(String, usize)> = order
        .into_iter()
        .map(|k| {
            let c = counts.get(&k).copied().unwrap_or(0);
            (k, c)
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Look for a string-array value under any of the supplied keys. Returns
/// the most-recent entry's value (newest-first) so the inferer reflects
/// the latest run.
pub(super) fn scan_evidence_string_list(entries: &[Value], key: &str) -> Option<Vec<String>> {
    for entry in entries.iter().rev() {
        if let Some(v) = pluck_string_list(entry, key) {
            return Some(v);
        }
        for nested in &[
            "evidence",
            "inner_dispatch",
            "inner_result",
            "typed_evidence",
        ] {
            if let Some(child) = entry.get(*nested) {
                if let Some(v) = pluck_string_list(child, key) {
                    return Some(v);
                }
            }
        }
    }
    None
}

pub(super) fn pluck_string(v: &Value, keys: &[&str]) -> Option<String> {
    let obj = v.as_object()?;
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub(super) fn pluck_string_list(v: &Value, key: &str) -> Option<Vec<String>> {
    let obj = v.as_object()?;
    let arr = obj.get(key)?.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|item| {
            item.as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(String::from)
        })
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}
