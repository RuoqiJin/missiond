;; Wave 37 dispatch-time pattern cards.

(pattern-cards wave37-request-receipt-projection-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave37

  (card add-request-local-projection-without-breaking-legacy-input
    :use-for [wave37-01-request-verification-receipt-v0]
    :summary "Add request-local artifact writers as optional projections; preserve legacy task-scoped readers and default JSON shapes."
    :recipe ["Keep readVerificationReceiptFile behavior stable for existing receipt-set inputs."
             "Add explicit request-id/request-dir helper paths rather than making every validation call write files."
             "Validate generated Lisp bytes using the same parser/checker before publishing."
             "Pin the optional projection in V3 isomorphism checks and fixtures."]
    :known-good ["scripts/task-runner-finalize-report.mjs"
                 "scripts/task-runner-append-event.mjs"])

  (card receipt-reuse-remains-advisory
    :use-for [wave37-01-request-verification-receipt-v0]
    :summary "Verification receipts accelerate evidence reuse but never replace source/report/commit verification."
    :recipe ["Do not widen isReceiptReusable rules."
             "Do not let request-local receipts make verify-task-runner-batch skip contract/report/memory/commit checks."
             "If a cross-layer smoke is added, assert receipt_coverage is additive only."
             "Keep no-receipts output byte-shape compatible."]
    :known-good ["scripts/check-verification-receipt.mjs"
                 "scripts/verify-task-runner-batch.mjs"]))
