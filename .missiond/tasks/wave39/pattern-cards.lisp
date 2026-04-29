;; Wave 39 dispatch-time pattern cards.

(pattern-cards wave39-task-scoped-event-files-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave39

  (card primary-event-file-with-legacy-projection
    :use-for [wave39-01-task-scoped-lifecycle-event-files-v0]
    :summary "Promote the event-file representation without breaking historical ledger readers."
    :recipe ["Start by tightening the Lisp/schema wording: task-scoped .event.lisp files are primary; task-lifecycle-events.lisp is a compatibility projection/input."
             "Add the checker shape before changing appenders, so every writer can validate bytes before rename."
             "Keep old --ledger callers working; add --events-dir as an additive path and prefer it in new dispatch code."
             "Wave-state should read event files when present and fall back to legacy ledger for old waves."]
    :known-good ["scripts/task-runner-append-event.mjs :: writeRequestLifecycleEventFile"
                 "scripts/check-task-lifecycle-events.mjs :: validateRequestLifecycleEventFile"])

  (card no-double-count-on-dual-input
    :use-for [wave39-01-task-scoped-lifecycle-event-files-v0]
    :summary "During migration, a task event may exist in both event files and a legacy ledger; state projection must not count it twice."
    :recipe ["Use event id or an idempotency key to dedupe merged lifecycle facts."
             "Fixtures should cover event-file only, ledger only, and both-present cases."
             "Preserve lifecycle_event_count semantics for task-runner-wave-state, but count unique event ids."]
    :known-good ["scripts/task-runner-wave-state.mjs :: readOptionalLifecycleEvents"
                 "scripts/verify-task-runner-batch.mjs :: lifecycle receipt finalized-report smoke"])

  (card atomic-create-under-directory-lock
    :use-for [wave39-01-task-scoped-lifecycle-event-files-v0]
    :summary "Sequence allocation for one-file events needs a directory-level lock, validate-before-create, and create-only final writes."
    :recipe ["Lock the events directory or a sibling .lock before scanning max seq."
             "Render candidate bytes and validate them through check-task-lifecycle-events before final placement."
             "Use a temp file plus atomic rename or open('wx') for final create; never overwrite an existing event file."
             "Dry fixtures should simulate at least two appends and assert monotonically increasing numeric filenames."]
    :known-good ["scripts/task-runner-append-event.mjs :: withLedgerLock"
                 "scripts/task-runner-append-event.mjs :: nextRequestEventSeq"]))
