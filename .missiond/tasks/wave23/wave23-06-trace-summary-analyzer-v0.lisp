;; Wave 23 task contract.

(task wave23-06-trace-summary-analyzer-v0
  :schema "missiond.task-contract.v1"
  :title "Session trace summary analyzer v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-01-session-trace-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Add a small read-only analyzer that summarizes session-trace facts into counts and bottleneck hints, without attempting router replacement yet."

  :write-scope
    ["scripts/analyze-session-trace.mjs"
     "scripts/check-session-trace.mjs"
     ".missiond/tasks/schema/session-trace-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave22/**"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["Add scripts/analyze-session-trace.mjs with --json, --dry-fixture, and one or more trace file inputs."
     "Summarize by task and backend: event counts, command/test counts, failure/retry counts, total duration_ms when present, files touched, commit count."
     "Emit conservative bottleneck hints only from observed facts; do not infer hidden ClaudeCode reasoning."
     "Do not implement router policy or model replacement in this task."]

  :acceptance
    ["node scripts/analyze-session-trace.mjs --dry-fixture"
     "node scripts/check-session-trace.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/analyze-session-trace.mjs scripts/check-session-trace.mjs .missiond/tasks/schema/session-trace-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): summarize session traces"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Analyzer output fields."
     "Bottleneck hint rules."
     "Non-goals."
     "Acceptance command results."])
