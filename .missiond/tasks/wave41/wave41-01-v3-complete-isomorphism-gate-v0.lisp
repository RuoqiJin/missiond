;; Wave 41 task contract.

(task wave41-01-v3-complete-isomorphism-gate-v0
  :schema "missiond.task-contract.v1"
  :title "v3 complete isomorphism gate v0"
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
  :context-atlas-path ".missiond/tasks/wave41/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave41/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Turn the current collection of V3 Lisp/code isomorphism checks into an explicit completion gate. Right now every per-surface V3 checker passes, but the blueprint implementation-map still labels six surfaces as code-aligned-partial and some checkers still require that partial status string. Graduate the implementation-map to code-aligned where the live checkers prove the Lisp contract, and add a single aggregate checker that fails if any implementation-map surface regresses to partial or if any per-surface checker fails."

  :write-scope
    [".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-code-isomorphism-complete.mjs"
     "scripts/check-v3-request-lisp-isomorphism.mjs"
     "scripts/check-v3-intent-alignment-isomorphism.mjs"
     "scripts/check-v3-plan-execution-isomorphism.mjs"
     "scripts/check-v3-workflow-isomorphism.mjs"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "scripts/check-v3-workstation-config-isomorphism.mjs"
     "scripts/check-lisp-blueprint-compression.mjs"]

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
     ".missiond/tasks/wave40/**"
     ".missiond/tasks/wave41/manifest.lisp"
     ".missiond/tasks/wave41/context-atlas.lisp"
     ".missiond/tasks/wave41/pattern-cards.lisp"
     ".missiond/tasks/wave41/wave41-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update .missiond/v3/missiond-blueprint.lisp first. In the implementation-map, graduate the six V3 surfaces that are currently code-aligned-partial to code-aligned only if the existing live checker for that surface proves the current note/code contract. Do not weaken the notes."
     "Add scripts/check-v3-code-isomorphism-complete.mjs as an aggregate completion gate. It should be read-only, deterministic, support --json and --dry-fixture, and fail when any expected implementation-map surface is missing, any implementation-map surface still has :status \"code-aligned-partial\", any expected surface lacks :status \"code-aligned\", :code, or :note, the compression-contract omits this aggregate command, or any per-surface V3 checker fails."
     "The aggregate checker should cover exactly these implementation surfaces unless the blueprint explicitly changes the V3 surface set: mission_request, mission_directive, mission_plan, mission_workflow, task-runner-cli, workstation-config."
     "Update all per-surface checkers that currently require code-aligned-partial so they now pin code-aligned and adjust their dry fixtures accordingly. Checkers that did not pin the status should stay compatible but may add a code-aligned needle if that improves the gate."
     "Add the aggregate command to the V3 compression-contract :checks list. If check-lisp-blueprint-compression needs to pin that command, update it narrowly."
     "Do not edit Rust or frontend code in this task. This is a Lisp/checker graduation task after the implementation work from waves 31-40."]

  :acceptance
    ["node scripts/check-v3-code-isomorphism-complete.mjs --dry-fixture"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "node scripts/check-v3-request-lisp-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-request-lisp-isomorphism.mjs"
     "node scripts/check-v3-intent-alignment-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-intent-alignment-isomorphism.mjs"
     "node scripts/check-v3-plan-execution-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-plan-execution-isomorphism.mjs"
     "node scripts/check-v3-workflow-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-workflow-isomorphism.mjs"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
     "node scripts/check-v3-workstation-config-isomorphism.mjs"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' .missiond/v3/missiond-blueprint.lisp scripts/check-v3-code-isomorphism-complete.mjs scripts/check-v3-request-lisp-isomorphism.mjs scripts/check-v3-intent-alignment-isomorphism.mjs scripts/check-v3-plan-execution-isomorphism.mjs scripts/check-v3-workflow-isomorphism.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/check-v3-workstation-config-isomorphism.mjs scripts/check-lisp-blueprint-compression.mjs"
     "git diff --check -- .missiond/v3/missiond-blueprint.lisp scripts/check-v3-code-isomorphism-complete.mjs scripts/check-v3-request-lisp-isomorphism.mjs scripts/check-v3-intent-alignment-isomorphism.mjs scripts/check-v3-plan-execution-isomorphism.mjs scripts/check-v3-workflow-isomorphism.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/check-v3-workstation-config-isomorphism.mjs scripts/check-lisp-blueprint-compression.mjs"]

  :commit
    (:required true
     :message "feat(v3): add complete code-isomorphism gate"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Which implementation-map surfaces were graduated and why."
     "Aggregate checker contract and dry-fixture cases."
     "Per-surface checker updates."
     "Acceptance command results."])
