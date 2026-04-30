/// wave-17 / task 04 — typed projection of `:rollback-policy` for the
/// conservative rollback descriptor pass. Resolved on the parser side
/// so the runtime can pivot without re-tokenising the raw string.
///
/// Three modes are recognised:
///   * `None`        — author wrote `"none"` (or omitted the policy
///                      entirely; absence on `DagNode::rollback_policy`
///                      is the SAME as `None`). Preserves the existing
///                      failure behaviour: failed node propagates taint
///                      per `:failure-policy`, no rollback descriptor
///                      is emitted.
///   * `Descriptor`  — record / surface a structured rollback
///                      descriptor (objective + owned files +
///                      acceptance commands + brief preview) on the
///                      response and evidence row. **Never dispatches.**
///                      Use this when the author wants downstream
///                      observers / humans to know what a rollback
///                      WOULD do without authorising the scheduler to
///                      execute it.
///   * `Workstation` — opt into automatic rollback dispatch through
///                      the existing wave-15 workstation-dispatch
///                      substrate. The scheduler ONLY dispatches when
///                      every safety condition holds (resolved target
///                      project, non-empty rollback objective, at
///                      least one owned file, dispatch strategy is on
///                      the inferable whitelist). Otherwise the row
///                      surfaces as `refused` with the failing
///                      condition spelled out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan_dag) enum RollbackPolicy {
    None,
    Descriptor,
    Workstation,
}

impl RollbackPolicy {
    pub(in crate::handlers::knowledge::plan_dag) fn as_wire(self) -> &'static str {
        match self {
            RollbackPolicy::None => "none",
            RollbackPolicy::Descriptor => "descriptor",
            RollbackPolicy::Workstation => "workstation",
        }
    }

    /// Parse a raw `:rollback-policy` value into a typed mode. Trims
    /// and lowercases the input; unknown values yield `None` (the
    /// parser also pushes them onto `unsupported_fields` so the typo
    /// surfaces in `node_hint_summary`).
    pub(in crate::handlers::knowledge::plan_dag) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(RollbackPolicy::None),
            "descriptor" => Some(RollbackPolicy::Descriptor),
            "workstation" => Some(RollbackPolicy::Workstation),
            _ => None,
        }
    }
}
