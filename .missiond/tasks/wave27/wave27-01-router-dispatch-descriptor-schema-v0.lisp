;; Wave 27 task contract.

(task wave27-01-router-dispatch-descriptor-schema-v0
  :schema "missiond.task-contract.v1"
  :title "Router dispatch descriptor schema v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave27-00-archive-wave26-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Introduce a machine-checkable router dispatch descriptor schema and checker. The descriptor is a handoff contract derived from router recommendation + backend readiness; it is not a runtime backend switch and must carry no-execution invariants explicitly."

  :write-scope
    [".missiond/tasks/schema/router-dispatch-descriptor-v1.lisp"
     "scripts/check-router-dispatch-descriptor.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave26/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/**"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/check-task-report.mjs"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["Define schema missiond.router-dispatch-descriptor.v1 with a top-level descriptor form, not a policy form."
     "Required fields must include task_id, recommended_backend, router_confidence, backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers, dry_run_only, runtime_replacement, no_execution, source_recommendation_schema, source_policy_path, and source_backend_registry_path."
     "dry_run_only must be literal true; runtime_replacement must be literal false; no_execution must be literal true. Reject strings like \"true\" and \"false\"."
     "router_apply_eligible=true is allowed only when backend_readiness_status is runtime-ready, backend_runtime_allowed is true, router_confidence is high, and router_apply_blockers is empty."
     "current-default must be accepted as a readiness status but rejected for router_apply_eligible=true."
     "Checker must support --json, --stdin, and --dry-fixture, use the shared missiond_lisp parser, and never shell out / call git / call network / call LLM."
     "Add fixtures for valid eligible runtime-ready, valid current-default blocked, string bool rejected, missing no_execution rejected, runtime_replacement=true rejected, current-default eligible rejected, unknown backend rejected, absolute path rejected, and malformed blocker vector rejected."]

  :acceptance
    ["node scripts/check-router-dispatch-descriptor.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/router-dispatch-descriptor-v1.lisp scripts/check-router-dispatch-descriptor.mjs"]

  :commit
    (:required true
     :message "feat(router): add dispatch descriptor schema"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Descriptor schema required fields."
     "Fixture count and rejection categories."
     "Acceptance command results."])
