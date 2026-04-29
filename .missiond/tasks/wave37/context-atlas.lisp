;; Wave 37 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave37-request-receipt-projection-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave37
  :goal "Project verification receipts into request-local Lisp artifacts without breaking legacy receipt-set readers."
  :read-order [".missiond/claudecode/wave37-shared-preamble.md"
               ".missiond/tasks/wave37/context-atlas.lisp"
               ".missiond/tasks/wave37/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Artifact contract already names request-local verification-receipt path; task-runner implementation note needs the code-aligned writer/projection."
      :grep ["artifact verification-receipt"
             "surface task-runner-cli"
             "verification-receipt final-report"])
    (file "scripts/check-verification-receipt.mjs"
      :purpose "Receipt parser, validator, reusable-receipt rules, and fixture suite. Best place to share render/write validation helpers if no new narrow helper is needed."
      :grep ["export const SCHEMA"
             "projectReceipt"
             "readVerificationReceiptFile"
             "validateReceiptObject"
             "runFixtures"
             "verification-receipt fixtures OK"])
    (file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :purpose "Pins task-runner lifecycle/final-report/receipt code surfaces against the V3 Lisp blueprint."
      :grep ["DEFAULT_FILES"
             "verification-receipt"
             "request-local final-report"
             "writeRequestFinalReportFile"])
    (file "scripts/verify-task-runner-batch.mjs"
      :purpose "Optional cross-layer smoke only. Do not change default JSON shape when receipts are omitted."
      :grep ["readVerificationReceiptFile"
             "validateReceiptObject"
             "receipt_coverage"
             "lifecycle receipt finalized-report smoke"])))
