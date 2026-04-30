use super::*;

/// Wrap the legacy `record_evidence` payload (`{"evidence": <opaque>}`) so
/// callers that want to migrate can stamp source/kind/schema_version on top
/// without losing the historical opaque body. Returns a JSON object the
/// caller passes straight to `append_plan_evidence_entry`.
///
/// `evidence_kind` defaults to [`kind::NOTE`] when caller does not pass one.
/// `source` defaults to [`source::RECORD_EVIDENCE_MANUAL`].
///
/// This keeps backward compatibility: the historical `{"evidence": ...}`
/// payload survives intact under the `evidence` key; the new top-level
/// stamps (`source`, `kind`, `schema_version`) are additive and existing
/// readers ignore unknown fields.
pub(crate) fn wrap_legacy_record_evidence(
    inner: Value,
    evidence_kind: Option<&str>,
    source_override: Option<&str>,
) -> Value {
    let mut m = Map::new();
    m.insert(
        "schema_version".to_string(),
        Value::String(EVIDENCE_SCHEMA_VERSION.to_string()),
    );
    m.insert(
        "source".to_string(),
        Value::String(
            source_override
                .unwrap_or(source::RECORD_EVIDENCE_MANUAL)
                .to_string(),
        ),
    );
    m.insert(
        "kind".to_string(),
        Value::String(evidence_kind.unwrap_or(kind::NOTE).to_string()),
    );
    m.insert("evidence".to_string(), inner);
    Value::Object(m)
}
