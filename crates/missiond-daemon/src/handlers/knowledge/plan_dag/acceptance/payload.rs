use serde_json::Value;

/// wave-17 / task 03 — best-effort detection of an explicit failure
/// signal in an inner-dispatch payload. Returns `Some(detail)` when the
/// payload structurally claims non-success, `None` otherwise.
///
/// Recognised shapes (all conservative — only loud signals count):
///   * `payload.error` is a non-empty string.
///   * `payload.success == false`.
///   * `payload.ok == false`.
///   * `payload.status` ∈ {"error", "failed", "fail"}.
///   * `payload.workstation_dispatch_status` starts with `"skipped_"`
///     or equals `"failed"` (matches the wave-15 substrate's
///     safe-descriptor refusal vocabulary).
pub(super) fn inner_payload_failure_signal(payload: &Value) -> Option<String> {
    let obj = payload.as_object()?;
    if let Some(s) = obj.get("error").and_then(|v| v.as_str()) {
        if !s.trim().is_empty() {
            return Some(format!("error=`{}`", s));
        }
    }
    if let Some(false) = obj.get("success").and_then(|v| v.as_bool()) {
        return Some("success=false".to_string());
    }
    if let Some(false) = obj.get("ok").and_then(|v| v.as_bool()) {
        return Some("ok=false".to_string());
    }
    if let Some(s) = obj.get("status").and_then(|v| v.as_str()) {
        let lc = s.trim().to_ascii_lowercase();
        if matches!(lc.as_str(), "error" | "failed" | "fail") {
            return Some(format!("status=`{}`", s));
        }
    }
    if let Some(s) = obj
        .get("workstation_dispatch_status")
        .and_then(|v| v.as_str())
    {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "failed" || lc.starts_with("skipped_") {
            return Some(format!("workstation_dispatch_status=`{}`", s));
        }
    }
    None
}

/// wave-17 / task 03 — pure helper: locate every required key NOT
/// present in the inner payload. The payload is searched at the
/// top-level object AND inside common nested holders (`evidence`,
/// `typed_evidence`, `inner_result.evidence`) so authors don't have to
/// guess where the substrate stashed the typed evidence. Order of
/// returned missing keys matches `required` for stable test output.
pub(super) fn inner_payload_missing_keys(payload: &Value, required: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    for key in required {
        if !inner_payload_contains_key(payload, key) {
            missing.push(key.clone());
        }
    }
    missing
}

fn inner_payload_contains_key(payload: &Value, key: &str) -> bool {
    match payload {
        Value::Object(map) => {
            if map.contains_key(key) {
                return true;
            }
            // Conservative descent into the well-known nested holders.
            for nested_key in [
                "evidence",
                "typed_evidence",
                "inner_result",
                "inner_dispatch",
                "result",
            ] {
                if let Some(child) = map.get(nested_key) {
                    if inner_payload_contains_key(child, key) {
                        return true;
                    }
                }
            }
            false
        }
        Value::Array(items) => items.iter().any(|v| inner_payload_contains_key(v, key)),
        _ => false,
    }
}
