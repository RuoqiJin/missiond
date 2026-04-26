;; Wave 20 task contract.

(task wave20-09-execution-event-legacy-metadata-sweep
  :schema "missiond.task-contract.v1"
  :title "ExecutionEvent legacy metadata sweep"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Audit and fill any remaining older ExecutionEvent variants that still lack optional dispatch metadata after Wave 19, without breaking serde backward compatibility."

  :write-scope
    ["crates/missiond-core/src/event/events/execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Inventory ExecutionEvent variants and identify which already carry dispatch_strategy/target_project/requested_cwd."
     "For any remaining lifecycle variant that should carry the triplet, add optional serde-default fields with skip_serializing_if."
     "Add old-JSON and new-JSON round-trip tests."
     "If no variant remains, make a no-op commit only if a test/doc assertion is added inside the write scope; otherwise report NO-OP."]

  :acceptance
    ["cargo test -p missiond-core --lib"
     "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-core/src/event/events/execution.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "feat(execution): complete dispatch metadata event coverage"
     :scope-check write-scope-only)

  :report
    ["Commit hash or NO-OP reason."
     "Variant inventory."
     "Serde compatibility proof."
     "Acceptance command results."])
