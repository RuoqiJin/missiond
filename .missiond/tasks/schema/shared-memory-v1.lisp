;; MissionD shared-memory ledger v1
;; Purpose: a Lisp-shaped append-only journal where parallel ClaudeCode
;; agents coordinate without prose chat: claims, observations, blockers,
;; commits, and handoff pointers.
;;
;; Loadbearing rule: the ledger file IS the one shared write target for
;; coordination. Every other write each agent performs MUST stay inside
;; its own task contract :write-scope. Appending here is allowed even
;; when the ledger path is not in the agent's :write-scope, because the
;; ledger is reserved for coordination metadata only — never code.

(shared-memory-schema missiond.shared-memory.v1
  :version "v1"
  :status "code-aligned initial — checker scripts/check-task-memory.mjs"
  :checker "scripts/check-task-memory.mjs"
  :seed ".missiond/tasks/wave19/shared-memory.lisp"

  (purpose
    "Give parallel ClaudeCode agents a single S-expression ledger to publish claims, observations, blockers, completions, corrections, and handoffs."
    "Replace ad hoc chat / out-of-band notes with a deterministic, machine-checkable record."
    "Keep entries small and orthogonal so MissionD plan-runner and reviewers can stream them without a parser dialect.")

  (write-rules
    "An agent appends entries to the ledger only while it owns a live :claim entry for its task id."
    "Each entry's :touched files MUST be a subset of the appending agent's task :write-scope, except :touched may be empty (e.g. an :observation about a file the agent only read)."
    "The ledger file itself is shared coordination memory and is exempt from the per-task :write-scope rule for append-only entry insertion. Editing or rewriting prior entries is forbidden."
    "Entries are append-only. Corrections are recorded as a new (correction ...) entry referencing the prior :id, never by editing history.")

  (file-shape
    :file ".missiond/tasks/<wave>/shared-memory.lisp"
    :form (shared-memory <wave-id>
            :schema "missiond.shared-memory.v1"
            :wave <wave-id>
            :created-at <iso8601>
            :sequence <monotonic-int>
            <entry> ...))

  (header-fields
    [:schema :wave :created-at :sequence])

  (entry-heads
    [claim observation blocker completion correction handoff])

  (entry-contract
    (:id "stable kebab-id, unique within file (e.g. wave19-04-claim-001)")
    (:task "task contract id this entry belongs to (e.g. wave19-04-shared-memory-ledger-v0)")
    (:agent "owning agent or workstation class (e.g. claudecode)")
    (:seq "monotonic integer per file, strictly increasing")
    (:at "ISO-8601 UTC timestamp; required when :seq alone would be ambiguous")
    (:touched "vector of repo-relative paths the entry references; may be empty")
    (:summary "single-line human-readable note; required for every entry")
    (:refs "optional vector of prior entry :id values for cross-reference (correction/handoff use this)"))

  (entry-shapes
    (claim
      :required [:id :task :agent :seq :summary]
      :optional [:at :touched :refs]
      :meaning "Agent declares it has begun work on the named task.")
    (observation
      :required [:id :task :agent :seq :summary]
      :optional [:at :touched :refs]
      :meaning "A fact discovered during the task; non-blocking, no action required from peers.")
    (blocker
      :required [:id :task :agent :seq :summary]
      :optional [:at :touched :refs]
      :meaning "Work cannot proceed; peers or operator must resolve. Reference the unblocking entry via :refs when cleared.")
    (completion
      :required [:id :task :agent :seq :summary :touched]
      :optional [:at :refs]
      :meaning "Agent reports task done. :touched lists the files actually changed. Must be inside the task's :write-scope.")
    (correction
      :required [:id :task :agent :seq :summary :refs]
      :optional [:at :touched]
      :meaning "Append-only fix for an earlier entry. :refs points to the prior entry id being corrected.")
    (handoff
      :required [:id :task :agent :seq :summary :refs]
      :optional [:at :touched]
      :meaning "Pointer to follow-up work or next agent. :refs points to the entry receiving the baton."))

  (validation-contract
    :file-must-have-header [:schema :wave :created-at :sequence]
    :unique [:id]
    :strictly-increasing [:seq]
    :path-style "all :touched paths are repo-relative; absolute paths and `..` are rejected"
    :id-style "lowercase kebab/dot/underscore; must match ^[a-z0-9][a-z0-9._-]*$"
    :task-id-style "matches the same id rule and SHOULD reference a task in .missiond/tasks/<wave>/"
    :timestamp-style "ISO-8601 with timezone, e.g. 2026-04-26T15:30:00Z"
    :no-prose "entries are S-expressions only; free-form prose belongs in :summary strings, never as bare atoms")

  (non-goals
    "The ledger is not a chat log; do not write conversational dialogue."
    "The ledger is not a build artifact registry; build outputs stay in their own paths."
    "The ledger is not a substitute for git history; :touched is informational, not transactional."))
