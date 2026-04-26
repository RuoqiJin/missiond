;; Wave 20 task contract.

(task wave20-05-unified-entry-machine-loop-smoke-v2
  :schema "missiond.task-contract.v1"
  :title "Unified entry machine loop smoke v2"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-04-machine-driven-dispatch-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add a deterministic smoke test proving unified_entry can drive directive/plan/task-contract/workstation handoff without relying on Markdown parsing."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     ".missiond/v2/*.lisp"
     "scripts/**"]

  :requirements
    ["Create a no-LLM, no-spawn smoke path that uses fixture task.lisp data and asserts machine contract fields survive every handoff."
     "Assert Markdown/rendered brief is not required in the machine-mode smoke."
     "Assert malformed task contract returns a structured error rather than prompt fallback."
     "Keep the test deterministic and local."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]

  :commit
    (:required true
     :message "test(intent): cover machine contract dispatch loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Smoke stages covered."
     "Proof Markdown is non-load-bearing."
     "Acceptance command results."])
