use super::{parse_backend_registry, FALLBACK_BACKEND, SCHEMA};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// wave25-03: trace-index threshold mirrors scripts/recommend-task-backend.mjs
/// `RICH_TRACE_THRESHOLD = 5`. Keep this constant and the Node CLI in lock-
/// step. The threshold counts the MAX of per-task vs per-backend events.
pub(super) const RICH_TRACE_THRESHOLD: u64 = 5;

/// wave25-03 trace-index status flavours surfaced on the recommendation
/// block. `Absent` is the default (no path supplied) and is observable on
/// the wire as the absence of `trace_index_*` fields.
#[derive(Debug, Clone)]
pub(super) enum TraceIndexInfo {
    /// No trace-index path was supplied. Block does NOT carry any
    /// `trace_index_*` fields (preserves wave24-04 byte-shape for
    /// callers that opted out).
    Absent,
    /// Path was supplied; file read + parsed; `by_task` / `by_backend`
    /// available for confidence scoring.
    Used {
        path: String,
        task_events: serde_json::Map<String, Value>,
        backend_events: serde_json::Map<String, Value>,
    },
    /// Path was supplied but the file does not exist on disk.
    Missing { path: String, warning: String },
    /// Path was supplied; std::fs::read_to_string returned an I/O error
    /// other than NotFound.
    Unreadable { path: String, warning: String },
    /// Path was supplied; serde_json failed to parse OR the top-level
    /// shape is not a JSON object.
    Malformed { path: String, warning: String },
}

pub(super) fn load_trace_index(input: Option<&str>) -> TraceIndexInfo {
    let Some(path_str) = input else {
        return TraceIndexInfo::Absent;
    };
    let path = path_str.to_string();
    let resolved = resolve_policy_path(&path); // same resolution rule
    let raw = match std::fs::read_to_string(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let warning = format!("trace-index read failed: {}", e);
            return if e.kind() == std::io::ErrorKind::NotFound {
                TraceIndexInfo::Missing { path, warning }
            } else {
                TraceIndexInfo::Unreadable { path, warning }
            };
        }
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return TraceIndexInfo::Malformed {
                path,
                warning: format!("trace-index JSON parse failed: {}", e),
            };
        }
    };
    let map = match value.as_object() {
        Some(m) => m,
        None => {
            return TraceIndexInfo::Malformed {
                path,
                warning: "trace-index top-level value is not a JSON object".to_string(),
            };
        }
    };
    let task_events = match map.get("by_task") {
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return TraceIndexInfo::Malformed {
                path,
                warning: "trace-index `by_task` is not a JSON object".to_string(),
            };
        }
        None => serde_json::Map::new(),
    };
    let backend_events = match map.get("by_backend") {
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return TraceIndexInfo::Malformed {
                path,
                warning: "trace-index `by_backend` is not a JSON object".to_string(),
            };
        }
        None => serde_json::Map::new(),
    };
    TraceIndexInfo::Used {
        path,
        task_events,
        backend_events,
    }
}

pub(super) fn events_for_task(by_task: &serde_json::Map<String, Value>, task_id: &str) -> u64 {
    bucket_events(by_task, task_id)
}

pub(super) fn events_for_backend(
    by_backend: &serde_json::Map<String, Value>,
    backend: &str,
) -> u64 {
    bucket_events(by_backend, backend)
}

fn bucket_events(map: &serde_json::Map<String, Value>, key: &str) -> u64 {
    map.get(key)
        .and_then(|v| v.get("events"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

pub(super) fn resolve_policy_path(input: &str) -> PathBuf {
    let p = Path::new(input);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        // The daemon runs out of the repo root in production; in
        // tests CWD points at the crate dir which still resolves
        // correctly because the policy path includes `.missiond/`.
        // Falling back to verbatim `input` here keeps the helper
        // free of repo-root-detection logic.
        PathBuf::from(input)
    }
}

pub(super) fn error_block(
    policy_source: &str,
    message: &str,
    trace: &TraceIndexInfo,
    registry: &BackendRegistryInfo,
    recommended_backend: Option<&str>,
    confidence: &str,
) -> Value {
    let mut block = json!({
        "applied": false,
        "confidence": "low",
        "policy_source": policy_source,
        "reasons": [format!("error: {}", message)],
        "recommended_backend": FALLBACK_BACKEND,
        "schema": SCHEMA,
        "status": "error",
    });
    attach_trace_index_fields(&mut block, trace);
    attach_backend_readiness_fields(
        &mut block,
        registry,
        recommended_backend.unwrap_or(FALLBACK_BACKEND),
        "error",
        confidence,
    );
    block
}

pub(super) fn rejected_block(
    policy_source: &str,
    message: &str,
    trace: &TraceIndexInfo,
    registry: &BackendRegistryInfo,
    recommended_backend: Option<&str>,
    confidence: &str,
) -> Value {
    let mut block = json!({
        "applied": false,
        "confidence": "low",
        "policy_source": policy_source,
        "reasons": [format!("rejected: {}", message)],
        "recommended_backend": FALLBACK_BACKEND,
        "schema": SCHEMA,
        "status": "rejected",
    });
    attach_trace_index_fields(&mut block, trace);
    attach_backend_readiness_fields(
        &mut block,
        registry,
        recommended_backend.unwrap_or(FALLBACK_BACKEND),
        "rejected",
        confidence,
    );
    block
}

pub(super) fn computed_block(
    policy_source: &str,
    backend: &str,
    confidence: &str,
    reasons: Vec<String>,
    trace: &TraceIndexInfo,
    registry: &BackendRegistryInfo,
) -> Value {
    let mut block = json!({
        "applied": false,
        "confidence": confidence,
        "policy_source": policy_source,
        "reasons": reasons,
        "recommended_backend": backend,
        "schema": SCHEMA,
        "status": "computed",
    });
    attach_trace_index_fields(&mut block, trace);
    attach_backend_readiness_fields(&mut block, registry, backend, "computed", confidence);
    block
}

// -------------------------------------------------------------------
// wave26-03: optional backend-readiness registry consumption.
//
// The registry seed at .missiond/router/router-backend-registry-v1.lisp
// (top form: `(router-backend-registry <id> :schema ... :version ...
// (backend :id ... :readiness_status ... :runtime_allowed ...
//          :apply_blockers [...] ...))`) is read OPTIONALLY when
// `router_backend_registry_path` is supplied AND mode=dry_run. The
// daemon extracts ONLY the four fields it needs per backend entry —
// every other key (`:substrate`, `:non-goals`, `:notes`, `:owner`,
// `:adapter_path`) is ignored gracefully so the wave26-01 schema can
// grow without forcing a daemon update. Failure modes are non-fatal:
// dispatch always continues; only the apply-eligibility surface
// degrades.
// -------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) struct BackendEntry {
    pub(super) id: String,
    pub(super) readiness_status: String,
    pub(super) runtime_allowed: bool,
    pub(super) apply_blockers: Vec<String>,
}

/// Backend-registry status flavours surfaced on the recommendation
/// block. `Absent` is the default (no path supplied) and is observable
/// on the wire as the absence of every `backend_*` field.
#[derive(Debug, Clone)]
pub(super) enum BackendRegistryInfo {
    /// No registry path was supplied. Block does NOT carry any
    /// `backend_*` field (preserves wave24-04 / wave25-03 byte-shape
    /// for callers that opted out).
    Absent,
    /// Path was supplied; file read + parsed; backend entries indexed
    /// by id for O(1) join against the recommended backend.
    Used {
        path: String,
        backends: Vec<BackendEntry>,
    },
    /// Path was supplied but the file does not exist on disk.
    Missing { path: String, warning: String },
    /// Path was supplied; std::fs::read_to_string returned an I/O error
    /// other than NotFound.
    Unreadable { path: String, warning: String },
    /// Path was supplied; the Lisp parser failed OR the top-level shape
    /// did not match `(router-backend-registry ...)` OR a backend entry
    /// was missing a required field / had an enum violation.
    Malformed { path: String, warning: String },
}

pub(super) fn load_backend_registry(input: Option<&str>) -> BackendRegistryInfo {
    let Some(path_str) = input else {
        return BackendRegistryInfo::Absent;
    };
    let path = path_str.to_string();
    let resolved = resolve_policy_path(&path);
    let raw = match std::fs::read_to_string(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let warning = format!("backend-registry read failed: {}", e);
            return if e.kind() == std::io::ErrorKind::NotFound {
                BackendRegistryInfo::Missing { path, warning }
            } else {
                BackendRegistryInfo::Unreadable { path, warning }
            };
        }
    };
    match parse_backend_registry(&raw) {
        Ok(backends) => BackendRegistryInfo::Used { path, backends },
        Err(msg) => BackendRegistryInfo::Malformed {
            path,
            warning: format!("backend-registry parse failed: {}", msg),
        },
    }
}

/// wave26-03: splice the optional `backend_*` fields onto a recommendation
/// block. `Absent` emits NO fields at all (preserves wave24-04 / wave25-03
/// byte-shape for callers that opted out). All other variants emit
/// `backend_registry_path` + `backend_registry_status`; degraded variants
/// additionally emit `backend_warning`. When `Used` AND the recommended
/// backend is present in the registry, the block also surfaces
/// `backend_readiness_status` + `backend_runtime_allowed` +
/// `router_apply_eligible` + `router_apply_blockers`. When `Used` AND
/// the backend is missing, `backend_registry_status="unknown_backend"`,
/// `backend_readiness_status="unknown"`, `router_apply_eligible=false`.
pub(super) fn attach_backend_readiness_fields(
    block: &mut Value,
    registry: &BackendRegistryInfo,
    recommended_backend: &str,
    status: &str,
    confidence: &str,
) {
    let Some(map) = block.as_object_mut() else {
        return;
    };
    match registry {
        BackendRegistryInfo::Absent => {
            // Intentionally emit NOTHING — preserves the byte-shape
            // for callers that did not opt in to wave26-03.
        }
        BackendRegistryInfo::Missing { path, warning } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            map.insert(
                "backend_registry_status".to_string(),
                Value::String("missing".to_string()),
            );
            map.insert(
                "backend_warning".to_string(),
                Value::String(warning.clone()),
            );
            map.insert("router_apply_eligible".to_string(), Value::Bool(false));
            map.insert(
                "router_apply_blockers".to_string(),
                Value::Array(vec![Value::String(
                    "backend registry file is missing".to_string(),
                )]),
            );
        }
        BackendRegistryInfo::Unreadable { path, warning } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            map.insert(
                "backend_registry_status".to_string(),
                Value::String("unreadable".to_string()),
            );
            map.insert(
                "backend_warning".to_string(),
                Value::String(warning.clone()),
            );
            map.insert("router_apply_eligible".to_string(), Value::Bool(false));
            map.insert(
                "router_apply_blockers".to_string(),
                Value::Array(vec![Value::String(
                    "backend registry file is unreadable".to_string(),
                )]),
            );
        }
        BackendRegistryInfo::Malformed { path, warning } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            map.insert(
                "backend_registry_status".to_string(),
                Value::String("malformed".to_string()),
            );
            map.insert(
                "backend_warning".to_string(),
                Value::String(warning.clone()),
            );
            map.insert("router_apply_eligible".to_string(), Value::Bool(false));
            map.insert(
                "router_apply_blockers".to_string(),
                Value::Array(vec![Value::String(
                    "backend registry file is malformed".to_string(),
                )]),
            );
        }
        BackendRegistryInfo::Used { path, backends } => {
            map.insert(
                "backend_registry_path".to_string(),
                Value::String(path.clone()),
            );
            let matched: Option<&BackendEntry> =
                backends.iter().find(|b| b.id == recommended_backend);
            match matched {
                None => {
                    // Recommended backend absent from registry — surface
                    // unknown_backend status and force eligible=false.
                    map.insert(
                        "backend_registry_status".to_string(),
                        Value::String("unknown_backend".to_string()),
                    );
                    map.insert(
                        "backend_readiness_status".to_string(),
                        Value::String("unknown".to_string()),
                    );
                    map.insert("router_apply_eligible".to_string(), Value::Bool(false));
                    map.insert(
                        "router_apply_blockers".to_string(),
                        Value::Array(vec![Value::String(format!(
                            "recommended_backend `{}` not in registry",
                            recommended_backend
                        ))]),
                    );
                }
                Some(entry) => {
                    map.insert(
                        "backend_registry_status".to_string(),
                        Value::String("used".to_string()),
                    );
                    map.insert(
                        "backend_readiness_status".to_string(),
                        Value::String(entry.readiness_status.clone()),
                    );
                    map.insert(
                        "backend_runtime_allowed".to_string(),
                        Value::Bool(entry.runtime_allowed),
                    );
                    // 6-condition apply-eligibility gate — every miss
                    // contributes a synthetic blocker so operators can
                    // see WHY the gate is closed.
                    let mut blockers: Vec<String> = Vec::new();
                    if status != "computed" {
                        blockers.push(format!("policy status is `{}`; computed required", status));
                    }
                    if confidence != "high" {
                        blockers.push(format!("confidence is `{}`; high required", confidence));
                    }
                    if !entry.runtime_allowed {
                        blockers.push(
                            "backend runtime_allowed is false; runtime-ready adapter required"
                                .to_string(),
                        );
                    }
                    if entry.readiness_status != "runtime-ready" {
                        blockers.push(format!(
                            "backend readiness_status is `{}`; runtime-ready required",
                            entry.readiness_status
                        ));
                    }
                    // Echo the registry's own apply_blockers verbatim
                    // when present (operator should see the registry's
                    // reasons even when the synthetic gate already
                    // closed for another reason).
                    for b in &entry.apply_blockers {
                        blockers.push(b.clone());
                    }
                    let eligible = blockers.is_empty();
                    map.insert("router_apply_eligible".to_string(), Value::Bool(eligible));
                    map.insert(
                        "router_apply_blockers".to_string(),
                        Value::Array(blockers.into_iter().map(Value::String).collect()),
                    );
                }
            }
        }
    }
}

/// wave25-03: splice the optional `trace_index_path` / `trace_index_status`
/// / `trace_index_warning` fields onto a recommendation block. `Absent`
/// emits NO fields at all (preserves wave24-04 byte-shape for callers
/// that opted out). All other variants emit `trace_index_path` +
/// `trace_index_status`; degraded variants additionally emit
/// `trace_index_warning`.
pub(super) fn attach_trace_index_fields(block: &mut Value, trace: &TraceIndexInfo) {
    let Some(map) = block.as_object_mut() else {
        return;
    };
    match trace {
        TraceIndexInfo::Absent => {
            // Intentionally emit NOTHING — keeps wave24-04 byte-shape
            // for callers that did not opt in to wave25-03.
        }
        TraceIndexInfo::Used { path, .. } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("used".to_string()),
            );
        }
        TraceIndexInfo::Missing { path, warning } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("missing".to_string()),
            );
            map.insert(
                "trace_index_warning".to_string(),
                Value::String(warning.clone()),
            );
        }
        TraceIndexInfo::Unreadable { path, warning } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("unreadable".to_string()),
            );
            map.insert(
                "trace_index_warning".to_string(),
                Value::String(warning.clone()),
            );
        }
        TraceIndexInfo::Malformed { path, warning } => {
            map.insert("trace_index_path".to_string(), Value::String(path.clone()));
            map.insert(
                "trace_index_status".to_string(),
                Value::String("malformed".to_string()),
            );
            map.insert(
                "trace_index_warning".to_string(),
                Value::String(warning.clone()),
            );
        }
    }
}
