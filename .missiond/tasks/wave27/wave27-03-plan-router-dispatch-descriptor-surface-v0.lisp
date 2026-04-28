;; Wave 27 task contract.

(task wave27-03-plan-router-dispatch-descriptor-surface-v0
  :schema "missiond.task-contract.v1"
  :title "mission_plan router dispatch descriptor surface v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave27-01-router-dispatch-descriptor-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Expose a router dispatch descriptor block from mission_plan execute dry-run mode using the existing Rust router recommendation/readiness path. This is a response surface only; dispatch must remain unchanged."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["scripts/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave26/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/**"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :requirements
    ["Add an optional mission_plan execute argument such as router_dispatch_descriptor=true; choose the smallest naming consistent with existing router_policy_* args and document it in the MCP schema."
     "Only honor the argument when router_policy_mode=dry_run. absent/off/default must preserve the old response shape and perform no additional file I/O."
     "Build the descriptor from the existing Rust dry-run recommendation + backend readiness fields; do not spawn Node and do not shell out."
     "Descriptor fields must match wave27-01 schema names where practical: recommended_backend, router_confidence, backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers, dry_run_only=true, runtime_replacement=false, no_execution=true."
     "If router_backend_registry_path is absent, descriptor should either be omitted or return a structured descriptor_status explaining registry_missing; do not fake readiness."
     "applied=false remains literal; dispatch_strategy / target_project / requested_cwd must remain unchanged."
     "Add tests proving off/default with descriptor flag does no I/O and remains byte-identical; dry_run with seed registry emits no_execution=true and router_apply_eligible=false for current-default."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::plan::tests"
     "cargo test -p missiond-daemon"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): surface router dispatch descriptors"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Argument name and response fields."
     "Byte-compat/off-mode tests."
     "Acceptance command results."])
