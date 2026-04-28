;; Wave 26 task contract.

(task wave26-01-router-backend-registry-v0
  :schema "missiond.task-contract.v1"
  :title "Router backend readiness registry v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-00-archive-wave25-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Add a machine-checkable backend readiness registry so router recommendations can explain whether a backend is advisory-only, unavailable, or runtime-ready before any future apply gate."

  :write-scope
    [".missiond/tasks/schema/router-backend-registry-v1.lisp"
     ".missiond/router/router-backend-registry-v1.lisp"
     "scripts/check-router-backend-registry.mjs"]

  :must-not-touch
    ["crates/**"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/check-router-policy.mjs"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/**"
     ".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Create schema missiond.router-backend-registry.v1 and a seed registry under .missiond/router/."
     "Backend ids must stay aligned with router-policy backend enum: claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker."
     "Each backend entry must expose readiness_status enum: current-default, advisory-only, runtime-ready, unavailable."
     "Each backend entry must expose runtime_allowed boolean, apply_blockers vector, substrate string or null-equivalent atom, and at least one non-goal."
     "Seed registry must mark claudecode as current-default and every non-claudecode backend as advisory-only or unavailable unless an actual runtime adapter exists."
     "Checker must reject unknown backend ids, duplicate ids, runtime_allowed=true without readiness_status=runtime-ready, runtime-ready without substrate, empty non-goals, and unknown top-level entry heads."
     "No code path may consume this registry at runtime in this task; this is schema + seed + checker only."]

  :acceptance
    ["node scripts/check-router-backend-registry.mjs --dry-fixture"
     "node scripts/check-router-backend-registry.mjs .missiond/router/router-backend-registry-v1.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/router-backend-registry-v1.lisp .missiond/router/router-backend-registry-v1.lisp scripts/check-router-backend-registry.mjs"]

  :commit
    (:required true
     :message "feat(router): add backend readiness registry"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Registry schema and seed shape."
     "Dry fixtures covered."
     "Acceptance command results."])

