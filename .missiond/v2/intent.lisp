;; ══════════════════════════════════════════════════════
;; MissionD — Intent v2
;; 按指挥官心智模型收拢: 七大板块
;;   一 · 记忆          — 系统记得什么 (库: schema + 状态)
;;   二 · worker        — PTY + LLM 接入 + 21 后台 worker + 编排 + engine (计算)
;;   三 · 工具          — 对外暴露的能力
;;   四 · 事件总线      — 进程内神经网络(入点 / 核心 / 出点)
;;   五 · 意图层        — 系统的自我描述 + 全局指令 + 动作/指令规约
;;   六 · 系统层        — 类型 + 传输 + RPC Gateway + 工具
;;   七 · 流程 (flow)   — 跨 pillar 的动作前后流程: memory 静态 + worker 计算 的编排
;; ══════════════════════════════════════════════════════

(intent missiond-v2
  (version "v2-draft-recursive-standard")
  (granularity L2-Topology)
  (created "2026-04-19")
  (parent "intent.lisp (v1, 27 个分文件)")
  (note "v2 按概念重组,不按物理代码层。v1 保留为历史参考。")

  ;; ── 2026-04-21 系统级导航资产 (开 pillar refactor 前必读) ──
  (navigation-assets
    (source-of-truth-index "intent-pillar-source-index.lisp"
      :desc "判真索引 — pillar code-truth registry; v1.3 (wave 22 task 08) 已覆盖 7 pillar baseline + 7 高变动语义区 + wave 13/14/15/16/17/18/19/20/21/22 全部 task (含 wave 19 machine-contract task SSOT 全闭环 + wave 20 machine-driven dispatch + scoped-commit 全 5 段闭环 + ExecutionEvent metadata 11 variants 闭环 + review auto-answer policy + LLM-augmented sonnet_suggest + wave 21 hooks installer + run verifier + execution-report integration + autonomous workstation propose + plan inference apply gate + LLM auto-approve propose + sonnet distill chain auto-apply + machine-contract autonomous loop smoke v3 + wave 22 hooks default-on doctor v2 + execution auto-run-verifier v2 + review LLM approve apply gate v1 + persisted plan inference apply v2 + autonomous workstation true spawn v1 + distill chain policy auto-sonnet v2 + autonomous loop apply smoke v4) 共 ~153 entry; 主 Lisp 压缩/拆分 cross-ref 锚点统一走这里")
    (drift-audit "drift-audit-2026-04-21.md"
      :desc "跨 pillar 代码 snapshot — worker/engine/infra footprint + bootstrap count + zombie + 跨 pillar 表 caller 精确数字")
    (refactor-methodology ".missiond/workflows/pillar-refactor.lisp"
      :desc "memory pillar 实战凝结方法论 — 5 phase × 原则 × anti-patterns × checklist")
    (architecture-dsl "architecture-dsl.lisp"
      :desc "可复用架构 DSL: pillar/function/flow/tool 的 ingress → logic-core → egress 结构与检查规则; v0.6 (wave 15 task 03) 加 R017 source-file-must-exist + R018 source-file-must-live-under-v2 + checker shard auto-discovery via collectSourceFileRefs; v0.5 (wave 14 task 07) 加 l2-shard-split-plan (5 shard designed → wave 15 task 02 EXECUTED) + execution-gate + per-shard moved-sections / retained-anchor / source-index-update-rule / checker-requirement / rollback-plan; v0.4 (wave 12 task 06) 加 R015/R016/section-entry-extended/phase-3.1 precompression checker (R015+R016 已 IMPLEMENTED 于 wave 14 task 05; R017+R018 + 自动 shard discovery 已 IMPLEMENTED 于 wave 15 task 03)")
    (precompression-note
      :desc "wave 13 task 05: 已执行 L1 安全压缩; wave 14 task 07: 写 L2 shard split plan; wave 15 task 02: L2 物理 split 已执行 (5 shard 创建 + 28 source-index 重定向 + section-id 全保 R008); wave 15 task 03: shard-aware checker 已落; wave 16 task 09: 回填 wave 16 全部 8 task; wave 17 task 09 / wave 18 task 10 / wave 19 task 12 / wave 20 task 10 / wave 21 task 09 / wave 22 task 08: 持续回填 + 升级条件; 后续压缩仍按 compression-policy 走; 详 architecture-dsl.lisp :: judgement-now / intent-pillar-source-index.lisp :: judgement-now :: wave-22-status-summary")
    (plan-dag-scheduler-design
      :desc "PLAN DAG scheduler — runtime v2 (wave 13 task 02 commit 8bb6110) + PlanNodeStateChanged variant + live EventRef 三层策略 (wave 14 task 02 commit 2e7789a) + paused 7th lifecycle + review-gate question-event trigger (wave 16 task 04 commit a51bc52) + per-node retry policy v0 (wave 16 task 05 commit d8f8a6e) 全部 code-aligned (partial); 完整 11-stage (claim-lease / rollback / acceptance / mark-plan-final) 与 paused-resume listener (paused 节点收到 QuestionEvent::Resolved 后自动 re-dispatch) 仍 architecture-designed pending"
      :flow-anchor ".missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler"
      :actor-anchor ".missiond/v2/intent-intent-layer.lisp :: section action-instruction-actor :: actor plan-dag-scheduler"
      :runtime-v2-anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.plan-dag-runtime-v2"
      :evidence-anchor ".missiond/v2/intent-memory.lisp :: module directive-layer :: file-first-artifacts :: artifact plan-node-state-projection"
      :live-event-ref-anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"
      :worker-cross-ref ".missiond/v2/intent-worker.lisp :: section claudecode-workstation-orchestration :: dispatch-decision-matrix + execution-strategy-record"
      :coordination-protocol "复用 memory :: board :: helper agent-execution-coordination (id-counters / claims-with-lease / audit-repair) — D010 教训不自建 ID 池"
      :status code-aligned-partial)
    (unified-entry-pipeline-v1
      :desc "wave 13 task 03 (commit 9759675) v0 → wave 14 task 04 (commit 338a3fb) v1 — 不新增 MCP tool (仍 83); v1 file-first / review-gate / scheduler args 转发; 每 response 加 artifact_refs (flat object) + pipeline_stage + next_step; 4 项 v0 non-goal 仍 surface (人/Codex review 必须)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"
      :code "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :status code-aligned)
    (evidence-collector-typed
      :desc "wave 13 task 01 (commit 88568a9) typed EvidenceEntry + wave 14 task 02 (commit 2e7789a) live EventRef 三层策略 (live id → deterministic id → unavailable 兜底); plan.rs + plan_dag.rs 已接入"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.evidence-collector-typed-helper / intent-layer.evidence-collector-event-ref"
      :code "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
      :status code-aligned)
    (file-first-writer-integration
      :desc "wave 14 task 01 (commit 00cbc1d) — 三类 artifact (directive alignment / PLAN.lisp / workflow methodology) 全走统一 helper file_artifacts::attempt_artifact_write → resolve_target_project_root → atomic_write_artifact; partial 语义; 6 file_* 响应字段; dead_code 全清"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry memory.directive-layer.file-first-writer-integration"
      :code "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs + directive.rs + plan.rs + workflow.rs"
      :status code-aligned)
    (review-gate-policy
      :desc "wave 14 task 03 (commit 96842cd) — review_gate policy enum (manual|emit_question|off) auto-create v1; deterministic id 'review:<scope>:<id>:v<v>:<action>[:<topic-hash>]'; default byte-identical legacy"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.review-gate-policy"
      :code "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
      :status code-aligned)
    (source-index-checker-r015-r016
      :desc "wave 14 task 05 (commit 5c60f82) — scripts/check-architecture-lisp.mjs 加 R015 mandatory-fields + R016 section-id-uniqueness; :compression-safe? value enum (true|false|yes|no|safe|unsafe|defer); --dry-fixture 5 fixtures PASS; --all-v2 14 文件 OK"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.source-index-checker.r015-r016-implemented"
      :code "scripts/check-architecture-lisp.mjs"
      :status code-aligned)
    (source-index-checker-r017-r018
      :desc "wave 15 task 03 (commit b861b9a) — scripts/check-architecture-lisp.mjs 加 R017 source-file-must-exist + R018 source-file-must-live-under-v2 + 自动 shard 发现 (collectSourceFileRefs data-driven); --dry-fixture 5 → 10; --all-v2 19 文件 OK (含 5 wave-15 shard auto-discovered)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.source-index-checker.r017-r018-implemented"
      :code "scripts/check-architecture-lisp.mjs"
      :status code-aligned)
    (l2-shard-split-executed
      :desc "wave 15 task 02 (commit 3f37d32) — L2 shard 物理 split 已执行: 5 shard 文件 (intent-execution-governance / intent-directive-artifacts / intent-plan-dag / intent-capability-governance / intent-workstation-policy); 6 parent stub 化; 28 source-index :source-file 重定向; 105 section-id 全保 (R008 + R016); 内容 byte-identical"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.l2-shard-split.executed"
      :code ".missiond/v2/intent-{execution-governance,directive-artifacts,plan-dag,capability-governance,workstation-policy}.lisp"
      :status code-aligned)
    (review-gate-resolution-v0
      :desc "wave 15 task 04 (commit 03513c0) + wave 16 task 01 (commit 01708be) — review-gate explicit resolution bridge v0 + workflow handler 接入; 显式 review_decision (approved|rejected|needs_changes) + review_actor + review_note + envelope validator 5 fail-fast 错误码; 接 directive (approve/archive) + plan (approve/mark/supersede) + workflow (resolve_review stamp-only on workflow row, methodology receipt-only); 不新增 MCP tool (仍 83); 升: code-aligned-partial → code-aligned (3 surface 全接)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs + workflow.rs"
      :status code-aligned)
    (review-gate-resolution-listener-v0
      :desc "wave 16 task 02 (commit 331d1c1) — review-gate QuestionEvent::Resolved subscriber listener v0; spawn_review_resolution_sub 与 spawn_decision_sub 并行; subscribe QuestionEvent::Resolved → 解析 deterministic review id + ack; conservative vocabulary mapping (approved/approve/yes/accepted → approved 等); 抽 pure ReviewResolvedDispatch planner + parse_subscriber_resolution_string 进 review_gate.rs; 改 mod knowledge → pub(crate) mod knowledge 让 bus subscriber import bridges; subscriber 仅 consume answer 不替人答 (auto_answer_review_question 仍 surface)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0"
      :code "crates/missiond-daemon/src/bus/v2_subscribers.rs + handlers/knowledge/review_gate.rs"
      :status code-aligned-partial)
    (workstation-dispatch-v0
      :desc "wave 15 task 05 (commit 615b249) + wave 16 task 03 (commit 8ffa9b2) — workstation-dispatch v0 opt-in + auto-inference v1; PLAN node :workstation-dispatch true → mission_task_delegate transport (不 claude -p); task brief 含 objective/owned/forbidden/acceptance/commit-policy + agent-team literal 恰好一次; SafeDescriptor 不静默 fallback prompt mode; wave 16 加 5 inference rules (target=mission_task_delegate / strategy ∈ 4 strategies / objective 非空 / scoping signal / 非 explicit false) + workstation_dispatch_source 5 值 (explicit_arg / plan_hint / inferred / disabled / not_applicable); 升: code-aligned-partial → code-aligned (auto-inference 对 mission_task_delegate scoped node 已落). 完全 autonomous spawn (无 hint, 跨所有 target) 仍 surface 不实现"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
      :status code-aligned)
    (plan-dag-paused-lifecycle
      :desc "wave 16 task 04 (commit a51bc52) — PLAN DAG paused 7th lifecycle + review-gate question-event trigger v0; 节点 :review-gate 'question-event' (+ 可选 :review-action / :review-text) 触发 paused; deterministic review id 'review:plan:<plan_id>:v<v>:plan-node:<sha256(node_id)[..16]>'; aggregate_status='dag_paused' / runner_status='review_gate_paused'; bus failure → 仍 pause + warning; 不实现 auto-resume (paused-resume listener 仍 pending)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.plan-dag-runtime-v2.paused-lifecycle"
      :code "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs + review_gate.rs"
      :status code-aligned-partial)
    (plan-dag-retry-policy-v0
      :desc "wave 16 task 05 (commit d8f8a6e) — PLAN DAG per-node retry policy v0; :retry-count (additional) / :max-attempts (total) / :retry-delay-ms cap 60s + cap 3 attempts; 每 attempt 写自己 evidence (attempt number); SafeDescriptor refusals (UnsupportedTarget/ProjectRootUnresolved/MissingObjective) 不 retry; failure-policy 与 retry 正交 (retry exhaust 后 propagate_taint); 完整 retry-N 与 exponential backoff / route-to-rollback 仍 pending"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.plan-dag-runtime-v2.retry-policy-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
      :status code-aligned-partial)
    (scoped-commit-enforce-v0
      :desc "wave 16 task 06 (commit 591d288) — scoped commit handoff daemon enforce v0 opt-in; mission_execution 接 enforce_scoped_commit (default false 字节兼容); 4 错误码 (COMMIT_HASH_REQUIRED / COMMIT_BLOCKER_REQUIRED / CLAIM_SCOPE_REQUIRED / SCOPED_COMMIT_VIOLATION); gate 在 allocate_id 之前 (rejected 不 bump state); scope-overlap 与 audit/claim 同一 scopes_overlap helper; daemon 不跑 git (commit_hash 由 caller 提供); response 字段 scoped_commit_enforced + scoped_commit_validation"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.scoped-commit-enforce-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
      :status code-aligned)
    (evidence-event-subscription-v0
      :desc "wave 16 task 07 (commit 0e6ee63) — evidence event subscriber 三档 live/log/unavailable; passive subscriber cache (cap 1024 FIFO) key 'plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>' 严格匹配 deterministic id; EventRef::new alias EventRef::live 保 wave-13/14 byte-compat; subscriber 严格 observation-only (不 mutate 主路径); 持久化 event-log 查询面 (按 plan_id/node_id/time-range 检索) 仍 pending"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.evidence-collector-event-ref"
      :code "crates/missiond-daemon/src/bus/v2_subscribers.rs + bootstrap.rs + handlers/knowledge/evidence_collector.rs"
      :status code-aligned)
    (unified-entry-e2e-smoke-v0
      :desc "wave 16 task 08 (commit a632a91) — unified-entry deterministic 4 hand-off smoke (no LLM, no spawn); s1 directive dry_run → s4 plan dry_run → s6 execute dry_run → s6 evidence sidecar; 全程断言 v0_non_goals 持续 surface (4 项: auto_approve_directive / auto_approve_plan / auto_answer_review_question / autonomous_workstation_dispatch); 是 wave 13 task 03 v0 + wave 14 task 04 v1 的 e2e contract 回归基线"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.e2e-smoke-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :status code-aligned)
    (extensible-domain-count-test
      :desc "wave 15 task 01 (commit ea90c5d) — domain count 不再 hardcode: rename test domain_all_length_is_12 → domain_all_includes_execution; assert Domain::ALL.contains(Execution) + len() >= 13 floor (extensible, 不锁精确 count); event-bus.lisp 正文 protected 不动, 仅在 source-index 加 metadata entry"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry event-bus.section.execution-event.domain-all-extensible-test"
      :code "crates/missiond-core/tests/event_dispatcher_integration.rs"
      :status code-aligned)
    ;; ── wave 19 + wave 20 + wave 21 + wave 22 machine-contract / dispatch SSOT additions ──
    (machine-contract-task-protocol-v1
      :desc "wave 19 task 02-08 (commits 77f1f2b/ba58f20/c95eba8/5d425e2/bfc72b7/405d13b) — task-contract v1 schema + verifier (5 项检查 read-only 0 mutating git) + report-contract v1 + shared-memory v1 ledger (6 entry types) + renderer dispatch brief v1 (4 新节 + agent-team literal 单实例 + verify command) + plan emit (.missiond/tasks/generated/<plan_id>/<node_id>.lisp emit before dispatch) + workstation consume (overlay_contract + MalformedTaskContract SafeDescriptor 绝不 fall back claude -p) + execution complete verify (4 字段 + claim scope ⊇ contract write-scope + daemon read-only). plan emit → workstation consume → execution verify 全闭环"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.machine-contract.task-contract-v1"
      :code ".missiond/tasks/schema/task-contract-v1.lisp + scripts/verify-task-contract.mjs + scripts/check-task-contract.mjs + scripts/check-task-report.mjs + scripts/check-task-memory.mjs + scripts/render-claudecode-task.mjs"
      :status code-aligned)
    (machine-driven-dispatch-v0
      :desc "wave 20 task 04 (commit 681c95d) — DispatchContractMode { Rendered (default), Machine } + dispatch_contract_mode arg / render_markdown shorthand; Machine 模式接通 wave-19/07 dormant consumer (run_workstation_dispatch_with_contract): emit 产出 contract path 后, dispatch 直接 forward path 给 consumer 读盘 (overlay onto brief, contract 是 SSOT). Markdown brief 变 optional 兼容元数据 (render_command 仍 surface). brief 加 ## Source contract preamble. response surface task_contract_source_path field. DAG path TaskContractDispatchCtx 在 scheduler 入口锁定 mode. unified_entry forward 4 emitter/dispatch knobs. **Lisp 真正成 dispatch SSOT, Markdown brief 不再 load-bearing**; 默认 Rendered 不破 wave-15..19 byte-shape"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.machine-contract.dispatch-machine-mode-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/plan.rs + plan_dag.rs + workstation_dispatch.rs + unified_entry.rs"
      :status code-aligned)
    (task-scope-index-guard-v1
      :desc "wave 20 task 01 (commit 1fc0fd6) — scripts/task-scope-guard.mjs staged/commit 双 mode (staged 默认 git diff --cached --name-only 与 task contract write-scope/must-not-touch 对账; commit mode delegates verify-task-contract.mjs); .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发 (其它 commit 不阻塞 — opt-in 不破现有 workflow); 9+3 fixtures 覆盖; 全程 0 mutating git (grep proof). Caveat: git config core.hooksPath .githooks 默认未启用 (要 caller 显式 git config 才生效)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.machine-contract.task-scope-index-guard-v1"
      :code "scripts/task-scope-guard.mjs + .githooks/pre-commit"
      :status code-aligned-partial)
    (renderer-scoped-commit-guard-v2
      :desc "wave 20 task 02 (commit b36cf6c) — render-claudecode-task.mjs Commit 节追加 'Pre-commit guard (staged scope)' 子步: 'MISSIOND_TASK_CONTRACT=<task-lisp> node scripts/task-scope-guard.mjs --mode staged' (env prefix + 显式 mode); 实战通过 (wave20-02..09 commit 全程跑 staged guard). brief → preflight (wave 18-08 + wave 20-03 contract scope) → staged guard (wave 20-02) → commit → verifier (wave 19-02) → execution complete verify (wave 19-08) 五段闭环"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.machine-contract.renderer-scoped-commit-guard-v2"
      :code "scripts/render-claudecode-task.mjs"
      :status code-aligned)
    (execution-preflight-task-contract-scope-v1
      :desc "wave 20 task 03 (commit fe835e8) — mission_execution(action='preflight_commit') 加 8 新 response 字段 (task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path); 当 caller 传 task_contract_path 时 daemon 读 contract 解析 write-scope + must-not-touch, 与 git status --porcelain=v1 staged/unstaged 集合做交叉; 无新错误码 (信息性 surface); grep proof 0 mutating git (仍只 git status); legacy byte-compat 当 task_contract_path absent"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.scoped-commit-worktree-preflight-task-contract-v1"
      :code "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
      :status code-aligned)
    (review-auto-answer-policy-v0
      :desc "wave 20 task 08 (commit 8adb0a8) — AutoAnswerPolicy { Off (default), DeterministicSafe, DryRun }; 复用 wave-18/07 ReviewAutomationContext + safety inspector 加 2 new guard (destructive-action / caller-decision); **3 hard invariants test-pinned**: I1 NEVER auto-reject (上游 Rejected demote 到 NeedsChanges + audit 'rule:rejection_demoted'); I2 destructive actions (archive|supersede|remove) NEVER auto-promote 即使 5 safety rule 全过; I3 完全 pure / sync / 不调 LLM. unified_entry forward auto_answer_policy + build_artifact_refs lift 5 outcome keys (auto_answer_policy / policy_result / selected_decision / safety_rule_results / requires_human). default off byte-compat. NO 新 MCP tool. NO 实时 LLM. NO destructive auto-promotion"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.review-auto-answer-policy-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs + unified_entry.rs"
      :status code-aligned)
    (execution-event-dispatch-metadata-full-v0
      :desc "wave 14-02 启动 (PlanNodeStateChanged) → wave 18-02 加 (Claimed + Completed) → wave 19-11 NO-OP 验证 Opened 已含 → wave 20-09 (commit 6e01e3f) sweep 8 legacy variants (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) 全加 optional dispatch_strategy/target_project/requested_cwd serde-default skip-serializing fields, 从 companion log read_dispatch_metadata_from_log 自动继承; serde back-compat triple-pinned per variant. **11 variants 全 dispatch trio 闭环, 不再有 dispatch metadata gap**"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry event-bus.section.execution-event.dispatch-metadata-legacy-sweep-v0"
      :code "crates/missiond-core/src/event/events/execution.rs + crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
      :status code-aligned)
    ;; ── wave 21 propose+apply-gate additions ──
    (hooks-path-installer-v1
      :desc "wave 21 task 01 (commit 44c74df) — scripts/install-missiond-hooks.mjs --check/--install/--json/--dry-fixture/--strict + scripts/check-missiond-hooks.mjs read-only doctor alias; --install runs `git config --local core.hooksPath .githooks` exactly once + no-op when already aligned; never --global/--system; .githooks/pre-commit 保 MISSIOND_TASK_CONTRACT env-gated; **opt-in repo-local only — 不擅自 default-on** (per task brief: 'Do not enable hooks globally; repo-local only')"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.machine-contract.hooks-path-installer-v1"
      :code "scripts/install-missiond-hooks.mjs + scripts/check-missiond-hooks.mjs + .githooks/pre-commit + .missiond/tasks/schema/task-contract-v1.lisp"
      :status code-aligned-partial)
    (task-run-verifier-v1
      :desc "wave 21 task 02 (commit 1335fa7) — scripts/verify-task-run.mjs 三合一 (task contract + report task_id + commit_hash + memory completion) post-run proof; --task/--report/--memory/--commit/--json/--dry-fixture/--allow-missing-memory; delegates contract verification to verify-task-contract.verifyContract; 12 dry-fixtures + 7 helper cases + 14 forbidden git verb proof + dogfood self-verify (verify-task-contract.mjs main() now gated by import.meta.url so importers don't trigger CLI parsing)"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.machine-contract.task-run-verifier-v1"
      :code "scripts/verify-task-run.mjs + scripts/verify-task-contract.mjs + .missiond/tasks/schema/{report-contract-v1,shared-memory-v1}.lisp"
      :status code-aligned)
    (execution-report-verifier-integration-v1
      :desc "wave 21 task 03 (commit 308426e) — mission_execution(complete) 加 4 新字段 (task_run_verifier_status enum / shared_memory_path / verifier_diagnostics / verified bool); verified=true 触发 daemon-internal sexp-parse cross-check (无新 dep, 用 in-tree sexp parser) + 4 structured error codes; 4 字段持久化为 Lisp keyword 进 completion entry; preflight_commit 也 echoes task_report_path/shared_memory_path advisory hints; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威; **daemon_never_invokes_mutating_git unit-pinned**"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.execution-report-verifier-integration-v1"
      :code "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs + crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
      :status code-aligned)
    (autonomous-workstation-llm-proposal-v0
      :desc "wave 21 task 04 (commit 68b84f1) — workstation_inference_mode=off|sonnet_suggest opt-in; WorkstationProposalGate 仅在 caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in 全空时才触发 propose; 4 propose 字段 × 3 confidence × 4 safety status; workstation_proposals[] cap 6; **propose only never auto-spawn — applied=false / auto_spawn=false 永钉死**; conservative whitelists target ∈ {mission_execution, mission_task_delegate, mission_flow_run} + dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} (prompt-fallback / unknown 排除); Sonnet unavailable surfaces status='llm_unavailable' + reason 钉 'no fallback to claude -p / prompt mode'; DAG mode preflight rejects sonnet_suggest INVALID_PARAM; attach_workstation_proposals_block 在每 dispatch 分支 splice 不污染 errors"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.workstation.autonomous-workstation-llm-proposal-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs + plan.rs + crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :status code-aligned-partial)
    (plan-inference-apply-gate-v1
      :desc "wave 21 task 05 (commit a18200b) — apply_inferred_fields=true opt-in apply gate 接 wave-18/06 deterministic + wave-20/07 LLM proposals; **6 道严格 gate**: caller_approval (llm_caller_approved 显式) / master_flag / confidence ∈ {high (deterministic) | high+medium (LLM)} / conflict_status=none / per-field safety / slot availability; **8 skip reason canonical**; apply_gate block 在每 dispatch 分支 surface stable shape; **persisted plan.sexp_text 永不 mutate** persist_inference_applied=false 永钉死 (persist_inference flag 仅 echo for future-wave wiring); strict shape (typo string \"true\" → INVALID_PARAM); ApplySafe back-compat preserved for dag_v1 inference branch"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.plan-dag.plan-inference-apply-gate-v1"
      :code "crates/missiond-daemon/src/handlers/knowledge/plan.rs + crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :status code-aligned-partial)
    (llm-auto-approve-proposal-v0
      :desc "wave 21 task 06 (commit e140773) — auto_approve_mode=off|sonnet_suggest opt-in for directive (approve|archive) + plan (approve|mark|supersede) review surfaces; ORTHOGONAL to wave-18/07 review_automation_policy AND wave-20/08 auto_answer_policy (3 knobs co-exist on response); **5 invariants test-pinned**: I1 NEVER auto-reject (rejected from model demoted to needs_changes + proposal_warnings[]); I2 destructive (archive|supersede|remove case-insensitive) short-circuit destructive_blocked WITHOUT calling Sonnet; I3 applied=false + requires_human=true 永钉死 (propose-only); I4 Sonnet unavailable surfaces llm_unavailable + reason, NO fallback to deterministic; I5 destructive_check ALWAYS sourced from is_destructive_review_action(action) via enforce_proposal_invariants helper; 22 unit tests; 10 dispatch branch sites; strict-enum parse (typo `auto_approve_mode=\"auto\"` → INVALID_PARAM); caller-supplied review_decision ALWAYS wins"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.llm-auto-approve-proposal-v0"
      :code "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs + directive.rs + plan.rs + crates/missiond-mcp/src/tools/knowledge/{directive,plan}.rs"
      :status code-aligned-partial)
    (sonnet-distill-chain-auto-apply-v1
      :desc "wave 21 task 07 (commit 4d494db) — auto_sonnet=true + auto_sonnet_approved=true 双 opt-in apply-gate 接 wave-20/06 cross-plan distill auto-trigger; **7 重 gate**: 双 opt-in + auto_chain_trigger=auto_safe + ALL 6 wave-20 deterministic safety rule + caller distill_mode != sonnet; auto-promote inner distill from dry_run to sonnet via direct call to action_distill_sonnet; **8 status taxonomy** (not_requested..applied_sonnet); **7 invariants test-pinned**: I1 default-off byte-shape; I2 dual opt-in (single typo cannot escalate); I3 reuse wave-20 trigger outcomes never relax; I4 caller-already-sonnet refusal; I5 Sonnet failure preserve inner payload (model_call_status=failed|invalid_output); I6 review_required=true PINNED on every successful auto-apply (receipt-only, no DB transition); I7 wave-19/20 blocks UNCHANGED purely additive; 16 + 4 unit tests; strict shape validation"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.plan-dag-runtime-v2.sonnet-distill-chain-auto-apply-v1"
      :code "crates/missiond-daemon/src/handlers/knowledge/workflow.rs + plan.rs + crates/missiond-mcp/src/tools/knowledge/workflow.rs"
      :status code-aligned-partial)
    (machine-contract-autonomous-loop-smoke-v3
      :desc "wave 21 task 08 (commit 8ba8723) — 15 deterministic e2e smoke tests across 4 handler files (unified_entry +3 / plan +3 / workstation_dispatch +3 / agent_execution +6); covers wave21-03 verifier 5-rule cross-check happy + 5 structured failure paths; pins wave21-04 I3+I4+I5 (workstation_proposals applied=false / auto_spawn=false / unavailable surfaces no fallback); pins wave21-06 I1+I2+I3+I5 (NEVER rejected / destructive_blocked / requires_human=true / destructive_check from helper); pins wave21-07 I1+I3+I7 (default-off / chain reuses trigger outcomes / chain block additive); **Markdown task_brief_preview NEVER projected into artifact_refs (non-load-bearing 二度钉死)**; machine dispatch SSOT task_contract_path == task_contract_source_path 钉死; no LLM, no spawn, no shell"
      :anchor ".missiond/v2/intent-pillar-source-index.lisp :: section-entry intent-layer.unified-entry-pipeline.machine-contract-autonomous-loop-smoke-v3"
      :code "crates/missiond-daemon/src/handlers/knowledge/{unified_entry,plan,workstation_dispatch,agent_execution}.rs"
      :status code-aligned))

  ;; ── v2 递归同构标准: 原子 / 分子 / pillar ──
  (recursive-architecture-standard
    :goal "所有 pillar 都按 ingress → logic-core → egress 描述; logic-core 内继续按功能递归展开"
    :shape
      ((pillar "pillar-ingress → pillar-core → pillar-egress")
      (function "ingress → logic-core(step s1/s2/...) → egress")
      (step "ordered action with owner pillar + reads/writes/emits/returns")
      (tool "schema ingress → dispatch logic-core → ToolResult/audit egress")
      (flow "trigger/state ingress → ordered cross-pillar steps → writes/emits/returns/downstream egress"))
    :ownership-rules
      ["memory owns durable schema/state"
       "event-bus owns append/subscribe/persistence log"
       "tools owns external endpoint schema/routing/audit"
       "worker owns runtime mechanics/execution"
       "intent-layer owns prescription/reasoning/self-description"
       "system-layer owns type/process/transport/RPC/pure runtime substrate"
       "flow owns cross-pillar choreography narrative"]
    :editing-rule "后续梳理任何功能时, 先定位所属 pillar, 再按 ingress/core/egress 下钻到 step")


  ;; ═══════════════════════════════════════════════════
  ;;  一 · 记忆 (Memory)
  ;;  系统的长期记忆 — 详见独立 lisp
  ;; ═══════════════════════════════════════════════════
  ;; 详细规格在 intent-memory.lisp (草稿),本处只作导航摘要
  (pillar memory
    :file ".missiond/v2/intent-memory.lisp"
    :status "v0.5.8 — 9 modules + directive artifacts + agent-execution dual-plane handoff (control + durability) + capability-usage-read-model semantic evidence v1 + directive-layer actor v0 全部 code-aligned partial; plan-node-state-projection 顶层字段 + plan-evidence-sidecar typed 已 code-aligned partial; wave 14 task 01: file-first writer integration 三类 artifact 主路径 code-aligned; wave 15 task 02: file-first-artifacts + capability-usage-read-model 物理 split 到 shard; wave 16 task 01/04/05/06: plan-node-state-projection 加 paused 7th lifecycle + per-attempt evidence; review-gate-resolution-v0 升 code-aligned. wave 19 task 04 + task 08: shared-memory v1 ledger (6 entry types + checker + seed) + mission_execution(complete) 加 task_contract_path/task_report_path/verifier_status 与 claim scope ⊇ contract write-scope. wave 20 task 03 + task 08: mission_execution preflight_commit 加 8 contract-scope 字段; review-gate auto-answer policy v0. **wave 21 task 03 (commit 308426e) 加 execution report-verifier integration v1**: mission_execution(complete) 加 4 新字段 (task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified) + verified=true 触发 daemon-internal sexp-parse cross-check (无新 dep) + 4 structured error codes (VERIFIED_REQUIRES_* / TASK_REPORT_TASK_ID_MISMATCH / TASK_REPORT_COMMIT_HASH_MISMATCH / TASK_CONTRACT_MALFORMED) + 4 字段持久化为 Lisp keyword 进 completion entry; **daemon_never_invokes_mutating_git unit-pinned**; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威. **wave 21 task 06 (commit e140773) 加 LLM auto-approve proposal v0**: directive (approve|archive) + plan (approve|mark|supersede) review surfaces 加 auto_approve_mode opt-in (5 invariants pinned: I1 never auto-reject / I2 destructive blocked WITHOUT LLM call / I3 applied=false + requires_human=true 永钉死 / I4 sonnet unavailable no fallback / I5 destructive_check from is_destructive_review_action helper); 三 review knob (review_automation_policy + auto_answer_policy + auto_approve_mode) 共存独立观测; persisted directive/plan/review state v0 永不被自动 mutate. 完整 11-stage scheduler 主线已 close; enforce-by-default scoped commit + git config core.hooksPath .githooks 默认启用 (wave21-01 仍 opt-in repo-local only) + LLM 自动 approve 真正落 (wave21-06 propose only) + persisted plan inference apply (wave21-05 persist_inference_applied=false 永钉死) 仍 pending (详 wave-13..21 anchors via intent-pillar-source-index.lisp)"
    :paradigm "4 mature modules (project-management / board / kb-manager / conversation-logs) 自治 + 系统支持 + 横切"

    (purpose "系统长期记忆: 4 个业务模块自治管理自己的表 + 底层系统支持层 + 横切")
    (storage "PostgreSQL via sqlx::PgPool")
    (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

    (migrated-out
      "embedding-provider → pillar 二 worker :: xjp-router-gateway (qwen3 独立 provider, code-aligned for embedding)"
      "gen-crud (Forge 冲压) → pillar 二 2.5 code-generation"
      "search-engines → pillar 二 2.6 search-engines (搜索是计算不是数据)"
      "event-bus 4 表 → pillar 四 §4.6 persistence-layer (event_log / subscriptions / blob_storage / dlq)")

    ;; ── 结构 (9 module: 5 business + 4 support + 横切) + 5 surface (v0.4.18 pillar-interfaces) ──
    (structure
      ;; ── 5 Business Modules — 各自 in/core/out + 显式 module-tables-owned ──
      (module project-management
        :desc   "项目作用域: 注册 + per-project 代码快照 intent.lisp 文件 + skills"
        :target "intent-memory.lisp :: module project-management"
        :owned-tables 5
        :v0.4.4-change "specs 4 表 (intent/plan/workflow/user_intents) 迁到 pillar 五 action-instruction-specs"
        :v0.4.16-correction "user_intents 实际从未迁出, 仍在 conversation-logs (trait=ConversationStore)"
        :v0.4.17-change "intent/plan/workflow 3 张从 pillar 五 回归 memory 新建 module directive-layer; v0.4.25 校正为 store-ready actor-pending"
        :v0.4.19-rename "命名去歧义: DB 表 intent → directive; module intent-layer → directive-layer; 避和 <project>/.missiond/intent.lisp (代码画像) 混淆"
        :mcp    "mission_project / mission_intent (只读 FILE) / mission_skill_*")

      (module board
        :desc   "任务队列: 27 列 7 态 FSM + autopilot + flow + agent_questions + prompt_snapshots"
        :target "intent-memory.lisp :: module board"
        :owned-tables 4
        :mcp    "mission_board_* (8 个) + mission_question")

      (module kb-manager
        :desc   "知识库: 语义记忆 + 代码索引 (ast/beacons) + 访问审计 + KB↔AST 链接"
        :target "intent-memory.lisp :: module kb-manager"
        :owned-tables 9
        :mcp    "mission_kb_* / mission_insight / mission_memory / mission_code_search / mission_universe_graph")

      (module conversation-logs
        :desc   "三引擎(Claude Code/Gemini/Codex)会话记录 + 派生分析 + user_intents"
        :target "intent-memory.lisp :: module conversation-logs"
        :owned-tables 15
        :v0.4.16-change "+1 user_intents 校正回归 (writer=intent_analyst, trait=ConversationStore)"
        :non-db-source "PTY JSONL (~/.claude/projects/{encoded}/*.jsonl)"
        :mcp    "mission_conversation_* / mission_retrospective_manage / mission_audit / mission_llm_trace")

      (module directive-layer
        :desc   "user utterance → lisp 指令编译 pipeline (directive → plan → workflow 三段式)"
        :target "intent-memory.lisp :: module directive-layer"
        :owned-tables 3
        :status "store+manager code-aligned partial (DirectiveLayerStore + Pg impl + mission_directive/plan/workflow read/control/draft surfaces exist; actors pending)"
        :future-writer "pillar 五 actor preferred: directive-compiler / plan-compiler / workflow-distiller; MCP tools are manager/read/control surface"
        :mcp    "mission_directive / mission_plan / mission_workflow (code-aligned partial)")

      ;; ── 4 Support Modules (v0.4.13-15 从 category system-support 分化 + v0.4.21 新增 embedding) ──
      (module llm-support
        :desc   "LLM 调用观测 — 请求日志 + 文件上传 + token 成本"
        :target "intent-memory.lisp :: module llm-support"
        :owned-tables 3
        :migrated-from "v0.4.13 category system-support :: global-observability"
        :mcp    "mission_llm_trace / mission_cost_report (🚧)")

      (module slot-support
        :desc   "Slot 运行时 — session 绑定 + learning-engine AI 任务 + dynamic slot lifecycle"
        :target "intent-memory.lisp :: module slot-support"
        :owned-tables 3
        :migrated-from "v0.4.14 category system-support :: compute-runtime"
        :mcp    "mission_slots / mission_slot_history / mission_compute_slot")

      (module system-support
        :desc   "系统级基础 — 告警 + router 归档 + vision 缓存 + infra 游标 + backfill + capability usage derived monitor + 4 legacy"
        :target "intent-memory.lisp :: module system-support"
        :owned-tables 14
        :migrated-from "v0.4.15 category 升格为 module (剩 LLM 3 + slot 3 分离后的 10 active + 4 legacy)"
        :mcp    "mission_incident / mission_router_chat / mission_sys_config / mission_sys_logs / mission_infra_query / mission_inbox (legacy)")

      (module embedding-support
        :desc   "embedding 列跨表治理 — 0 张独占表, 管 5 承载表 + 1 audit 表的列契约 (column-ownership)"
        :target "intent-memory.lisp :: module embedding-support"
        :owned-tables 0
        :special-nature "column-ownership vs row-ownership 双轨: 本 module 管 '列契约 + policy', 承载表的行归原 module (kb-manager / conversation-logs / project-management)"
        :migrated-from "v0.4.21 cross-cutting :: capability embedding-storage-governance 升格")

      ;; 横切能力
      (cross-cutting
        :desc   "db-trait-abstraction (9 store) / retention-policy / migrations-runner (embedding 治理 v0.4.21 已升格为 module)"
        :target "intent-memory.lisp :: cross-cutting")

      ;; Pillar Interfaces — 正交维度 (v0.4.18)
      (pillar-interfaces
        :desc   "5 surface (mcp / worker-trait / frontend / cross-pillar / external-filesystem) × 9 module 正交矩阵"
        :target "intent-memory.lisp :: pillar-interfaces"
        :binding "每个 writer/reader 通过 :binds-to 指向 surface; 96 个 writer/reader 100% 覆盖"))

    ;; ── 关键基础设施位置 (快速导航) ──
    (key-locations
      (mission-store-trait    :at "crates/missiond-core/src/db/traits.rs  — 9 store 超 trait (v0.4.20 修正, 原 13)")
      (projects-table         :at "crates/missiond-core/src/db/pg/project.rs")
      (board-table            :at "crates/missiond-core/src/db/board.rs")
      (knowledge-table        :at "crates/missiond-core/src/db/knowledge.rs")
      (conversation-table     :at "crates/missiond-core/src/db/conversation.rs")
      (audit-table            :at "crates/missiond-core/src/db/audit.rs")
      (timeline-ssot          :at "pillar 四 event_log (SSOT, v1.3.0+) — 原 timeline.rs 代码待 cutover 后删")
      (intent-loader          :at "crates/missiond-daemon/src/handlers/knowledge/intent.rs")
      (lisp-survey-worker     :at "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs")
      (conversation-logger    :at "crates/missiond-daemon/src/workers/local/conversation_logger.rs")
      (embedding-worker       :at "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs  — 生成路径在 pillar 二 2.3")
      (context-pipeline       :at "crates/missiond-daemon/src/context/")
      (flow-engine-v2         :at "crates/missiond-daemon/src/engine/flow/")
      (migrations             :at "crates/missiond-core/migrations/"))

    :maturity-ladder "v0.4.x 演进: 4 成熟模块 → 5+3 (v0.4.15 category 升格) → 5+4 (v0.4.21 embedding-support 新建) + pillar-interfaces (v0.4.18) 正交维度 + 命名去歧义 (v0.4.19)"
    :note "此 pillar 只列导航; 详细模块内部 in/core/out 在 intent-memory.lisp")



  ;; ═══════════════════════════════════════════════════
  ;;  二 · worker (Worker Layer)
  ;;  MissionD 如何驱动外部 / 后台计算
  ;;  = 三种传输介质 (PTY / LLM API / 本地) + 统一编排
  ;; ═══════════════════════════════════════════════════
  (pillar worker
    :canonical-ref ".missiond/v2/intent-worker.lisp"
    :canonical-status "v0.5 phase-C 2026-04-27 — recursive contract + xjp-router provider + mission_execution manager + project-root spawn cwd + claudecode-workstation-orchestration policy + dual-plane scoped-commit handoff + dispatch_strategy/target_project/requested_cwd companion log + plan-runner v0 + auto-selection v1 + PLAN DAG runtime v2 全部 code-aligned partial; wave 14-18: ExecutionEvent::PlanNodeStateChanged + live EventRef + workstation-dispatch v0 + auto-inference v1 + scoped commit enforce v0 + claim-lease v0 + paused-resume + acceptance + rollback + finalize/distill + autonomous PLAN field inference v0 deterministic + scoped-commit worktree preflight v0. wave 19 task 07/08: workstation task-contract consumer v0 + execution task-contract completion verification v0. wave 20 task 03/04/09: mission_execution preflight_commit 加 8 contract-scope 字段; machine-driven dispatch v0 (Lisp 真成 dispatch SSOT, Markdown brief 不再 load-bearing); ExecutionEvent legacy metadata sweep v0 (11 variants 全闭环). **wave 21 task 03/04 (commits 308426e/68b84f1) 闭环**: execution report-verifier integration v1 (mission_execution(complete) 加 4 新字段 task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified + verified=true 触发 daemon-internal sexp cross-check + 4 structured error codes + daemon_never_invokes_mutating_git unit-pinned, daemon NEVER spawns Node); autonomous workstation LLM proposal v0 (workstation_inference_mode=off|sonnet_suggest opt-in + WorkstationProposalGate 仅在 caller+PLAN 全空才 propose + 4 propose 字段 × 3 confidence × 4 safety status + workstation_proposals[] cap 6 + **propose only never auto-spawn — applied=false / auto_spawn=false 永钉死** + DAG mode preflight rejects sonnet_suggest INVALID_PARAM). 完整 autonomous workstation true spawn (wave21-04 propose only) / enforce-by-default scoped commit + git config core.hooksPath .githooks 默认启用 (wave21-01 仍 opt-in repo-local only) / report-contract checker auto-invoke from daemon (wave21-03 仍 caller-supplied verified flag) 仍 pending (详 wave-13..21 anchors)"
    :v0.1-archive ".missiond/v2/drafts/gptpro/intent-worker.lisp"
    :v0.2-gptpro-archive ".missiond/v2/drafts/gptpro/intent-worker-v0.2.lisp"
    :execution-log ".missiond/v2/worker-pillar-execution.lisp"
    (purpose "系统如何把计算派出去 — 三种传输 + 统一调度抽象")

    ;; ── 2.1 PTY 传输: 直接控制 CLI 进程 ──
    (section pty
      (desc "对 Claude / Gemini / Codex CLI 的终端级感知 + 操作,把终端当一等公民")

      (component pty-manager
        (desc "多会话管理器: 生命周期 + 调度 + 异常处理")
        :target "crates/missiond-pty/src/manager.rs")

      (component session
        (desc "单个 PTY 会话: 读写 / 截屏 / 异常 / 增量提取")
        :target "crates/missiond-pty/src/session.rs"
        :children ("screenshot 截屏" "extractor 增量提取" "anomaly 异常检测"))

      (component semantic-parser
        (desc "PTY 输出 → 结构化状态: idle / running / confirm / title / tool-call / fingerprint")
        :target "crates/semantic-terminal (独立外部 crate)"
        :tracks ("state 状态" "confirm 确认对话框" "tool 工具调用" "title 终端标题" "fingerprint 指纹识别"))

      (component pty-event-worker
        (desc "监听 PTY 状态变更,发射 slot 事件,自动批准已知确认弹窗")
        :target "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
        :emits "SlotBecameIdle / SlotStuck"
        :auto-approves "'don't ask again' / 'always' / 'trust' / '不再' 关键词")

      (component slot-manager
        (desc "计算位(slot)管理: 常驻 Claude CLI 实例池 + 动态按需调度")
        :target "crates/missiond-daemon/src/slot_manager/"
        :authority "SlotManager 是槽位生命周期的唯一权威")

      (component slot-orchestrator
        (desc "按 slot 角色驱动对应 PTY 控制器,代码中 CC/Gemini 两类控制器; project-bound spawn cwd 必须是目标项目根")
        :target "crates/missiond-daemon/src/slot_orchestrator/"
        :children ("cc_controller.rs — Claude Code PTY 控制器"
                   "gemini_controller.rs — Gemini CLI PTY 控制器"
                   "agent.rs — agent 任务派发"))

      (component conversation-ingestion
        (desc "PTY 写出 JSONL 后的解包路由 — conversation-logger worker 的后端")
        :target "crates/missiond-daemon/src/events_sync.rs"
        :routes "handle_new_events (实时增量) / backfill_conversation_events (启动一次性回填)"
        :helpers "extract_visible_text / extract_tool_names_csv / extract_tool_names"
        :writes-to "conversation_messages / conversation_events"
        :consumer "conversation-logger worker (local)"
        :note "文件名 events_sync.rs 是历史遗留,与 TimelineEvent 总线无关,实际只处理 PTY JSONL → DB"))

    ;; ── 2.2 LLM 接入层: 多模型统一门面 ──
    (section llm-gateways
      (desc "按 model 路由到 API 或 PTY,叠加限流 / 重试 / 观测")
      :target "crates/missiond-daemon/src/llm/"

      (component llm-gateway
        (desc "顶层 trait + 工厂,按 model_id 分派到具体 gateway")
        :target "llm/llm_gateway.rs")

      (component llm-gate
        (desc "跨 gateway 共享的限流闸门 (并发 / rate-limit)")
        :target "llm/llm_gate.rs")

      (component sonnet-gateway
        (desc "Claude Sonnet API chat gateway")
        :target "llm/sonnet_gateway.rs"
        :routes-to "Anthropic API (chat)"
        :used-by "translation / arch-maintenance / retro / lisp-survey workers"
        :embedding-removed "embedding 已迁 xjp-router-gateway; sonnet 只做 chat")

      (component xjp-router-gateway
        (desc "QWEN3 embedding 独立 provider adapter; 未来可扩 chat/rerank")
        :target "llm/xjp_router_client.rs"
        :routes-to "xjp-router HTTP /embed on Windows 12900KF + RTX3090Ti"
        :used-by "embedding-worker → kb_embeddings / ast_embeddings / turn_topics"
        :status "code-aligned for embedding; chat/rerank deferred"
        :embedding-invariant "qwen3 是唯一 embedding provider, 禁止降级兜底 — 失败直接报错")

      (component gemini-gateway
        (desc "Gemini 多路适配 — driver 分派 + CLI PTY / Cloud API / File API 三种模式")
        :target "llm/{gemini_driver,gemini_cli,gemini_client,gemini_file_api,gemini_pty}.rs"
        :modes "CLI PTY / Cloud API / File API")

      (component codex-gateway
        (desc "Codex = Claude Code PTY 模式 — 经 slot_orchestrator/cc_controller")
        :target "llm/codex_cli.rs + slot_orchestrator/cc_controller.rs"
        :routes-to "Claude Code CLI via PTY")

      (component minimax-gateway
        (desc "⚠ 已弃用,保留向后兼容,生产路径已迁 Sonnet")
        :target "llm/{minimax_gateway,minimax_client}.rs"
        :status "deprecated")

      (component prompts
        (desc "跨 gateway 共享 system / task prompt 模板")
        :target "llm/prompts.rs"))

    ;; ── 2.3 后台 worker 集群: 19 个计算租户 ──
    (section workers 19
      (desc "反应式 + 定时 + 外部触发的后台计算单元,按执行介质分组")
      :target "crates/missiond-daemon/src/workers/"
      :v1.3.0-change  "sonnet 组 briefing_worker 删除 (SSOT cutover, commit 6789509); 6 → 5"
      :v0.4.12-change "codex 组 step_narrator 删除 (narration 表下线); 2 → 1; 总 20 → 19"

      (group sonnet 5
        :examples "embedding / translation / arch-maintenance / retro / lisp-survey"
        :routes-via "SonnetGateway (直接 API)"
        :target "workers/sonnet/"
        :writes-to-memory
          ("embedding → kb-manager(knowledge.embedding_vec + ast) + conv-logs(message_embeddings + topic_vectors) + project-mgmt(skill_topics)"
           "translation → conv-logs(message_translations)"
           "arch-maintenance → kb-manager(knowledge category=architecture)"
           "retro → conv-logs(retrospective_results)"
           "lisp-survey → 项目 .missiond/intent.lisp 文件 (project-management)"))
      (group codex 1
        :examples "vision"
        :routes-via "Claude Code PTY via slot_orchestrator/cc_controller"
        :target "workers/codex/"
        :writes-to-memory "vision → system-support(image_descriptions)"
        :v0.4.12-removed "step_narrator.rs 随 message_narrations 表下线")
      (group gemini 1
        :examples "strategy"
        :routes-via "Gemini CLI PTY via slot_orchestrator/gemini_controller"
        :target "workers/gemini/")
      (group local 12
        :examples "conversation-logger / conversation-organizer / pty-event / tagger-chunker / experience-harvester / reconcile / gemini-reconcile / ast-sync / code-prefetch / codex-ingestion / gemini-logger / xjpcode-briefing"
        :routes-via "纯本地计算,无 LLM 依赖"
        :target "workers/local/"
        :note "数量最多,承担 JSONL 摄入 / 分块 / 打标 / 时间线同步 / 外部状态对账"
        :writes-to-memory
          ("conversation-logger / codex-ingestion / gemini-logger / gemini-reconcile → conv-logs (三引擎摄入)"
           "conversation-organizer → conv-logs(turns + tool_calls)"
           "experience-harvester / tagger-chunker → kb-manager(knowledge)"
           "ast-sync → kb-manager(ast_nodes/beacons)"
           "gemini-reconcile → system-support(reconcile_watermarks)")))

    ;; ── 2.4 编排: 生命周期 + 级联控制 ──
    (section orchestration
      (desc "worker 注册 / spawn / 级联 pause-resume 的统一治理")

      (component worker-registry
        (desc "BackgroundWorker trait 注册 + spawn + ControlTree 依赖自动注入")
        :target "workers/registry.rs"

        (trait BackgroundWorker
          (const KIND :type "WorkerKind" :enum "Sonnet / Codex / Gemini / Local")
          (method name         :returns "&str")
          (method extra-deps   :returns "Vec<Dependency>")
          (method run          :args "ctx: WorkerContext"))

        (struct WorkerRegistry :desc "全局注册表 + spawn 入口")
        (struct WorkerHandle   :desc "单个 worker 句柄 — 停止 / 状态查询")
        (struct WorkerContext  :desc "注入: ControlManager + AppState + shutdown signal")
        (struct WorkerInfo     :desc "对外元信息")

        :invariant "KIND 必须匹配 worker 所在子目录; ControlTree provider 依赖由 spawn_worker 自动注入")

      (component control-tree
        (desc "细粒度级联 pause / resume — worker / 数据流 / 项目 三层隔离")
        :target "crates/missiond-daemon/src/control_tree.rs"

        (struct ControlTree
          (field global-paused       :type "bool"                          :desc "全局总闸")
          (field providers           :type "HashMap<CtlProvider, bool>"    :desc "按 LLM provider 暂停 (Sonnet/Codex/Gemini)")
          (field domains             :type "HashMap<CtlDomain, bool>"      :desc "按数据域暂停 (Memory/Embedding/KB/...)")
          (field workers             :type "HashMap<String, bool>"         :desc "按 worker 名显式覆盖")
          :workers-semantics "true=强制暂停; false=强制恢复(调试 override); 不存在=跟随级联"
          (field slot-roles          :type "HashMap<String, bool>"         :desc "按槽位角色暂停")
          (field projects            :type "HashMap<String, bool>"         :desc "按 project_id 暂停(项目级数据流隔离)")
          (field domain-paused-at    :type "HashMap<CtlDomain, i64>"       :desc "域暂停时间戳,仅信息性不参与判断"))

        (cascade-priority
          :method "is_effectively_paused(worker_name, deps: &[Dependency])"
          :order "三级优先,从高到低:"
          (p1 worker-explicit-override
            :semantics "workers[name]=true → 恒暂停; =false → 恒恢复(debug); 不存在 → 跟随级联")
          (p2 global-kill-switch
            :semantics "global_paused=true → 全部 worker 暂停(除非 worker 有显式覆盖)")
          (p3 provider-domain-cascade
            :semantics "逐个检查 Dependency::Provider / Dependency::Domain; 任一 true → 暂停"))

        (struct ControlManager
          :pattern "push-based watch broadcast (NOT polling)"
          :transport "tokio::watch::channel<ControlTree>"
          :semantics "变更 → send_modify() 原子更新 → 所有订阅者经 changed().await 收通知"
          :persistence "control_tree.json — 崩溃恢复经 spawn_blocking 写入"
          :worker-await "Worker 在 select! 中 await watch::Receiver::changed() — 零成本异步推送,非 HashMap 轮询"
          (mutations "set_global_paused / set_provider / set_domain / set_worker / set_slot_role / set_project"))

        :project-pause-note "is_project_paused(id) 由 handler 独立检查,不属于 is_effectively_paused() 的 worker 级联 — 项目控制数据流,不控制 worker")

      ;; ── 驱动 memory state transitions 的 engine (非 worker 也非 dispatcher) ──
      (component autopilot
        (desc "任务队列自主推进引擎 — tick 扫 board → CAS 占用 → 派发 → lease 回收")
        :target "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
        :tick "5-10s 可配"
        :tick-pipeline "memory-scheduler → extraction-check → board-task-dispatch → flow-progression → supervision-check"

        (dispatch-logic
          "list_autopilot_tasks: WHERE auto_execute=1 AND status='open' ORDER BY (assignee 存在) → order_idx"
          "claim_board_task(id, autopilot_id, 'pty_slot') 原子 CAS 占用"
          "claim 成功 → status→running + 派给 assignee 或自动选 slot"
          "list_running_autopilot_tasks: 监控已 claim 任务的租约"
          "lease 超期 → recover_stale_running_tasks 强制 reset open")

        (writes-to-memory
          :primary "board_tasks (CAS claim / status 推进 / lease 回收)"
          :auxiliary "prompt_snapshots (save_prompt_snapshot: task 执行时 prompt + KB citation 存档)")

        :serves "pillar 一 memory :: module board :: path task-queue-lifecycle"
        :scan-reads "board_tasks (内部决策前置读取, 自读自写闭环)"
        :invariant "CAS 原子保证多 executor 并发安全; lease 保证崩溃后任务可回收")

      (component flow-engine-v2
        (desc "声明式工作流编排引擎 — flow YAML 节点的运行时执行器")
        :target "crates/missiond-daemon/src/engine/flow/{mod,runner,handlers,loader}.rs"
        :node-types 5 "LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
        :loads "$MISSIOND_HOME/flows/*.yaml (pillar 五 :: workflows :kind executable)"

        (execution-model
          "逐节点执行; 每节点完成后 persist_context → update_board_task(flow_context JSON)"
          "flow_phase 推进 + 变量插值"
          "支持分支 + 并行 (ParallelSlotTasks)")

        (writes-to-memory
          :primary "board_tasks.flow_context (每节点 persist 保证崩溃可恢复)"
          :storage "flow_context JSONB 独立于 status 变动存续")

        :serves "pillar 一 memory :: module board :: path task-queue-lifecycle"
        :invariant "每节点执行后必须 persist; 失败时上游节点结果保留"))

    ;; ── 2.5 Code Generation (Forge 冲压) ──
    (section code-generation
      (desc "Forge 冲压: Lisp → IR → Rust; build-time + MCP 运行时触发")
      :cross-ref "pillar 五 intent-layer :: component forge (冲压器本体, 独立仓库)"
      :migrated-in "memory v0.2 :: 1.4 cross-cutting :: gen-crud (2026-04-19)"

      (component gen-crud
        (desc "CRUD 代码按领域分文件, 模式驱动 — 改 lisp 不改代码")
        :target "crates/missiond-core/src/db/gen_{kb,board,conversation,compute,knowledge,misc,pipeline,skill,audit}.rs")

      (component gen-types
        (desc "跨 crate 共享的枚举 + 结构体")
        :target "crates/missiond-core/src/types/gen_types.rs")

      (component gen-server
        (desc "MCP JSON-RPC 服务器骨架")
        :target "crates/missiond-mcp/src/gen_server.rs")

      (invocation
        :build-time "Forge CLI 冲压 (外部构建步骤, ~/Projects/jarvis-forge)"
        :runtime    "mission_forge_build MCP tool (daemon 运行时触发)"))

    ;; ── 2.6 Search Engines (搜索引擎) ──
    (section search-engines
      (desc "四路查询 + 融合打分 — 所有需要从记忆检索的计算")
      :migrated-in "memory v0.3 :: module search-engines (2026-04-19) — 搜索是计算不是数据, 归 worker"
      :rationale "搜索引擎是 computation, 被搜索的内容才是 memory"

      ;; 索引维护来源
      (index-sources
        (source migrations-defs
          :desc "HNSW / GIN FTS / trigram 索引在 SQL migration 中定义"
          :code "crates/missiond-core/migrations/*.sql")

        (source embedding-column-writes
          :desc "embedding 列写入 → HNSW 索引增量更新 (pgvector 原生)"
          :writer   "2.3 workers :: embedding-worker (sonnet 组)"
          :provider "2.2 llm-gateways :: xjp-router-gateway (qwen3 路由, code-aligned)"
          :writes   "5 张表 embedding_vec 列 (knowledge / conversation_topic_vectors / message_embeddings / skill_topics / ast_nodes)"
          :governance "契约见 pillar 一 memory :: cross-cutting :: capability embedding-storage-governance (v0.4.6+)"
          :invariant "禁止降级兜底, 失败直接报错")

        (source db-write-auto-index
          :desc "PG 原生机制 — 写入相关表时 GIN FTS / trigram 索引自动更新"
          :mechanism "PostgreSQL GIN / pg_trgm 扩展原生支持"))

      ;; 四路引擎
      (engine vector-hnsw
        :desc   "HNSW 近邻搜索 — 语义相似"
        :impl   "pgvector 扩展"
        :index  "knowledge.embedding_vec / conversations.summary_embedding"
        :dim    512
        :query  "ORDER BY embedding_vec <=> $query_vec LIMIT K")

      (engine fulltext-gin
        :desc   "GIN FTS 全文索引 — 关键词匹配"
        :impl   "PostgreSQL to_tsvector + GIN"
        :index  "knowledge.content / conversations.summary / messages.content"
        :query  "tsvector @@ plainto_tsquery($q)")

      (engine fuzzy-trigram
        :desc   "trigram 模糊字符串匹配 — 拼写容错 / 子串匹配"
        :impl   "pg_trgm 扩展"
        :query  "col ILIKE '%$q%' + similarity(col, $q) > threshold")

      (engine tag-exact
        :desc   "category / tags 精确过滤 — 结构化索引"
        :query  "WHERE category = $cat AND tags @> ARRAY[$t]")

      ;; 融合打分
      (component fusion-ranker
        :desc     "四路结果融合"
        :strategy "向量分 + FTS 分 + trigram 分 + tag 过滤, 加权聚合"
        :scoping  "叠加 pillar 一 memory :: project-management :: scope-mechanism (project_id OR NULL)"
        :code     "daemon/src/handlers/knowledge/kb.rs + context/retrieval.rs")

      ;; 消费者 (谁在用搜索)
      (consumers
        (consumer mcp-kb-search
          :tools "mission_kb_search / mission_kb_query / mission_kb_ops"
          :invoked-by "Claude Code / 前端 / Agent")

        (consumer mcp-insight-recall
          :tools "mission_insight / mission_memory / mission_code_search"
          :focus "综合洞察 / 记忆召回 / 代码语义搜索")

        (consumer context-pipeline-retrieval
          :code    "daemon/src/context/{pipeline,retrieval}.rs"
          :purpose "为 LLM 调用拼 prompt 的语义检索"
          :budget  "estimate_tokens + allocate_budget 多源边际打分"
          :note    "最密集的搜索消费者 — 每次 LLM 调用都触发")

        (consumer mcp-universe-graph
          :tool  "mission_universe_graph"
          :reads "跨项目 KB 索引 → 生成实体/关系图"))))


  ;; ═══════════════════════════════════════════════════
  ;;  三 · 工具 (Tools)
  ;;  MCP 协议 + 对外暴露的全部能力
  ;; ═══════════════════════════════════════════════════
  (pillar tools
    :canonical-ref ".missiond/v2/intent-tools.lisp"
    :canonical-status "v0.7 phase-C 2026-04-26 — 83 actual tools (tool count invariant); mission_directive/plan/workflow actor v0 + plan-runner v0 + auto-selection v1 + methodology compiler v0 + generated flow loader + mission_execution dispatch_strategy companion log + mission_capability_usage semantic evidence v1 + mission_plan record_evidence typed wrap 全部 code-aligned partial; wave 14 task 01/03: write_file + review_gate args 已 code-aligned; wave 15 task 02/04/05: 5 implemented-surface 物理 split + mission_directive/plan 加 review_decision/review_actor/review_note args + mission_plan execute 加 workstation_dispatch args. wave 16 task 01/03/04/05/06 (commits 01708be/8ffa9b2/a51bc52/d8f8a6e/591d288): mission_workflow(action='resolve_review') schema 加 5 字段 (3 surface 全接); mission_plan execute workstation_dispatch 加 5 inference rules + workstation_dispatch_source 5 值; mission_plan execute 加 PLAN node :review-gate 'question-event' / :review-action / :review-text / :retry-count / :max-attempts / :retry-delay-ms hints (paused 7th + retry v0); mission_execution 加 enforce_scoped_commit (opt-in default false 字节兼容) + 4 错误码 + scoped_commit_enforced/scoped_commit_validation response; tool count 仍 83 不变. autonomous workstation spawn (无 hint 全自动) / 完整 11-stage PLAN DAG / semantic lifting / forge compiler / enforce-by-default scoped commit + git 仓库挂钩 仍 pending (详 wave-13 + wave-14 + wave-15 + wave-16 anchors)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-tools.lisp"
    (purpose "通过 MCP JSON-RPC 协议暴露给 Claude Code / 其他 Agent 的能力集")

    (component mcp-server
      (desc "stdio JSON-RPC 服务器,MCP 协议入口")
      :target "crates/missiond-mcp")

    (component dispatch
      (desc "请求 → 域 → handler 的路由分派")
      :target "crates/missiond-daemon/src/infra/mcp_client.rs")

    (component tool-schema
      (desc "所有工具的 JSON Schema 声明(当前 83 个工具,4 大域)")
      :target ".missiond/intent-mcp-defs.lisp"
      :count "67+ tools")

    (domains
      (compute    "slot / task / worker / job / cascade")
      (sysinfra   "permission / config / log / daemon / infra / power")
      (knowledge  "kb / board / cascade / skill / memory / intent")
      (comm       "conversation / pty / question / router_chat / timeline / inbox"))

    (component tool-call-log
      (desc "所有工具调用的执行记录,供审计 / 统计 / 回放")
      :tables "tool_calls"))


  ;; ═══════════════════════════════════════════════════
  ;;  四 · 事件总线 (Event Bus) — Log-as-Bus
  ;;  追加式日志即总线,7 步流水线处理 + tail-and-pull 订阅
  ;; ═══════════════════════════════════════════════════
  ;; 详细规格在独立 lisp(v1.3.4 architecture-unlocked),本处只作导航摘要
  (pillar event-bus
    :file ".missiond/v2/intent-event-bus.lisp"
    :execution-log ".missiond/v2/intent-event-bus-execution.lisp"
    :lock-status "architecture-unlocked v1.3.4 — direct edit allowed; Domain::Execution + CapabilityUsage ObservabilityEvent code-aligned, current domain count 13"
    :paradigm "Log-as-Bus(追加式日志是唯一真理源,不是 broadcast + 补漏)"

    (purpose "进程内神经网络 — 追加式日志 + 类型化 topic 路由 + 游标式订阅")

    (one-line-spec
      "DB seq + 13 domain topic + at-least-once + batch-ack cursor (双阈值) "
      "+ subscription-name PK + pause=drop/live-resume + >8KB side-channel "
      "+ producer-ack-after-commit + no-global-min-replay + tail-and-pull catch-up")

    ;; ── 结构 ──
    (structure
      (section-4.1 ingress
        :desc    "唯一入口 log.append(event, opts)"
        :target  "crates/missiond-core/src/event/log/mod.rs")

      (section-4.2 core
        :desc    "7 步流水线(上到下对应执行顺序,前 4 同步 / 后 3 异步)"
        :target  "crates/missiond-core/src/event/pipeline/"
        (step-1 guard   :at "pipeline/step1_guard/"   :does "因果深度 ≤ 10 + 类型解析")
        (step-2 decide  :at "pipeline/step2_decide/"  :does "claim-check 8KB 阈值 + ephemeral 决策")
        (step-3 commit  :at "pipeline/step3_commit/"  :does "批处理 INSERT + BIGSERIAL seq + dedup")
        (step-4 ack     :at "pipeline/step4_ack/"     :does "oneshot 回 producer")
        (step-5 tail    :at "pipeline/step5_tail/"    :does "Dispatcher 长轮询 event_log")
        (step-6 gate    :at "pipeline/step6_gate/"    :does "control-plane 暂停域过滤")
        (step-7 fanout  :at "pipeline/step7_fanout/"  :does "Topic<T> broadcast 扇出"))

      (section-4.3 egress
        :desc    "tail-and-pull 两阶段 + cursor + 6 个 combinators"
        :target  "crates/missiond-core/src/event/subscription/")

      (section-4.4 cross-cutting
        :desc    "causation-guard + metrics + 9 chaos tests + InMemoryBus"
        :targets
          ("crates/missiond-core/src/event/pipeline/step1_guard/causation.rs"
           "crates/missiond-core/src/event/metrics/"
           "crates/missiond-core/tests/event_chaos.rs"
           "crates/missiond-core/src/event/in_memory/"))

      (section-4.5 deferred
        :desc "FreezeAndCatchUp + Prometheus backend 已声明未实现"))

    ;; ── 关键基础设施位置(快速导航)──
    (key-locations
      (log-schema         :at "crates/missiond-core/migrations/20260419000000_event_log.sql")
      (domain-types       :at "crates/missiond-core/src/event/events/ (13 个 domain enum)")
      (log-trait          :at "crates/missiond-core/src/event/log/mod.rs")
      (log-writer         :at "crates/missiond-core/src/event/pipeline/step3_commit/log_writer.rs")
      (dispatcher         :at "crates/missiond-core/src/event/pipeline/step5_tail/")
      (subscription-api   :at "crates/missiond-core/src/event/subscription/api.rs")
      (daemon-bus-glue    :at "crates/missiond-daemon/src/bus/")
      (ws-bridge          :at "crates/missiond-daemon/src/bus/ws_bridge.rs  — 前端 wire-format 字节级保留")
      (retention-cron     :at "crates/missiond-daemon/src/bus/retention_cron.rs"))

    ;; ── 重构来龙去脉 ──
    (refactor-lineage
      :migrated-from "v1: DaemonEvent god enum + Timeline Writer + event_router 8 consumers + 4 MPSC bypass + sweeper"
      :migrated-to   "v2: 13 domain enum + event_log 单一真理源 + Dispatcher live-only + 14 typed subscribers"
      :branch        "refactor/event-bus-v2 (merged to main commit e139ecf, 2026-04-19)"
      :refactor-commits 16
      :refactor-summary ".missiond/v2/_refactor-summary.md"
      :methodology-template ".missiond/workflows/bus-refactor.lisp")

    :note "worker 集群 / worker-registry / control-tree 在 pillar 二;此 pillar 只管事件基础设施")


  ;; ═══════════════════════════════════════════════════
  ;;  五 · 意图层 (Intent Layer)
  ;;  系统的自我描述 + 自感知 + 自演化
  ;; ═══════════════════════════════════════════════════
  (pillar intent-layer
    :canonical-ref ".missiond/v2/intent-intent-layer.lisp"
    :canonical-status "v0.4 phase-B 2026-04-26 — unified-entry pipeline actor v0 + unified-entry pipeline v0 internal helper + evidence-collector typed EvidenceEntry + plan-dag-scheduler runtime v2 + capability-evolution-governance semantic evidence v1 + workstation-dispatch-policy operational-practice 全部 code-aligned partial; wave 14 task 01-04: file-first writer integration / PlanNodeStateChanged variant + live EventRef 三层策略 / review-gate auto-create policy enum / unified-entry pipeline v1 全部 code-aligned; wave 15 task 02/04/05: 4 sections 物理 split + review-gate explicit resolution bridge v0 + workstation-dispatch v0 opt-in. wave 16 task 01-08 (commits 01708be/331d1c1/8ffa9b2/a51bc52/d8f8a6e/591d288/0e6ee63/a632a91): workflow handler 接入 review-resolution → review-gate-resolution-v0 升 code-aligned (3 surface 全接); review-gate QuestionEvent::Resolved subscriber listener v0 (event bus live subscription 已落 — observation-only consume); workstation-dispatch auto-inference v1 → workstation-dispatch-v0 升 code-aligned (mission_task_delegate scoped node); PLAN DAG paused 7th lifecycle + review-gate question-event trigger; per-node retry policy v0 (cap 3 attempts / cap 60s / SafeDescriptor 不 retry); scoped commit handoff daemon enforce v0 opt-in; evidence subscriber 三档 live/log/unavailable (cap 1024 FIFO cache); unified-entry e2e smoke v0 deterministic 4 hand-off no-LLM. 4 项 v0 non-goal 中 review-resolution / workstation-dispatch / review listener 已分别部分缓解; auto_approve_directive / auto_approve_plan / autonomous_workstation_dispatch 完整版 + 完整 11-stage scheduler (claim-lease / rollback / acceptance / mark-plan-final) + paused-resume listener / 高阶 semantic lifting / forge compiler / planner-class model alias 仍 pending (详 wave-13 + wave-14 + wave-15 + wave-16 anchors)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-intent-layer.lisp"
    (purpose "元层: 系统如何描述自己, 如何感知变化, 如何演进, 以及全局用户指令")

    (component intent-files
      (desc ".missiond/*.lisp 意图声明, 按主题拆分并行加载")
      :granularities "L1-Blueprint / L2-Topology / L3-Implementation"
      :count "27 files (v1) + this v2 draft")

    (component intent-graph
      (desc "文件间 module-link 关系, 构成有向图, 可供可视化 / 治理")
      :target "forge-daemon/src/intent_graph.rs")

    (component forge
      (desc "外部冲压器: Lisp 意图 → IR → Rust 代码 (Generation Gap 隔离)")
      :location "~/Projects/jarvis-forge (独立仓库)"
      :breaks-if "codegen-pattern-change / ir-whitelist-change")

    (component lisp-survey-worker
      (desc "检测 ContextualCommitDetected → 差量更新对应项目的 intent.lisp")
      :target "workers/sonnet/lisp_survey_worker.rs"
      :debounce "60s per project_id"
      :prevents-self-loop "slot_id == lisp-surveyor 的 commit 自动跳过")

    (component governance
      (desc "治理规则 / lint / 模式声明: strict-codegen / descriptive / experimental")
      :target "forge-daemon/src/governance.rs")

    ;; ── 全局 CLAUDE.md · 跨项目永久用户指令 ──
    (component global-claudemd
      (desc "全域总纲 — 指挥官对 Claude 的跨项目永久指令")
      :path "~/.claude/CLAUDE.md"
      :scope global-user
      :format "Markdown + 可选 YAML frontmatter"
      :purpose "全局偏好 / 行为约束 / 宇宙总纲 — 每次会话必加载"
      :loaded-by "Claude Code 系统启动自动加载进 system prompt"
      :writer "用户手动编辑 / Claude Code Edit tool"
      :nature "元层约束 — 非业务记忆 (项目级约束见 memory pillar :: project-management :: helper project-claudemd-manager)"
      :rationale "放 pillar 五 而非 memory pillar: 此文件是"系统如何被指挥"的声明, 属元层")

    (component global-claudemd-manager
      (desc "全局 ~/.claude/CLAUDE.md 的读/写/reload 管理")
      :actions "read / edit / reload"
      :code "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs + crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
      :mcp "mission_global_instruction (read/edit/reload)"
      :readers "Claude Code 每次会话启动"
      :writers "mission_global_instruction(action=edit) / 用户手动 / Claude Code Edit tool (文件层)"
      :status "code-aligned; read/edit full, reload manual-reload-required because Claude Code owns session bootstrap"
      :cross-ref "项目级 <project>/CLAUDE.md 的 manager 在 memory pillar :: project-management :: helper project-claudemd-manager")

    ;; ═══════════════════════════════════════════════════
    ;; Action-Instruction Specs — 动作与指令规约
    ;;   (v0.4.4 从 memory pillar 迁入)
    ;;   区别:memory 只存'项目代码真实状态的 intent.lisp';
    ;;         本 section 管所有'描述动作和指令'的 DB 表 + 文件
    ;; ═══════════════════════════════════════════════════
    (section action-instruction-specs
      (desc "所有描述'应该做什么 / 如何做'的规约 — DB 表 (schema 归 memory directive-layer) + Lisp/YAML 文件")
      :migrated-from "memory pillar :: project-management (4 tables) + non-db-forms (3 variants + 1 form) in v0.4.4"
      :rationale "memory 记'是什么'(facts); 本层记'应该做什么'(prescriptions) — 分层原则"
      :v0.4.19-note "DB 表 schema 实际在 memory :: module directive-layer 管, 本 section 只做概念性 cross-ref; intent 表 → directive 表 rename 同步"

      ;; ── DB 表 (v0.4.17: schema 归 memory directive-layer module, 本 section 只概念性描述) ──
      ;; v0.4.16: user_intents 移回 memory :: conversation-logs (writer=intent_analyst, trait=ConversationStore)
      ;; v0.4.17: intent/plan/workflow 3 张从 pillar 五 action-specs 剥离到 memory :: directive-layer
      ;;          原因: 按 'memory=库' 原则, schema + trait 接口归 memory; pillar 五 actor 是未来 writer
      ;;          撤回 v0.4.16 drop-candidate 误判 — 用户澄清这是 '刚建未启用' 预留 schema
      (component directive-spec-db
        (desc "user utterance → lisp 指令编译记录 — 三段式 pipeline 第一段")
        :table "directive"
        :schema-owned-by "memory :: module directive-layer :: plumbing directive-compilation"
        :cross-ref "intent-memory.lisp :: module directive-layer"
        :status "store+manager code-aligned partial"
        :future-writer "pillar 五 directive-compiler actor; mission_directive 是管理面 (compile dry-run/persist draft)")
        :v0.4.19-rename "原名 intent 表 → directive 表 (避命名歧义: 和 <project>/.missiond/intent.lisp 代码画像文件区分)"
        :vs-per-project-intent "memory :: project-management 里的 <project>/.missiond/intent.lisp 是 factual 代码快照; 本表是 'Jarvis 对用户话的 lisp 指令编译'")

      (component plan-spec-db
        (desc "directive 编译出的执行 DAG — 绑 board_task + 版本 + FSM")
        :schema-owned-by "memory :: module directive-layer :: plumbing plan-execution"
        :cross-ref "intent-memory.lisp :: module directive-layer"
        :status "store+manager code-aligned partial"
        :future-writer "pillar 五 plan-compiler actor — plan 编译 / FSM 迁移 / supersede-chain 策略; mission_plan 当前提供 dry-run/draft + execute bridge")

      (component workflow-spec-db
        (desc "从成功 plan 蒸馏的可复用模板 — 带 match_rules + 统计")
        :schema-owned-by "memory :: module directive-layer :: plumbing workflow-templates"
        :cross-ref "intent-memory.lisp :: module directive-layer"
        :status "store+manager code-aligned partial"
        :future-writer "pillar 五 workflow-distiller actor — distillation 算法 / 匹配阈值 / LRU 策略; mission_workflow 当前提供 match/apply/read-only/distill dry-run")

      ;; v0.4.16: user-intents-db component 已删除 — 该表归属 memory :: conversation-logs
      ;; writer: engine/learning_engine/intent_analyst.rs
      ;; readers: intent_analyst self + autopilot.rs:1496 (get_recent_intents)
      ;; trait: ConversationStore::insert_user_intent + 5 查询方法 (traits.rs:136-150)

      ;; ── Lisp / YAML 文件 (3 类) ──
      (component system-level-intent-files
        (desc "系统主架构 + pillar 级细节规约 Lisp 文件")
        :paths (".missiond/v2/intent.lisp 系统主架构"
                ".missiond/v2/intent-event-bus.lisp architecture-unlocked v1.3.3"
                ".missiond/v2/intent-memory.lisp v0.5.4"
                ".missiond/intent-db-*.lisp Forge 源 lisp"
                ".missiond/intent-pillar-*.lisp v1 分 pillar lisp")
        :purpose "系统自我描述 + Forge 冲压源"
        :moved-from "memory :: non-db-forms :: lisp-spec-files variant system-main/detail (v0.4.4)"
        :note "本层所有 intent*.lisp 都描述'系统应该如何'; 项目 intent.lisp (code snapshot) 不在这里")

      (component workflows
        (desc "统一工作流规约 — 两种 kind 共同表达'多步工作流', 但受众/粒度/执行性不同")
        :unified-in "v0.4.5 (原 workflow-lisp-templates + flow-yaml-templates 合并为单一 component)"
        :design-rationale "两种 kind 形式差异大但概念一致 — 保留各自格式优势, 统一纳管"

        (kind methodology
          (desc "Lisp 方法论模板 — 人类 / agent 参考的 SSOT; 机器执行需先编译成 executable YAML")
          :path ".missiond/workflows/*.lisp"
          :consumers "human + mission_intent tool + agent 参考 + future methodology compiler"
          :granularity "抽象叙事 — phases / principles / anti-patterns / baseline-numbers / decision-authority"
          :examples "bus-refactor.lisp (11-phase 事件总线重构方法论)"
          :executability "human-readable source; machine execution via F-methodology-to-executable-compile → generated YAML → mission_flow_run")

        (kind executable
          (desc "YAML 声明式节点编排 — flow-engine-v2 运行时执行")
          :path "$MISSIOND_HOME/flows/*.yaml"
          :loader "daemon/src/engine/flow/loader.rs"
          :executor "pillar 二 2.4 orchestration :: flow-engine-v2"
          :parser "serde_yaml::from_str::<FlowDefinition>"
          :granularity "具体机器操作 — 5 node types: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
          :executability "✓ 机器执行")

        (relationship-between-kinds
          :overlap "都描述'多步工作流'"
          :split-axis "受众 (human vs machine) + 粒度 (抽象 vs 具体) + 执行性"
          :why-not-unify-format "Lisp 富元数据给人看, YAML 轻量 schema 给 flow-engine 消费; 硬统一两边都难用"
          :cross-ref-convention "可约定同名对照 (如 bus-refactor.lisp ↔ bus-refactor.yaml), 非强制"
          :future-possibility "已升级为 architecture target: F-methodology-to-executable-compile; 当前代码对齐待实现"))

      ;; ── Manager ──
      (component specs-manager
        (desc "action/instruction specs 的读/写/reload — 大部分 TBD")
        :actions "compile / approve / list / get / supersede / match / record-execution / sync-with-file"
        :status "manager/tools code-aligned partial — mission_directive / mission_plan / mission_workflow 提供 read/control/draft persistence/execute bridge; runtime writer actor 未实现"
        :files-status "intent/workflow/flow 文件层已有 readers (mission_intent / flow-engine-v2); writers 多为手动编辑"
        :cross-ref "memory :: project-management :: path project-code-snapshot (读 per-project 代码快照 FILE, 职责不同)"
        :future-work "实现 directive-compiler / plan-compiler / workflow-distiller actor, 并把 dry-run compile/distill 升级为 LLM-backed writer"))


  ;; ═══════════════════════════════════════════════════
  ;;  六 · 系统层 (System Layer)
  ;;  类型 + 传输 + RPC Gateway + 工具 — 运行时底座 (DB / 观测 已迁入 pillar 一)
  ;; ═══════════════════════════════════════════════════
  (pillar system-layer
    :canonical-ref ".missiond/v2/intent-system-layer.lisp"
    :canonical-status "v0.2 phase-B 2026-04-25 (runtime substrate + sysinfra/daemon/control surfaces)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-system-layer.lisp"
    (purpose "无业务语义的运行时底座 — 类型 / 进程 / 传输 / RPC / 工具; DB 与观测已迁入 pillar 一 memory")

    ;; ── 6.1 核心共享类型 ──
    (section core-types
      (desc "跨 crate 共享的枚举 + 结构体 — 单一真理源")
      :target "crates/missiond-core/src/types/gen_types.rs (Forge-generated)"
      :v1-cross-ref "intent-types.lisp"

      (enums "BoardTaskStatus / EngineeringPhase / TaskStatus / EventType / AsyncJobStatus / AgentQuestionStatus / IncidentSeverity / IncidentSource / CliEngine / Lifecycle / SlotTrait")
      (structs "BoardTask / ConversationMessage / KnowledgeEntry / Task / InboxMessage / TaskEvent / AgentQuestion / IncidentRow / DynamicSlot / SkillTopic / ToolCallRecord")

      :serde "derive Serialize/Deserialize + as_str/from_str — DB / IPC / JSON-RPC 共享"
      :authority "枚举定义合法状态迁移 (例: BoardTaskStatus Open→Running→Done,禁跳)")

    ;; ── 6.2 进程与传输 ──
    (section process-transport
      (desc "守护进程生命周期 + IPC/WS 传输 + 全局状态 + 监督")
      :v1-cross-ref "intent-pillar-transport-bootstrap.lisp"

      (component bootstrap
        (desc "main.rs 启动序列 — 6 阶段严格依赖顺序")
        :target "crates/missiond-daemon/src/main.rs"
        (phases 6
          (p1 "DB pool + embed_model + event_bus")
          (p2 "ProjectRegistry 从 PG 加载")
          (p3 "PTYManager + SlotManager + MissionControl")
          (p4 "LLM gateways: sonnet / gemini / codex / (minimax 可选)")
          (p5 "Context pipeline + WorkerRegistry + ControlTree")
          (p6 "21 workers spawn + autopilot + ipc-handler + ws-server"))
        :invariant "每阶段依赖前一阶段;ProjectRegistry 必须早于 message_handler;event_bus 必须早于任何 handler(防事件丢失)")

      (component app-state
        (desc "全局共享状态 — Arc<RwLock<...>> 贯穿所有 handler")
        :target "crates/missiond-daemon/src/state.rs"
        :fields "db pool / event_bus / slot_manager / llm_gateway / context_pipeline / project_registry / 4 MPSC senders"
        :invariant "只读访问 (RwLock 只 read),启动后不再 write — 状态权威在 DB + event_bus")

      (component ws-server
        (desc "WebSocket 服务器 — 前端订阅端")
        :target "crates/missiond-core/src/ws/server.rs"
        :sub-components ("screenshot-broker — 异步截屏流分发"
                         "jarvis-trace — trace span 分发给客户端"))

      (component ipc
        (desc "mcp ↔ daemon 双向通信 (Unix socket / TCP)")
        :target "crates/missiond-core/src/ipc/mod.rs")

      (component supervisor
        (desc "worker 健康监控 + 重启")
        :target "crates/missiond-daemon/src/supervisor.rs"))

    ;; ── 6.3 RPC Gateway ──
    (section rpc-gateway
      (desc "JSON-RPC 服务器 — stdio(MCP 协议)+ IPC(daemon)双传输")
      :target "crates/missiond-mcp/src/gen_server.rs (Forge-generated)"
      :v1-cross-ref "intent-rpc-gateway.lisp"

      (methods "initialize / notifications/initialized / tools/list / tools/call / ping")

      (dispatch
        :rule "数据驱动: tool_name → handler 映射,非硬编码 match"
        :scope "83 tools × 4 domains / 8 legacy groups (schema 归 pillar 三)")

      (error-codes "UNKNOWN_TOOL / UNKNOWN_ACTION / MISSING_PARAM / INVALID_PARAM / NOT_FOUND / PERMISSION_DENIED / IPC_TIMEOUT / SPAWN_FAILED / DB_ERROR")

      :role "纯 plumbing — pillar 三 持有 tool schema,这里只负责路由 + 错误码")

    ;; ── 6.4 纯工具模块 ──
    (section pure-utils
      (desc "无 I/O 无状态的确定性工具函数 — 横向复用")
      :v1-cross-ref "intent-pure-utility.lisp"

      (component semantic-parsing-helpers
        :target "crates/missiond-core/src/semantic/gen_parsing.rs"
        :functions "is_spinner_char / split_args / extract_phase_from_parens / sanitize_line / has_activity_timer / is_idle_prompt"
        :consumer "extractor pipeline (pillar 二 2.1 PTY)")

      (component string-safety
        :target "crates/missiond-core/src/util/gen_string_helpers.rs"
        :functions "safe_byte_truncate / safe_char_truncate"
        :desc "UTF-8 边界安全截断,CJK 多字节字符不会断开"
        :rationale "替换代码库里所有 &s[..N] 危险切片")

      (component token-budget
        :target "crates/missiond-daemon/src/context/gen_budget.rs"
        :functions "estimate_tokens (英文 /4, 中文 /2) / allocate_budget (N 源 + 边际递减)"
        :consumer "context 窗口规划")))


  ;; ═══════════════════════════════════════════════════
  ;;  七 · 流程 (Flow)
  ;;  跨 pillar 的动作前后流程 — 把 memory 静态与 worker 计算串联成 narrative
  ;; ═══════════════════════════════════════════════════
  (pillar flow
    :canonical-ref ".missiond/v2/intent-flow.lisp"
    :canonical-status "v0.7 phase-C 2026-04-27 — 83 actual tools indexed + F-intent-alignment-plan-execution-loop 8 stages 统一入口 + 双 review gate + plan-runner v0 + F-execution-log-governance + F-scoped-commit-handoff + F-methodology / F-capability-usage / F-workstation-dispatch + PLAN DAG runtime v2 + unified-entry pipeline v0 internal helper 全部 code-aligned partial; wave 14-18 持续闭环 (file-first writer + PlanNodeStateChanged + review-gate v1 + workstation-dispatch v0 + auto-inference + scoped commit enforce v0 + claim-lease + paused-resume + acceptance + rollback + finalize/distill + cross-plan distill + autonomous PLAN field inference + review automation policy + scoped-commit worktree preflight + ExecutionEvent dispatch metadata v1). wave 19 task 02-10: machine-contract task SSOT 全闭环. wave 20 task 01-09 全闭环: task-scope-index-guard v1 + renderer scoped-commit guard v2 + execution preflight contract scope v1 + machine-driven dispatch v0 (Lisp 真成 dispatch SSOT) + unified-entry machine-loop smoke v2 + cross-plan distill auto-trigger v1 + LLM-augmented plan inference v0 sonnet_suggest + review auto-answer policy v0 + ExecutionEvent legacy metadata sweep v0 (11 variants 闭环). **wave 21 task 01-08 全闭环 propose+apply-gate 范式 (commits 44c74df/1335fa7/308426e/68b84f1/a18200b/e140773/4d494db/8ba8723)**: hooks-path installer v1 (opt-in repo-local only — 不擅自 default-on) + task-run verifier v1 (三合一 + 14 forbidden git verb proof + dogfood self-verify) + execution report-verifier integration v1 (mission_execution(complete) 加 4 新字段 + verified=true daemon-internal sexp cross-check + 4 structured error codes + daemon_never_invokes_mutating_git unit-pinned) + autonomous workstation LLM proposal v0 (workstation_inference_mode opt-in + 4 propose 字段 × 3 confidence × 4 safety + propose only never auto-spawn — applied=false / auto_spawn=false 永钉死 + DAG mode refuse) + plan inference apply gate v1 (apply_inferred_fields opt-in + 6 道严格 gate + 8 skip reason canonical + persisted plan.sexp_text 永不 mutate persist_inference_applied=false 永钉死) + LLM auto-approve proposal v0 (auto_approve_mode opt-in for directive/plan + 5 invariants I1-I5 + 22 unit tests + 10 dispatch branch sites + 3 review knob 共存 ORTHOGONAL) + sonnet distill chain auto-apply v1 (auto_sonnet 双 opt-in + 7 重 gate + 8 status taxonomy + 7 invariants I1-I7) + machine-contract autonomous loop smoke v3 (15 cross-wave invariant tests + Markdown non-load-bearing 二度钉死 + machine dispatch SSOT 钉死). **tool count 仍 83 不变**. **propose-only 与 explicit apply-gate 范式覆盖 4 路通道, persisted state v0 永不被自动 mutate**. 完整 LLM auto-apply 真正落 / autonomous workstation true spawn (wave21-04 propose only) / persisted plan inference apply (wave21-05 persist_inference_applied=false 永钉死) / git config core.hooksPath .githooks 默认 default-on (wave21-01 仍 opt-in repo-local only) / sonnet 完全自动接 chain 不需 dual opt-in (wave21-07 仍 dual opt-in required) / frontend Lisp 仍 pending (详 wave-13..21 anchors via intent-pillar-source-index.lisp)"
    :gptpro-v0.1-archive ".missiond/v2/drafts/gptpro/intent-flow.lisp"
    (purpose "跨 pillar 的动作前后流程 — 把 memory 静态与 worker 计算串联成 narrative")
    (rationale "v0.4.7 从 board 拆出 autopilot/flow-engine 后, 丢失了 end-to-end narrative; 本 pillar 补上")

    (principle
      :memory "状态 (snapshot) — 记什么是什么"
      :worker "机制 (engine) — 做怎么做"
      :flow   "编排 (choreography) — 串什么时候什么顺序做什么")

    (naming-convention
      :stage-id "s1 / s2 / ..."
      :at-target "pillar-X :: module/section :: component (跨 pillar 跳点)"
      :writes    "产生什么数据变动"
      :emits     "产生什么 DomainEvent (可选)")

    (flows-catalog
      :scope "当前 board-centric; 可扩展到 KB mutation / conversation ingestion / retro / context assembly"
      :count 5

      ;; ── Flow 1: 任务主生命周期 ──
      (flow board-task-main-lifecycle
        (desc "任务从创建到完成 — board 最核心的 end-to-end")
        (trigger "mission_board_create / decomposed child / autopilot scan auto_execute=1")

        (stages
          (s1 create
            :at     "pillar 一 memory :: board :: mcp-board-lifecycle"
            :writes "board_tasks status=open"
            :emits  "BoardTaskCreated")

          (s2 scan-decide
            :at      "pillar 二 2.4 :: autopilot"
            :reads   "board_tasks WHERE auto_execute=1 AND status=open"
            :decides "是否 claim + 派给哪个 slot / worker")

          (s3 atomic-claim
            :at     "pillar 一 memory :: board :: state-machine"
            :writes "status=running + claim_executor_id + lease_expires_at"
            :atomicity "SQL CAS — open→running 原子操作"
            :emits  "BoardTaskClaimed + BoardTaskStatusChanged")

          (s4 execute
            :at     "pillar 二 2.1 PTY slot / 2.3 workers / 2.4 flow-engine-v2"
            :action "实际执行任务; 有 flow_template 则走 flow-engine 逐节点"
            :side-effects "autopilot.save_prompt_snapshot → prompt_snapshots"
            :flow-ref "flow-engine-v2-node-execution (若走节点模式)")

          (s5 report-completion
            :at     "pillar 一 memory :: board :: core-operations"
            :writes "status=done/failed + claim_executor_id 清除 + lease 释放"
            :emits  "BoardTaskStatusChanged")

          (s6 downstream-cascade
            :at     "pillar 二 2.4 :: autopilot"
            :action "检查 depends_on 的下游 → unblock 或 retry-cascade"
            :optional true))

        (alternative-path lease-recovery
          :trigger   "autopilot tick 发现 lease_expires_at < now() 且 status=running"
          :at        "pillar 二 2.4 :: autopilot"
          :action    "调 BoardStore::recover_stale_running_tasks"
          :writes    "status=open + claim 清除"
          :rationale "executor 崩溃不留僵尸任务"))

      ;; ── Flow 2: 任务拆解 ──
      (flow board-task-decompose
        (desc "父任务 AI 分析 → 子任务 DAG")
        (trigger "mission_board_decompose(task_id, slot_id, hints)")
        (stages
          (s1 request
            :at     "pillar 一 memory :: board :: mcp-board-lifecycle"
            :action "派 slot 做分析")
          (s2 analyze
            :at     "pillar 二 2.1 :: PTY slot"
            :action "slot LLM 执行 AI 分析, 产出结构化 subtask plan")
          (s3 write-dag
            :at     "pillar 一 memory :: board :: core-operations"
            :writes "新 board_tasks rows (parent_id + depends_on JSONB)"
            :emits  "BoardTaskCreated (每个子任务一次)"))
        (result "Parent task + DAG of children with dependency links"))

      ;; ── Flow 3: Agent 提问阻塞 (已实现 auto-unblock) ──
      (flow agent-question-block-resume
        (desc "Agent 卡住 → 提问 → task 被 block → 回答后 auto-unblock")
        (trigger "mission_question create with task_id")
        (stages
          (s1 question-create
            :at     "pillar 一 memory :: board :: helper agent-questions"
            :writes "agent_questions status=pending"
            :side-effect "CAS UPDATE board_tasks SET status=blocked WHERE id=task_id"
            :serves "flow 暂停 — executor 不再 claim 此任务")

          (s2 human-answer
            :at     "用户手动 / 其他 agent / Claude Code 交互"
            :writes "agent_questions status=answered + answer text"
            :code   "db/question.rs :: answer_agent_question()")

          (s3 auto-unblock
            :at     "pillar 一 memory :: board :: answer_agent_question (同事务)"
            :trigger "answer_agent_question() 检查 task 所有 pending 问题是否全部 answered/dismissed"
            :writes "board_tasks status=blocked→open (仅当最后一个问题解决时)"
            :emits  "QuestionEvent::Resolved 到 event-bus"
            :code   "db/question.rs:156-170"))
        (status "✓ auto-unblock 已实现 — 之前标的 gap 是错的, v0.4.12 修正"))

      ;; ── Flow 4: Autopilot tick 流水线 ──
      (flow autopilot-tick-pipeline
        (desc "autopilot 每 5-10s 的完整 tick — 多个子流程依次跑")
        (trigger "autopilot 计时器 (5-10s)")
        (stages
          (s1 memory-scheduler
            :at "pillar 二 2.4 :: autopilot"
            :action "扫待唤醒的 reminder / 提醒 (若有)")
          (s2 extraction-check
            :at "pillar 二 2.4 :: autopilot"
            :action "检查 extract-worker 状态 + 进度")
          (s3 board-task-dispatch
            :at "pillar 二 2.4 :: autopilot"
            :flow-ref "board-task-main-lifecycle s2-s4 (scan + claim + 派发)")
          (s4 flow-progression
            :at "pillar 二 2.4 :: flow-engine-v2"
            :action "推进所有 flow_template 非空的 running task 一个节点")
          (s5 supervision-check
            :at "pillar 二 2.4 :: autopilot"
            :action "lease recovery (见 Flow 1 alternative-path) + 僵尸 slot 检测")))

      ;; ── Flow 5: Flow-engine 节点执行 ──
      (flow flow-engine-v2-node-execution
        (desc "flow_template YAML 节点的运行时执行 — board 的可选子流")
        (trigger "board_task.status=running 且 flow_template 非空")
        (stages
          (s1 load-yaml
            :at    "pillar 二 2.4 :: flow-engine-v2 :: loader"
            :reads "$MISSIOND_HOME/flows/<flow_template>.yaml"
            :parses-to "FlowDefinition (serde_yaml)")
          (s2 execute-node
            :at    "pillar 二 2.4 :: flow-engine-v2 :: runner"
            :types "LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
            :action "按节点类型分派, 变量插值 + 执行")
          (s3 persist-context
            :at     "pillar 一 memory :: board :: data-model"
            :writes "board_tasks.flow_context (JSONB) — 节点产出 + 状态"
            :invariant "每节点完成必须 persist — 崩溃恢复基础")
          (s4 advance-or-complete
            :at "pillar 二 2.4 :: flow-engine-v2 :: runner"
            :decides "flow_phase++ / 分支 / 全部完成则 report (→ Flow 1 s5)")))

      ;; ── 未覆盖的候选 (待扩展) ──
      (future-flows
        (kb-mutation-to-indexed      "mission_kb_mutate → knowledge 写 → embedding-worker → HNSW 索引")
        (conversation-jsonl-ingest   "PTY JSONL → conversation-logger → DB → briefing → embedding")
        (retrospective-trigger       "会话结束信号 → retro-worker → retrospective_results")
        (context-assembly            "LLM 调用前 → ContextPipeline → KB + conversations 拼 prompt")
        (project-init                "mission_project init → projects row + intent_path 解析 + 初始 lisp-survey"))))

) ;; end intent missiond-v2
