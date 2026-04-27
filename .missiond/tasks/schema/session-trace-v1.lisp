;; MissionD session-trace v1
;; Purpose: a Lisp-shaped append-only ledger of factual telemetry emitted by
;; agents while they execute MissionD tasks. Worker reports explain WHY a task
;; succeeded or failed; the session-trace records WHAT happened — concrete
;; events such as dispatch, file reads/edits, shell commands, test runs, and
;; commits — without prose narration.
;;
;; The trace is the load-bearing input for future router/backend optimization
;; (latency/cost analysis, retry policy tuning, plan-runner reasoning) and is
;; therefore MissionD-owned, independent of any single worker's prose output.
;;
;; Loadbearing rule: trace events are facts, not explanations. :summary is a
;; single-line label, never a paragraph. Optional fields hold structured data
;; (file paths, exit codes, durations); narrative belongs in the worker
;; report, never here.

(session-trace-schema missiond.session-trace.v1
  :version "v1"
  :status "code-aligned — checker scripts/check-session-trace.mjs; seed .missiond/tasks/wave23/session-trace.lisp"
  :checker "scripts/check-session-trace.mjs"
  :seed ".missiond/tasks/wave23/session-trace.lisp"

  (purpose
    "Provide MissionD with factual, machine-parsable per-task execution telemetry independent of free-form worker reports."
    "Make dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events queryable by analyzers, planners, and router policy drafters."
    "Decouple telemetry collection from prose so router and backend optimization can reason about timing, exit codes, and file scope without parsing narrative.")

  (write-rules
    "An agent appends entries to the trace only while it is executing the named task; the file is append-only."
    "Events MUST be facts: :summary is a single short line; structured data (paths, exit codes, durations) belongs in dedicated optional fields."
    "Editing or rewriting prior entries is forbidden. Mistakes are corrected by appending a new (trace-event ... :kind observation ...) entry referencing the prior :id via :trace_refs."
    "All :files / :report_path / :trace_refs / :command paths MUST be repo-relative. Absolute paths and parent-traversal segments are rejected by the checker."
    "Multiple agents may append to the same trace file; :id uniqueness and strictly increasing :seq enforce ordering.")

  (file-shape
    :file ".missiond/tasks/<wave>/session-trace.lisp"
    :form (session-trace <wave-id>
            :schema "missiond.session-trace.v1"
            :wave <wave-id>
            :created-at <iso8601>
            :sequence <monotonic-int>
            <trace-event> ...))

  (header-fields
    [:schema :wave :created-at :sequence])

  (entry-heads
    [trace-event])

  (event-kinds
    [dispatch start read edit command test commit complete failure retry observation])

  (kind-meaning
    (dispatch     "Task contract dispatched to an agent or workstation; first event in a task's trace.")
    (start        "Agent acknowledged the task and began execution (after dispatch).")
    (read         "Agent read one or more files; :files lists the paths read.")
    (edit         "Agent edited one or more files; :files lists the paths written.")
    (command      "Agent ran a shell or tool command; :command holds the command line, :exit_code its result, :duration_ms wall-clock time.")
    (test         "Agent ran an acceptance / verification command; same shape as command but classified for analyzers.")
    (commit       "Agent created a git commit; :commit_hash is the resolved SHA, :files lists the changed paths if known.")
    (complete     "Agent reports the task done; :report_path points at the machine report when written.")
    (failure      "Agent hit an unrecoverable error in the current attempt; :summary describes the proximate cause.")
    (retry        "Agent or orchestrator scheduled a retry; :trace_refs may point at the failure entry being retried.")
    (observation  "Non-actionable fact captured during execution (e.g. environment detection, drift notice). Also used for trace corrections."))

  (entry-contract
    (:id "stable kebab-id, unique within file (e.g. wave23-01-trace-001)")
    (:seq "monotonic integer per file, strictly increasing across all events")
    (:at "ISO-8601 timestamp with timezone; required for every event")
    (:task "task contract id this event belongs to")
    (:backend "executing backend or workstation class (e.g. claudecode, codex-orchestrator, sonnet-worker)")
    (:kind "one of the event-kinds above")
    (:summary "single-line factual label; never a paragraph")
    (:agent "optional agent / session identifier when distinct from :backend")
    (:files "optional vector of repo-relative paths touched by this event; absolute paths rejected")
    (:command "optional repo-relative or shell command string actually executed; absolute paths inside the command argv are rejected")
    (:exit_code "optional integer process exit code; positive, zero, or negative — must be an integer")
    (:duration_ms "optional non-negative integer wall-clock duration in milliseconds")
    (:commit_hash "optional git commit SHA (full or short) for kind=commit events")
    (:report_path "optional repo-relative path to the machine report .lisp; absolute paths rejected")
    (:memory_refs "optional vector of shared-memory entry :id values referenced by this event")
    (:trace_refs "optional vector of prior trace-event :id values referenced by this event (used by retry / observation corrections)"))

  (required-event-fields
    [:id :seq :at :task :backend :kind :summary])

  (optional-event-fields
    [:agent :files :command :exit_code :duration_ms :commit_hash :report_path :memory_refs :trace_refs])

  (validation-contract
    :file-must-have-header [:schema :wave :created-at :sequence]
    :unique [:id]
    :strictly-increasing [:seq]
    :path-style "all :files / :report_path / :trace_refs / :command paths are repo-relative; absolute paths and `..` segments are rejected"
    :id-style "lowercase kebab/dot/underscore; must match ^[a-z0-9][a-z0-9._-]*$"
    :timestamp-style "ISO-8601 with timezone, e.g. 2026-04-28T15:30:00Z or 2026-04-28T15:30:00+08:00"
    :duration-style ":duration_ms is a non-negative integer (milliseconds)"
    :exit-code-style ":exit_code is any integer (positive, zero, or negative)"
    :no-prose "events are S-expressions only; narrative belongs in worker reports, not here")

  (checker-contract
    :input ".missiond/tasks/**/session-trace.lisp + ad hoc files"
    :modes [single-file dry-fixture]
    :flags [--json --dry-fixture]
    :rejects
      ["duplicate :id values"
       "non-increasing :seq"
       "invalid timestamps (non ISO-8601 with timezone)"
       "absolute paths in :files / :report_path / :trace_refs / :command"
       "missing :task / :backend / :kind"
       "malformed :duration_ms (not a non-negative integer)"
       "malformed :exit_code (not an integer)"
       "unknown :kind value"
       "unknown entry head (only trace-event allowed)"
       "schema mismatch"]
    :non-goal
      "checker does NOT execute or replay any traced command; it only validates structure.")

  (non-goals
    "The trace is not a conversation log; do not embed dialogue."
    "The trace is not a substitute for the worker report; reports explain why, traces record what."
    "The trace is not a build artifact registry; build outputs stay in their own paths."))
