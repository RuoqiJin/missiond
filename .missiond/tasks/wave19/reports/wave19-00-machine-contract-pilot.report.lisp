;; Sample completion report — schema example only.
;; This file demonstrates the shape of a valid missiond.report-contract.v1
;; submission. It is NOT a proof that wave19-00-machine-contract-pilot was
;; actually executed; it exists so checkers and consumers have a concrete
;; reference document to validate against.

(report wave19-00-machine-contract-pilot
  :schema "missiond.report-contract.v1"
  :task_id "wave19-00-machine-contract-pilot"
  :status done
  :commit_hash "0000000000000000000000000000000000000000"
  :files_changed
    ["docs/machine-contract-pilot.md"]

  :acceptance_results
    [(:command "git diff --check -- docs/machine-contract-pilot.md"
      :exit_code 0
      :ok true
      :notes "no whitespace errors")
     (:command "node scripts/check-task-contract.mjs .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract check OK")]

  :scope_deviations []

  :notes
    "Schema example for wave19-03 report-contract-v1; commit_hash is a placeholder zero-SHA.")
