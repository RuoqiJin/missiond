(report wave30-05-lifecycle-receipt-smoke-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave30-05-lifecycle-receipt-smoke-v0"
  :status done
  :commit_hash "119ce7c5241088a535660e6f564e05470e392986"
  :files_changed ["scripts/verify-task-runner-batch.mjs"]
  :acceptance_results
    [(:command "node scripts/task-runner-finalize-report.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/task-runner-parent-hotfix.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/task-runner-append-event.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-lifecycle-events.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-staged-source-hygiene.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-verification-receipt.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/verify-task-runner-batch.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/plan-task-runner.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true)
     (:command "perl -ne 'exit 1 if /\\x00/' scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/check-staged-source-hygiene.mjs scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs scripts/plan-task-runner.mjs" :exit_code 0 :ok true)
     (:command "git diff --check -- scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/check-staged-source-hygiene.mjs scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs scripts/plan-task-runner.mjs" :exit_code 0 :ok true)]
  :major_decisions
    [(:decision "Cross-layer smoke lives in verify-task-runner-batch fixtures."
      :why "It is the join point that already sees finalized reports, memory completions, receipts, and manifest nodes.")
     (:decision "The smoke fixture uses actual helper APIs rather than an opaque shell script."
      :why "Failures identify the closest broken layer: hygiene, event append, finalizer, receipt, batch verifier, or ready-queue.")]
  :notes
    ["Fixture starts from worker draft commit aa10aa1, appends parent_hotfix event aa10aa2, finalizes the report, validates receipt reuse, and verifies the batch."
     "The same fixture asserts a ready-queue follower hard-depends on anchor and soft-refers to a slow peer without waiting for the slow peer."])
