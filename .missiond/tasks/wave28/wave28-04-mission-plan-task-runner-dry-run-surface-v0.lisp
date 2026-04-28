;; Wave 28 task contract.

(task wave28-04-mission-plan-task-runner-dry-run-surface-v0
  :schema "missiond.task-contract.v1"
  :title "mission_plan task-runner dry-run surface v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave28-01-task-runner-manifest-schema-v0"
               "wave28-02-task-runner-plan-cli-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier smoke
  :dispatch-group "C"
  :estimated-minutes 60
  :heartbeat-minutes 10
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Expose a daemon/MCP dry-run surface for task-runner manifests through mission_plan execute. The surface should read a manifest and return deterministic runner-plan facts, but it must not spawn workers, mutate git, or execute task contracts."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
     ".missiond/v2/**"
     ".missiond/tasks/**"
     ".missiond/claudecode/**"
     "scripts/**"]

  :requirements
    ["Add optional mission_plan execute args task_runner_manifest_path and task_runner_mode, where only absent/off/dry_run are accepted in v0; apply/auto/unknown must reject before plan lookup."
     "Dry-run response must surface manifest_status, wave, productive_only, batches, critical_path_minutes, total_estimated_minutes, verification_tier_counts, overlap_diagnostics, and applied=false literal."
     "Do not spawn, do not call Node, do not call shell, do not mutate git, do not dispatch mission_task_delegate. Implement a small in-Rust reader/projector or a narrow parser sufficient for manifest facts."
     "Absent/off mode must be byte-identical to the current baseline and must not read task_runner_manifest_path even if supplied."
     "Malformed/missing manifest should be non-fatal in dry_run and return manifest_status plus warning fields, not panic."
     "MCP schema description must state dry-run only and no execution."]

  :acceptance
    ["cargo test -p missiond-daemon task_runner --lib"
     "cargo test -p missiond-daemon --lib"
     "cargo test -p missiond-mcp --lib"
     "cargo build --workspace"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :commit
    (:required true
     :message "feat(plan): surface task runner manifest dry-run"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New args and response fields."
     "Byte-compat/no-I/O proof for off/default."
     "Acceptance command results."])
