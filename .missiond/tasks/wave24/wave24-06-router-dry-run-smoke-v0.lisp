;; Wave 24 task contract.

(task wave24-06-router-dry-run-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "Router dry-run smoke v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave24-04-plan-router-dry-run-surface-v0"
               "wave24-05-renderer-router-context-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Add deterministic smoke coverage proving router-policy artifacts, CLI recommendation, renderer context, and mission_plan dry-run surface remain advisory and non-mutating."

  :write-scope
    ["scripts/recommend-task-backend.mjs"
     "scripts/build-session-trace-index.mjs"
     "scripts/render-claudecode-task.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     ".missiond/v2/**"
     ".missiond/tasks/wave23/**"
     ".missiond/tasks/wave24/wave24-*.lisp"]

  :requirements
    ["Add smoke tests or dry fixtures covering the full dry-run chain: trace index -> recommendation -> renderer -> plan response."
     "Pin invariants: dry_run_only=true, applied=false, no runtime backend replacement, no LLM call, no spawn, no mutating git."
     "Prefer existing test modules and fixture style; do not add a new MCP tool."]

  :acceptance
    ["node scripts/build-session-trace-index.mjs --dry-fixture"
     "node scripts/recommend-task-backend.mjs --dry-fixture"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "git diff --check -- scripts/recommend-task-backend.mjs scripts/build-session-trace-index.mjs scripts/render-claudecode-task.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :commit
    (:required true
     :message "test(plan): smoke router dry-run flow"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Smoke invariants pinned."
     "Acceptance command results."])
