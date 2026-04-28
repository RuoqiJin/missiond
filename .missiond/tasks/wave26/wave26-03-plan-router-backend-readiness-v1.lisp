;; Wave 26 task contract.

(task wave26-03-plan-router-backend-readiness-v1
  :schema "missiond.task-contract.v1"
  :title "mission_plan router backend readiness v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-01-router-backend-registry-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Expose backend readiness and apply blockers on mission_plan(router_policy_mode=dry_run) by optionally reading the backend registry in Rust, without changing dispatch."

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
     ".missiond/tasks/**"
     ".missiond/router/**"]

  :requirements
    ["Add optional router_backend_registry_path to mission_plan execute schema."
     "Read the backend registry only when router_policy_mode=dry_run and the path is supplied."
     "Parse a minimal Rust subset of missiond.router-backend-registry.v1; do not spawn Node or shell out."
     "Add fields under router_recommendation: backend_registry_path, backend_registry_status, backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers."
     "Registry missing/unreadable/malformed must be non-fatal: surface backend_registry_status/warning and keep router_apply_eligible=false."
     "router_apply_eligible must never change dispatch; applied remains literal false."
     "Off/default mode must remain byte-identical with no file I/O, even when router_backend_registry_path and router_policy_trace_index_path are supplied."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): surface router backend readiness"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New arg and response fields."
     "Proof off/default mode does no file I/O."
     "Acceptance command results."])

