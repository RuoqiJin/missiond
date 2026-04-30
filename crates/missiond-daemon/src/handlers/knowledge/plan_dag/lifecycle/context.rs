/// Pre-built immutable evidence parameters that vary per call to
/// `action_execute_dag_v1`. The scheduler captures these once so each
/// per-node evidence emit doesn't re-thread the same args through.
pub(in crate::handlers::knowledge::plan_dag) struct EvidenceCtx<'a> {
    pub(in crate::handlers::knowledge::plan_dag) plan_id: uuid::Uuid,
    /// wave-17 / task 03 — captured here so the deterministic
    /// acceptance pause id (which carries the plan version segment for
    /// resolver routing) can be derived without re-fetching the plan
    /// row from every emit site.
    pub(in crate::handlers::knowledge::plan_dag) plan_version: i32,
    pub(in crate::handlers::knowledge::plan_dag) project_arg: Option<&'a str>,
    pub(in crate::handlers::knowledge::plan_dag) cwd_arg: Option<&'a str>,
    pub(in crate::handlers::knowledge::plan_dag) target_project_arg: Option<&'a str>,
}
