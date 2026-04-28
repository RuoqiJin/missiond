(report wave30-01-parent-hotfix-finalizer-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave30-01-parent-hotfix-finalizer-v0"
  :status done
  :commit_hash "be5bf73794711c6eb4baf256eb2d609b780c9fc3"
  :files_changed ["scripts/task-runner-finalize-report.mjs"
                  "scripts/task-runner-parent-hotfix.mjs"
                  "scripts/check-task-report.mjs"
                  "scripts/verify-task-runner-batch.mjs"
                  ".missiond/tasks/schema/report-contract-v1.lisp"]
  :acceptance_results
    [(:command "node scripts/task-runner-finalize-report.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/task-runner-parent-hotfix.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-report.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/verify-task-runner-batch.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true)
     (:command "perl -ne 'exit 1 if /\\x00/' scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/verify-task-runner-batch.mjs" :exit_code 0 :ok true)
     (:command "git diff --check -- scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/verify-task-runner-batch.mjs .missiond/tasks/schema/report-contract-v1.lisp" :exit_code 0 :ok true)]
  :major_decisions
    [(:decision "Parent hotfix finalization is orchestrator-owned and read-only by default."
      :why "Wave29-03 showed the worker cannot record a parent hotfix that happens after the worker exits.")
     (:decision "Finalizer renders deterministic report-contract v1 bytes and preserves worker commit in :agent_commit_hash."
      :why "Legacy readers keep using :commit_hash as final truth while lineage consumers retain the worker commit.")
     (:decision "Batch verifier resolves verification hash as verified > final > commit_hash."
      :why "Finalized reports must verify the post-hotfix commit while accepting memory summaries that cite worker lineage hashes.")]
  :notes
    ["Wave29-03 drift fixture pinned with worker d36de80 and parent d842b1d."
     "No git mutation, no spawn, no network, no LLM in the new helpers."])
