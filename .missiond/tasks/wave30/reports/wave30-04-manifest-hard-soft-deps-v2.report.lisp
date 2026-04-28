(report wave30-04-manifest-hard-soft-deps-v2
  :schema "missiond.report-contract.v1"
  :task_id "wave30-04-manifest-hard-soft-deps-v2"
  :status done
  :commit_hash "a82b60c6707ec61198edddfac1e261322b57a0f7"
  :files_changed [".missiond/tasks/schema/task-runner-manifest-v2.lisp"
                  "scripts/check-task-runner-manifest.mjs"
                  "scripts/plan-task-runner.mjs"
                  "scripts/render-wave-briefs.mjs"]
  :acceptance_results
    [(:command "node scripts/check-task-runner-manifest.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/plan-task-runner.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/render-wave-briefs.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-runner-manifest.mjs .missiond/tasks/wave30/manifest.lisp" :exit_code 0 :ok true)
     (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true)
     (:command "perl -ne 'exit 1 if /\\x00/' scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs" :exit_code 0 :ok true)
     (:command "git diff --check -- .missiond/tasks/schema/task-runner-manifest-v2.lisp scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs" :exit_code 0 :ok true)]
  :major_decisions
    [(:decision "Manifest v2 adds :hard_deps and :soft_refs without breaking v1 :depends_on."
      :why "Existing manifests and v1 consumers remain conservative; v2 ready-queue can release on hard deps only.")
     (:decision "Ready-queue ignores soft refs and projects them only for audit."
      :why "Wave29-03 exposed a delay where context references were treated like blocking dependencies.")
     (:decision "Thin briefs render soft refs as context only."
      :why "Workers still see useful references without turning them into dispatch barriers.")]
  :notes
    ["Fixture wave30-04-ready-queue-ignores-soft-refs releases follower at t=10 while soft slow peer finishes at t=90."
     "Renderer fixture checks the Soft References section says context only and not dispatch dependencies or blockers."])
