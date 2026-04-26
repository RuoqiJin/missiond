;; ══════════════════════════════════════════════════════
;; MissionD v2 — Machine Contract Layer
;; 目的: 把 Lisp 从高密度说明书升级成 agent 之间的契约语言。
;;       task.lisp 是 SSOT; Markdown 是 ClaudeCode 执行视图。
;; ══════════════════════════════════════════════════════

(machine-contract-layer missiond
  :version "v0.2"
  :status "code-aligned full (wave 19 task 02-08 全 close) — task-contract v1 schema + checker + verifier (5 项检查 + read-only 0 mutating git, wave19-02 commit 77f1f2b) + report-contract v1 schema + checker (wave19-03 commit ba58f20) + shared-memory v1 schema + checker + seed (wave19-04 吸入 wave19-02 commit 77f1f2b) + renderer dispatch brief v1 (4 新节 + agent-team literal 单实例 + verify command, wave19-05 commit c95eba8) + plan task-contract emitter v0 (mission_plan emit_task_contract opt-in + .missiond/tasks/generated/<plan_id>/<node_id>.lisp + emit before dispatch, wave19-06 commit 5d425e2) + workstation task-contract consumer v0 (overlay_contract + MalformedTaskContract SafeDescriptor + 绝不 fall back claude -p, wave19-07 commit bfc72b7) + execution task-contract completion verification v0 (mission_execution(complete) 加 4 字段 + 2 新错误码 + claim scope ⊇ contract write-scope + daemon 仍 read-only, wave19-08 commit 405d13b); plan emit → workstation consume → execution verify 全闭环; ClaudeCode Markdown brief 仍是当前 dispatch 表层; 真正 machine-driven autonomous dispatch (无 Markdown) / git pre-commit hook / sonnet 自动接 chain (5 rule 全过) 仍 future"
  :schema ".missiond/tasks/schema/task-contract-v1.lisp"
  :checker "scripts/check-task-contract.mjs"
  :verifier "scripts/verify-task-contract.mjs"
  :report-schema ".missiond/tasks/schema/report-contract-v1.lisp"
  :report-checker "scripts/check-task-report.mjs"
  :shared-memory-schema ".missiond/tasks/schema/shared-memory-v1.lisp"
  :shared-memory-checker "scripts/check-task-memory.mjs"
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
      :checks ["write-scope subset" "acceptance commands" "commit message" "must-not-touch unchanged" "task-id in commit message" "commit hash exists" "report status / acceptance_results / commit_hash present"]
      :command "node scripts/verify-task-contract.mjs <task.lisp> --commit <hash> [--json] [--dry-fixture]"
      :report-checker "node scripts/check-task-report.mjs <report.lisp> [--dry-fixture]"
      :shared-memory-checker "node scripts/check-task-memory.mjs <shared-memory.lisp> [--dry-fixture]"
      :status "code-aligned (wave 19 task 02 commit 77f1f2b + task 03 commit ba58f20 + task 04 吸入 commit 77f1f2b)"
      :future "auto-invoke verifier inside mission_execution(complete) when task_contract_path supplied (wave 19 task 08 已加 metadata 钩子, daemon 仍由 caller 触发 verifier)"))

  (task-report-v1
    :required-fields [:task_id :status :commit_hash :files_changed :acceptance_results :scope_deviations :notes]
    :status-values [draft in_progress blocked done]
    :reject-conditions ["missing task_id" "invalid status enum" "empty acceptance_results when status=done" "absolute file paths"]
    :checker "scripts/check-task-report.mjs (10 fixtures, --dry-fixture)"
    :sample ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp")

  (shared-memory-v1
    :purpose "design-time 共享笔记 (区分于 mission_execution claims slot — claims 是 runtime 强协议, ledger 是 design-time 软协议)"
    :entry-types [claim observation blocker completion correction handoff]
    :required-per-entry [:entry-id :task-id :timestamp-or-seq :touched-files-repo-relative]
    :reject-conditions ["duplicate entry-id" "invalid timestamp" "absolute file paths" "empty entry"]
    :write-rule "agents append entries only inside their claimed write-scope; ledger 本身是 sole shared write target for coordination"
    :checker "scripts/check-task-memory.mjs (13 fixtures, --dry-fixture)"
    :seed ".missiond/tasks/wave19/shared-memory.lisp")

  (task-contract-v1
    :required-fields [:schema :title :kind :status :owner :goal :write-scope :must-not-touch :acceptance :commit]
    :schema-value "missiond.task-contract.v1"
    :status-values [draft ready running blocked done archived]
    :commit-scope-check-values [write-scope-only none not-required]
    :current-checker "validates required fields, non-empty write-scope/acceptance, explicit must-not-touch, commit message/scope-check, repo-relative paths, exact overlap between write-scope and must-not-touch")

  (current-files
    (schema-task ".missiond/tasks/schema/task-contract-v1.lisp")
    (schema-report ".missiond/tasks/schema/report-contract-v1.lisp")
    (schema-shared-memory ".missiond/tasks/schema/shared-memory-v1.lisp")
    (pilot ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp")
    (rendered-pilot ".missiond/claudecode/wave19-00-machine-contract-pilot.md")
    (sample-report ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp")
    (seed-shared-memory ".missiond/tasks/wave19/shared-memory.lisp")
    (checker-task "scripts/check-task-contract.mjs")
    (verifier-task "scripts/verify-task-contract.mjs")
    (checker-report "scripts/check-task-report.mjs")
    (checker-shared-memory "scripts/check-task-memory.mjs")
    (renderer "scripts/render-claudecode-task.mjs")
    (parser "scripts/lib/missiond_lisp.mjs"))

  (non-goals-v0
    ["Do not replace Markdown immediately; it remains the execution view (wave 19 仍以 ClaudeCode rendered Markdown 派工)."
     "Do not auto-dispatch from task.lisp until verifier/report loop exists (wave 19 closed: verifier wave19-02 + report wave19-03 + plan emitter wave19-06 + workstation consumer wave19-07 + execution completion wave19-08; remaining: 真正 machine-driven dispatch 不渲染 Markdown)."
     "Do not start frontend Lisp in this wave (continue postpone)."
     "Do not interpret arbitrary Common Lisp; this is MissionD data Lisp only."])

  (next-steps
    [";; wave 19 closures (all done):"
     "DONE wave19-02 — Add task-contract verifier: task.lisp + git commit -> pass/fail (5 项检查, --commit/--json/--dry-fixture, read-only 0 mutating git)."
     "DONE wave19-03 — Add report-contract Lisp shape for ClaudeCode completion reports (7 字段 + 10 fixtures + sample report)."
     "DONE wave19-04 — Add shared-memory ledger v0 (6 entry types + 13 fixtures + seed wave19/shared-memory.lisp). Note: 文件被 wave19-02 一并入 commit 77f1f2b, 功能正确但 commit 归属错."
     "DONE wave19-05 — Renderer dispatch brief v1 (4 新节 + agent-team literal 单实例 + verify command)."
     "DONE wave19-06 — mission_plan emit task.lisp for eligible workstation nodes (.missiond/tasks/generated/<plan_id>/<node_id>.lisp; emit before dispatch; default off byte-compat)."
     "DONE wave19-07 — workstation_dispatch consume task.lisp (overlay_contract + MalformedTaskContract SafeDescriptor; legacy brief byte-identical absent contract; 绝不 fall back claude -p)."
     "DONE wave19-08 — mission_execution(complete) record verifier_status / task_contract_path / task_report_path; enforce_scoped_commit + task_contract_path → require commit_hash + claim scope ⊇ contract write-scope; daemon 仍 read-only."
     ";; remaining future:"
     "Implement true machine-driven autonomous dispatch (without ClaudeCode Markdown rendering)."
     "Wire report-contract checker auto-invoke into mission_execution(complete) path."
     "Auto-seed shared-memory ledger claim entry on parallel workstation spawn."
     "After backend loop stabilizes, reuse the same contract style for timeline-edit operations."
     "git pre-commit hook executing scope check (worktree-level)."]))
