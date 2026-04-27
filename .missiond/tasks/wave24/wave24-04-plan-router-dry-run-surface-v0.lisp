;; Wave 24 task contract.

(task wave24-04-plan-router-dry-run-surface-v0
  :schema "missiond.task-contract.v1"
  :title "mission_plan router dry-run surface v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave24-03-router-recommendation-cli-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Expose a dry-run-only router recommendation block on mission_plan(action=execute) so callers can see future backend selection without changing actual dispatch."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/**"]

  :requirements
    ["Add optional args router_policy_mode and router_policy_path or equivalent minimal names to mission_plan execute schema."
     "Only support off/default and dry_run in this task; any apply/auto mode must return INVALID_PARAM."
     "Dry-run block must be informational and must not alter target, dispatch_strategy, workstation_dispatch, auto_spawn, or evidence side effects."
     "Implement recommendation with a pure Rust deterministic helper using existing execute args; do not spawn Node or call scripts."
     "Response block should include status, recommended_backend, confidence, reasons, policy_source, and applied=false."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): expose router dry-run recommendation"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New args and response block."
     "Proof that dispatch behavior is unchanged."
     "Acceptance command results."])
