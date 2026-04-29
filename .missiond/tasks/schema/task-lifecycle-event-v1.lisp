;; MissionD task lifecycle event v1
;; Purpose: an append-only, orchestrator-owned event log for task lifecycle
;; facts. During migration, tooling projects these events back into
;; shared-memory.lisp and session-trace.lisp so existing validators keep
;; working while workers stop hand-editing max-seq ledgers.

(task-lifecycle-event-schema missiond.task-lifecycle-event.v1
  :version "v1"
  :status "code-aligned -- checker scripts/check-task-lifecycle-events.mjs; append helper scripts/task-runner-append-event.mjs; projection helper scripts/project-task-lifecycle-ledger.mjs; wave31 adds cancelled lifecycle facts for wrong-dispatch rollback; wave39 promotes task-scoped one-event files as the primary path while the legacy ledger remains a compatibility projection/input"
  :checker "scripts/check-task-lifecycle-events.mjs"
  :append-helper "scripts/task-runner-append-event.mjs"
  :projector "scripts/project-task-lifecycle-ledger.mjs"

  (purpose
    "Record task dispatches, claims, trace starts, reads, worker commits, parent hotfixes, finalized reports, verification receipts, completions, cancellations, and issues as durable lifecycle facts."
    "Give cooperating workers and orchestrator tools one append helper so seq/id allocation is centralized instead of copied into ad hoc edits."
    "Project current shared-memory/session-trace facts from this ledger during migration.")

  (accepted-shapes
    "Two task-scoped Lisp shapes are accepted by the checker and writers."
    (shape standalone-task-event-file
      :primary true
      :file ".missiond/tasks/<wave>/events/<seq>.event.lisp"
      :form (lifecycle-event
              :schema "missiond.task-lifecycle-event.v1"
              :wave <wave-id>
              :id <event-id>
              :task <task-id>
              :actor_role <role>
              :event_kind <kind>
              :commit_role <role>
              :seq <monotonic-int>
              :at <iso8601>
              :touched [<repo-relative-path> ...]
              :summary <text>
              <optional-fields...>)
      :note "One lifecycle-event form per file; the standalone task-scoped form carries :schema=missiond.task-lifecycle-event.v1 and :wave so the file can be validated in isolation. Files are numeric-sequenced (000001.event.lisp, 000002.event.lisp, ...). When a directory of these files is checked together, the checker enforces unique :id and strictly-increasing :seq across the whole directory.")
    (shape legacy-task-lifecycle-event-log
      :primary false
      :compatibility true
      :file ".missiond/tasks/<wave>/task-lifecycle-events.lisp"
      :form (task-lifecycle-event-log <ledger-id>
              :schema "missiond.task-lifecycle-event.v1"
              :wave <wave-id>
              :created-at <iso8601>
              :sequence <monotonic-int>
              <lifecycle-event> ...)
      :note "Legacy single-file ledger; remains a compatibility projection/input. Existing --ledger callers continue to work; new dispatch code should prefer --events-dir."))

  (file-shape
    :file ".missiond/tasks/<wave>/task-lifecycle-events.lisp"
    :form (task-lifecycle-event-log <ledger-id>
            :schema "missiond.task-lifecycle-event.v1"
            :wave <wave-id>
            :created-at <iso8601>
            :sequence <monotonic-int>
            <lifecycle-event> ...))

  (event-kinds
    [dispatch claim trace_start read worker_commit parent_hotfix finalized_report receipt completion cancelled issue])

  (commit-roles
    [none worker parent_hotfix final receipt verified])

  (actor-roles
    "Free-form compact ids such as worker, orchestrator, verifier, prepare-task-runner-wave, codex-wave30-worker-03. The checker enforces id shape, not a closed enum.")

  (event-contract
    (:id "stable kebab/dot/underscore id unique within the file.")
    (:task "task contract id; must match ^[a-z0-9][a-z0-9._-]*$.")
    (:actor_role "compact role or tool id that emitted the lifecycle fact.")
    (:event_kind "one of dispatch | claim | trace_start | read | worker_commit | parent_hotfix | finalized_report | receipt | completion | cancelled | issue.")
    (:commit_role "one of none | worker | parent_hotfix | final | receipt | verified. Use none when no commit is associated.")
    (:seq "strictly increasing integer within the file.")
    (:at "ISO-8601 timestamp with timezone.")
    (:touched "vector of repo-relative files referenced by the event; may be empty.")
    (:summary "single-line factual label.")
    (:commit_hash "optional git commit SHA, 7-64 lowercase/uppercase hex chars.")
    (:report_path "optional repo-relative finalized report path.")
    (:receipt_path "optional repo-relative verification receipt path.")
    (:refs "optional vector of prior lifecycle event ids.")
    (:legacy_memory_id "optional projected shared-memory entry id override.")
    (:legacy_trace_id "optional projected session-trace event id override."))

  (required-event-fields
    [:id :task :actor_role :event_kind :commit_role :seq :at :touched :summary])

  (optional-event-fields
    [:commit_hash :report_path :receipt_path :refs :legacy_memory_id :legacy_trace_id])

  (validation-contract
    :file-must-have-header [:schema :wave :created-at :sequence]
    :unique [:id]
    :strictly-increasing [:seq]
    :known-event-kinds [dispatch claim trace_start read worker_commit parent_hotfix finalized_report receipt completion cancelled issue]
    :known-commit-roles [none worker parent_hotfix final receipt verified]
    :path-fields [:touched :report_path :receipt_path]
    :path-style "repo-relative only; absolute paths, ~, empty paths, and .. traversal are rejected"
    :id-style "lowercase kebab/dot/underscore; must match ^[a-z0-9][a-z0-9._-]*$"
    :task-id-style "same id style as task contracts"
    :commit-hash-style "^[0-9a-fA-F]{7,64}$"
    :timestamp-style "ISO-8601 with timezone, e.g. 2026-04-28T15:30:00Z"
    :no-prose "events are S-expressions; narrative belongs in :summary only.")

  (concurrency-contract
    "scripts/task-runner-append-event.mjs uses a sibling .lock file with exclusive create, validates the ledger after every append, writes a temp file, then atomically renames it over the ledger."
    "For standalone task-scoped event files, scripts/task-runner-append-event.mjs locks the events directory via a sibling .lock file, allocates the next numeric sequence under the lock, validates the candidate event bytes through scripts/check-task-lifecycle-events.mjs, and creates the final file via fs.openSync(file, 'wx') so two cooperating writers cannot overwrite the same numeric file."
    "This serializes cooperating local filesystem writers. It cannot protect against tools that ignore the lock, non-atomic network filesystems, or manual edits performed while the lock exists.")

  (projection-contract
    "dispatch projects to shared-memory observation and session-trace dispatch."
    "claim projects to shared-memory claim and session-trace start."
    "trace_start projects to session-trace start and, for bootstrap/tool events, may project an observation into shared-memory."
    "read projects to session-trace read."
    "worker_commit and parent_hotfix project to session-trace commit; parent_hotfix also projects a shared-memory observation."
    "finalized_report and receipt project to trace observation/test-like facts and shared-memory observations."
    "completion projects to shared-memory completion and session-trace complete."
    "cancelled cancels a not-yet-committed dispatch attempt so task-runner-wave-state can requeue the task without fabricating a completion."
    "issue projects to shared-memory blocker and session-trace failure."))
