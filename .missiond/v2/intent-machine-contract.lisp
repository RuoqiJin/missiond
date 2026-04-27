;; ══════════════════════════════════════════════════════
;; MissionD v2 — Machine Contract Layer
;; 目的: 把 Lisp 从高密度说明书升级成 agent 之间的契约语言。
;;       task.lisp 是 SSOT; Markdown 是 ClaudeCode 执行视图。
;; ══════════════════════════════════════════════════════

(machine-contract-layer missiond
  :version "v0.4"
  :status "code-aligned full (wave 19 task 02-08 全 close + wave 20 task 01-09 + wave 21 task 01-08 propose+apply-gate 闭环) — task-contract v1 schema + checker + verifier (5 项检查 + read-only 0 mutating git, wave19-02 commit 77f1f2b) + report-contract v1 schema + checker (wave19-03 commit ba58f20) + shared-memory v1 schema + checker + seed (wave19-04 吸入 wave19-02 commit 77f1f2b) + renderer dispatch brief v1 (4 新节 + agent-team literal 单实例 + verify command, wave19-05 commit c95eba8) + plan task-contract emitter v0 (mission_plan emit_task_contract opt-in + .missiond/tasks/generated/<plan_id>/<node_id>.lisp + emit before dispatch, wave19-06 commit 5d425e2) + workstation task-contract consumer v0 (overlay_contract + MalformedTaskContract SafeDescriptor + 绝不 fall back claude -p, wave19-07 commit bfc72b7) + execution task-contract completion verification v0 (mission_execution(complete) 加 4 字段 + 2 新错误码 + claim scope ⊇ contract write-scope + daemon 仍 read-only, wave19-08 commit 405d13b). wave 20 进一步闭环: task-scope-index-guard v1 (scripts/task-scope-guard.mjs staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发, wave20-01 commit 1fc0fd6, default off-by-git-config-not-启用) + renderer scoped-commit guard v2 (Commit 节加 task-scope-guard --mode staged 子步, wave20-02 commit b36cf6c) + execution preflight task-contract scope v1 (mission_execution preflight_commit 加 8 新字段 task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path, wave20-03 commit fe835e8, 0 mutating git) + machine-driven dispatch v0 (DispatchContractMode {Rendered, Machine} + dispatch_contract_mode arg / render_markdown shorthand + Lisp 真成 dispatch SSOT + Markdown brief 不再 load-bearing, wave20-04 commit 681c95d) + unified-entry machine-loop smoke v2 (6 smoke tests + build_artifact_refs lift 8 个 machine-contract 字段 + Markdown non-load-bearing 钉死, wave20-05 commit d308fae) + review auto-answer policy v0 (auto_answer_policy off|deterministic_safe|dry_run + 5+2 rules + 3 hard invariants I1 never reject / I2 destructive never auto-promote / I3 no LLM, wave20-08 commit 8adb0a8) + ExecutionEvent legacy metadata sweep v0 (8 legacy variants 全加 dispatch trio, wave20-09 commit 6e01e3f, 11 variants 全闭环). **wave 21 task 01-08 全闭环 propose+apply-gate (commits 44c74df/1335fa7/308426e/68b84f1/a18200b/e140773/4d494db/8ba8723)**: hooks-path installer v1 (scripts/install-missiond-hooks.mjs --check/--install/--json/--dry-fixture + scripts/check-missiond-hooks.mjs 只读 doctor + .githooks/pre-commit 保 env-gated, wave21-01 commit 44c74df, **opt-in repo-local only — 不擅自 default-on**) + task-run verifier v1 (scripts/verify-task-run.mjs 三合一 task-contract + report task_id + commit_hash + memory completion + 14 forbidden git verb proof + dogfood self-verify, wave21-02 commit 1335fa7) + execution report-verifier integration v1 (mission_execution(complete) 加 4 新字段 task_run_verifier_status/shared_memory_path/verifier_diagnostics/verified + verified=true 触发 daemon-internal sexp parse cross-check + 4 structured error codes VERIFIED_REQUIRES_* / TASK_REPORT_TASK_ID_MISMATCH / TASK_REPORT_COMMIT_HASH_MISMATCH / TASK_CONTRACT_MALFORMED + daemon_never_invokes_mutating_git unit-pinned, wave21-03 commit 308426e) + autonomous workstation LLM proposal v0 (workstation_inference_mode=off|sonnet_suggest opt-in + 4 propose 字段 target/dispatch_strategy/objective/scope × 3 confidence × 4 safety status + WorkstationProposalGate 仅 caller+PLAN 全空才 propose + **propose only, never auto-spawn — applied=false / auto_spawn=false 永钉死** + Sonnet unavailable surfaces llm_unavailable + DAG mode refuse, wave21-04 commit 68b84f1) + plan inference apply gate v1 (apply_inferred_fields=true opt-in apply gate 接 wave-18/06 deterministic + wave-20/07 LLM proposals + 6 道 gate caller-approval / master-flag / confidence / conflict / safety / slot-availability + 8 skip reason canonical + persisted plan.sexp_text 永不 mutate / persist_inference_applied=false 永钉死, wave21-05 commit a18200b) + LLM auto-approve proposal v0 (auto_approve_mode=off|sonnet_suggest opt-in for directive/plan review surfaces + propose-only 在每 dispatch 分支 surface llm_auto_approve_proposal 块 + **5 invariants test-pinned**: I1 never auto-reject (rejected demoted to needs_changes) / I2 destructive (archive|supersede|remove) short-circuit destructive_blocked WITHOUT LLM call / I3 applied=false + requires_human=true 永钉死 / I4 sonnet unavailable no fallback to deterministic / I5 destructive_check ALWAYS sourced from is_destructive_review_action not model + 22 unit tests, wave21-06 commit e140773) + sonnet distill chain auto-apply v1 (auto_sonnet=true + auto_sonnet_approved=true 双 opt-in apply-gate 接 wave-20/06 cross-plan distill auto-trigger + 7 重 gate (双 opt-in + auto_chain_trigger=auto_safe + ALL 6 wave-20 deterministic safety rule + caller distill_mode != sonnet) + auto-promote inner distill from dry_run to sonnet + 8 status taxonomy (not_requested..applied_sonnet) + **7 invariants test-pinned**: I1 default-off byte-shape / I2 dual opt-in required / I3 reuse wave-20 trigger outcomes never relax / I4 caller-already-sonnet refusal / I5 sonnet failure preserve inner payload / I6 review_required=true PINNED on every successful auto-apply (receipt-only) / I7 wave-19/20 blocks UNCHANGED purely additive, wave21-07 commit 4d494db) + machine-contract autonomous loop smoke v3 (15 new tests covering wave21-03..07 cross-wave invariants + Markdown task_brief_preview 仍 NEVER projected into artifact_refs (non-load-bearing 二度钉死) + machine dispatch SSOT task_contract_path == task_contract_source_path 钉死 + no LLM/spawn/shell, wave21-08 commit 8ba8723). **8 wave-21 commits 全闭环了 propose-only 与 explicit apply-gate 范式** — workstation dispatch / plan field inference / review gate / sonnet distill chain 4 路 LLM/inference 通道全部进入 'propose surface for human, apply only under explicit caller approval' 模式; **persisted state (plan.sexp_text / review row / completion log) 永不被 v0 自动 mutate**. 完全无 hint 的 autonomous workstation true spawn / persisted plan inference apply / hooks default-on / frontend Lisp 仍 future"
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
    (pre-commit-hook ".githooks/pre-commit")
    ;; wave 21 additions
    (hooks-installer "scripts/install-missiond-hooks.mjs")
    (hooks-doctor "scripts/check-missiond-hooks.mjs")
    (run-verifier "scripts/verify-task-run.mjs"))

  (non-goals-v0
    ["Markdown remains the ergonomic ClaudeCode execution view, but wave 20-04 machine mode 已让 Markdown 不再 load-bearing — caller 可关 render_markdown=false 让 dispatch 直接读 Lisp contract; wave 21-08 smoke 二度钉死 task_brief_preview NEVER 进 artifact_refs."
     "Do not auto-dispatch from task.lisp until verifier/report loop exists (wave 19 closed + wave 20-04 machine-driven dispatch v0 + wave 21-02/03 task-run verifier + execution-side verified gate 已落: plan emit → workstation consume → execution verify + caller-supplied verified flag triggers daemon-internal cross-check; remaining: 完全无 hint 的 autonomous spawn / report-contract checker auto-invoke from daemon)."
     "Do not auto-apply LLM/inference proposals in v0 — workstation dispatch (wave21-04) / plan field inference apply gate (wave21-05) / review auto-approve (wave21-06) / sonnet distill chain (wave21-07) 全部 propose-only 且 persisted state 永不 mutate; apply 必须 caller 显式 opt-in 且 LLM 仅 surface evidence."
     "Do not enable hooks default-on — wave 21-01 hooks installer 是 opt-in repo-local only, 不擅自 git config core.hooksPath .githooks; enforce-by-default 仍 future."
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
     ";; wave 21 closures (all done — propose+apply-gate paradigm):"
     "DONE wave21-01 — hooks-path installer v1: scripts/install-missiond-hooks.mjs (--check/--install/--json/--dry-fixture/--strict) + scripts/check-missiond-hooks.mjs read-only doctor alias + .githooks/pre-commit 保 MISSIOND_TASK_CONTRACT env-gated; --install runs git config --local core.hooksPath .githooks once, no-op when aligned, never --global/--system; **opt-in repo-local only — 不擅自 default-on** (commit 44c74df)."
     "DONE wave21-02 — task-run verifier v1: scripts/verify-task-run.mjs 三合一 (task contract + report task_id + commit_hash + memory completion) + 12 dry-fixtures + 7 helper cases + 14 forbidden git verb proof + dogfood self-verify; verify-task-contract.mjs main() gated by import.meta.url so importers don't trigger CLI parsing (commit 1335fa7)."
     "DONE wave21-03 — execution report-verifier integration v1: mission_execution(complete) 加 4 新字段 (task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified) + verified=true 触发 daemon-internal sexp-parse cross-check 加 4 structured error codes (VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH + TASK_REPORT_REQUIRED/MALFORMED + TASK_REPORT_TASK_ID_MISMATCH + TASK_REPORT_COMMIT_HASH_MISMATCH + TASK_CONTRACT_MALFORMED) + daemon_never_invokes_mutating_git unit test pinned; daemon NEVER spawns Node — wave21-02 verifier 仍是 out-of-process 权威 (commit 308426e)."
     "DONE wave21-04 — autonomous workstation LLM proposal v0: workstation_inference_mode=off|sonnet_suggest opt-in (default off byte-shape preserved); WorkstationProposalGate 仅在 caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in 全空才 propose; 4 propose 字段 target/dispatch_strategy/objective/scope × 3 confidence (high/medium/low) × 4 safety (Safe/InvalidTarget/InvalidStrategy/UnsupportedTarget); workstation_proposals[] cap 6; **propose only, never auto-spawn — applied=false / auto_spawn=false 永钉死**; Sonnet unavailable status=llm_unavailable + reason 钉 'no fallback to claude -p / prompt mode'; DAG mode preflight rejects sonnet_suggest INVALID_PARAM (commit 68b84f1)."
     "DONE wave21-05 — plan inference apply gate v1: apply_inferred_fields=true opt-in apply gate 接 wave-18/06 deterministic + wave-20/07 LLM proposals; **6 道 gate** (caller_approval / master_flag / confidence ∈ {high, medium} / conflict=none / per-field safety / slot availability) + llm_caller_approved (object/array shape) strict-shape validated; 8 skip reason canonical (apply_gate_not_requested / caller_value_already_set / caller_value_conflict / below_apply_threshold / llm_not_caller_approved / llm_confidence_too_low / llm_conflict_present / llm_safety_check_failed / deterministic_inferred_already_applied); **persisted plan.sexp_text 永不被 mutate** (persist_inference_applied=false 永钉死) — persist_inference flag echoed for future wave (commit a18200b)."
     "DONE wave21-06 — LLM auto-approve proposal v0: auto_approve_mode=off|sonnet_suggest opt-in for directive (approve|archive) + plan (approve|mark|supersede) review surfaces; ORTHOGONAL to wave-18/07 review_automation_policy AND wave-20/08 auto_answer_policy (3 knobs co-exist on response); **5 invariants test-pinned**: I1 NEVER auto-reject (rejected from model demoted to needs_changes + proposal_warnings[]); I2 destructive (archive|supersede|remove case-insensitive) short-circuit destructive_blocked WITHOUT calling Sonnet; I3 applied=false + requires_human=true 永钉死 (propose-only); I4 sonnet unavailable surfaces llm_unavailable + reason, NO fallback to deterministic; I5 destructive_check ALWAYS sourced from is_destructive_review_action(action), overwriting model output via enforce_proposal_invariants helper; 22 unit tests; 10 dispatch branch sites (commit e140773)."
     "DONE wave21-07 — sonnet distill chain auto-apply v1: auto_sonnet=true + auto_sonnet_approved=true 双 opt-in apply-gate 接 wave-20/06 cross-plan distill auto-trigger; **7 重 gate** (双 opt-in + auto_chain_trigger=auto_safe + ALL 6 wave-20 deterministic safety rule + caller distill_mode != sonnet); auto-promote inner distill from dry_run to sonnet via direct call to action_distill_sonnet; **8 status taxonomy**: not_requested | disabled | skipped_no_trigger | skipped_rules_failed | skipped_caller_approval_missing | skipped_already_sonnet | skipped_inner_error | applied_sonnet; **7 invariants test-pinned**: I1 default-off byte-shape; I2 dual opt-in (single typo cannot escalate); I3 reuse wave-20 trigger outcomes never relax; I4 caller-already-sonnet refusal; I5 Sonnet failure preserve inner payload (model_call_status=failed|invalid_output); I6 review_required=true PINNED on every successful auto-apply (receipt-only, no DB transition); I7 wave-19/20 blocks UNCHANGED (purely additive); 16 + 4 unit tests (commit 4d494db)."
     "DONE wave21-08 — machine-contract autonomous loop smoke v3: 15 new deterministic e2e tests across 4 handler files (unified_entry +3 / plan +3 / workstation_dispatch +3 / agent_execution +6); covers wave21-03 verifier 5-rule cross-check happy + 5 structured failure paths; pins wave21-04 I3 + I4 + I5 (workstation_proposals applied=false / auto_spawn=false / unavailable surfaces no fallback); pins wave21-06 I1 + I2 + I3 + I5 (NEVER rejected / destructive_blocked / requires_human=true / destructive_check from helper); pins wave21-07 I1 + I3 + I7 (default-off / chain reuses trigger outcomes / chain block additive); **Markdown task_brief_preview NEVER projected into artifact_refs (non-load-bearing 二度钉死)**; machine dispatch SSOT task_contract_path == task_contract_source_path 钉死; no LLM, no spawn, no shell (commit 8ba8723)."
     ";; remaining future:"
     "git config core.hooksPath .githooks 默认 default-on (wave21-01 仍 opt-in repo-local only) + worktree-level scope enforce."
     "Plan inference apply gate persisted plan-text mutation (wave21-05 persist_inference_applied=false 永钉死, 完整 persist 仍 future)."
     "Autonomous workstation true spawn (wave21-04 propose only, applied=false / auto_spawn=false 钉死; 完整 auto-spawn 仍 future)."
     "LLM auto-approve full apply (wave21-06 propose only, applied=false / requires_human=true 钉死; 完整 LLM 自动 approve 仍 future)."
     "Wire report-contract checker auto-invoke into mission_execution(complete) path (wave21-03 仍 caller-supplied verified flag, daemon 不主动 spawn verifier)."
     "Auto-seed shared-memory ledger claim entry on parallel workstation spawn."
     "After backend loop stabilizes, reuse the same contract style for timeline-edit operations."
     "Frontend Lisp 仍 postpone (本 wave 不开)."]))
