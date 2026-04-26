;; MissionD task-contract v1
;; Purpose: make Lisp the machine-readable SSOT for task dispatch while
;; Markdown remains a ClaudeCode-compatible rendered view.

(task-contract-schema missiond.task-contract.v1
  :version "v1"
  :status "code-aligned initial — checker + renderer scripts implemented"
  :checker "scripts/check-task-contract.mjs"
  :renderer "scripts/render-claudecode-task.mjs"

  (purpose
    "S-expressions carry the machine contract: scope, forbidden files, dependencies, acceptance, commit policy, and report requirements."
    "Markdown is an execution view rendered from the contract for current ClaudeCode ergonomics."
    "MissionD plan-runner can later consume this contract directly without depending on Markdown parsing.")

  (required-task-fields
    [:schema :title :kind :status :owner :goal :write-scope :must-not-touch :acceptance :commit])

  (field-contract
    (:schema "must equal missiond.task-contract.v1")
    (:id "second form of (task <id> ...); lowercase kebab/dot/underscore id")
    (:kind "code-alignment | lisp-only | docs | review | smoke | ops")
    (:status "draft | ready | running | blocked | done | archived")
    (:owner "agent or workstation class")
    (:depends-on "vector of task ids; optional")
    (:dispatch-strategy "fresh-code-alignment | resident-lisp | agent-team | mixed | manual; optional")
    (:write-scope "non-empty vector of repo-relative paths or globs")
    (:must-not-touch "vector of repo-relative paths or globs; may be empty but must be explicit")
    (:requirements "ordered vector of human-readable implementation requirements")
    (:acceptance "non-empty vector of shell commands or verifier commands")
    (:commit "property list: :required boolean + :message + :scope-check"))

  (commit-contract
    :required-fields [:required :scope-check]
    :required-when-commit-required [:message]
    :scope-check-values [write-scope-only none not-required]
    :rule "When :required true, staged files must be inside :write-scope unless the task explicitly records a deviation.")

  (renderer-contract
    :input ".missiond/tasks/**/*.lisp"
    :output ".missiond/claudecode/<task-id>.md"
    :default "refuse overwrite unless --force"
    :non-goal "renderer does not invent scope, acceptance, or commit policy; missing fields are checker errors"))
