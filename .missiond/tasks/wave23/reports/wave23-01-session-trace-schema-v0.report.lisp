;; Wave 23 task report.

(report wave23-01-session-trace-schema-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave23-01-session-trace-schema-v0"
  :status done
  :commit_hash "2f173ee97839"
  :files_changed
    [".missiond/tasks/schema/session-trace-v1.lisp"
     ".missiond/tasks/wave23/session-trace.lisp"
     "scripts/check-session-trace.mjs"]
  :acceptance_results
    [(:command "node scripts/check-session-trace.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "22 fixture cases across 18 categories: 3 pass cases (bootstrap, all 11 kinds, optional fields populated, negative exit code) and 15 fail-rule categories (schema, header, entry-head, duplicate-id, seq, timestamp, abs-files, abs-report-path, abs-command, trace-refs, missing-task, missing-backend, missing-kind, unknown-kind, duration, exit-code, unknown-field).")
     (:command "node scripts/check-session-trace.mjs .missiond/tasks/wave23/session-trace.lisp"
      :exit_code 0
      :ok true
      :notes "Seed file validates as-is: 1 trace, 1 bootstrap observation event. No seed adjustments needed.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "57 task contracts across all waves remain green.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 architecture lisp files unchanged and green; this task did not touch .missiond/v2/*.lisp.")
     (:command "git diff --check -- .missiond/tasks/schema/session-trace-v1.lisp .missiond/tasks/wave23/session-trace.lisp scripts/check-session-trace.mjs scripts/lib/missiond_lisp.mjs"
      :exit_code 0
      :ok true
      :notes "No whitespace errors on the four write-scope paths.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "Hooks preflight aligned: core.hooksPath == .githooks, .githooks/pre-commit present and executable.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "3 staged files all inside :write-scope; zero matches against :must-not-touch.")
     (:command "MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp git commit -m \"feat(tasks): add session trace contract\""
      :exit_code 0
      :ok true
      :notes "Commit 2f173ee97839: 3 files, +1204 insertions, 0 deletions. Pre-commit scope-guard hook re-ran cleanly inside the commit.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "Subject equals contract :commit :message; every changed_file ⊆ :write-scope; must-not-touch intersection empty.")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave23/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "Ledger now has 5 entries (1 bootstrap + wave23-00 claim/completion + wave23-01 claim/completion); validation green.")]
  :notes "Schema decisions worth flagging: (1) Single entry head trace-event with an enumerated :kind field, modeled after the report-contract pattern rather than the multi-head shared-memory pattern — gives one shape to the analyzer instead of eleven. (2) :exit_code accepts negative integers (signals: -9 = SIGKILL etc.); :duration_ms is strict non-negative integer milliseconds. (3) :command paths are inspected token-by-token for absolute paths after stripping shell variable assignments (FOO=/abs/path), so leaked developer-machine paths get flagged. (4) :trace_refs uses the id-ref shape (kebab-id atoms/strings), so a slash-prefixed ref like ../wave22-trace-001 is rejected by the id regex rather than a path check. (5) scripts/lib/missiond_lisp.mjs was NOT modified — every helper needed (parseLisp, head, isList, readKeywordProps, keywordPropText, nodeText, nodeToStringArray) already existed; lib stayed in the staging set as a no-op git add per the protocol. (6) Seed at .missiond/tasks/wave23/session-trace.lisp validated unmodified; no additive edits were necessary.")
