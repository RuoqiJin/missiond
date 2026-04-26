;; Wave 19 task contract.

(task wave19-11-execution-opened-dispatch-metadata-v1
  :schema "missiond.task-contract.v1"
  :title "ExecutionEvent Opened dispatch metadata v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-08-execution-task-contract-completion-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Extend the older ExecutionEvent::Opened path with dispatch metadata so consumers do not need companion log reads for the first lifecycle event."

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
    ["Add optional dispatch_strategy, target_project, and requested_cwd fields to ExecutionEvent::Opened with serde defaults and skip_serializing_if so legacy JSON remains readable."
     "Wire mission_execution(open) to publish these fields when present."
     "Keep existing event ids and companion log behavior intact."
     "Add round-trip tests for old JSON without fields and new JSON with fields."]

  :acceptance
    ["cargo test -p missiond-core --lib"
     "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-core/src/event/events/execution.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "feat(execution): include dispatch metadata on opened events"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Serde backward compatibility proof."
     "Event publish wiring notes."
     "Acceptance command results."])
