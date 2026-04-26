;; ══════════════════════════════════════════════════════
;; MissionD v2 — Machine Contract Layer
;; 目的: 把 Lisp 从高密度说明书升级成 agent 之间的契约语言。
;;       task.lisp 是 SSOT; Markdown 是 ClaudeCode 执行视图。
;; ══════════════════════════════════════════════════════

(machine-contract-layer missiond
  :version "v0.3"
  :status "code-aligned full (wave 19 task 02-08 全 close + wave 20 task 01-09 进一步闭环) — task-contract v1 schema + checker + verifier (5 项检查 + read-only 0 mutating git, wave19-02 commit 77f1f2b) + report-contract v1 schema + checker (wave19-03 commit ba58f20) + shared-memory v1 schema + checker + seed (wave19-04 吸入 wave19-02 commit 77f1f2b) + renderer dispatch brief v1 (4 新节 + agent-team literal 单实例 + verify command, wave19-05 commit c95eba8) + plan task-contract emitter v0 (mission_plan emit_task_contract opt-in + .missiond/tasks/generated/<plan_id>/<node_id>.lisp + emit before dispatch, wave19-06 commit 5d425e2) + workstation task-contract consumer v0 (overlay_contract + MalformedTaskContract SafeDescriptor + 绝不 fall back claude -p, wave19-07 commit bfc72b7) + execution task-contract completion verification v0 (mission_execution(complete) 加 4 字段 + 2 新错误码 + claim scope ⊇ contract write-scope + daemon 仍 read-only, wave19-08 commit 405d13b). wave 20 进一步闭环: task-scope-index-guard v1 (scripts/task-scope-guard.mjs staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发, wave20-01 commit 1fc0fd6, default off-by-git-config-not-启用) + renderer scoped-commit guard v2 (Commit 节加 task-scope-guard --mode staged 子步, wave20-02 commit b36cf6c) + execution preflight task-contract scope v1 (mission_execution preflight_commit 加 8 新字段 task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path, wave20-03 commit fe835e8, 0 mutating git) + machine-driven dispatch v0 (DispatchContractMode {Rendered, Machine} + dispatch_contract_mode arg / render_markdown shorthand + Lisp 真成 dispatch SSOT + Markdown brief 不再 load-bearing, wave20-04 commit 681c95d) + unified-entry machine-loop smoke v2 (6 smoke tests + build_artifact_refs lift 8 个 machine-contract 字段 + Markdown non-load-bearing 钉死, wave20-05 commit d308fae) + review auto-answer policy v0 (auto_answer_policy off|deterministic_safe|dry_run + 5+2 rules + 3 hard invariants I1 never reject / I2 destructive never auto-promote / I3 no LLM, wave20-08 commit 8adb0a8) + ExecutionEvent legacy metadata sweep v0 (8 legacy variants 全加 dispatch trio, wave20-09 commit 6e01e3f, 11 variants 全闭环). brief → preflight (含 contract scope) → staged guard → commit → verifier + execution complete verify 五段闭环; plan emit → workstation consume → execution verify 全闭环; Lisp 真正成 dispatch SSOT (Markdown 仍存为兼容视图但 non-load-bearing); 真正 machine-driven autonomous spawn (完全无 hint) / git config core.hooksPath .githooks 默认启用 / sonnet 完全自动接 chain (5+ rule 全过) / LLM-augmented PLAN inference apply (sonnet_suggest 仅 suggest 不 apply) 仍 future"
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
    (parser "scripts/lib/missiond_lisp.mjs")
    ;; wave 20 additions
    (scope-guard "scripts/task-scope-guard.mjs")
    (pre-commit-hook ".githooks/pre-commit"))

  (non-goals-v0
    ["Markdown remains the ergonomic ClaudeCode execution view, but wave 20-04 machine mode 已让 Markdown 不再 load-bearing — caller 可关 render_markdown=false 让 dispatch 直接读 Lisp contract."
     "Do not auto-dispatch from task.lisp until verifier/report loop exists (wave 19 closed + wave 20-04 machine-driven dispatch v0 已落: plan emit → workstation consume → execution verify; remaining: 完全无 hint 的 autonomous spawn)."
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
     ";; wave 20 closures (all done):"
     "DONE wave20-01 — task-scope-index-guard v1: scripts/task-scope-guard.mjs staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发 + 9+3 fixtures + 0 mutating git (commit 1fc0fd6). Caveat: git config core.hooksPath .githooks 默认未启用."
     "DONE wave20-02 — renderer scoped-commit guard v2: render-claudecode-task.mjs Commit 节加 task-scope-guard --mode staged 子步 + MISSIOND_TASK_CONTRACT env prefix (commit b36cf6c)."
     "DONE wave20-03 — execution preflight task-contract scope v1: mission_execution preflight_commit 加 8 新字段对账 contract scope; 0 mutating git; legacy byte-compat (commit fe835e8)."
     "DONE wave20-04 — machine-driven dispatch v0: DispatchContractMode {Rendered, Machine} + dispatch_contract_mode arg / render_markdown shorthand; Lisp 真成 dispatch SSOT, Markdown brief 不再 load-bearing (commit 681c95d)."
     "DONE wave20-05 — unified-entry machine-loop smoke v2: 6 smoke tests + build_artifact_refs lift 8 个 machine-contract 字段 + Markdown non-load-bearing 钉死 (commit d308fae)."
     "DONE wave20-06 — cross-plan distill auto-trigger v1: auto_chain_trigger default 'never' / 'deterministic_only' + 6 trigger rule (commit 3669ebc)."
     "DONE wave20-07 — LLM-augmented plan inference v0: infer_plan_fields=sonnet_suggest opt-in / suggest only / applied=false 钉死; DAG mode 拒 (commit 6bb935a). Caveat: 完整 LLM-augmented apply 仍 future."
     "DONE wave20-08 — review auto-answer policy v0: auto_answer_policy off|deterministic_safe|dry_run + 5+2 rules + 3 hard invariants I1 never reject / I2 destructive never auto-promote / I3 no LLM (commit 8adb0a8)."
     "DONE wave20-09 — ExecutionEvent legacy metadata sweep v0: 8 legacy variants 全加 dispatch trio; 11 variants 全闭环 (commit 6e01e3f)."
     ";; remaining future:"
     "Implement complete LLM auto-approve (4 v0 non-goals: auto_approve_directive / auto_approve_plan / auto_answer_review_question fully LLM / autonomous_workstation_dispatch full)."
     "git config core.hooksPath .githooks 默认启用 + worktree-level scope enforce + autonomous spawn 完全无 hint."
     "sonnet 完全自动接 distill chain (5+ rule 自动满足) + LLM-augmented PLAN inference apply (sonnet_suggest 升级到 medium confidence + LLM corroboration → auto apply)."
     "Wire report-contract checker auto-invoke into mission_execution(complete) path."
     "Auto-seed shared-memory ledger claim entry on parallel workstation spawn."
     "After backend loop stabilizes, reuse the same contract style for timeline-edit operations."]))
