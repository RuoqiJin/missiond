use serde_json::{json, Value};

/// wave-17 / task 07 — workstation-dispatch scoped-commit handoff defaults.
///
/// These constants are surfaced verbatim on every dispatch response under
/// `scoped_commit_required` / `scoped_commit_policy`. They pin the policy so
/// downstream callers (Claude / agent-team / observers) can assert the
/// invariant without re-reading the brief text.
///
/// Important: the `enforced-on-complete` value describes the *brief contract*,
/// NOT the daemon-level `mission_execution(action=complete)` default. The
/// legacy `enforce_scoped_commit` flag still defaults to `false` so callers
/// who wire completions outside the workstation-dispatch pipeline keep their
/// audit-only behaviour. The brief explicitly instructs the worker to opt
/// into enforcement when calling completion.
pub(crate) const SCOPED_COMMIT_REQUIRED: bool = true;
pub(crate) const SCOPED_COMMIT_POLICY: &str = "enforced-on-complete";

/// Outcome of a workstation-dispatch evaluation. The variants are surfaced
/// directly into the response so callers can route on
/// `workstation_dispatch_status` without re-walking the inner payload.
#[derive(Debug)]
pub(crate) enum WorkstationDispatchOutcome {
    /// Inner `mission_task_delegate` returned non-error. `inner_payload`
    /// carries the delegated task's response.
    ///
    /// wave-20 / task 04 — `task_contract_source_path` carries the
    /// resolved on-disk task-contract v1 path WHEN the dispatch consumed
    /// the contract directly (machine-driven mode). It is `None` for
    /// the legacy / rendered path so the response stays byte-compatible
    /// with wave-15..19.
    Dispatched {
        task_brief: String,
        task_brief_path: Option<String>,
        task_contract_source_path: Option<String>,
        evidence_path: Option<String>,
        evidence_error: Option<String>,
        inner_payload: Value,
    },
    /// Inner handler returned an error result; we surface it verbatim and
    /// do NOT mark plan executing — caller decides whether to retry.
    InnerError {
        task_brief: String,
        inner_payload: Value,
    },
    /// dry_run: brief built, nothing dispatched, no evidence written.
    DryRun { task_brief: String },
    /// Pre-flight failed (project root unresolved, wrong target, etc).
    /// We refuse to dispatch and refuse to silently fall back to prompt
    /// mode — the descriptor explains why so the caller can fix and retry.
    SafeDescriptor {
        reason: SafeDescriptorReason,
        task_brief: Option<String>,
    },
}

/// Why we refused to dispatch. Each variant maps to a deterministic
/// `workstation_dispatch_status` string the caller can match on.
#[derive(Debug, Clone)]
pub(crate) enum SafeDescriptorReason {
    /// Caller pointed at a non-`mission_task_delegate` target — workstation
    /// dispatch only ever wraps the task_delegate substrate.
    UnsupportedTarget(String),
    /// Project root could not be resolved (no signal / unknown id /
    /// relative cwd / cwd outside any registered project).
    ProjectRootUnresolved(String),
    /// Caller did not provide an objective and the plan hints were empty,
    /// so the brief would have been content-free.
    MissingObjective,
    /// wave-19 / task 07 — `task_contract_path` was supplied but the file
    /// is missing, unreadable, or fails the narrow task-contract v1
    /// parse. We refuse to fall back to the legacy natural-language path
    /// because the contract is the SSOT — silently downgrading would
    /// hide an authoring bug. Carries the absolute path + a typed reason.
    MalformedTaskContract { path: String, reason: String },
    /// The delegated worker is expected to close out through
    /// `mission_execution(action=complete)`, so live dispatch first
    /// pre-opens that companion audit log. If this fails, dispatch is
    /// refused before a worker receives an impossible handoff contract.
    CompletionLogUnavailable(String),
}

impl SafeDescriptorReason {
    pub(crate) fn status(&self) -> &'static str {
        match self {
            SafeDescriptorReason::UnsupportedTarget(_) => "skipped_unsupported_target",
            SafeDescriptorReason::ProjectRootUnresolved(_) => "skipped_project_root_unresolved",
            SafeDescriptorReason::MissingObjective => "skipped_missing_objective",
            SafeDescriptorReason::MalformedTaskContract { .. } => "skipped_malformed_task_contract",
            SafeDescriptorReason::CompletionLogUnavailable(_) => {
                "skipped_completion_log_unavailable"
            }
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            SafeDescriptorReason::UnsupportedTarget(t) => {
                format!(
                    "workstation-dispatch v0 only wraps `mission_task_delegate`, got `{}`",
                    t
                )
            }
            SafeDescriptorReason::ProjectRootUnresolved(r) => r.clone(),
            SafeDescriptorReason::MissingObjective => {
                "workstation-dispatch v0 requires either an explicit objective or a plan hint; \
                 refusing to dispatch a content-free task brief"
                    .to_string()
            }
            SafeDescriptorReason::MalformedTaskContract { path, reason } => {
                format!(
                    "task_contract_path `{}` is malformed: {} — refusing to fall back to the \
                     legacy natural-language brief because the contract is the SSOT",
                    path, reason
                )
            }
            SafeDescriptorReason::CompletionLogUnavailable(reason) => reason.clone(),
        }
    }
}

impl WorkstationDispatchOutcome {
    pub(crate) fn status(&self) -> &'static str {
        match self {
            WorkstationDispatchOutcome::Dispatched { .. } => "dispatched",
            WorkstationDispatchOutcome::InnerError { .. } => "inner_returned_error",
            WorkstationDispatchOutcome::DryRun { .. } => "dry_run_no_dispatch",
            WorkstationDispatchOutcome::SafeDescriptor { reason, .. } => reason.status(),
        }
    }
}

/// Render the workstation-dispatch outcome into the JSON object plan.rs /
/// plan_dag.rs splice into their response. Centralised so both call sites
/// emit the same field names.
pub(crate) fn outcome_to_response_fields(
    outcome: &WorkstationDispatchOutcome,
    dispatch_strategy: &str,
) -> Value {
    let mut m = serde_json::Map::new();
    m.insert(
        "workstation_dispatch_status".to_string(),
        json!(outcome.status()),
    );
    m.insert("dispatch_strategy".to_string(), json!(dispatch_strategy));
    // Wave-17 / Task 07 — every dispatch (live, dry-run, inner-error,
    // and safe-descriptor) carries the scoped-commit policy contract so
    // observers can assert the invariant without parsing the brief text.
    // The policy is fixed at the workstation-dispatch layer; legacy
    // callers of `mission_execution(action=complete)` keep their default
    // `enforce_scoped_commit=false` behaviour untouched.
    m.insert(
        "scoped_commit_required".to_string(),
        json!(SCOPED_COMMIT_REQUIRED),
    );
    m.insert(
        "scoped_commit_policy".to_string(),
        json!(SCOPED_COMMIT_POLICY),
    );
    match outcome {
        WorkstationDispatchOutcome::Dispatched {
            task_brief,
            task_brief_path,
            task_contract_source_path,
            evidence_path,
            evidence_error,
            inner_payload,
        } => {
            m.insert(
                "task_brief_preview".to_string(),
                json!(truncate_brief_preview(task_brief)),
            );
            if let Some(p) = task_brief_path {
                m.insert("task_brief_path".to_string(), json!(p));
            }
            // wave-20 / task 04 — when the dispatch consumed the
            // task-contract v1 file directly, surface the resolved
            // source path so observers (CI, PR review, audit) can prove
            // the Lisp was load-bearing rather than the rendered
            // markdown brief. Absent on the legacy / rendered path so
            // the wire shape stays byte-compatible with wave-15..19.
            if let Some(p) = task_contract_source_path {
                m.insert("task_contract_source_path".to_string(), json!(p));
            }
            if let Some(p) = evidence_path {
                m.insert("evidence_path".to_string(), json!(p));
            }
            if let Some(e) = evidence_error {
                m.insert("evidence_error".to_string(), json!(e));
            }
            // wave-47 / task 01 — surface a stable top-level identifier for
            // the BoardTask the substrate just delegated. The inner
            // `mission_task_delegate` response embeds the full DB BoardTask
            // row under `task_id` (the variable was named after its
            // semantic role rather than the wire type — see
            // crates/missiond-daemon/src/handlers/compute/task_delegate.rs::handle
            // where `task_id = state.store.create_board_task(...)` returns
            // the full row, then is serialized as `"task_id": <row>`). The
            // BoardTask UUID lives at `task_id.id` (camelCase per the
            // store's serde derives). Project that UUID to
            // `delegated_board_task_id` so observers (and the wave-47 v3
            // request-flow real-dispatch smoke) can pin a single stable
            // field name without parsing a nested struct.
            if let Some(id) = extract_inner_board_task_id(&inner_payload) {
                m.insert("delegated_board_task_id".to_string(), json!(id));
            }
            m.insert("inner_result".to_string(), inner_payload.clone());
        }
        WorkstationDispatchOutcome::InnerError {
            task_brief,
            inner_payload,
        } => {
            m.insert(
                "task_brief_preview".to_string(),
                json!(truncate_brief_preview(task_brief)),
            );
            m.insert("inner_result".to_string(), inner_payload.clone());
        }
        WorkstationDispatchOutcome::DryRun { task_brief } => {
            m.insert(
                "task_brief_preview".to_string(),
                json!(truncate_brief_preview(task_brief)),
            );
        }
        WorkstationDispatchOutcome::SafeDescriptor { reason, task_brief } => {
            m.insert(
                "workstation_dispatch_reason".to_string(),
                json!(reason.detail()),
            );
            if let Some(brief) = task_brief {
                m.insert(
                    "task_brief_preview".to_string(),
                    json!(truncate_brief_preview(brief)),
                );
            }
        }
    }
    Value::Object(m)
}

/// wave-47 / task 01 — robustly extract the delegated BoardTask UUID from
/// the inner `mission_task_delegate` payload. Tolerates two shapes:
///   * `task_id`: full BoardTask object with a `.id` UUID string (current
///     daemon behaviour — see compute/task_delegate.rs::handle which feeds
///     the BoardTask returned by `create_board_task` straight into the
///     response under the `task_id` key);
///   * `task_id`: bare UUID string (defensive fallback in case
///     compute/task_delegate.rs is later tightened to surface a string).
/// Returns `None` when neither shape matches so the caller can fall back
/// to inspecting `inner_result` itself; `outcome_to_response_fields`
/// always emits the full `inner_result` alongside, so this projection is
/// purely an ergonomic affordance, never a load-bearing schema change.
pub(crate) fn extract_inner_board_task_id(inner_payload: &Value) -> Option<String> {
    let task_id = inner_payload.get("task_id")?;
    if let Some(s) = task_id.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(s) = task_id.get("id").and_then(|v| v.as_str()) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Trim the brief for the response preview field. The full text already
/// reaches the inner handler via `objective`; we just want a humane
/// preview on the response.
pub(crate) fn truncate_brief_preview(brief: &str) -> String {
    const MAX: usize = 800;
    if brief.len() <= MAX {
        return brief.to_string();
    }
    let mut end = MAX;
    while end > 0 && !brief.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &brief[..end])
}
