use serde_json::{json, Value};

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
pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceMode {
    InnerStatus,
    EvidenceKeys,
    Manual,
}

impl AcceptanceMode {
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
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
    pub(in crate::handlers::knowledge::plan_dag) fn parse(raw: &str) -> Option<Self> {
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
pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceRequires {
    AllSucceeded,
    AnySucceeded,
    EvidenceKeys,
}

impl AcceptanceRequires {
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
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
    pub(in crate::handlers::knowledge::plan_dag) fn parse(raw: &str) -> Option<Self> {
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
pub(in crate::handlers::knowledge::plan_dag) enum AcceptanceStatus {
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
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
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
pub(in crate::handlers::knowledge::plan_dag) struct AcceptanceEvaluation {
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
    pub(in crate::handlers::knowledge::plan_dag) fn is_inactive(&self) -> bool {
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
    pub(in crate::handlers::knowledge::plan_dag) fn to_json(&self) -> Value {
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
pub(in crate::handlers::knowledge::plan_dag) struct AcceptanceFanInOutcome {
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
    pub(in crate::handlers::knowledge::plan_dag) fn to_json(&self) -> Value {
        json!({
            "mode": self.mode.as_wire(),
            "source_nodes": self.source_nodes,
            "passed": self.passed,
            "reason": self.reason,
        })
    }
}
