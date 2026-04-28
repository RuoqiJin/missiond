;; Wave 27 task contract.

(task wave27-02-router-dispatch-descriptor-cli-v0
  :schema "missiond.task-contract.v1"
  :title "Router dispatch descriptor CLI v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave27-01-router-dispatch-descriptor-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Build a read-only CLI that turns a task contract plus router policy, optional trace index, and backend registry into a router dispatch descriptor. This CLI must not execute the descriptor or switch backend."

  :write-scope
    ["scripts/build-router-dispatch-descriptor.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave26/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/**"
     "scripts/check-router-dispatch-descriptor.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/check-task-report.mjs"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["Create scripts/build-router-dispatch-descriptor.mjs as a new read-only CLI."
     "Consume --task, --policy, optional --trace-index, required --backend-registry for descriptor mode, plus --json and --dry-fixture."
     "Reuse exported helpers from recommend-task-backend.mjs and check-router-backend-registry.mjs in-process; do not duplicate policy matching logic."
     "Emit schema missiond.router-dispatch-descriptor.v1 and all fields required by wave27-01."
     "Descriptor must preserve router_apply_eligible exactly from the readiness-annotated recommendation, but still set runtime_replacement=false and no_execution=true."
     "When readiness is missing, malformed, or unknown backend, emit a valid descriptor with router_apply_eligible=false and explicit blockers."
     "Output must be deterministic with stable key order."
     "No shell-out, no child_process, no git, no HTTP/network, no LLM, no backend execution."]

  :acceptance
    ["node scripts/build-router-dispatch-descriptor.mjs --dry-fixture"
     "node scripts/build-router-dispatch-descriptor.mjs --task .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp --json | node scripts/check-router-dispatch-descriptor.mjs --stdin"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/build-router-dispatch-descriptor.mjs"]

  :commit
    (:required true
     :message "feat(router): build dispatch descriptors"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "CLI flags and output fields."
     "How no_execution/runtime_replacement invariants are enforced."
     "Acceptance command results."])
