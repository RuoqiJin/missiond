;; Wave 26 task contract.

(task wave26-06-router-readiness-smoke-v1
  :schema "missiond.task-contract.v1"
  :title "Router readiness smoke v1"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-02-router-recommendation-readiness-v1" "wave26-03-plan-router-backend-readiness-v1" "wave26-04-report-router-readiness-fields-v0" "wave26-05-renderer-router-readiness-context-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Add cross-layer smoke coverage proving backend readiness annotations stay advisory and cannot apply runtime replacement."

  :write-scope
    ["scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/check-task-report.mjs"
     "scripts/render-claudecode-task.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     ".missiond/tasks/schema/*.lisp"
     ".missiond/router/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/**"
     ".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Node smoke: recommend-task-backend dry fixture covers seed registry and a synthetic runtime-ready fixture; seed registry must keep apply_eligible=false for advisory-only backends."
     "Node smoke: evaluator dry fixture aggregates apply_eligible_count and by_backend_readiness."
     "Report checker smoke: valid readiness fields and invalid literal-string booleans."
     "Renderer smoke: rendered brief contains router-backend-registry command and --backend-registry flag while retaining advisory/dry-run only literals."
     "Rust smoke: mission_plan dry_run with registry path emits readiness fields and applied=false; off/default with registry path remains byte-identical and does no file I/O."
     "Static audit: active router readiness paths must not use std::process::Command / tokio::process / LLM clients / network clients / mutating git."]

  :acceptance
    ["node scripts/recommend-task-backend.mjs --dry-fixture"
     "node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp > /tmp/wave26-router-smoke.md"
     "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs scripts/check-task-report.mjs scripts/render-claudecode-task.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs"]

  :commit
    (:required true
     :message "test(router): smoke backend readiness loop"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Pinned invariants."
     "Test count deltas."
     "Acceptance command results."])

