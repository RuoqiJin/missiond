;; Wave 22 task contract.

(task wave22-04-persisted-plan-inference-apply-v2
  :schema "missiond.task-contract.v1"
  :title "Persisted PLAN inference apply v2"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-05-plan-inference-apply-gate-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Allow inferred PLAN fields to persist into plan.sexp_text only through an explicit versioned apply path with preview, audit sidecar, and rollback metadata."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Add explicit persist_inference=true gate; default remains preview/apply-in-memory only."
     "Require apply_inferred_fields=true, caller_approved=true, and a matching proposal_hash."
     "Before mutation, return or record original_sexp_hash and resulting_sexp_hash."
     "Persist only by creating a new plan version or existing project pattern equivalent; do not overwrite without version/audit."
     "Append typed evidence entry with applied_fields, skipped_fields, proposal_hash, original hash, resulting hash, and rollback pointer."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): persist inferred PLAN fields with audit"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Persist gate fields."
     "Version/audit behavior."
     "Rollback metadata."
     "Acceptance command results."])
