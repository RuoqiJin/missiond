;; Wave 37 shared-memory ledger.

(shared-memory wave37
  :schema "missiond.shared-memory.v1"
  :wave wave37
  :created-at "2026-04-29T00:00:00+08:00"
  :sequence 1

  (observation
    :id wave37-bootstrap-001
    :task wave37-01-request-verification-receipt-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-29T00:00:00+08:00"
    :touched [".missiond/tasks/wave37/manifest.lisp"
              ".missiond/tasks/wave37/context-atlas.lisp"
              ".missiond/tasks/wave37/pattern-cards.lisp"
              ".missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp"]
    :summary "Wave37 theme: project verification receipts into request-local Lisp artifacts while preserving legacy task-scoped receipt compatibility.")

  (observation
    :id wave37-bootstrap-002
    :task wave37-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-29T01:41:23Z"
    :touched [".missiond/claudecode/wave37-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave37-01-request-verification-receipt-v0-claim-003
    :task wave37-01-request-verification-receipt-v0
    :agent claudecode
    :seq 3
    :at "2026-04-29T02:00:00+08:00"
    :touched [".missiond/claudecode/wave37-shared-preamble.md"
              ".missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp"
              ".missiond/tasks/wave37/context-atlas.lisp"
              ".missiond/tasks/wave37/pattern-cards.lisp"]
    :summary "Claiming wave37-01: add request-local verification-receipt projection writer/helpers to scripts/check-verification-receipt.mjs and pin them in the V3 blueprint + isomorphism check.")

  (completion
    :id wave37-01-request-verification-receipt-v0-completion-004
    :task wave37-01-request-verification-receipt-v0
    :agent claudecode
    :seq 4
    :at "2026-04-29T03:00:00+08:00"
    :touched ["scripts/check-verification-receipt.mjs"
              "scripts/check-v3-task-lifecycle-isomorphism.mjs"
              "scripts/verify-task-runner-batch.mjs"
              ".missiond/v3/missiond-blueprint.lisp"]
    :summary "wave37-01 complete: added renderRequestVerificationReceipt + validateRequestVerificationReceiptSource + writeRequestVerificationReceiptFile (atomic create-only with replace-when-byte-identical, validateReceiptObject + structural revalidation before rename, malformed/traversal id rejection); pinned helper names + path in V3 blueprint task-runner-cli surface and check-v3-task-lifecycle-isomorphism.mjs; added 9-step writer fixture (--dry-fixture) and a wave37-01 cross-layer smoke in verify-task-runner-batch.mjs; legacy (verification-receipt-set ...) inputs unchanged, --receipts JSON shape unchanged."))
