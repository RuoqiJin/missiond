;; Wave 25 task contract.

(task wave25-03-plan-router-trace-index-confidence-v1
  :schema "missiond.task-contract.v1"
  :title "mission_plan router trace-index confidence v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave25-00-archive-wave24-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Bring mission_plan(router_policy_mode=dry_run) closer to the Node recommendation CLI by accepting an optional trace-index JSON path and using it only for confidence scoring."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/**"]

  :requirements
    ["Add optional router_trace_index_path or router_policy_trace_index_path to mission_plan execute schema."
     "Only read the trace-index file when router_policy_mode=dry_run and the path is supplied."
     "Use serde_json to parse the existing build-session-trace-index JSON shape; do not spawn Node or shell out."
     "Confidence parity target: matched rule + >=5 events for task or backend => high; 1..4 => medium; 0/missing => low; no match remains low fallback."
     "Missing or malformed trace-index must not fail dispatch; surface router_recommendation.trace_index_status/warning and keep applied=false."
     "Off/default mode must remain byte-identical with no file I/O."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): score router dry-run with trace index"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New arg and response fields."
     "Proof that off/default mode does no file I/O and dispatch is unchanged."
     "Acceptance command results."])

