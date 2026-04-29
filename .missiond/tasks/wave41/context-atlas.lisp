;; Wave 41 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave41-v3-complete-isomorphism-gate-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave41
  :goal "Graduate V3 implementation-map surfaces from partial labels to a complete code-isomorphism gate."
  :read-order [".missiond/claudecode/wave41-shared-preamble.md"
               ".missiond/tasks/wave41/context-atlas.lisp"
               ".missiond/tasks/wave41/pattern-cards.lisp"
               ".missiond/tasks/wave41/wave41-01-v3-complete-isomorphism-gate-v0.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-request-lisp-isomorphism.mjs"
               "scripts/check-v3-intent-alignment-isomorphism.mjs"
               "scripts/check-v3-plan-execution-isomorphism.mjs"
               "scripts/check-v3-workflow-isomorphism.mjs"
               "scripts/check-v3-task-lifecycle-isomorphism.mjs"
               "scripts/check-v3-workstation-config-isomorphism.mjs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 authority. Six implementation-map surfaces currently say code-aligned-partial even though the six live checkers pass."
      :grep ["implementation-map"
             "surface mission_request"
             "surface mission_directive"
             "surface mission_plan"
             "surface mission_workflow"
             "surface task-runner-cli"
             "surface workstation-config"
             "code-aligned-partial"
             "compression-contract"])
    (file "scripts/check-v3-request-lisp-isomorphism.mjs"
      :purpose "mission_request surface checker. It already passes live; consider adding/keeping a code-aligned surface needle."
      :grep ["DEFAULT_FILES"
             "checkFiles"
             "buildFixture"
             "mission_request"
             "v3 request Lisp/code isomorphism"])
    (file "scripts/check-v3-intent-alignment-isomorphism.mjs"
      :purpose "mission_directive surface checker. It currently requires code-aligned-partial."
      :grep ["surface mission_directive"
             "code-aligned-partial"
             "buildFixture"])
    (file "scripts/check-v3-plan-execution-isomorphism.mjs"
      :purpose "mission_plan surface checker. It currently requires code-aligned-partial."
      :grep ["surface mission_plan"
             "code-aligned-partial"
             "buildFixture"])
    (file "scripts/check-v3-workflow-isomorphism.mjs"
      :purpose "mission_workflow surface checker. It currently requires code-aligned-partial."
      :grep ["surface mission_workflow"
             "code-aligned-partial"
             "buildFixture"])
    (file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :purpose "task-runner-cli surface checker. It currently requires code-aligned-partial."
      :grep ["surface task-runner-cli"
             "code-aligned-partial"
             "buildFixture"])
    (file "scripts/check-v3-workstation-config-isomorphism.mjs"
      :purpose "workstation-config surface checker. It pins the workstation config contract and may need a code-aligned needle."
      :grep ["workstation-config"
             "code-aligned"
             "buildFixture"])
    (file "scripts/check-lisp-blueprint-compression.mjs"
      :purpose "Blueprint compression checker. Update only if needed to include the new aggregate command in fixtures or needles."
      :grep ["compression-contract"
             "implementation-map"
             "check-v3"])))
