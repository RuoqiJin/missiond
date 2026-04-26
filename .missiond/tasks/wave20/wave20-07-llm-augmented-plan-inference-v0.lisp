;; Wave 20 task contract.

(task wave20-07-llm-augmented-plan-inference-v0
  :schema "missiond.task-contract.v1"
  :title "LLM-augmented PLAN field inference v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add an explicit Sonnet-assisted PLAN field inference mode that proposes fields with evidence, but never silently overrides deterministic high-confidence inference."

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
    ["Add explicit inference_mode=\"sonnet\" or equivalent; default deterministic behavior unchanged."
     "If Sonnet is unavailable, return structured LLM_UNAVAILABLE and do not mutate plan state."
     "LLM output must be validated into a proposal object with field, value, confidence, evidence, and conflict status."
     "Only caller-approved or dry-run proposal output is required in v0; do not auto-apply low/medium confidence LLM fields."]

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
     :message "feat(plan): add explicit LLM field inference proposals"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Inference mode name."
     "Validation contract."
     "Mutation boundaries."
     "Acceptance command results."])
