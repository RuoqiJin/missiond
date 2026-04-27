;; Wave 24 task contract.

(task wave24-03-router-recommendation-cli-v0
  :schema "missiond.task-contract.v1"
  :title "Router recommendation CLI v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave24-01-router-policy-schema-v1"
               "wave24-02-trace-corpus-index-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Add a read-only CLI that consumes a task contract, router policy, and trace corpus index to emit an explainable backend recommendation without changing runtime dispatch."

  :write-scope
    ["scripts/recommend-task-backend.mjs"
     "scripts/check-router-policy.mjs"
     "scripts/build-session-trace-index.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/tasks/schema/*.lisp"
     ".missiond/tasks/wave23/**"
     ".missiond/tasks/wave24/wave24-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["CLI shape: node scripts/recommend-task-backend.mjs --task <task.lisp> --policy <router-policy.lisp> [--trace-index <json>] [--json] [--dry-fixture]."
     "Recommendation output must include backend, confidence, matched_rules, rejected_rules, non_goals, and dry_run_only=true."
     "Use deterministic inputs only; do not call LLMs, ClaudeCode, git, or shell commands."
     "If evidence is insufficient, recommend claudecode with confidence low and reason insufficient_trace_history."
     "Do not mutate task contracts, router policy, trace index, or runtime dispatch."]

  :acceptance
    ["node scripts/recommend-task-backend.mjs --dry-fixture"
     "node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp --policy .missiond/router/router-policy-v1.lisp --json"
     "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/recommend-task-backend.mjs scripts/check-router-policy.mjs scripts/build-session-trace-index.mjs"]

  :commit
    (:required true
     :message "feat(tasks): recommend backend from trace policy"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Recommendation JSON shape."
     "Dry-run/non-mutating guarantees."
     "Acceptance command results."])
