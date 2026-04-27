;; Wave 21 / Task 09 — Lisp backfill wave 21 status.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave21/wave21-09-lisp-backfill-wave21-status.lisp

(report wave21-09-lisp-backfill-wave21-status
  :schema "missiond.report-contract.v1"
  :task_id "wave21-09-lisp-backfill-wave21-status"
  :status done
  :commit_hash "9670e1f4627c"
  :files_changed
    [".missiond/v2/intent-machine-contract.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"
     ".missiond/v2/intent-flow.lisp"
     ".missiond/v2/intent-intent-layer.lisp"
     ".missiond/v2/intent-tools.lisp"
     ".missiond/v2/intent-plan-dag.lisp"
     ".missiond/v2/intent-workstation-policy.lisp"
     ".missiond/v2/intent-execution-governance.lisp"
     ".missiond/v2/intent.lisp"]

  :acceptance_results
    [(:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK (all 9 :write-scope shards + 11 untouched shards: intent-capability-governance / intent-directive-artifacts / intent-event-bus / intent-event-bus-execution / intent-memory / intent-memory-execution / intent-memory-history / intent-system-layer / intent-worker / worker-pillar-execution / architecture-dsl).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "37 tasks OK (covers wave19-* / wave20-* / wave21-* contracts + 2 schema fixtures). No regressions in any task contract.")
     (:command "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in any of the 9 :write-scope v2 lisp files.")]

  :scope_deviations []

  :notes
    "wave21-09 backfilled committed implementation truth for wave 21 task 01-08 (8 commits: 44c74df / 1335fa7 / 308426e / 68b84f1 / a18200b / e140773 / 4d494db / 8ba8723).

Anchors updated:
- machine-contract-layer :version v0.3 → v0.4 (intent-machine-contract.lisp); status string appended with full wave 21 task 01-08 narrative including invariants pinned per task; current-files block grows 3 wave21 additions (hooks-installer / hooks-doctor / run-verifier); non-goals-v0 block reframed (propose-only paradigm + persisted state永不 mutate + hooks opt-in repo-local + Markdown 二度钉死 + frontend postpone); next-steps block adds 8 wave21 DONE items + 8 remaining future items.
- intent-pillar-source-index.lisp: new wave-21-backfill v1.2 block (regions 69-78) with 8 anchor entries (hooks-path-installer-v1 / task-run-verifier-v1 / execution-report-verifier-integration-v1 / autonomous-workstation-llm-proposal-v0 / plan-inference-apply-gate-v1 / llm-auto-approve-proposal-v0 / sonnet-distill-chain-auto-apply-v1 / machine-contract-autonomous-loop-smoke-v3) + 2 status-upgrades (review-automation-policy-v0 note扩加第三 review knob auto_approve_mode propose-only with 5 invariants; task-contract-v1 note扩加真正 6 段闭环 brief→preflight→staged guard→commit→verifier→execution complete verify); deferred-coverage v1.2 paragraph; new wave-21-status-summary 11项; new wave21-task-09-non-goal 12项; pre-compression-checklist updated with wave 21 task 09 +8 entry; next-step rewritten with wave 21 propose+apply-gate paradigm next推进路径; commit-block嵌 8 commits with primary-targets + tests metadata.
- pillar-shard status strings updated to reflect wave 21 work: intent-flow.lisp (added wave 21 propose+apply-gate paradigm overview + 6段闭环); intent-intent-layer.lisp (added wave 21 task 01-08 detail with all invariants); intent-tools.lisp (tool count仍 83 invariant maintained, added wave 21 args/fields per surface); intent-plan-dag.lisp (added wave 21 task 04/05/07 detail for autonomous workstation propose / plan inference apply gate / sonnet distill chain auto-apply); intent-workstation-policy.lisp (added wave 21 task 03/04 detail for execution report-verifier integration + autonomous workstation propose); intent-execution-governance.lisp (added wave 21 task 01/02/03 detail with daemon-internal verified gate 6段闭环).
- intent.lisp: source-of-truth-index :desc bumped to v1.2 wave 21 task 09 with explicit ~145 entry count and 8 wave-21 task narrative; precompression-note bumped to wave 21 task 09 + judgement-now :: wave-21-status-summary anchor; navigation-assets cluster +8 wave21 entries (hooks-path-installer-v1 / task-run-verifier-v1 / execution-report-verifier-integration-v1 / autonomous-workstation-llm-proposal-v0 / plan-inference-apply-gate-v1 / llm-auto-approve-proposal-v0 / sonnet-distill-chain-auto-apply-v1 / machine-contract-autonomous-loop-smoke-v3); pillar memory :status appended wave 21-03 (execution report-verifier integration v1) + wave 21-06 (LLM auto-approve proposal v0 with 3 review knob co-existence ORTHOGONAL); pillar worker :canonical-status appended wave 21-03/04 (execution report-verifier + autonomous workstation LLM proposal); pillar tools :canonical-status appended wave 21-01..08 with `tool count仍 83 不变` invariant.

Proposal-only vs applied status distinctions (代码真实状态, 不假装完整 apply):
1. hooks-path-installer-v1 → code-aligned-partial: scripts落但 git config core.hooksPath .githooks 默认未启用 (per task brief: opt-in repo-local only — 不擅自 default-on); enable per clone via 显式 --install.
2. autonomous-workstation-llm-proposal-v0 → code-aligned-partial: workstation_inference_mode=sonnet_suggest opt-in propose 已落, 但 applied=false / auto_spawn=false 永钉死 (`propose only never auto-spawn`); 完整 autonomous workstation true spawn 仍 future.
3. plan-inference-apply-gate-v1 → code-aligned-partial: apply_inferred_fields=true opt-in apply gate 已落 (deterministic high + LLM caller-approved {high, medium}), 但 persisted plan.sexp_text 永不 mutate (persist_inference_applied=false 永钉死); 完整 persisted plan inference apply (sonnet_suggest 升级到真改 plan.sexp_text 落盘) 仍 future.
4. llm-auto-approve-proposal-v0 → code-aligned-partial: auto_approve_mode=sonnet_suggest opt-in propose 已落 for directive (approve|archive) + plan (approve|mark|supersede), 但 applied=false / requires_human=true 永钉死; 完整 LLM auto-approve apply (proposal 升级到真落 review row state transition) 仍 future. ORTHOGONAL to wave-18/07 + wave-20/08 — 三 review knob 共存 (review_automation_policy + auto_answer_policy + auto_approve_mode) 独立观测.
5. sonnet-distill-chain-auto-apply-v1 → code-aligned-partial: auto_sonnet 双 opt-in apply-gate 已落 (auto_promote inner distill from dry_run to sonnet via direct call to action_distill_sonnet), 但仍 dual opt-in required (auto_sonnet=true + auto_sonnet_approved=true); 完全自动 sonnet 接 chain (无需 caller dual opt-in, plan-runner 全局推断) 仍 future.

Code-aligned (full): execution-report-verifier-integration-v1 (wave21-03 daemon-internal verified gate 6段闭环 with 4 structured error codes + daemon_never_invokes_mutating_git unit-pinned + daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威); task-run-verifier-v1 (wave21-02 三合一 + 14 forbidden git verb proof + dogfood self-verify + verify-task-contract.mjs main() gated by import.meta.url); machine-contract-autonomous-loop-smoke-v3 (wave21-08 15 e2e tests + Markdown non-load-bearing 二度钉死 + machine dispatch SSOT 钉死 + no LLM/spawn/shell).

Cross-wave invariants reflected in this backfill: I1-06 (LLM auto-approve NEVER rejected) / I2-06 (destructive_blocked WITHOUT calling Sonnet) / I3-06 (applied=false + requires_human=true 永钉死) / I4-06 (sonnet unavailable no fallback) / I5-06 (destructive_check ALWAYS from is_destructive_review_action helper) / I3-04 (workstation proposals applied=false + bundle auto_spawn=false 永钉死) / I1-07 (auto_sonnet default-off byte-shape) / I2-07 (dual opt-in required) / I3-07 (chain reuses wave-20 trigger outcomes never relax) / I7-07 (chain block additive — wave-19/20 blocks UNCHANGED) / wave-19 task-contract / wave-20 dispatch-contract / wave21-03 verifier 5-rule cross-check.

Remaining pending (照实记录, 不假装完整 apply):
- 完整 LLM apply for review approve (wave21-06 propose only, 完整 apply 仍 future, closing wave 14-04 留下的 4 v0 non-goal 之 auto_approve_directive + auto_approve_plan + auto_answer_review_question)
- persisted plan inference apply (wave21-05 persist_inference_applied=false 永钉死, 完整 persist 升级到真改 plan.sexp_text 落盘 仍 future)
- autonomous workstation true spawn (wave21-04 propose only / applied=false / auto_spawn=false 永钉死, 完整 autonomous workstation true spawn 仍 future)
- git config core.hooksPath .githooks 默认 default-on (wave21-01 仍 opt-in repo-local only — per task brief: 'Do not enable hooks globally; repo-local only')
- sonnet 完全自动接 distill chain 不需 dual opt-in (wave21-07 仍 dual opt-in required, 完全自动 sonnet 接 chain plan-runner 全局推断 仍 future)
- report-contract checker auto-invoke from daemon (wave21-03 仍 caller-supplied verified flag, daemon 不主动 spawn verifier — daemon NEVER spawns Node 是显式护栏)
- Auto-seed shared-memory ledger claim entry on parallel workstation spawn
- frontend Lisp 仍 postpone (本 wave 不开 frontend)
- 完整 enforce-by-default scoped commit (claim-lease / scoped-commit 切 enforce-by-default + git pre-commit hook 默认启用 + git hooksPath 默认指向 .githooks for worktree-level scope check)

Acceptance command results:
- node scripts/check-architecture-lisp.mjs --all-v2 → 20 files OK
- node scripts/check-task-contract.mjs --all → 37 tasks OK
- git diff --check (9 files) → no whitespace errors

Pre-commit task-scope-guard --mode staged: 9 staged files all in :write-scope (no out-of-scope or forbidden touched).
Post-commit verify-task-contract: 9670e1f4627c against wave21-09 contract OK (commit message + scope check write-scope-only + must-not-touch clean + acceptance presence all OK).
Shared-memory check: ledger 1, entries 21 (claim-001 + completion-001 for wave21-09 appended; all prior wave21 entries preserved verbatim per append-only protocol).

Report written out-of-scope at .missiond/tasks/wave21/reports/wave21-09-lisp-backfill-wave21-status.report.lisp (intentionally untracked per wave21 protocol — :write-scope is v2 lisp only)."

  :sources
    ["wave 21 task contracts: .missiond/tasks/wave21/wave21-{00..08}-*.lisp"
     "wave 21 commit hashes: git log 6a7d4e1..8ba8723"
     "wave 20-10 backfill template: commit b94cb7b (wave-20-backfill block in intent-pillar-source-index.lisp)"
     "wave 21 shared-memory ledger: .missiond/tasks/wave21/shared-memory.lisp (entries seq 1-21 for full wave 21 history)"
     "wave 21 reports cluster: .missiond/tasks/wave21/reports/wave21-{00..08}-*.report.lisp"])
