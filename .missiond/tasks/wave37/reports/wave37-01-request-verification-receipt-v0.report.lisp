;; Wave 37 task report.
;; Schema: missiond.report-contract.v1

(report wave37-01-request-verification-receipt-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave37-01-request-verification-receipt-v0"
  :status done
  :commit_hash "2a2f8df1ba40"
  :files_changed
    ["scripts/check-verification-receipt.mjs"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "scripts/verify-task-runner-batch.mjs"
     ".missiond/v3/missiond-blueprint.lisp"]
  :acceptance_results
    [(result :command "node scripts/check-verification-receipt.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "16 structural + 8 reuse-helper + 9 request-local-writer fixtures across 18 categories; new wave37-01-request-projection-writer category exercises happy-path create-only write, on-disk revalidation through the CLI checker, double-write rejection, replace-mode byte-identical no-op, replace-mode byte-different rejection, .. traversal rejection, absolute receipt id rejection, malformed request id rejection, invalid receipt object rejection, and renderer stale wave/task mismatch rejection.")
     (result :command "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "fixture re-extended for the receiptChecker entry and the request-local writer needles; v3 task lifecycle Lisp/code isomorphism check OK.")
     (result :command "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
             :exit_code 0
             :ok true
             :note "real-tree run pins the request-local receipt writer path .missiond/requests/<request_id>/receipts/<receipt_id>.lisp plus renderRequestVerificationReceipt + validateRequestVerificationReceiptSource + writeRequestVerificationReceiptFile in the V3 blueprint and in scripts/check-verification-receipt.mjs.")
     (result :command "node scripts/verify-task-runner-batch.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "19 fixtures pass; the wave30-05 lifecycle/receipt/finalized-report smoke now also projects the same receipt into a temp request-local receipts directory and re-parses it through readVerificationReceiptFile + isReceiptReusable as a defence-in-depth cross-layer check; default --receipts JSON shape unchanged when --receipts is omitted.")
     (result :command "node scripts/check-lisp-blueprint-compression.mjs"
             :exit_code 0
             :ok true
             :note "v1 manifest + v3 blueprint compression contract still holds after extending the task-runner-cli :note.")
     (result :command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "blueprint architecture-lisp check OK on the updated file.")
     (result :command "perl -ne 'exit 1 if /\\x00/' scripts/check-verification-receipt.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "no NUL bytes in any of the four touched files.")
     (result :command "git diff --check -- scripts/check-verification-receipt.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "no whitespace-error or conflict markers in the staged write-scope files.")]
  :notes "Closes the wave37 task-runner Lisp-isomorphism gap for verification receipts.\n\nArtifact shape (request-local projection): a single (verification-receipt <receipt-id> :schema \"missiond.verification-receipt.v1\" :version \"v1\" :wave <wave> :task_id <task-id> :commit_hash \"<hex>\" :command \"<cmd>\" :exit_code <int> :tier <local|smoke|full> [:started_at + :finished_at] [:duration_ms <int>] [:files [...]] [:notes \"...\"]) Lisp form persisted at .missiond/requests/<request_id>/receipts/<receipt_id>.lisp. The schema is identical to the legacy single-receipt form so verify-task-runner-batch and downstream planners read both shapes through the same parser.\n\nWriter / projection helper: three exported helpers in scripts/check-verification-receipt.mjs — renderRequestVerificationReceipt(receipt, {requestId, receiptId}) renders the Lisp source after running validateReceiptObject + a wave/task-prefix invariant; validateRequestVerificationReceiptSource(source, file) parses the bytes, asserts exactly one (verification-receipt ...) form, and runs the same validateForms used by the CLI checker; writeRequestVerificationReceiptFile({requestReceiptsDir, requestId, receipt, receiptId, mode}) renders + writes a tmp + revalidates the on-disk bytes + atomically claims the target via fs.openSync(target, 'wx'), with mode='create-only' (default) refusing to overwrite an existing file and mode='replace' allowing only a byte-identical idempotent re-render. The writer rejects malformed REQUEST_ID_RE / RECEIPT_ID_RE ids, '..' or '.' segments and path separators in receipt_id, paths that escape the resolved requestReceiptsDir, and any receipt object that fails validateReceiptObject. There is no new top-level CLI flag — the helpers are the public surface so callers (task-runner finalizer, planners) drive projection programmatically.\n\nBackward-compat: legacy task-scoped (verification-receipt-set ...) Lisp files are untouched. readVerificationReceiptFile, projectReceipt, validateReceiptObject, isReceiptReusable, and the existing CLI behavior (including --dry-fixture pass/fail fixtures and the wave29-07 reuse-helper smoke) keep their pre-wave37 byte semantics. verify-task-runner-batch's default JSON shape with --receipts omitted is unchanged; the additive wave37-01 cross-layer smoke writes the same smokeReceipt to a temp request-local directory inside its own fs.mkdtemp + try/finally so it does not leak tmp directories and does not modify the receipt_coverage projection."
  :verification_tier local)
