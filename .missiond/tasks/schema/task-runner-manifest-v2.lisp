;; MissionD task-runner-manifest v2
;; Purpose: additive hard/soft dependency split for ready-queue dispatch.
;; v1 remains valid. :depends_on remains the v1 hard-dependency field.
;; v2 adds explicit :hard_deps and :soft_refs so new dispatchers can release
;; tasks from hard deps only while renderers still surface contextual refs.

(task-runner-manifest-schema missiond.task-runner-manifest.v2
  :version "v2"
  :status "code-aligned — scripts/check-task-runner-manifest.mjs accepts v1 and v2; scripts/plan-task-runner.mjs ready-queue uses :hard_deps when declared and never blocks on :soft_refs; scripts/render-wave-briefs.mjs renders :soft_refs as context only"
  :checker "scripts/check-task-runner-manifest.mjs"
  :planner "scripts/plan-task-runner.mjs"
  :renderer "scripts/render-wave-briefs.mjs"

  (purpose
    "Remove the Wave29-03 ambiguity where a downstream task waited for soft reference work even though its real hard dependency was already satisfied."
    "Keep v1 compatibility: existing manifests with only :depends_on keep identical behavior."
    "Let v2 manifests name context-only references without turning them into ready-queue blockers.")

  (file-shape
    :form (task-runner-manifest <manifest-id>
            :schema "missiond.task-runner-manifest.v2"
            :wave <wave-id>
            :brief_mode <brief-mode-atom>
            :shared_preamble_path <repo-relative-path>
            :productive_only <literal-bool>
            <node> ...))

  (node-contract
    (:depends_on
      "Required for v1 compatibility. In v1-only manifests it is the hard dependency set. In v2 manifests it MUST still include every :hard_deps entry so legacy consumers remain conservative.")
    (:hard_deps
      "Optional vector of task ids. When declared, ready-queue scheduling treats this as the exact hard-dependency set that blocks dispatch. Every entry MUST resolve to a node in the same manifest, MUST NOT self-reference, and MUST also appear in :depends_on.")
    (:soft_refs
      "Optional vector of task ids. Context only. Every entry MUST resolve to a node in the same manifest and MUST NOT self-reference, but soft refs NEVER affect topological batches, ready_at, finish_at, barrier_finish_at, or critical-path calculations. Renderers may include them in briefs as guidance."))

  (scheduler-contract
    "group-barrier and ready-queue scheduling use the effective hard dependency set."
    "effective_hard_deps(node) = node.:hard_deps when :hard_deps is declared, otherwise node.:depends_on."
    "soft_refs are excluded from dependency graphs and overlap-edge injection. They may appear in output for audit/context only.")

  (renderer-contract
    "Thin briefs render :soft_refs under a Soft References section."
    "The section must say they are context only and not dispatch dependencies or blockers.")

  (validation-contract
    :accepted-schemas
      ["missiond.task-runner-manifest.v1" "missiond.task-runner-manifest.v2"]
    :optional-node-fields
      [:hard_deps :soft_refs]
    :rejects
      [":hard_deps is not a vector/list"
       ":soft_refs is not a vector/list"
       ":hard_deps / :soft_refs entry malformed"
       ":hard_deps / :soft_refs self-reference"
       ":hard_deps / :soft_refs reference not found in the same manifest"
       ":hard_deps entry not present in :depends_on, which would break v1 compatibility"]))
