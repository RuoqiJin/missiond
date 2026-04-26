;; ══════════════════════════════════════════════════════
;; MissionD v2 — Machine Contract Layer
;; 目的: 把 Lisp 从高密度说明书升级成 agent 之间的契约语言。
;;       task.lisp 是 SSOT; Markdown 是 ClaudeCode 执行视图。
;; ══════════════════════════════════════════════════════

(machine-contract-layer missiond
  :version "v0.1"
  :status "code-aligned initial — task-contract v1 schema + checker + renderer + pilot task landed"
  :schema ".missiond/tasks/schema/task-contract-v1.lisp"
  :checker "scripts/check-task-contract.mjs"
  :renderer "scripts/render-claudecode-task.mjs"

  (purpose
    "S-expressions carry machine boundaries: ownership, dependencies, acceptance, commit policy, review gate, rollback, evidence."
    "Markdown remains a rendered view for current ClaudeCode ergonomics."
    "MissionD plan-runner can later dispatch directly from Lisp without parsing natural-language task briefs.")

  (artifact-roles
    (intent-alignment-lisp
      :role "why / boundary / non-goal / acceptance intent"
      :machine-contract "records objective, scope, affected pillars, explicit non-goals, review gate owner")
    (plan-lisp
      :role "how / executable DAG / node dispatch / acceptance / rollback"
      :machine-contract "records node ids, dependencies, target tool, dispatch strategy, project root, claim/lease, review gate, commit policy")
    (workflow-lisp
      :role "reuse / distillation / trigger and match rules"
      :machine-contract "records applicability, parameters, disabled cases, evidence requirements, version chain")
    (shared-memory-lisp
      :role "runtime ledger"
      :machine-contract "records claims, decisions, issues, evidence, commit handoff, resume pointers")
    (task-lisp
      :role "dispatch contract"
      :machine-contract "records write-scope, must-not-touch, dependencies, acceptance commands, commit scope-check, report fields"))

  (pipeline
    (s1-author-task-contract
      :input "operator / MissionD plan-runner objective"
      :output ".missiond/tasks/<wave>/<task-id>.lisp"
      :guard "task-contract checker must pass")
    (s2-render-claudecode-view
      :input "task.lisp"
      :output ".missiond/claudecode/<task-id>.md"
      :command "node scripts/render-claudecode-task.mjs <task.lisp>"
      :note "renderer refuses overwrite unless --force")
    (s3-dispatch
      :input "rendered Markdown + machine contract id"
      :substrate "resident-lisp / fresh-code-alignment / agent-team / workstation-dispatch"
      :rule "Markdown is compatibility view; task.lisp remains SSOT")
    (s4-report
      :input "ClaudeCode report + scoped commit"
      :output "shared-memory.lisp / mission_execution companion log / evidence sidecar")
    (s5-verify
      :input "task.lisp + report + git diff/commit"
      :checks ["write-scope subset" "acceptance commands" "commit message" "must-not-touch unchanged"]
      :future "scripts/verify-task-report.mjs"))

  (task-contract-v1
    :required-fields [:schema :title :kind :status :owner :goal :write-scope :must-not-touch :acceptance :commit]
    :schema-value "missiond.task-contract.v1"
    :status-values [draft ready running blocked done archived]
    :commit-scope-check-values [write-scope-only none not-required]
    :current-checker "validates required fields, non-empty write-scope/acceptance, explicit must-not-touch, commit message/scope-check, repo-relative paths, exact overlap between write-scope and must-not-touch")

  (current-files
    (schema ".missiond/tasks/schema/task-contract-v1.lisp")
    (pilot ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp")
    (rendered-pilot ".missiond/claudecode/wave19-00-machine-contract-pilot.md")
    (checker "scripts/check-task-contract.mjs")
    (renderer "scripts/render-claudecode-task.mjs")
    (parser "scripts/lib/missiond_lisp.mjs"))

  (non-goals-v0
    ["Do not replace Markdown immediately; it remains the execution view."
     "Do not auto-dispatch from task.lisp until verifier/report loop exists."
     "Do not start frontend Lisp in this wave."
     "Do not interpret arbitrary Common Lisp; this is MissionD data Lisp only."])

  (next-steps
    ["Add task-contract verifier: task.lisp + git commit/report -> pass/fail."
     "Add report-contract Lisp shape for ClaudeCode completion reports."
     "Teach mission_plan/workstation_dispatch to emit task.lisp first, then render Markdown."
     "After backend loop stabilizes, reuse the same contract style for timeline-edit operations."]))
