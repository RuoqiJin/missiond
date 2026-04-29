;; Wave 41 task-runner manifest.

(task-runner-manifest wave41-v3-complete-isomorphism-gate-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave41
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave41-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Graduate V3 implementation-map surfaces from partial labels to an executable complete Lisp/code isomorphism gate."
  :generated_at "2026-04-29T03:14:37Z"
  :generator "codex-parent"

  (node :task_id wave41-01-v3-complete-isomorphism-gate-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-code-isomorphism-complete.mjs"
                      "scripts/check-v3-request-lisp-isomorphism.mjs"
                      "scripts/check-v3-intent-alignment-isomorphism.mjs"
                      "scripts/check-v3-plan-execution-isomorphism.mjs"
                      "scripts/check-v3-workflow-isomorphism.mjs"
                      "scripts/check-v3-task-lifecycle-isomorphism.mjs"
                      "scripts/check-v3-workstation-config-isomorphism.mjs"
                      "scripts/check-lisp-blueprint-compression.mjs"]))
