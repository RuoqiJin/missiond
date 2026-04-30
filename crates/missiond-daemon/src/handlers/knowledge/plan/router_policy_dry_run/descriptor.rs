use super::readiness::BackendRegistryInfo;
use super::{FALLBACK_BACKEND, SCHEMA};
use missiond_core::types::Plan;
use serde_json::Value;

// -------------------------------------------------------------------
// wave27-03: optional router dispatch descriptor surface.
//
// Splice a `router_dispatch_descriptor` sub-object onto an existing
// recommendation block, mirroring the wave27-01 schema
// (`missiond.router-dispatch-descriptor.v1`). The descriptor is a
// PURE PROJECTION of the wave24-04 / wave25-03 / wave26-03 fields
// already on the block — no new file I/O happens here, no backend is
// ever invoked, and dispatch is never altered. Three invariants are
// hard-coded as `Value::Bool` literals so the descriptor cannot be
// mis-promoted to a runtime apply signal:
//
//   - dry_run_only        = Value::Bool(true)   (locked)
//   - runtime_replacement = Value::Bool(false)  (locked)
//   - no_execution        = Value::Bool(true)   (locked)
//
// Registry semantics:
//   * `BackendRegistryInfo::Absent` (i.e. no `router_backend_registry_path`
//     supplied) → top-level `descriptor_status="registry_missing"` is
//     surfaced on the recommendation block; the descriptor BODY is
//     OMITTED so a downstream consumer cannot mistake the absence of
//     readiness for "ready". This matches the wave27-01 schema's
//     refusal to fake readiness.
//   * Any other registry state → emit a structured descriptor body.
//     For degraded states (Missing / Unreadable / Malformed /
//     unknown_backend) the recommendation block already carries the
//     synthetic `router_apply_eligible=false` / `router_apply_blockers`
//     fields plus (for unknown_backend) `backend_readiness_status="unknown"`;
//     the descriptor projects those off the block as-is. For Missing /
//     Unreadable / Malformed the block has no `backend_readiness_status`
//     at all — the descriptor falls back to the synthetic
//     `unknown` enum value (legal per wave27-01 readiness-statuses)
//     and `backend_runtime_allowed=false`.
pub(super) fn attach_router_dispatch_descriptor(
    block: &mut Value,
    plan: &Plan,
    registry: &BackendRegistryInfo,
    policy_path_input: &str,
) {
    let Some(map) = block.as_object_mut() else {
        return;
    };
    // Branch 1: registry path was not supplied. Surface a structured
    // status field on the recommendation block; the descriptor body
    // is intentionally omitted because the wave27-01 schema requires
    // backend_readiness_status / backend_runtime_allowed values that
    // we cannot honestly produce without consulting a registry.
    if matches!(registry, BackendRegistryInfo::Absent) {
        map.insert(
            "descriptor_status".to_string(),
            Value::String("registry_missing".to_string()),
        );
        return;
    }
    // Branch 2: registry path supplied (any state — Used / Missing /
    // Unreadable / Malformed / unknown_backend). Build the descriptor
    // body by reading the wave26-03 fields back off the block. They
    // are guaranteed to be present except in the Missing / Unreadable
    // / Malformed paths where readiness/runtime_allowed are NOT set
    // upstream — for those we fall back to the synthetic `unknown`
    // enum + `false` runtime-allowed.
    let recommended_backend = map
        .get("recommended_backend")
        .and_then(|v| v.as_str())
        .unwrap_or(FALLBACK_BACKEND)
        .to_string();
    let router_confidence = map
        .get("confidence")
        .and_then(|v| v.as_str())
        .unwrap_or("low")
        .to_string();
    let backend_readiness_status = map
        .get("backend_readiness_status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let backend_runtime_allowed = map
        .get("backend_runtime_allowed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_apply_eligible = map
        .get("router_apply_eligible")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_apply_blockers: Vec<Value> = map
        .get("router_apply_blockers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let source_backend_registry_path = registry_path(registry).to_string();

    let mut descriptor = serde_json::Map::new();
    descriptor.insert(
        "schema".to_string(),
        Value::String("missiond.router-dispatch-descriptor.v1".to_string()),
    );
    descriptor.insert(
        "task_id".to_string(),
        Value::String(plan.board_task_id.clone()),
    );
    descriptor.insert(
        "recommended_backend".to_string(),
        Value::String(recommended_backend),
    );
    descriptor.insert(
        "router_confidence".to_string(),
        Value::String(router_confidence),
    );
    descriptor.insert(
        "backend_readiness_status".to_string(),
        Value::String(backend_readiness_status),
    );
    descriptor.insert(
        "backend_runtime_allowed".to_string(),
        Value::Bool(backend_runtime_allowed),
    );
    descriptor.insert(
        "router_apply_eligible".to_string(),
        Value::Bool(router_apply_eligible),
    );
    descriptor.insert(
        "router_apply_blockers".to_string(),
        Value::Array(router_apply_blockers),
    );
    // wave27-01 LOCKED INVARIANTS — these are hard-coded literal bools
    // and MUST NEVER be derived from any other field. If any future
    // change tries to compute these, the descriptor is no longer a
    // safe handoff record (per wave27-01 cross-wave-invariant text).
    descriptor.insert("dry_run_only".to_string(), Value::Bool(true));
    descriptor.insert("runtime_replacement".to_string(), Value::Bool(false));
    descriptor.insert("no_execution".to_string(), Value::Bool(true));
    descriptor.insert(
        "source_recommendation_schema".to_string(),
        Value::String(SCHEMA.to_string()),
    );
    descriptor.insert(
        "source_policy_path".to_string(),
        Value::String(policy_path_input.to_string()),
    );
    descriptor.insert(
        "source_backend_registry_path".to_string(),
        Value::String(source_backend_registry_path),
    );

    map.insert(
        "router_dispatch_descriptor".to_string(),
        Value::Object(descriptor),
    );
}

/// Extract the registry path string from any non-`Absent`
/// `BackendRegistryInfo` variant. `Absent` should never reach this
/// helper (the caller branches before).
fn registry_path(registry: &BackendRegistryInfo) -> &str {
    match registry {
        BackendRegistryInfo::Absent => "",
        BackendRegistryInfo::Used { path, .. }
        | BackendRegistryInfo::Missing { path, .. }
        | BackendRegistryInfo::Unreadable { path, .. }
        | BackendRegistryInfo::Malformed { path, .. } => path.as_str(),
    }
}
