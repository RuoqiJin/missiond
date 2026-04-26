;; MissionD task-contract v1
;; Purpose: make Lisp the machine-readable SSOT for task dispatch while
;; Markdown remains a ClaudeCode-compatible rendered view.

(task-contract-schema missiond.task-contract.v1
  :version "v1"
  :status "code-aligned initial — checker + renderer + verifier + scope-guard scripts implemented"
  :checker "scripts/check-task-contract.mjs"
  :renderer "scripts/render-claudecode-task.mjs"
  :verifier "scripts/verify-task-contract.mjs"
  :scope-guard "scripts/task-scope-guard.mjs"
  :pre-commit-hook ".githooks/pre-commit"

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
    :machine-context-rendered
      ["task :kind / :status / :owner"
       ":dispatch-strategy when present"
       ":depends-on as code-fenced ids"
       "shared-memory ledger path .missiond/tasks/<wave>/shared-memory.lisp when the file exists on disk"
       "expected report-contract path .missiond/tasks/<wave>/reports/<task-id>.report.lisp when the wave id is derivable"
       "pre-commit scoped-index guard line `node scripts/task-scope-guard.mjs --task <task.lisp> --mode staged` immediately after the git-add step when :commit :required is true"
       "MISSIOND_TASK_CONTRACT=<task.lisp> env-var prefix on the rendered git commit line when :commit :required is true (mirrors the .githooks/pre-commit activation contract)"
       "verify-task-contract command line in the Commit section when :commit :required is true"
       "literal '使用 agent-team提高效率' rendered exactly once in a 'Dispatch Note' section when :dispatch-strategy is agent-team"]
    :backward-compatibility
      ["existing fields (kind/status/owner/dispatch_strategy/depends_on/Goal/Ownership/Must Not Touch/Requirements/Acceptance Commands/Commit/Report) keep their prior wording and ordering"
       "new sections (Dispatch Note, Shared Memory, Report Contract, verify-task-contract command) are additive and conditional"
       "scoped commit guard v2 (task-scope-guard --mode staged + MISSIOND_TASK_CONTRACT prefix) extends the existing Commit section in place; renders only when :commit :required is true and never replaces the git add or git commit lines"]
    :non-goal "renderer does not invent scope, acceptance, or commit policy; missing fields are checker errors")

  (verifier-contract
    :input ".missiond/tasks/**/*.lisp + git commit (default HEAD)"
    :flags [--commit --json --dry-fixture]
    :checks
      ["commit hash resolves and is reported"
       "commit subject (first line) equals contract :commit :message when present"
       "every changed file ⊆ :write-scope when :commit :scope-check is write-scope-only"
       "no changed file overlaps :must-not-touch (always enforced regardless of :scope-check)"]
    :read-only
      ["never runs git add/commit/reset/checkout/stash/push/merge/rebase"
       "only invokes git rev-parse, git log, git show with --pretty=format and --name-only"]
    :glob-semantics "shared with checker via scripts/lib/missiond_lisp.mjs (pathMatchesPattern + pathMatchesAny)")

  (scope-guard-contract
    :purpose "Pre-commit defense against the Wave 19 git-index pollution failure where parallel sessions cross-staged files outside their task scope."
    :input ".missiond/tasks/**/*.lisp + git index (staged files) | git commit"
    :flags [--task --mode --commit --json --dry-fixture]
    :modes
      [(staged
         "reads `git diff --cached --name-only -z` and fails if any staged path is outside :write-scope (when :scope-check is write-scope-only) or matches :must-not-touch (always enforced)"
         "intended to fire from a pre-commit hook so the commit is blocked before the index is locked in")
       (commit
         "delegates to verify-task-contract semantics (subject + scope + forbidden) by reusing readCommit + verifyContract from scripts/verify-task-contract.mjs"
         "accepts --commit <hash> to verify any commit, defaults to HEAD")]
    :shared-logic
      ["loadContract / loadContractFromSource / verifyContract / readCommit imported from scripts/verify-task-contract.mjs"
       "pathMatchesAny imported from scripts/lib/missiond_lisp.mjs"]
    :read-only
      ["never runs git add/commit/reset/checkout/stash/push/merge/rebase"
       "staged-mode only invokes git diff --cached --name-only --no-renames -z"
       "commit-mode only invokes git rev-parse, git log, git show with --pretty=format and --name-only"]
    :pre-commit-hook
      (:path ".githooks/pre-commit"
       :activation "MISSIOND_TASK_CONTRACT env var must point at a task.lisp; otherwise hook exits 0"
       :enable "git config core.hooksPath .githooks (per-clone opt-in)"
       :delegates-to "scripts/task-scope-guard.mjs --mode staged"
       :read-only true)))
