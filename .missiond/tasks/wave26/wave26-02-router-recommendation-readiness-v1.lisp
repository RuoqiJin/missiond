;; Wave 26 task contract.

(task wave26-02-router-recommendation-readiness-v1
  :schema "missiond.task-contract.v1"
  :title "Router recommendation readiness annotations v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-01-router-backend-registry-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Teach the Node recommendation and corpus-evaluation CLIs to consume the backend readiness registry and report apply eligibility and blockers while staying advisory-only."

  :write-scope
    ["scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"]

  :must-not-touch
    ["crates/**"
     "scripts/check-router-backend-registry.mjs"
     "scripts/check-router-policy.mjs"
     "scripts/build-session-trace-index.mjs"
     ".missiond/router/**"
     ".missiond/tasks/schema/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/**"
     ".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add optional --backend-registry <path> to recommend-task-backend.mjs and evaluate-router-policy-corpus.mjs."
     "When supplied, parse the registry in-process; do not spawn check-router-backend-registry.mjs."
     "Recommendation output should include backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers, and backend_registry_path when a registry was supplied."
     "router_apply_eligible must be true only when policy is valid, recommendation status is computed, confidence is high, registry backend exists, backend runtime_allowed=true, readiness_status=runtime-ready, and apply_blockers is empty."
     "With the seed registry, recommendations should remain router_apply_eligible=false for non-runtime-ready backends."
     "Corpus evaluator should aggregate by_backend_readiness and apply_eligible_count without changing the existing top-level keys' meanings."
     "Without --backend-registry, outputs remain backward-compatible except for additive fields only if a registry is explicitly supplied."]

  :acceptance
    ["node scripts/recommend-task-backend.mjs --dry-fixture"
     "node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
     "node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave26/wave26-01-router-backend-registry-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp --json"
     "node scripts/evaluate-router-policy-corpus.mjs --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --backend-registry .missiond/router/router-backend-registry-v1.lisp --json"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs"]

  :commit
    (:required true
     :message "feat(router): annotate recommendations with backend readiness"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New CLI flags and output fields."
     "Proof advisory/dry-run invariants remain."
     "Acceptance command results."])

