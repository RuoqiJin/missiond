;; Wave 37 task contract.

(task wave37-01-request-verification-receipt-v0
  :schema "missiond.task-contract.v1"
  :title "request-local verification receipt projection v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave37/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave37/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Close the next task-runner Lisp-isomorphism gap: verification receipts should have a request-local writer/projection under .missiond/requests/<request_id>/receipts/<receipt_id>.lisp, while the legacy task-scoped receipt set remains a compatibility input. Keep receipt reuse advisory and keep existing batch-verifier behavior backward-compatible."

  :write-scope
    ["scripts/check-verification-receipt.mjs"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "scripts/verify-task-runner-batch.mjs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :must-not-touch
    ["crates/**"
     "packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/**"
     ".missiond/tasks/wave35/**"
     ".missiond/tasks/wave36/**"
     ".missiond/tasks/wave37/manifest.lisp"
     ".missiond/tasks/wave37/context-atlas.lisp"
     ".missiond/tasks/wave37/pattern-cards.lisp"
     ".missiond/tasks/wave37/wave37-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update .missiond/v3/missiond-blueprint.lisp first. The task-runner surface should state that verification receipts can be projected to .missiond/requests/<request_id>/receipts/<receipt_id>.lisp, while legacy receipt-set files remain compatibility inputs."
     "Add a deterministic request-local receipt writer/projection. It may live in check-verification-receipt.mjs as exported helpers or in a narrowly named task-runner receipt helper, but it must render a single verification-receipt Lisp artifact with schema missiond.verification-receipt.v1."
     "The writer must validate generated receipt bytes with the existing receipt validator before rename/create. Reject absolute paths, .. traversal, malformed request ids, malformed receipt ids, and invalid receipt objects."
     "Use atomic writes and avoid overwriting unrelated receipt files. If overwrite is supported, it must be explicit; default behavior should be create-only or deterministic safe replace of the same generated artifact."
     "Keep existing verification receipt checking and verify-task-runner-batch --receipts behavior backward-compatible. Existing dry fixtures should still pass without requiring request-local args."
     "Extend check-v3-task-lifecycle-isomorphism.mjs so the V3 Lisp/code contract pins the request-local receipt writer path and helper names."
     "Add at least one fixture that writes a request-local receipt under a temp .missiond/requests/<request_id>/receipts directory and then validates it through check-verification-receipt.mjs."
     "Optionally update verify-task-runner-batch.mjs only for a cross-layer smoke fixture; do not change its default JSON shape when --receipts is omitted."]

  :acceptance
    ["node scripts/check-verification-receipt.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' scripts/check-verification-receipt.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs .missiond/v3/missiond-blueprint.lisp"
     "git diff --check -- scripts/check-verification-receipt.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs .missiond/v3/missiond-blueprint.lisp"]

  :commit
    (:required true
     :message "feat(tasks): project request verification receipts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Request-local verification-receipt artifact shape."
     "Writer/projection helper or CLI entrypoint."
     "Backward-compat behavior for existing receipt-set inputs."
     "Acceptance command results."])
