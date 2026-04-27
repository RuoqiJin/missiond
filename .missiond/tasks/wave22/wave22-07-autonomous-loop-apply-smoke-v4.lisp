;; Wave 22 task contract.

(task wave22-07-autonomous-loop-apply-smoke-v4
  :schema "missiond.task-contract.v1"
  :title "Autonomous loop apply smoke v4"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave22-02-execution-auto-run-verifier-v2"
               "wave22-03-review-llm-approve-apply-gate-v1"
               "wave22-04-persisted-plan-inference-apply-v2"
               "wave22-05-autonomous-workstation-true-spawn-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add a deterministic smoke test that exercises the explicit apply gates for review, plan inference, workstation spawn, and execution verification without real LLM/spawn/git mutation."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-core/src/event/events/execution.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Use fixture proposal hashes and fixture task/report/memory data; no real LLM, no real spawn, no mutating git."
     "Assert each apply gate rejects missing proposal_hash and accepts the valid fixture path."
     "Assert Markdown remains non-load-bearing."
     "Assert failed verification blocks completion when enforce_scoped_commit=true."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
     "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]

  :commit
    (:required true
     :message "test(intent): cover autonomous apply gates"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Apply gates covered."
     "No real side-effect proof."
     "Acceptance command results."])
