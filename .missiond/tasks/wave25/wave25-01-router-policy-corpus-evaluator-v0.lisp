;; Wave 25 task contract.

(task wave25-01-router-policy-corpus-evaluator-v0
  :schema "missiond.task-contract.v1"
  :title "Router policy corpus evaluator v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave25-00-archive-wave24-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Add a read-only evaluator CLI that runs the dry-run router recommendation across a corpus of task contracts and trace-index evidence, producing a measurable policy report without changing runtime dispatch."

  :write-scope
    ["scripts/evaluate-router-policy-corpus.mjs"]

  :must-not-touch
    ["crates/**"
     "scripts/recommend-task-backend.mjs"
     "scripts/build-session-trace-index.mjs"
     "scripts/check-router-policy.mjs"
     ".missiond/v2/**"
     ".missiond/tasks/schema/*.lisp"
     ".missiond/tasks/wave24/**"
     ".missiond/tasks/wave25/wave25-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["CLI shape: node scripts/evaluate-router-policy-corpus.mjs --policy <router-policy.lisp> [--tasks-root .missiond/tasks] [--trace-index <json>] [--json] [--dry-fixture]."
     "The evaluator must read task contracts, reuse the existing router recommendation logic if exported cleanly or implement a small deterministic wrapper without shelling out."
     "Output schema must be missiond.router-policy-evaluation.v0 with totals, by_backend, by_confidence, fallback_count, rejected_count, per_task rows, and policy_path."
     "If --trace-index is absent, build the index in-process from existing session traces, not by spawning build-session-trace-index.mjs."
     "Hard guarantees: read-only, no shell, no git, no LLM, no HTTP, no runtime dispatch mutation."
     "Include dry fixtures covering empty corpus, multi-task corpus, fallback rows, rejected policy, and deterministic stable JSON."]

  :acceptance
    ["node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
     "node scripts/evaluate-router-policy-corpus.mjs --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --json"
     "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/evaluate-router-policy-corpus.mjs"]

  :commit
    (:required true
     :message "feat(tasks): evaluate router policy over trace corpus"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Evaluation JSON shape."
     "Read-only/no-shell/no-git proof."
     "Acceptance command results."])

