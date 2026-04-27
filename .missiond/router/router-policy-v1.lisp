;; MissionD router-policy v1 — seed policy.
;; Schema: .missiond/tasks/schema/router-policy-v1.lisp
;;
;; READ-ONLY ADVISORY DATA. This policy file is :dry-run-only true and
;; :runtime-replacement false. Nothing in MissionD reads this at runtime
;; to alter dispatch — wave24-03 will build a read-only recommendation
;; CLI that ALSO honors :dry-run-only. ClaudeCode remains the default
;; runtime backend for all live tasks.
;;
;; Backend enum (from the schema): claudecode, missiond-llm-router,
;; deterministic-checker, patch-worker, verifier-worker.

(router-policy seed-v1
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  :description "Seed router-policy capturing the obvious dry-run recommendations distilled from wave22 + wave23 trace observations. Three illustrative rules: (1) docs-only tasks → claudecode (current default), (2) checker-only tasks with deterministic acceptance → deterministic-checker (advisory only), (3) post-commit verification of an existing commit → verifier-worker (advisory only). All rules are recommendations; live dispatch still goes through claudecode."

  (rule
    :id r-docs-to-claudecode
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode
                :reasoning "Docs tasks are interactive and benefit from ClaudeCode's editor + repo navigation; no automated patch worker fits.")
    :non-goals
      ["does not replace runtime dispatch"
       "does not select a specific Claude model slot — slot selection stays with missiond-llm-router"
       "does not apply to code-alignment tasks even when they include docs edits"]
    :notes "Mirrors current wave22/wave23 behaviour: every docs task ran on claudecode and committed without retry.")

  (rule
    :id r-deterministic-checker-tasks
    :priority 20
    :when ((all (kind code-alignment)
                (path-glob "scripts/check-*.mjs")))
    :recommend (:backend deterministic-checker
                :reasoning "When the only acceptance bar is a deterministic checker script, a pure-script worker can validate without invoking an LLM at all.")
    :non-goals
      ["does not replace runtime dispatch — claudecode still authors the checker; only the validation pass is advisory-routed to deterministic-checker"
       "does not apply to checker tasks that also touch crates/** (those need patch-worker for cargo verification)"
       "does not imply the deterministic-checker backend exists yet — this is a forward-looking recommendation"])

  (rule
    :id r-post-commit-verifier
    :priority 30
    :when ((any (kind review)
                (kind smoke)))
    :recommend (:backend verifier-worker
                :reasoning "Review and smoke kinds confirm an existing commit against contracts; a read-only verifier worker is a tighter fit than a full ClaudeCode session.")
    :non-goals
      ["does not replace runtime dispatch"
       "does not authorize the verifier-worker to mutate the repo or the index"
       "does not apply to ops kind (ops requires interactive judgment, stays on claudecode)"]
    :notes "Aligns with verify-task-contract.mjs + task-scope-guard.mjs read-only postures already in the repo."))
