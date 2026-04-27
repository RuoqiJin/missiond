;; Wave 21 task contract.

(task wave21-08-machine-contract-autonomous-loop-smoke-v3
  :schema "missiond.task-contract.v1"
  :title "Machine contract autonomous loop smoke v3"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-03-execution-report-verifier-integration-v1"
               "wave21-04-autonomous-workstation-llm-proposal-v0"
               "wave21-05-plan-inference-apply-gate-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add an end-to-end deterministic smoke that exercises task.lisp dispatch, scoped preflight, report contract, shared-memory completion, and run verifier without relying on Markdown."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
     "crates/missiond-core/src/event/events/execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Use fixture task.lisp/report/shared-memory data; no real LLM, no real spawn, no mutating git."
     "Assert Markdown path is absent or metadata-only."
     "Assert the run verifier fields would pass with the fixture commit metadata."
     "Assert malformed task/report/memory data produce structured failures."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
     "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :commit
    (:required true
     :message "test(intent): cover autonomous machine-contract loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Smoke stages covered."
     "Proof Markdown is non-load-bearing."
     "Acceptance command results."])
