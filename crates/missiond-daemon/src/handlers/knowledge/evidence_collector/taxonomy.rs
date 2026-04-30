/// Companion sidecar directory under a project root. Mirrors the constant in
/// `plan.rs`; duplicated here so the lower-level
/// [`append_entry_to_project_root`] writer can be tested without taking an
/// `AppState` (which carries half the daemon).
///
/// `#[allow(dead_code)]`: only referenced by the `#[cfg(test)]`-only
/// [`append_entry_to_project_root`] writer. Production callers go through
/// `super::plan::append_plan_evidence_entry` which holds its own copy of the
/// path constant. We intentionally duplicate to avoid leaking that internal
/// const just for the test surface; the duplication is explicitly noted in
/// the docstring above.
#[allow(dead_code)]
pub(super) const COMPANION_DIR: &str = ".missiond/v2/plans";

/// Schema version stamped onto every evidence entry produced through this
/// builder. Bump when the `EvidenceEntry` shape gains a non-additive field so
/// downstream consumers can route on it explicitly. Adding optional fields
/// does NOT require a bump; renaming / repurposing existing fields does.
pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "v0";

/// Canonical `source` tags surfaced in evidence entries. The legacy
/// `"plan_runner_dispatch"` / `"plan_dag_node_dispatch"` strings are kept as
/// the wire form so existing readers (audit dashboards, intent-event-bus
/// consumers, scoped-commit handoff metadata) stay byte-identical.
///
/// `record_evidence_manual` is the new tag for the `mission_plan(action=
/// record_evidence)` manual entry — the prior wire form had no `source`,
/// only a `kind`. Callers that want to keep emitting the legacy untagged
/// form can still use `EvidenceCollector::legacy_record_evidence` which
/// preserves the historical shape.
pub(crate) mod source {
    /// Manual `mission_plan(action=record_evidence)` entry written by an
    /// agent / human caller. Always treat this as un-vetted: the collector
    /// does no schema validation on the inner `evidence` payload.
    pub(crate) const RECORD_EVIDENCE_MANUAL: &str = "record_evidence_manual";

    /// Single-node v0 plan-runner internal dispatch (plan.rs ::
    /// `action_execute_internal`). Wire-compatible with the historical
    /// `kind="plan_runner_dispatch"` entries.
    pub(crate) const PLAN_RUNNER_DISPATCH: &str = "plan_runner_dispatch";

    /// Per-node DAG scheduler dispatch (plan_dag.rs).
    pub(crate) const PLAN_DAG_NODE_DISPATCH: &str = "plan_dag_node_dispatch";

    /// Workstation-dispatch v0 (workstation_dispatch.rs) — the conservative
    /// opt-in path that augments a `mission_task_delegate` call with a
    /// scoped task brief (objective / owned-files / forbidden-files /
    /// acceptance commands / commit policy). Distinguished from the bare
    /// `plan_runner_dispatch` source so audit consumers can tell when the
    /// task brief was injected and when only the legacy passthrough ran.
    pub(crate) const WORKSTATION_DISPATCH: &str = "workstation_dispatch";
}

/// Canonical `kind` taxonomy. We keep this open (callers can pass arbitrary
/// strings) but ship the well-known names as constants so the call sites
/// don't drift typos. Mirrors the historical sidecar shape:
///   - `dispatch`  : an inner handler was invoked, payload carries the
///                   `inner_result` / `inner_error` projection.
///   - `verification` : verification commands ran (test / lint / build) and
///                      we want to capture command list + summary + outcome.
///   - `git_diff` : git diff stat / changed-file list snapshot.
///   - `commit`   : commit hash / commit status handoff metadata.
///   - `note`     : free-form caller note (manual `record_evidence`).
pub(crate) mod kind {
    pub(crate) const DISPATCH: &str = "dispatch";
    /// `#[allow(dead_code)]`: future plan-runner verification step (cargo
    /// test / lint / build summary) — wave-12 reserved this slot for the
    /// upcoming verification-evidence wiring (intent-flow.lisp ::
    /// F-intent-alignment-plan-execution-loop :: s7 verification-runner).
    /// Not yet emitted by any call site, but documented in the public
    /// taxonomy so the wire contract is stable when wiring lands.
    #[allow(dead_code)]
    pub(crate) const VERIFICATION: &str = "verification";
    /// `#[allow(dead_code)]`: future git-diff snapshot for plan evidence
    /// (paired with VERIFICATION above; the verification runner attaches a
    /// `git diff --stat` payload alongside the test results).
    #[allow(dead_code)]
    pub(crate) const GIT_DIFF: &str = "git_diff";
    /// `#[allow(dead_code)]`: future scoped-commit handoff metadata (wave-12
    /// task-01 commit_hash / commit_status round-trip — covered by
    /// `commit_metadata_round_trip_via_typed_setter` test).
    #[allow(dead_code)]
    pub(crate) const COMMIT: &str = "commit";
    pub(crate) const NOTE: &str = "note";
}
