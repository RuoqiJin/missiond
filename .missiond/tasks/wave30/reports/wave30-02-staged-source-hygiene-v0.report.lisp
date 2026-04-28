(report wave30-02-staged-source-hygiene-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave30-02-staged-source-hygiene-v0"
  :status done
  :commit_hash "fb144ca5b9bece1fa38ee64da8f6e268c668c1e1"
  :files_changed ["scripts/check-staged-source-hygiene.mjs"
                  "scripts/check-missiond-hooks.mjs"
                  "scripts/install-missiond-hooks.mjs"
                  ".githooks/pre-commit"]
  :acceptance_results [(:command "node scripts/check-staged-source-hygiene.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/check-missiond-hooks.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/install-missiond-hooks.mjs --dry-fixture" :exit_code 0 :ok true)
                       (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true)
                       (:command "perl -ne 'exit 1 if /\\x00/' scripts/check-staged-source-hygiene.mjs scripts/check-missiond-hooks.mjs scripts/install-missiond-hooks.mjs .githooks/pre-commit" :exit_code 0 :ok true)
                       (:command "git diff --check -- scripts/check-staged-source-hygiene.mjs scripts/check-missiond-hooks.mjs scripts/install-missiond-hooks.mjs .githooks/pre-commit" :exit_code 0 :ok true)]
  :trace_refs [".missiond/tasks/wave30/session-trace.lisp"]
  :source_hygiene_checks ["staged raw NUL byte detection via staged blobs"
                          "staged whitespace diagnostics via git diff --cached --check"
                          "task-scope guard readiness and staged delegation"]
  :hook_integration "Repo-local pre-commit remains opt-in behind MISSIOND_TASK_CONTRACT and invokes the read-only staged hygiene checker before task-scope guard delegation."
  :mutation_boundary "No global hook installation; check/doctor modes are read-only and install mode remains limited to git config --local core.hooksPath .githooks."
  :nul_byte_fixture "Dry fixture writes NUL bytes through Buffer.from([97, 0, 98, 10]) in a temp file; no raw NUL bytes are stored in repository source.")
