;; Wave 52 dispatch-time context atlas.

(context-atlas wave52-contract-artifact-validation-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave52
  :goal "Make verify-task-contract validate Lisp artifacts touched by the verified commit."
  :read-order [".missiond/claudecode/wave52-shared-preamble.md"
               ".missiond/tasks/wave52/context-pack.lisp"
               ".missiond/tasks/wave52/wave52-01-contract-artifact-validation-v0.lisp"
               ".missiond/tasks/wave52/context-atlas.lisp"
               ".missiond/tasks/wave52/pattern-cards.lisp"
               "scripts/verify-task-contract.mjs"
               "scripts/check-session-trace.mjs"
               "scripts/check-task-memory.mjs"
               "scripts/check-task-report.mjs"
               "scripts/check-task-lifecycle-events.mjs"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-task-lifecycle-isomorphism.mjs"]

  (global-anchors
    (file ".missiond/tasks/wave52/context-pack.lisp"
      :purpose "Shard authority for this code worker."
      :grep ["wave52-integration-plan-001"
             "contract-artifact-validation"
             "7f462d17"])
    (file "scripts/verify-task-contract.mjs"
      :purpose "Primary implementation target. Currently checks commit message, write-scope, and must-not-touch only."
      :grep ["export function verifyContract"
             "export function readCommit"
             "runFixtures"
             "This verifier is read-only"])
    (file "scripts/check-session-trace.mjs"
      :purpose "Existing validator that rejects :kind acceptance; use by child-process or commit-byte temp file validation rather than duplicating its schema."
      :grep ["export const KIND_VALUES"
             "'test'"
             "'observation'"
             "unknown :kind values"])
    (file "scripts/check-task-memory.mjs"
      :purpose "Existing shared-memory ledger validator for touched shared-memory.lisp artifacts."
      :grep ["shared-memory check OK"
             "ENTRY_SHAPES"
             "strictly increasing"])
    (file "scripts/check-task-report.mjs"
      :purpose "Existing report validator for touched report artifacts."
      :grep ["missiond.report-contract.v1"
             ":parent_patches"
             "task-report fixtures OK"])
    (file "scripts/check-task-lifecycle-events.mjs"
      :purpose "Existing lifecycle ledger/event validator for task-lifecycle-events.lisp and events/*.event.lisp artifacts."
      :grep ["validateLifecycleEventFiles"
             "validateTaskScopedLifecycleEventFile"
             "task-lifecycle-events fixtures OK"])
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 task-runner-cli invariant text."
      :grep ["surface task-runner-cli"
             "verify-task-runner-batch imports lifecycle validation"
             "project-task-lifecycle-ledger backfills shared-memory/session-trace compatibility facts"])
    (file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :purpose "Checker that pins the task-runner-cli lifecycle surface into code."
      :grep ["surface task-runner-cli"
             "requireAll"
             "verify-task-runner-batch"])))
