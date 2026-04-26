;; Wave 19 task contract.

(task wave19-03-report-contract-v1
  :schema "missiond.task-contract.v1"
  :title "ClaudeCode report contract v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-00-machine-contract-pilot"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Define a machine-readable completion report Lisp contract and checker so ClaudeCode results can be verified without parsing prose."

  :write-scope
    [".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp"
     "scripts/check-task-report.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     "scripts/check-task-contract.mjs"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["Add report-contract-v1.lisp describing fields: task_id, status, commit_hash, files_changed, acceptance_results, scope_deviations, notes."
     "Add scripts/check-task-report.mjs with --dry-fixture and single-file validation."
     "Create one sample report for wave19-00-machine-contract-pilot under .missiond/tasks/wave19/reports/ with status draft or done as appropriate; it is a schema example, not proof of task execution."
     "Checker must reject missing task_id, invalid status, empty acceptance_results when status=done, and absolute file paths."]

  :acceptance
    ["node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-task-report.mjs .missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp scripts/check-task-report.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add machine-readable task reports"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Report schema fields."
     "Checker dry-fixture results."
     "Acceptance command results."])
