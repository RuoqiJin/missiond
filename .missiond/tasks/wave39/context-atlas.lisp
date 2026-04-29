;; Wave 39 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave39-task-scoped-event-files-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave39
  :goal "Make task-scoped lifecycle events first-class one-event files while preserving legacy task-lifecycle-events.lisp compatibility."
  :read-order [".missiond/claudecode/wave39-shared-preamble.md"
               ".missiond/tasks/wave39/context-atlas.lisp"
               ".missiond/tasks/wave39/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
               ".missiond/tasks/wave39/wave39-01-task-scoped-lifecycle-event-files-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 authority. Top-level artifact lifecycle already says lifecycle-event is one event per file; implementation-map task-runner-cli still admits a compatibility ledger main path."
      :grep ["artifact lifecycle-event"
             "one event per file"
             "surface task-runner-cli"
             "task-scoped compatibility lifecycle ledger"
             "request-local one-event files"])
    (file ".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
      :purpose "Task lifecycle event schema and migration contract."
      :grep ["task-lifecycle-event-log"
             "known-event-kinds"
             "scripts/task-runner-append-event.mjs"
             "standalone"
             "one event per file"])
    (file "scripts/check-task-lifecycle-events.mjs"
      :purpose "Structural checker; currently validates legacy logs and request-local event files."
      :grep ["REQUEST_EVENT_SCHEMA"
             "validateLifecycleEventFiles"
             "validateLifecycleEventLog"
             "validateRequestLifecycleEventFile"
             "eventFromNode"
             "strictly greater"])
    (file "scripts/task-runner-append-event.mjs"
      :purpose "Append helper; currently serializes writes to the legacy ledger and request-local event projection."
      :grep ["appendLifecycleEvent"
             "withLedgerLock"
             "writeRequestLifecycleEventFile"
             "renderRequestLifecycleEventFile"
             "nextRequestEventSeq"])
    (file "scripts/task-runner-wave-state.mjs"
      :purpose "State projector; should consume task-scoped event files before/falling back to legacy ledger."
      :grep ["readOptionalLifecycleEvents"
             "defaultLifecyclePath"
             "lifecycle_event_count"
             "parentHotfixUnfinalized"])
    (file "scripts/task-runner-next-action.mjs"
      :purpose "Dispatch action planner; emits dispatch lifecycle events through appendLifecycleEvent."
      :grep ["emitDispatchEventsForActions"
             "appendLifecycleEvent"
             "requestEventsDir"])
    (file "scripts/task-runner-dispatch.mjs"
      :purpose "Read-only descriptor builder plus optional dispatch event append path."
      :grep ["--emit-dispatch-events"
             "requestEventsDir"
             "delegate_calls"])
    (file "scripts/task-runner-submit-dispatch.mjs"
      :purpose "Live submitter; should pass the task-scoped events-dir to dispatch emission."
      :grep ["submitDispatch"
             "--request-events-dir"
             "dispatch_result"])
    (file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :purpose "Cross-layer checker that must pin the new task-scoped event-file path."
      :grep ["task-scoped compatibility lifecycle ledger"
             "request-local one-event files"
             "task-runner-append-event is the only cooperative mutation helper"
             "buildFixture"])))
