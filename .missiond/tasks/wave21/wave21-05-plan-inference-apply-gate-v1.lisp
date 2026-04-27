;; Wave 21 task contract.

(task wave21-05-plan-inference-apply-gate-v1
  :schema "missiond.task-contract.v1"
  :title "PLAN inference apply gate v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-07-llm-augmented-plan-inference-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add a controlled apply gate for PLAN field inference proposals, so deterministic high-confidence fields can be applied only under explicit caller approval."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Add explicit apply_inferred_fields=true gate; default remains suggest-only."
     "Allow apply only for deterministic high-confidence proposals with no conflict, or for LLM proposals explicitly marked caller_approved=true."
     "Return applied_fields, skipped_fields, conflict_fields, and resulting_plan_preview."
     "Do not mutate persisted plan text unless existing action already has persist=true or an explicit persist_inference=true flag is supplied."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): gate applying inferred PLAN fields"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Apply gate flags."
     "Safety rules."
     "Persistence boundary."
     "Acceptance command results."])
