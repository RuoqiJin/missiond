;; Wave 23 task contract.

(task wave23-01-session-trace-schema-v0
  :schema "missiond.task-contract.v1"
  :title "Session trace schema and checker v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-00-archive-wave22-task-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Create the MissionD-owned session-trace Lisp schema and checker so factual task execution telemetry can be collected independently of worker prose reports."

  :write-scope
    [".missiond/tasks/schema/session-trace-v1.lisp"
     ".missiond/tasks/wave23/session-trace.lisp"
     "scripts/check-session-trace.mjs"
     "scripts/lib/missiond_lisp.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave22/**"
     ".missiond/claudecode/wave22-*.md"]

  :requirements
    ["Define session-trace-v1 as append-only data Lisp for facts, not explanations."
     "Entry heads or kind values must cover at least: dispatch, start, read, edit, command, test, commit, complete, failure, retry, observation."
     "Required event fields: :id, :seq, :at, :task, :backend, :kind, :summary."
     "Optional fields should include: :agent, :files, :command, :exit_code, :duration_ms, :commit_hash, :report_path, :memory_refs, :trace_refs."
     "Add scripts/check-session-trace.mjs with --dry-fixture, --json, and single-file validation."
     "Reject duplicate ids, non-increasing seq, invalid timestamps, absolute paths, missing task/backend/kind, and malformed duration/exit codes."]

  :acceptance
    ["node scripts/check-session-trace.mjs --dry-fixture"
     "node scripts/check-session-trace.mjs .missiond/tasks/wave23/session-trace.lisp"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- .missiond/tasks/schema/session-trace-v1.lisp .missiond/tasks/wave23/session-trace.lisp scripts/check-session-trace.mjs scripts/lib/missiond_lisp.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add session trace contract"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Trace event kinds."
     "Checker dry-fixture coverage."
     "Acceptance command results."])
