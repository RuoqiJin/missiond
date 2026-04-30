use super::*;

/// Structured-error code returned when the caller flips `auto_spawn=true`
/// without echoing `workstation_proposal_hash`. Pinned as a constant so
/// dashboards can grep for the load-bearing failure reason without
/// inspecting the gate block. Mirrors the wave-22 / task 03 pattern
/// (`APPLY_GATE_MISSING_PROPOSAL_HASH`).
pub(crate) const AUTO_SPAWN_MISSING_PROPOSAL_HASH: &str = "AUTO_SPAWN_MISSING_PROPOSAL_HASH";

/// Structured-error code returned when the caller-supplied
/// `workstation_proposal_hash` does not match the bundle's deterministic
/// hash. The strongest "the proposals you saw are not the proposals we
/// have" signal — surfacing it BEFORE the spawn substrate runs is the
/// contract's hard requirement.
pub(crate) const AUTO_SPAWN_PROPOSAL_HASH_MISMATCH: &str = "AUTO_SPAWN_PROPOSAL_HASH_MISMATCH";

/// Structured-error code returned when the caller flips `auto_spawn=true`
/// but supplies a non-bool / non-string shape for the gate args. Caller
/// typos must fail fast so they can never silently degrade to skip.
/// Mirrors `APPLY_GATE_INVALID_PARAM` (wave-22 / task 03).
pub(crate) const AUTO_SPAWN_INVALID_PARAM: &str = "AUTO_SPAWN_INVALID_PARAM";

/// Caller-supplied opt-in inputs for the wave-22 / task 05 auto-spawn
/// gate. Strict-shape: `auto_spawn` / `caller_approved` /
/// `preflight_status_acceptable` are bool-only (literal strings `"true"`
/// / `"false"` are rejected so a typo cannot silently flip the gate);
/// `workstation_proposal_hash` and `task_contract_path` are string-only.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkstationAutoSpawnInput {
    /// Caller opted into the gate (`auto_spawn=true`).
    pub auto_spawn: bool,
    /// Caller-supplied SHA-256 hash (32 hex chars) of the bundle they
    /// inspected. Required when `auto_spawn=true`.
    pub proposal_hash: Option<String>,
    /// Caller's second opt-in flag confirming human intent.
    /// Required-truthy when `auto_spawn=true`.
    pub caller_approved: bool,
    /// Caller-supplied `task_contract_path` (relative against the
    /// project root or absolute). Required when `auto_spawn=true`.
    pub task_contract_path: Option<String>,
    /// Caller's acknowledgement that hooks / preflight state is
    /// acceptable. Required-truthy when `auto_spawn=true`. The daemon
    /// does NOT run hooks itself — this is the explicit operator
    /// confirmation surface.
    pub preflight_status_acceptable: bool,
    /// True iff the caller explicitly supplied any of the gate fields
    /// (used to differentiate "caller opted out" from "caller never saw
    /// the knob" so the response stays byte-identical for the latter).
    pub explicit: bool,
}

/// Strict pre-flight validator for the wave-22 / task 05 auto-spawn
/// args. Rejects any non-bool / non-string shape so caller typos fail
/// fast with structured errors. Pure / no I/O. Mirrors
/// `parse_llm_approve_apply_gate_input` (wave-22 / task 03).
pub(crate) fn parse_workstation_auto_spawn_input(
    args: &Value,
) -> std::result::Result<WorkstationAutoSpawnInput, (String, String)> {
    let mut input = WorkstationAutoSpawnInput::default();

    let auto_spawn_v = args.get("auto_spawn");
    let hash_v = args.get("workstation_proposal_hash");
    let caller_v = args.get("workstation_caller_approved");
    let path_v = args.get("task_contract_path");
    let preflight_v = args.get("preflight_status_acceptable");
    input.explicit = auto_spawn_v.is_some()
        || hash_v.is_some()
        || caller_v.is_some()
        || path_v.is_some()
        || preflight_v.is_some();

    if let Some(v) = auto_spawn_v {
        if v.is_null() {
            // null behaves like absent
        } else if let Some(b) = v.as_bool() {
            input.auto_spawn = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "auto_spawn must be a boolean (true|false); got {} \
                     — string `\"true\"` is REJECTED so a typo cannot silently flip the gate",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = hash_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                input.proposal_hash = Some(trimmed.to_string());
            }
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "workstation_proposal_hash must be a string (SHA-256 hex truncated to 32 chars); \
                     got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = caller_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(b) = v.as_bool() {
            input.caller_approved = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "workstation_caller_approved must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = path_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                input.task_contract_path = Some(trimmed.to_string());
            }
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "task_contract_path must be a string (relative against project root or absolute); \
                     got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    if let Some(v) = preflight_v {
        if v.is_null() {
            // treat as absent
        } else if let Some(b) = v.as_bool() {
            input.preflight_status_acceptable = b;
        } else {
            return Err((
                AUTO_SPAWN_INVALID_PARAM.to_string(),
                format!(
                    "preflight_status_acceptable must be a boolean (true|false); got {}",
                    proposal_json_kind(v)
                ),
            ));
        }
    }

    Ok(input)
}
