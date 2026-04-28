(report wave30-03-atomic-lifecycle-event-log-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave30-03-atomic-lifecycle-event-log-v0"
  :status done
  :commit_hash "6c67509992586771cd78bd3ed572ef2dc8c3a900"
  :files_changed [".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
                  "scripts/task-runner-append-event.mjs"
                  "scripts/check-task-lifecycle-events.mjs"
                  "scripts/project-task-lifecycle-ledger.mjs"
                  "scripts/prepare-task-runner-wave.mjs"]
  :acceptance_results [(:command "node scripts/check-task-lifecycle-events.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/task-runner-append-event.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/project-task-lifecycle-ledger.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/prepare-task-runner-wave.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true)
                       (:command "perl -ne 'exit 1 if /\\x00/' scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/project-task-lifecycle-ledger.mjs scripts/prepare-task-runner-wave.mjs" :exit_code 0 :ok true)
                       (:command "git diff --check -- .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/project-task-lifecycle-ledger.mjs scripts/prepare-task-runner-wave.mjs" :exit_code 0 :ok true)]
  :trace_refs [".missiond/tasks/wave30/session-trace.lisp"]
  :event_schema "missiond.task-lifecycle-event.v1"
  :event_kinds [claim trace_start read worker_commit parent_hotfix finalized_report receipt completion issue]
  :append_concurrency_boundary "task-runner-append-event uses a sibling .lock file, rereads under lock, validates the candidate ledger, writes a temp file, then renames atomically. This serializes cooperating local writers only; manual edits, tools that ignore the lock, stale locks until timeout, and non-atomic network filesystems remain outside the guarantee."
  :projection_behavior ["claim -> shared-memory claim + session-trace start"
                        "trace_start/read -> session-trace start/read; bootstrap trace_start can also project the legacy shared-memory observation"
                        "worker_commit/parent_hotfix -> session-trace commit, with parent_hotfix also projecting a shared-memory observation"
                        "finalized_report/receipt -> shared-memory observation + session-trace observation/test"
                        "completion -> shared-memory completion + session-trace complete"
                        "issue -> shared-memory blocker + session-trace failure"]
  :notes "prepare-task-runner-wave now builds bootstrap lifecycle event records in memory and projects them back to the existing ledgers, preserving the CLI side effects while moving bootstrap logic onto the new projection layer.")
