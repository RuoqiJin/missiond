;; Wave 40 task contract.

(task wave40-01-parent-hotfix-report-preservation-v0
  :schema "missiond.task-contract.v1"
  :title "parent hotfix report preservation v0"
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
  :context-atlas-path ".missiond/tasks/wave40/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave40/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Close the report-preservation gap exposed by wave39 parent hotfix finalization. The Lisp architecture says a final report is the worker report plus finalized lineage, but task-runner-finalize-report currently reconstructs a minimal report and can drop rich worker fields such as :acceptance_results, :notes, trace refs, timing notes, and optional report-contract extensions. Make parent-hotfix finalization a sparse Lisp projection that preserves existing worker report detail while adding/updating lineage fields."

  :write-scope
    [".missiond/v3/missiond-blueprint.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/task-runner-finalize-report.mjs"
     "scripts/task-runner-parent-hotfix.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :must-not-touch
    ["crates/**"
     "packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/**"
     ".missiond/tasks/wave35/**"
     ".missiond/tasks/wave36/**"
     ".missiond/tasks/wave37/**"
     ".missiond/tasks/wave38/**"
     ".missiond/tasks/wave39/**"
     ".missiond/tasks/wave40/manifest.lisp"
     ".missiond/tasks/wave40/context-atlas.lisp"
     ".missiond/tasks/wave40/pattern-cards.lisp"
     ".missiond/tasks/wave40/wave40-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update Lisp first. In .missiond/v3/missiond-blueprint.lisp and .missiond/tasks/schema/report-contract-v1.lisp, state that parent-hotfix finalization is a sparse report projection: it preserves the worker report's existing non-lineage fields and only patches final lineage fields unless an explicit replacement option is supplied."
     "Fix scripts/task-runner-finalize-report.mjs so finalizeReportSource/finalizeReportFile keep worker report detail. Existing :acceptance_results must be preserved by default. If --acceptance-command is supplied, append that verification result rather than replacing worker acceptance unless an explicit replacement mode already exists and is documented."
     "Preserve optional report-contract fields that are already present in the worker report, at least :notes, :verification_tier, :time_sinks, :major_decisions, :unexpected_work, :blockers, :trace_refs, router recommendation/readiness/dispatch fields, and verification receipt fields. Prefer a generic Lisp property preservation path over hand-copying only this list."
     "Make task-runner-parent-hotfix.mjs use the preservation path. The helper should still be read-only by default and should only mutate report bytes when --write-report is supplied."
     "Add a dry fixture reproducing the wave39 class: a worker report with multiple acceptance results and notes is finalized with a parent patch; the final report must keep those acceptance entries and notes while adding :agent_commit_hash, :final_commit_hash, :verified_commit_hash, and :parent_patches."
     "Add or update checker/smoke coverage so verify-task-runner-batch and check-v3-task-lifecycle-isomorphism pin the preservation contract. Backward compatibility for minimal old reports must remain green."
     "Do not rewrite historical reports or wave39 artifacts in this task. The wave39 report is only evidence for a fixture; code and schema should be fixed forward."]

  :acceptance
    ["node scripts/task-runner-finalize-report.mjs --dry-fixture"
     "node scripts/task-runner-parent-hotfix.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/report-contract-v1.lisp scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs"
     "git diff --check -- .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/report-contract-v1.lisp scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs"]

  :commit
    (:required true
     :message "feat(tasks): preserve hotfix report detail"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Lisp/report-contract wording added before code changes."
     "Preservation implementation shape: sparse AST/property patch vs minimal reconstruction."
     "What fields are preserved and what lineage fields are patched."
     "Wave39-style preservation fixture result."
     "Acceptance command results."])
