;; Wave 23 task contract.

(task wave23-02-renderer-report-trace-fields-v1
  :schema "missiond.task-contract.v1"
  :title "Renderer and report trace fields v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-01-session-trace-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Surface the session-trace path in rendered task briefs and add worker explanation fields to report-contract without making report prose the factual source."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     "scripts/check-task-report.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp"
     ".missiond/claudecode/wave23-01-session-trace-schema-v0.md"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     "scripts/check-session-trace.mjs"
     ".missiond/tasks/wave22/**"]

  :requirements
    ["Render session_trace path when .missiond/tasks/<wave>/session-trace.lisp exists."
     "In rendered briefs, instruct workers to append claim/completion to shared-memory and append factual trace entries only when their task contract allows the trace ledger as shared coordination output."
     "Extend report-contract optional fields for worker explanations: :time_sinks, :major_decisions, :unexpected_work, :blockers, :trace_refs."
     "Report checker should validate these fields structurally but not treat them as facts."
     "Re-render wave23-01 as a golden example."]

  :acceptance
    ["node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp"
     "git diff --check -- scripts/render-claudecode-task.mjs scripts/check-task-report.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp .missiond/claudecode/wave23-01-session-trace-schema-v0.md"]

  :commit
    (:required true
     :message "feat(tasks): surface session trace in briefs and reports"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered sections added."
     "Report explanation fields."
     "Acceptance command results."])
