use serde_json::{json, Value};
use std::collections::HashMap;

use super::{DagNode, NodeResult, NodeState};

/// wave-17 / task 03 — typed projection of `:acceptance-mode` for the
/// deterministic acceptance evaluator. Resolved on the parser side so the
/// runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `InnerStatus` — accept when the inner dispatch returned Ok and the
///     inner payload does not carry an explicit non-success status.
///   * `EvidenceKeys` — accept when the inner payload (object or array of
///     objects under `evidence` / `typed_evidence`) contains every key
///     declared in `:acceptance-evidence-keys`.
///   * `Manual`      — never auto-accept; always surface as
///     `acceptance_status="manual_required"` so a human / follow-up
///     pipeline must approve the node.
///
/// `None` (returned by [`DagNode::acceptance_mode_kind`]) means the
/// author did not declare a mode. The evaluator then falls back to the
/// default policy: any declared `:acceptance-commands` triggers
/// `manual_required` (we refuse to run shell from PLAN.lisp); no hints
/// at all preserves the wave-13 succeed-on-dispatch contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcceptanceMode {
    InnerStatus,
    EvidenceKeys,
    Manual,
}

impl AcceptanceMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AcceptanceMode::InnerStatus => "inner_status",
            AcceptanceMode::EvidenceKeys => "evidence_keys",
            AcceptanceMode::Manual => "manual",
        }
    }

    /// Parse a raw `:acceptance-mode` value into a typed mode. Trims and
    /// lowercases the input; `_` and `-` separators are interchangeable
    /// so authors can write either `inner_status` or `inner-status`.
    /// Unknown values yield `None` (the caller — the parser — also pushes
    /// them onto `unsupported_fields` so the typo surfaces in
    /// `node_hint_summary`).
    pub(super) fn parse(raw: &str) -> Option<Self> {
        let lc = raw.trim().to_ascii_lowercase();
        match lc.as_str() {
            "inner_status" | "inner-status" => Some(AcceptanceMode::InnerStatus),
            "evidence_keys" | "evidence-keys" => Some(AcceptanceMode::EvidenceKeys),
            "manual" => Some(AcceptanceMode::Manual),
            _ => None,
        }
    }
}

/// wave-18 / task 03 — typed projection of `:acceptance-requires` for
/// the cross-node acceptance fan-in evaluator. Resolved on the parser
/// side so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `AllSucceeded` — fan-in passes when every node listed in
///                      `:acceptance-depends-on` reached terminal state
///                      `Succeeded`.
///   * `AnySucceeded` — fan-in passes when at least one listed node
///                      reached terminal state `Succeeded`.
///   * `EvidenceKeys` — fan-in passes when the `:acceptance-source-node`'s
///                      `inner_payload` contains every key declared in
///                      `:acceptance-evidence-keys`. Reuses the wave-17
///                      sidecar shape (top-level + well-known nested
///                      holders); the scheduler NEVER re-runs the source
///                      node — it only inspects the recorded payload.
///
/// `None` (returned by [`DagNode::acceptance_requires_kind`]) means the
/// author either did not declare the field OR wrote an unrecognised
/// value. The validator raises a structured error in that case if the
/// node also declared `:acceptance-depends-on`, so the typo cannot
/// silently degrade fan-in to "no gate".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcceptanceRequires {
    AllSucceeded,
    AnySucceeded,
    EvidenceKeys,
}

impl AcceptanceRequires {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AcceptanceRequires::AllSucceeded => "all_succeeded",
            AcceptanceRequires::AnySucceeded => "any_succeeded",
            AcceptanceRequires::EvidenceKeys => "evidence_keys",
        }
    }

    /// Parse a raw `:acceptance-requires` value. Trims + lowercases;
    /// `_` and `-` separators are interchangeable so authors can write
    /// either `all_succeeded` or `all-succeeded`. Unknown values yield
    /// `None` so the parser can land them in `unsupported_fields` AND
    /// the validator can raise a structured error instead of silently
    /// degrading fan-in to a no-op.
    pub(super) fn parse(raw: &str) -> Option<Self> {
        let lc = raw.trim().to_ascii_lowercase();
        match lc.as_str() {
            "all_succeeded" | "all-succeeded" => Some(AcceptanceRequires::AllSucceeded),
            "any_succeeded" | "any-succeeded" => Some(AcceptanceRequires::AnySucceeded),
            "evidence_keys" | "evidence-keys" => Some(AcceptanceRequires::EvidenceKeys),
            _ => None,
        }
    }
}

/// wave-17 / task 03 — outcome of the deterministic acceptance phase.
/// Drives whether a successful dispatch becomes `Succeeded`, `Failed`,
/// or `Paused (manual_required)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcceptanceStatus {
    /// Author declared no acceptance hints — preserve the wave-13
    /// succeed-on-dispatch contract.
    NotEvaluated,
    /// Acceptance evaluator approved the run.
    Accepted,
    /// Acceptance evaluator refused (e.g. evidence_keys missing).
    Rejected,
    /// Acceptance cannot be proven without human input (manual mode, or
    /// declared commands without a safe evaluator). Node pauses; the
    /// scheduler MUST NOT execute any declared shell commands.
    ManualRequired,
}

impl AcceptanceStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            AcceptanceStatus::NotEvaluated => "not_evaluated",
            AcceptanceStatus::Accepted => "accepted",
            AcceptanceStatus::Rejected => "rejected",
            AcceptanceStatus::ManualRequired => "manual_required",
        }
    }
}

/// wave-17 / task 03 — pure result of evaluating a node's acceptance
/// hints. Carries every field the response and evidence rows surface so
/// callers don't have to re-derive them from the node + payload.
#[derive(Debug, Clone)]
pub(super) struct AcceptanceEvaluation {
    pub status: AcceptanceStatus,
    /// Resolved typed mode. `None` when the author did not declare
    /// `:acceptance-mode` (or wrote an unrecognised value).
    pub mode: Option<AcceptanceMode>,
    /// Declared acceptance commands surfaced verbatim — NEVER executed.
    /// The evaluator captures them so the response + evidence rows make
    /// the author intent visible to humans / downstream pipelines that
    /// might run them out-of-band.
    pub commands: Vec<String>,
    /// Required evidence keys declared via `:acceptance-evidence-keys`
    /// (only meaningful for `evidence_keys` mode but surfaced regardless
    /// so observers can see the declared contract).
    pub evidence_keys: Vec<String>,
    /// Human-readable explanation of the decision. Always populated.
    pub reason: String,
    /// wave-18 / task 03 — cross-node acceptance fan-in outcome. `None`
    /// when the author did not declare any fan-in hints; `Some(...)`
    /// captures the resolved mode + source nodes + result + reason so
    /// downstream observers can audit the decision without re-deriving
    /// it from the prior nodes' evidence.
    pub fan_in: Option<AcceptanceFanInOutcome>,
}

impl AcceptanceEvaluation {
    /// Convenience: this evaluation produced no acceptance signal at all
    /// (no hints declared). Used by the scheduler to skip the
    /// acceptance-evidence emit and preserve the v2 byte-shape.
    pub(super) fn is_inactive(&self) -> bool {
        matches!(self.status, AcceptanceStatus::NotEvaluated)
            && self.mode.is_none()
            && self.commands.is_empty()
            && self.evidence_keys.is_empty()
            && self.fan_in.is_none()
    }

    /// Project the evaluation as a JSON block suitable for
    /// `node_results[].acceptance` / `evidence.acceptance`. Stable shape
    /// — every field is always present so consumers don't have to
    /// branch on absence. The `fan_in` block is omitted when the
    /// author did not opt into cross-node fan-in so the wave-17
    /// byte-shape is preserved for callers that did not declare it.
    pub(super) fn to_json(&self) -> Value {
        let mut v = json!({
            "status": self.status.as_wire(),
            "mode": self.mode.map(|m| m.as_wire()),
            "commands": self.commands,
            "evidence_keys": self.evidence_keys,
            "reason": self.reason,
        });
        if let Some(f) = &self.fan_in {
            v["fan_in"] = f.to_json();
        }
        v
    }
}

/// wave-18 / task 03 — pure result of evaluating cross-node acceptance
/// fan-in. Always carries the resolved mode + source nodes + decision
/// so observers can audit the gate without re-walking prior nodes'
/// evidence.
#[derive(Debug, Clone)]
pub(super) struct AcceptanceFanInOutcome {
    pub mode: AcceptanceRequires,
    /// Source nodes that participated in this fan-in evaluation, in
    /// the order the author declared them. For `evidence_keys` mode
    /// this is a single-element list (the resolved
    /// `:acceptance-source-node`).
    pub source_nodes: Vec<String>,
    /// `true` iff the fan-in passed (gate satisfied). When `false`,
    /// the parent acceptance evaluation flips its status to `Rejected`.
    pub passed: bool,
    /// Human-readable explanation of the decision. Always populated.
    pub reason: String,
}

impl AcceptanceFanInOutcome {
    pub(super) fn to_json(&self) -> Value {
        json!({
            "mode": self.mode.as_wire(),
            "source_nodes": self.source_nodes,
            "passed": self.passed,
            "reason": self.reason,
        })
    }
}

/// wave-17 / task 03 — pure deterministic acceptance evaluator. NEVER
/// runs shell. Decides one of the four [`AcceptanceStatus`] values based
/// on the node's hints + the inner dispatch payload.
///
/// Decision tree (in order):
///   1. No hints at all (no mode + no commands + no keys) →
///      `NotEvaluated`. The caller preserves the wave-13
///      succeed-on-dispatch contract.
///   2. Mode = `Manual` → `ManualRequired` (always pauses, regardless of
///      payload). Reason: `"manual mode declared"`.
///   3. Mode = `InnerStatus` → `Accepted` iff `dispatch_succeeded` AND
///      the inner payload does not carry an explicit failure status
///      (`success=false`, `error` string, or `status="error"`).
///      Otherwise `Rejected` with a reason explaining the mismatch.
///   4. Mode = `EvidenceKeys` → `Accepted` iff every required key is
///      present in the inner payload's typed-evidence projection;
///      otherwise `Rejected` with the missing-key list. Empty key list
///      degrades to `ManualRequired` (an empty contract cannot prove
///      anything).
///   5. Mode unset but `:acceptance-commands` declared → `ManualRequired`
///      (we refuse to run shell). Reason captures the command count so
///      observers can tell why the gate triggered.
///   6. Otherwise (mode unset, no commands, only stray keys) →
///      `ManualRequired` so the author's typo surfaces loudly.
///
/// `dispatch_succeeded` is the boolean we already computed from the
/// inner classification. The evaluator never re-derives it from the
/// payload — that would risk drifting from the dispatch judgment.
pub(super) fn evaluate_node_acceptance(
    node: &DagNode,
    inner_payload: &Value,
    dispatch_succeeded: bool,
) -> AcceptanceEvaluation {
    let commands =
        super::super::plan::split_lisp_string_list(node.acceptance_commands_raw.as_deref());
    let evidence_keys =
        super::super::plan::split_lisp_string_list(node.acceptance_evidence_keys_raw.as_deref());
    let mode_raw = node.acceptance_mode_raw.as_deref().unwrap_or("").trim();
    let mode = if mode_raw.is_empty() {
        None
    } else {
        AcceptanceMode::parse(mode_raw)
    };

    if mode.is_none() && commands.is_empty() && evidence_keys.is_empty() {
        return AcceptanceEvaluation {
            status: AcceptanceStatus::NotEvaluated,
            mode: None,
            commands,
            evidence_keys,
            reason: "no acceptance hints declared".to_string(),
            fan_in: None,
        };
    }

    // wave-18 / task 03 — when the node opted into cross-node fan-in
    // AND did NOT declare a per-node `:acceptance-mode`, the
    // `:acceptance-evidence-keys` list is owned by the fan-in
    // evaluator (its `evidence_keys` mode reads them off the source
    // node's payload). Surface a `NotEvaluated` per-node decision in
    // that case so `apply_acceptance_fan_in` is the sole decider; the
    // wave-17 "keys without mode → manual_required" warning would
    // otherwise pre-empt fan-in.
    if mode.is_none() && commands.is_empty() && node.has_acceptance_fan_in() {
        return AcceptanceEvaluation {
            status: AcceptanceStatus::NotEvaluated,
            mode: None,
            commands,
            evidence_keys,
            reason: "per-node acceptance deferred to cross-node fan-in evaluator".to_string(),
            fan_in: None,
        };
    }

    match mode {
        Some(AcceptanceMode::Manual) => AcceptanceEvaluation {
            status: AcceptanceStatus::ManualRequired,
            mode,
            commands,
            evidence_keys,
            reason: "acceptance-mode=manual; human approval required".to_string(),
            fan_in: None,
        },
        Some(AcceptanceMode::InnerStatus) => {
            if !dispatch_succeeded {
                return AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "inner_status: dispatch classification was not Ok".to_string(),
                    fan_in: None,
                };
            }
            if let Some(detail) = inner_payload_failure_signal(inner_payload) {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: format!(
                        "inner_status: inner payload reports non-success ({})",
                        detail
                    ),
                    fan_in: None,
                }
            } else {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Accepted,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "inner_status: dispatch Ok and payload carries no error signal"
                        .to_string(),
                    fan_in: None,
                }
            }
        }
        Some(AcceptanceMode::EvidenceKeys) => {
            if evidence_keys.is_empty() {
                return AcceptanceEvaluation {
                    status: AcceptanceStatus::ManualRequired,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "evidence_keys mode declared but :acceptance-evidence-keys is empty"
                        .to_string(),
                    fan_in: None,
                };
            }
            let missing = inner_payload_missing_keys(inner_payload, &evidence_keys);
            if missing.is_empty() {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Accepted,
                    mode,
                    commands,
                    evidence_keys,
                    reason: "evidence_keys: all required keys present in inner payload".to_string(),
                    fan_in: None,
                }
            } else {
                AcceptanceEvaluation {
                    status: AcceptanceStatus::Rejected,
                    mode,
                    commands,
                    evidence_keys,
                    reason: format!("evidence_keys: missing required keys {:?}", missing),
                    fan_in: None,
                }
            }
        }
        None => {
            // Mode unset but the author declared SOME acceptance hint.
            // We refuse to execute shell from PLAN.lisp, so the only
            // safe default is to surface the gate as manual_required.
            let reason = if !commands.is_empty() {
                format!(
                    "acceptance commands declared ({} item(s)) without :acceptance-mode; \
                     PLAN DAG never runs shell — manual approval required",
                    commands.len()
                )
            } else {
                format!(
                    "acceptance evidence keys declared ({} item(s)) without :acceptance-mode; \
                     manual approval required",
                    evidence_keys.len()
                )
            };
            AcceptanceEvaluation {
                status: AcceptanceStatus::ManualRequired,
                mode,
                commands,
                evidence_keys,
                reason,
                fan_in: None,
            }
        }
    }
}

/// wave-18 / task 03 — apply cross-node acceptance fan-in on top of the
/// per-node evaluation. Pure: never touches the bus, never executes
/// shell, only inspects the prior nodes' terminal lifecycle state and
/// recorded `inner_payload`. Runs AFTER `evaluate_node_acceptance`; the
/// per-node status acts as a precondition:
///
///   * `NotEvaluated` (no per-node hints) — fan-in still runs because
///     `:acceptance-depends-on` is itself an opt-in. Pass flips status
///     to `Accepted`; fail flips it to `Rejected`.
///   * `Accepted`     — fan-in pass keeps `Accepted`; fail flips to
///                       `Rejected`.
///   * `Rejected` / `ManualRequired` — the per-node decision dominates.
///                       Fan-in is recorded for audit but does NOT
///                       override the parent decision (we don't promote
///                       a rejected node to accepted, and we don't
///                       de-pause a manual_required node).
///
/// `prior_results` is the scheduler's `results_by_id` snapshot keyed by
/// node id; each entry's `state` and `inner_payload` are the source of
/// truth. Missing source entries (which the validator forbids at build
/// time) collapse to a fan-in failure with a loud reason — defence in
/// depth in case the scheduler ever calls this without the validator.
pub(super) fn apply_acceptance_fan_in(
    base: AcceptanceEvaluation,
    node: &DagNode,
    prior_results: &HashMap<String, &NodeResult>,
) -> AcceptanceEvaluation {
    if !node.has_acceptance_fan_in() {
        return base;
    }
    // SAFETY: `has_acceptance_fan_in` already proved both halves are
    // present + recognised, so the unwraps below cannot fire.
    let mode = node.acceptance_requires_kind().expect(
        "has_acceptance_fan_in implies acceptance_requires_kind() is Some — \
         validator must have raised earlier",
    );
    let source_nodes: Vec<String> = node.acceptance_depends_on.clone();

    // Per-node evaluation must dominate when it already produced a
    // terminal "do not accept" signal. We still record the fan-in for
    // audit so observers can see what the gate would have decided.
    let parent_dominates = matches!(
        base.status,
        AcceptanceStatus::Rejected | AcceptanceStatus::ManualRequired
    );

    let outcome = match mode {
        AcceptanceRequires::AllSucceeded => {
            let mut failing: Vec<String> = Vec::new();
            for id in &source_nodes {
                let succeeded = prior_results
                    .get(id)
                    .map(|r| matches!(r.state, NodeState::Succeeded))
                    .unwrap_or(false);
                if !succeeded {
                    failing.push(id.clone());
                }
            }
            if failing.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: true,
                    reason: format!(
                        "all_succeeded: every source node ({}) reached succeeded",
                        source_nodes.len()
                    ),
                }
            } else {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: format!("all_succeeded: source node(s) not succeeded: {:?}", failing),
                }
            }
        }
        AcceptanceRequires::AnySucceeded => {
            let mut succeeded_any = false;
            for id in &source_nodes {
                if let Some(r) = prior_results.get(id) {
                    if matches!(r.state, NodeState::Succeeded) {
                        succeeded_any = true;
                        break;
                    }
                }
            }
            if succeeded_any {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: true,
                    reason: "any_succeeded: at least one source node reached succeeded".to_string(),
                }
            } else {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: format!(
                        "any_succeeded: no source node ({}) reached succeeded",
                        source_nodes.len()
                    ),
                }
            }
        }
        AcceptanceRequires::EvidenceKeys => {
            // Validator guarantees `acceptance_source_node` is set AND
            // present in `acceptance_depends_on` AND in the plan, but
            // we defend in depth — a missing entry fails the gate
            // loudly instead of silently passing.
            let source_id = node.acceptance_source_node.clone().unwrap_or_default();
            let single_source = vec![source_id.clone()];
            let keys = super::super::plan::split_lisp_string_list(
                node.acceptance_evidence_keys_raw.as_deref(),
            );
            if source_id.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: source_nodes.clone(),
                    passed: false,
                    reason: "evidence_keys: :acceptance-source-node is missing".to_string(),
                }
            } else if keys.is_empty() {
                AcceptanceFanInOutcome {
                    mode,
                    source_nodes: single_source,
                    passed: false,
                    reason: "evidence_keys: :acceptance-evidence-keys is empty — nothing to prove"
                        .to_string(),
                }
            } else {
                match prior_results.get(&source_id) {
                    None => AcceptanceFanInOutcome {
                        mode,
                        source_nodes: single_source,
                        passed: false,
                        reason: format!(
                            "evidence_keys: source node `{}` produced no result",
                            source_id
                        ),
                    },
                    Some(r) => {
                        let missing = inner_payload_missing_keys(&r.inner_payload, &keys);
                        if missing.is_empty() {
                            AcceptanceFanInOutcome {
                                mode,
                                source_nodes: single_source,
                                passed: true,
                                reason: format!(
                                    "evidence_keys: source node `{}` carries every required key",
                                    source_id
                                ),
                            }
                        } else {
                            AcceptanceFanInOutcome {
                                mode,
                                source_nodes: single_source,
                                passed: false,
                                reason: format!(
                                    "evidence_keys: source node `{}` missing keys {:?}",
                                    source_id, missing
                                ),
                            }
                        }
                    }
                }
            }
        }
    };

    let mut next = base;
    let fan_in_passed = outcome.passed;
    let fan_in_reason = outcome.reason.clone();
    next.fan_in = Some(outcome);

    if parent_dominates {
        // Per-node decision wins; fan-in is informational only.
        return next;
    }

    if fan_in_passed {
        // NotEvaluated → Accepted, Accepted → Accepted (status stable).
        if matches!(next.status, AcceptanceStatus::NotEvaluated) {
            next.status = AcceptanceStatus::Accepted;
            next.reason = format!("acceptance_fan_in: {}", fan_in_reason);
        }
    } else {
        next.status = AcceptanceStatus::Rejected;
        next.reason = format!("acceptance_fan_in: {}", fan_in_reason);
    }
    next
}

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
fn inner_payload_failure_signal(payload: &Value) -> Option<String> {
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
fn inner_payload_missing_keys(payload: &Value, required: &[String]) -> Vec<String> {
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

/// wave-17 / task 03 — deterministic id format used when an acceptance
/// evaluation needs to surface a manual-required pause. Distinct from
/// the wave-16 / task 04 review-gate id format so the wave-17 / task 01
/// resume helper does NOT accidentally re-dispatch acceptance pauses
/// (its validator hard-requires `action=plan-node` AND the node still
/// carrying `:review-gate "question-event"` — neither holds for an
/// acceptance pause).
///
/// Layout: `acceptance:plan:<plan_id>:v<version>:<node_id>`.
pub(super) fn derive_acceptance_pause_id(
    plan_id: uuid::Uuid,
    plan_version: i32,
    node_id: &str,
) -> String {
    format!("acceptance:plan:{}:v{}:{}", plan_id, plan_version, node_id)
}
