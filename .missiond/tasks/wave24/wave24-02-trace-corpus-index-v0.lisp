;; Wave 24 task contract.

(task wave24-02-trace-corpus-index-v0
  :schema "missiond.task-contract.v1"
  :title "Session trace corpus index v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-06-trace-summary-analyzer-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Add a read-only trace corpus indexer that scans .missiond/tasks/**/session-trace.lisp and produces stable per-task/per-backend aggregate JSON for router policy experiments."

  :write-scope
    ["scripts/build-session-trace-index.mjs"
     "scripts/analyze-session-trace.mjs"
     "scripts/check-session-trace.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/tasks/schema/*.lisp"
     ".missiond/tasks/wave23/**"
     ".missiond/tasks/wave24/wave24-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Prefer a new scripts/build-session-trace-index.mjs that imports parseTraceEvents from check-session-trace.mjs."
     "The indexer must be read-only: stdout/stderr only, no file writes."
     "Output stable JSON with totals, by_task, by_backend, by_wave, bottleneck_tags, and source_files."
     "Support --json, --dry-fixture, and explicit trace file paths; default scan path may be .missiond/tasks/**/session-trace.lisp."
     "Do not make router recommendations here; this task only builds the corpus index."]

  :acceptance
    ["node scripts/build-session-trace-index.mjs --dry-fixture"
     "node scripts/build-session-trace-index.mjs --json .missiond/tasks/wave23/session-trace.lisp"
     "node scripts/check-session-trace.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/build-session-trace-index.mjs scripts/analyze-session-trace.mjs scripts/check-session-trace.mjs"]

  :commit
    (:required true
     :message "feat(tasks): index session trace corpus"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Index output shape."
     "Read-only proof."
     "Acceptance command results."])
