;; ═════════════════════════════════════════════════════════════
;; MissionD — Intent-Layer Pillar (phase-B recursive-contract v0.4)
;; 目标: 元层 — 系统如何描述自己 / 被指挥 / 演化 / 认知推理
;; 底稿: gptpro intent-intent-layer.lisp (159 行 starter) + v2/intent.lisp 占位
;;       + worker v0.4 标的 intent-layer ownership 迁移项
;; 大原则: prescription / 认知 / 元层 归本 pillar; facts / runtime 归 memory + worker
;; ═════════════════════════════════════════════════════════════

(pillar intent-layer
  :version "v0.4"
  :status "phase-B recursive architecture contract 2026-04-27 — stimulus/directive → reasoning → prescription/output; unified-entry pipeline (message → alignment → plan → execution → workflow) 持续演进: directive-compiler v0 / plan-compiler v0 / plan-runner v0 + auto-selection v1 / workflow-distiller v0 / methodology compiler v0 / generated flow loader / unified-entry pipeline v0 internal helper / evidence-collector typed EvidenceEntry / capability-evolution-governance semantic evidence v1 / workstation-dispatch-policy operational-practice / plan-dag-scheduler runtime v2 全部 code-aligned partial; wave 14-18 持续闭环 (file-first writer + PlanNodeStateChanged + live ref + review-gate v1 + workstation-dispatch v0 + auto-inference + paused 7th + retry v0 + scoped commit enforce v0 + evidence subscriber 三档 + e2e smoke + paused-resume hook + claim-lease v0 + acceptance evaluator + rollback policy + finalize/distill + cross-node acceptance fan-in + cascade rollback + cross-plan distill chain + autonomous PLAN field inference + review automation policy + scoped-commit worktree preflight + ExecutionEvent dispatch metadata v1). wave 19 task 02-10: machine-contract task SSOT 全闭环. wave 20 task 01-09 全闭环: task-scope-index-guard v1 + renderer scoped-commit guard v2 + execution preflight contract scope v1 + machine-driven dispatch v0 (Lisp 真成 dispatch SSOT) + unified-entry machine-loop smoke v2 + cross-plan distill auto-trigger v1 + LLM-augmented plan inference v0 sonnet_suggest + review auto-answer policy v0 + ExecutionEvent legacy metadata sweep v0 (11 variants 闭环). wave 21 task 01-08 全闭环 propose+apply-gate: hooks-path installer v1 (opt-in repo-local only) + task-run verifier v1 + execution report-verifier integration v1 + autonomous workstation LLM proposal v0 (propose only) + plan inference apply gate v1 (persisted plan.sexp_text 永不 mutate) + LLM auto-approve proposal v0 (propose only) + sonnet distill chain auto-apply v1 (双 opt-in) + machine-contract autonomous loop smoke v3 (15 cross-wave invariants pinned). **wave 22 task 01-07 全闭环 explicit-gate-promotion + auto-verifier + smoke v4 (commits 49555c4/02ac627/4b55cb4/fee6567/162a303/2423d4b/6b2125c)**: hooks default-on doctor v2 (default mode = --check 只读 doctor / --install 唯一 mutation / 4 reason codes / renderer renderHooksDoctorPreflight() 块在 commit section 上方; **doctor default-on ≠ real install default-on**) + execution auto-run-verifier v2 (daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process / verification_source='daemon-auto-verifier' 主路径 / 3 新错误码 / legacy verified=true degrade 'legacy-caller-claim' / 8 new tests / 绝不 spawn Node-shell-mutating-git) + review LLM approve apply gate v1 (apply_llm_auto_approve + proposal_hash deterministic SHA-256 + caller_approved 3 strict opt-in + **6 道严格 gate** G1-G6 + 3 新错误码 / 只 legacy quiet `action=approve` 路径触发 DB transition / wave-21/06 5 invariants 全 preserved 5 dedicated tests) + persisted plan inference apply v2 (persist_inference + caller_approved + proposal_hash 4 opt-in + plan_insert(version=max+1) + plan_supersede(old_id) + 2 新错误码 / **真改 plan.sexp_text rollback handle through predecessor** / wave-21/05 6 invariants 全 preserved 7 dedicated tests / **I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死 v2 用 SEPARATE `persisted_apply` block surface**) + autonomous workstation true spawn v1 (auto_spawn + workstation_caller_approved + preflight_acceptable 4 opt-in + **12-rule gate matrix** G1-G12 + 3 新错误码 + 15 status taxonomy / 走 mission_task_delegate substrate **绝不 claude -p** / wave-21/04 4 invariants 全 preserved 4 dedicated tests) + distill chain policy auto-sonnet v2 (auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} / **dual opt-in 移除** policy 选择即 attestation / legacy 双 opt-in 仍 back-compat additive coexists / wave-21/07 7 invariants 全 preserved 7 dedicated tests I7 验证 4 块 coexistence) + autonomous loop apply smoke v4 (9 new deterministic smoke tests across 5 in-scope files / **22 cross-wave invariants pinned** wave21-04 4 + wave21-05 6 + wave21-06 5 + wave21-07 7 / no real LLM/spawn/mutating git). **brief → preflight → staged guard → commit → verifier (post-commit) → execution complete verify (daemon-internal **auto_run_task_run_verifier** 4 路径全提供主路径 'daemon-auto-verifier') 六段闭环 wave-22 强化**. **explicit-apply-gate 范式覆盖 4 路通道**: review LLM approve / persisted plan inference (v2 真改 plan.sexp_text + plan_supersede rollback) / autonomous workstation true spawn (走 mission_task_delegate) / sonnet distill chain (policy-driven 单一 attestation). 4 项 v0 non-goal 中 review-resolution / workstation-dispatch / review listener / review automation / review auto-answer / LLM auto-approve apply (wave22-03 已落) / sonnet distill chain (wave22-06 policy-driven 已落) / autonomous workstation true spawn (wave22-05 已落) / persisted plan inference apply (wave22-04 真改 plan.sexp_text 已落) 已分别部分缓解; 完整 LLM 自主无任何 caller opt-in / Sonnet 真无任何 attestation / git hooks default-on real install (wave22-01 仍 doctor only) / 完全 autonomous workstation 无 PLAN hint 无 task_contract_path (wave22-05 仍要求 task_contract_path + 12-rule gate) / 高阶 semantic lifting / forge compiler 仍 pending (详 wave-13..22 anchors via intent-pillar-source-index.lisp)"
  :predecessor "drafts/gptpro/intent-intent-layer.lisp (159 行 starter)"
  :target-path ".missiond/v2/intent-intent-layer.lisp"
  :wave-23-status-summary
    ["machine-contract 新增 session-trace v1: schema/checker/analyzer + daemon append + plan/workstation trace propagation"
     "report-contract 新增五个解释字段, 但事实仍以 session-trace.lisp 为准"
     "trace-derived router policy 仅是 architecture-designed; runtime LLM router / ClaudeCode replacement 不在 wave 23 code-aligned 范围"]
  :wave-24-status-summary
    ["machine-contract 新增 router-policy v1 / trace corpus index / router recommendation CLI 三件套, 把 Wave23 router 草案推进到 code-aligned dry-run advisory"
     "mission_plan execute 现在可用 router_policy_mode=dry_run 暴露 recommendation block, 但 applied=false 永远为字面量 false"
     "router_policy_mode=apply / auto / unknown 在 plan lookup 前 INVALID_PARAM, 防止 caller 误以为已可 runtime 替换"
     "frontend Lisp 仍 postpone; 本波只补 machine-contract / plan surface / renderer advisory"]
  :wave-25-status-summary
    ["machine-contract 新增 router-policy corpus evaluator / report fields / measurement smoke, 让 router dry-run 从 advisory 进入可统计可验收状态"
     "mission_plan execute 新增 router_policy_trace_index_path, 只在 dry_run mode 读取; used/missing/unreadable/malformed 都是 non-fatal evidence surface"
     "renderer 输出 recommend-task-backend 命令与 MAY report-field 指引, 但 Markdown 仍非 SSOT 且不切换 backend"
     "frontend Lisp 仍 postpone; 本波只补 router measurement / report / plan surface / renderer advisory"]
  :wave-26-status-summary
    ["machine-contract 新增 router-backend-registry v1 + readiness annotations: 5 backend enum, readiness current-default/advisory-only/runtime-ready/unavailable, unknown sentinel, apply blockers"
     "mission_plan execute 新增 router_backend_registry_path, 只在 router_policy_mode=dry_run 读取; off/default 即使带 registry+trace-index 路径也不 I/O"
     "report-contract / renderer 增 readiness 字段与 check-router-backend-registry / --backend-registry 上下文, 但 Markdown 仍非 SSOT 且明确 MUST NOT switch backend"
     "frontend Lisp 与 runtime backend replacement 仍 postpone; 本波只补 router readiness / apply-blocker evidence loop"]
  :wave-27-status-summary
    ["machine-contract 新增 router-dispatch-descriptor v1: descriptor schema/checker + build CLI + report fields + renderer context"
     "mission_plan execute 新增 router_dispatch_descriptor bool, 只在 router_policy_mode=dry_run 下返回 no-execution handoff descriptor"
     "descriptor 是 ephemeral 生成物, 不新增静态 :router-dispatch-descriptor-path task 字段, 避免 stale-by-design"
     "frontend Lisp 与 runtime backend replacement 仍 postpone; 本波只补 router dispatch descriptor handoff loop"]
  :wave-28-status-summary
    ["machine-contract 新增 task-runner manifest v1: productive_only、dispatch_group、verification_tier、estimated_minutes、heartbeat_minutes、write_scope overlap policy 成为机器可检字段"
     "renderer/brief 进入 thin+preamble 模式: task Lisp/manifest 仍是 SSOT, Markdown 只保留任务特异信息 + shared preamble 指针"
     "mission_plan task_runner_mode=dry_run 把 manifest planner 暴露给现有 surface, 但不执行、不 spawn、不切 backend"
     "下一步效率主线: context-atlas / pattern-card / commit-lineage / verification receipt / ready-queue 调度; frontend Lisp 仍 postpone"]
  :wave-29-status-summary
    ["efficiency 主线已实际落地: context atlas / pattern cards / prepare-task-runner-wave / verification receipts / ready-queue planner / 8-layer smoke 全部 code-aligned"
     "GPT Pro 结论被收敛成一个 intent-layer 要求: MissionD 不能靠 worker 自己猜最终状态, 必须由 orchestrator 维护任务生命周期真相并投影 finalized report"
     "parent 后行 hotfix 是 Wave29 暴露的核心机制缺口: sub-agent 退场后不知道 final commit, 所以 actor 责任必须上收为 lifecycle protocol"
     "Wave30 主线不是继续做旁路优化, 而是让 intent-alignment.lisp → PLAN.lisp → workflow.lisp 的任务执行链拥有 event log、finalizer、receipt、source-hygiene 这些自举执行所需的硬边界"]

  :actual-state-sources
    [".missiond/v2/intent.lisp :: pillar intent-layer (v0.4.17+ 已有详细占位)"
     ".missiond/v2/intent-worker.lisp v0.5 (DC014 决策: 7 sub learning-engine + flow-engine v1 迁入本 pillar; project-root spawn cwd)"
     ".missiond/v2/intent-memory.lisp v0.5.4 (directive-layer artifacts + execution protocol + capability usage read-model)"
     ".missiond/v2/intent-flow.lisp v0.7 (methodology compile + execution governance + capability usage monitoring + project-root spawn cwd)"
     ".missiond/intent-pillar-engines.lisp :: learning-engine + flow-engine v1 (ground truth)"
     ".missiond/intent-pillar-state-machines.lisp :: engineering-phase + extraction-phase FSM"
     ".missiond/intent-pillar-event-workers.lisp :: lisp-survey-worker flow"
     ".missiond/intent-mcp-defs.lisp :: mission_intent + mission_forge_* schema"
     ".missiond/workflows/*.lisp (methodology 库 — pillar-refactor.lisp 等)"
     "$MISSIOND_HOME/flows/*.yaml (executable flow library)"]

  :design-correction-sources
    ["drafts/gptpro/intent-intent-layer.lisp (继承 5 component / 4 egress 大框架)"
     "worker v0.4 DC014 + dual-ownership 标注 (lisp_survey / arch_maintenance)"
     "intent-memory.lisp v0.5.4 :: module directive-layer (schema/file artifacts owned by memory, writer 归本 pillar)"]

  :historical-footprint-sources
    ["learning-engine 从 worker engine-cluster 迁入 (v0.3 拆分, worker 留 BackgroundWorker 触发) / flow-engine v1 vs v2 (v1=project lifecycle phases 本 pillar; v2=YAML declarative worker pillar) / intent 表 rename 为 directive 表 (v0.4.19, schema 归 memory)"]

  ;; ══════════════════════════════════════════════════════════
  ;; phase-A-decisions — 边界与迁移
  ;; ══════════════════════════════════════════════════════════
  (phase-A-decisions
    (Q-IL1
      :question "本 pillar 与 memory pillar 的边界?"
      :decision "memory = 库 (facts + schema + trait); intent-layer = prescription + 认知推理 + 元描述"
      :examples
        ["memory: user_intents 表 schema + ConversationStore trait"
         "intent-layer: intent_analyst 分析逻辑 (writer)"
         "memory: directive/plan/workflow 3 表 schema"
         "intent-layer: directive-plan-workflow 编译 pipeline 逻辑 (writer TBD)"])

    (Q-IL2
      :question "learning-engine 7 sub 是否全搬过来?"
      :decision "accept — 全搬. 在本 pillar 组织为 3 subsection (decision / extraction / analysis). 触发机制仍在 worker pillar"
      :related-worker-v0.3 "DC014 boundary-shift"
      :rationale "7 sub 都是'感知+推理+模式归纳'语义, 非 timer/dispatch mechanics")

    (Q-IL3
      :question "flow-engine v1 (flow_engine.rs) 归属?"
      :decision "accept — 搬本 pillar. v1 是 project lifecycle phases (Investigate → Plan → Execute 认知推进), 与 methodology workflows 同属元层"
      :distinguish "flow-engine v2 (YAML declarative runtime) 仍归 worker pillar"
      :state-machine-crossref "engineering-phase FSM 归本 pillar")

    (Q-IL4
      :question "双重归属 worker 如何标?"
      :decision "worker pillar = 触发 (event subscribe + BackgroundWorker impl); 本 pillar = 语义 ownership + 执行 prompt 内容"
      :dual-owned ["lisp_survey_worker (worker v0.4 已标)"
                   "arch_maintenance_worker (worker v0.4 已标)"]
      :cross-ref "每 dual-owned worker 在本 pillar 有独立 section, 详述 prompt/ slot / 产物 ownership; worker pillar 只描述触发点")

    (Q-IL5
      :question "directive-layer 3 表 (directive/plan/workflow) 当前 writer 是谁?"
      :decision "store+manager code-aligned partial — memory 已有 DirectiveLayerStore trait + Pg impl; mission_directive/plan/workflow 管理面已实现; LLM writer actor 仍待实现"
      :status "下一步: 本 pillar 新建 directive-compiler / plan-compiler / workflow-distiller 三 actor; MCP 工具继续作为管理面")

    (Q-IL6
      :question "global CLAUDE.md manager 当前 vs 未来?"
      :decision "当前: mission_global_instruction 提供 daemon 侧 read/edit 管理; Claude Code 仍只在会话启动读 ~/.claude/CLAUDE.md"
      :status "code-aligned; reload 只能返回 manual-reload-required, 不伪造热重载")

    (Q-IL7
      :question "workflows methodology lisp 与 executable YAML 的关系?"
      :decision "两种 kind 保留分离: methodology Lisp 是人类/agent SSOT; 机器执行走 F-methodology-to-executable-compile 生成 YAML 副本, 再由 mission_flow_run 跑"
      :cross-ref "memory pillar :: project-management 曾有 workflow-lisp-templates + flow-yaml-templates 两 component, v0.4.5 合并为单 workflows component 分 2 kind")

    (Q-IL8
      :question "intent-layer 是否调用 tools / worker / memory?"
      :decision "本 pillar 是 prescription source, 主向下派发 (→ tools 通过 MCP, → worker 通过 BackgroundWorker 触发, → memory 通过 trait writer)"
      :anti-pattern "本 pillar 不应被 worker / memory 调用 (反向依赖)"
      :exception "worker::lisp_survey_worker 触发 本 pillar::lisp-survey-update 是双向 — 但只是 trigger, 不是数据反流"))

  (purpose "元层 — 系统自我描述 + 指挥规约 + 认知推理 + 代码演化的唯一 ownership 源")

  (recursive-architecture-contract
    :shape "pillar = ingress → logic-core → egress; cognitive-function = stimulus/directive ingress → reasoning steps → prescription/output egress"
    :unit "directive/plan/workflow/action 是意识层原子链; learning/decision/extraction 是认知分子"
    :rule-1 "intent-layer 只拥有 prescription / reasoning / self-description, 不拥有 durable schema 或 runtime mechanics"
    :rule-2 "每个功能必须写清楚从什么刺激进入、经过哪些推理步骤、产出什么指令/文件/事件"
    :rule-3 "worker 只触发本 pillar 的认知函数; 认知函数的语义 ownership 始终在本 pillar"
    :rule-4 "凡是会派发执行的功能, egress 必须指向 tools/flow/worker 边界, 禁止暗调")

  (pillar-ingress
    (entry-1
      :source "人类 / 指挥官 编写或修改的 lisp / markdown / yaml 规约文件"
      :examples [".missiond/v2/*.lisp"
                 ".missiond/workflows/*.lisp (methodology)"
                 "$MISSIOND_HOME/flows/*.yaml (executable)"
                 "~/.claude/CLAUDE.md (global)"
                 "<project>/CLAUDE.md (project)"
                 "<project>/.missiond/intent.lisp (code snapshot)"])

    (entry-2
      :source "ContextualCommitDetected 事件 (worker 侧触发, 本 pillar 消费)"
      :consumers ["lisp-survey 更新 project intent.lisp"
                  "arch-maintenance 更新 architecture manifest"])

    (entry-3
      :source "MCP 工具调用"
      :tools ["mission_intent (read/section/summary/list)"
              "mission_forge_build / mission_forge_lint"
              "mission_project (init/context)"
              "future: mission_directive / mission_plan / mission_workflow / mission_execution"])

    (entry-4
      :source "认知推理 sub-engines 的自发触发"
      :mechanisms ["extraction pipeline on new conversation"
                   "decision cascade on agent question"
                   "idle-explore on system idle"
                   "historical-scan on backlog catch-up"])

    (entry-5
      :source "指挥官通过 Claude Code 会话启动自动加载 ~/.claude/CLAUDE.md"
      :mechanism "Claude Code 系统启动读取全局 CLAUDE.md 作 system prompt — 当前绕过 daemon"))

  (pillar-core
    :contract "intent-layer 把刺激/指令/代码变化转成系统 prescription, 再通过 tools/flow/worker/memory 边界落地"

    (function self-description-governance
      (ingress
        :sources [".missiond/v2/*.lisp" "<project>/.missiond/intent.lisp" "CLAUDE.md" "architecture manifests"])
      (logic-core
        (step s1 "读取或接收自我描述文件变化")
        (step s2 "按 pillar 边界判断 ownership 与 drift")
        (step s3 "生成 lint/repair/survey/arch-maintenance prescription"))
      (egress
        :file-writes ["<project>/.missiond/intent.lisp" "managed CLAUDE.md sections" "arch manifest"]
        :to-tools ["mission_intent" "mission_forge_lint" "mission_forge_build"]))

    (function directive-plan-workflow-chain
      (ingress
        :sources ["user/directive utterance" "mission_directive / mission_plan / mission_workflow manager surfaces"])
      (logic-core
        (step s1 "directive: 解析用户意图与约束")
        (step s2 "plan: 分解目标、风险、依赖、验收标准")
        (step s3 "workflow: 选择 methodology/executable flow 或生成 action instructions")
        (step s4 "dispatch: 把 prescription 交给 tools/flow/worker 边界"))
      (egress
        :writes "memory :: directive-layer tables"
        :to-flow "named flow or methodology workflow"
        :to-tools "MCP tool call plan"))

    (function learning-and-decision-engine
      (ingress
        :sources ["new conversation/session" "agent question" "idle time" "historical backlog" "retrospective"])
      (logic-core
        (step s1 "extraction: 从会话/事件提取事实、主题、风险")
        (step s2 "decision: 对问题/不确定性做级联判断")
        (step s3 "analysis: 形成 user_intents / insights / retrospective actions")
        (step s4 "feedback: 把学习产物写入 memory 并可触发后续 flow"))
      (egress
        :writes ["user_intents" "conversation_turns.intent_group_id" "agent_questions decisions"]
        :emits "认知分析完成/decision 相关事件"))

    (function project-lifecycle-prescription
      (ingress
        :sources ["board task flow_phase" "mission_submit_phase_result" "engineering-phase FSM"])
      (logic-core
        (step s1 "读取当前 engineering phase")
        (step s2 "校验阶段产物与 gate")
        (step s3 "决定下一阶段、是否拦截、是否要求人工审批"))
      (egress
        :to-memory "board_tasks.flow_phase / flow_context"
        :to-event-bus "QuestionEvent::Created when gated"
        :to-flow "F-board-submit-phase"))

    (core-invariants
      (core-1 "intent files = 系统自我描述语料 (27+ lisp 文件生态)")
      (core-2 "forge = Lisp → IR → Rust 冲压器, 隔离 Generation Gap")
      (core-3 "action-instruction specs = directive / plan / workflow 三段 prescription 链")
      (core-4 "learning-engine 7 sub = 认知/学习/分析核心")
      (core-5 "flow-engine v1 = project lifecycle phases 推进, 属元层")
      (core-6 "lisp-survey + arch-maintenance = 项目代码变化 → intent 文件演化感知器")
      (core-7 "workflows 双 kind: methodology-lisp + executable-yaml")
      (core-8 "本 pillar 管'系统应该如何', memory 管'系统现在记得什么', worker 管'系统在跑什么'")))

  (pillar-egress
    (egress-1 "→ memory pillar: 写 directive-layer 3 表 (schema 归 memory, writer 归本) + user_intents + conversation_turns.intent_group_id (via intent_analyst)")
    (egress-2 "→ worker pillar: 通过 event-bus 驱动 lisp_survey / arch_maintenance 触发")
    (egress-3 "→ tools pillar: 提供 mission_intent / mission_forge_* 后端逻辑")
    (egress-4 "→ flow pillar: workflows 定义 + engineering-phase FSM + methodology lisp")
    (egress-5 "→ 文件系统: <project>/.missiond/intent.lisp 更新 / <project>/CLAUDE.md managed sections / arch manifest / forge 输出代码")
    (egress-6 "→ event-bus: 发射 CascadeTriggered / CascadeCompleted / 认知分析完成事件")

    (cross-pillar-notes
      (memory
        :principle "prescription 写方 on 本 pillar, schema + trait on memory"
        :writers-owned-here
          ["intent_analyst → ConversationStore::insert_user_intent"
           "future directive-compiler → DirectiveLayerStore (pg/directive.rs 17 方法已存在)"]
        :tables-prescription-owned
          ["directive (store-ready actor-pending)"
           "plan (store-ready actor-pending)"
           "workflow (store-ready actor-pending)"]
        :tables-cognitive-produced
          ["user_intents" "conversation_turns.intent_group_id"])

      (worker-migration-in
        :principle "worker v0.4 DC014 决策: runtime-mechanics 留 worker, 认知/学习归本 pillar"
        :migrated-ownership
          [(learning-engine-7-sub "decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / intent_analyst / timeline_analyst — 代码文件仍在 crates/missiond-daemon/src/engine/learning_engine/, 但语义 primary-ownership 在此 pillar")
           (flow-engine-v1 "flow_engine.rs 的 board-phase-engine 推进逻辑")
           (dual-owned-triggers "lisp_survey_worker + arch_maintenance_worker — 触发在 worker, prompt/slot/产物语义在此")]
        :state-machines-owned-here
          [(engineering-phase "Investigate → Consult → Plan → Execute → Finalize, 老图 intent-pillar-state-machines.lisp")
           (extraction-phase "Idle → Sending → WaitingForIdleness → Complete, 老图同")])

      (tools
        :surface "mission_intent / mission_forge_build / mission_forge_lint / mission_project + 未来 mission_directive / mission_plan / mission_workflow"
        :dispatch-principle "tools schema 归 tools pillar, handler 逻辑归本 pillar")

      (flow
        :relationship "workflows (methodology + executable) 的 SSOT 在此, 消费/执行在 flow pillar 与 worker::flow-engine-v2")

      (system-layer
        :not-direct "本 pillar 不直接穿越 system-layer, 但 forge 外部 CLI 算 system process")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.1 Self-Description Files — 自描述 lisp 文件生态
  ;; ══════════════════════════════════════════════════════════
  (section self-description-files
    :desc "系统自我描述的 lisp 文件总体 — 27+ file 生态 + 跨文件 graph 治理"
    :granularities ["L1-Blueprint (.missiond/v2/intent.lisp 总纲)"
                    "L2-Topology (.missiond/v2/intent-*.lisp 各 pillar)"
                    "L3-Implementation (.missiond/intent-db-*.lisp Forge 源 + .missiond/intent-pillar-*.lisp v1 地图)"]

    (component intent-files
      :desc ".missiond/*.lisp 意图声明, 按主题拆分并行加载"
      :count "27 v1 files + 3 frozen v2 (memory/event-bus/worker) + 1 v2 总纲 + 3 草稿 v0.1 (本 pillar/tools/flow)"
      :file-ownership
        [("L1 总纲 .missiond/v2/intent.lisp" :owner "本 pillar 主架构视图")
         ("L2 frozen v2 pillar 文件" :owner "各自 pillar")
         ("L3 旧地图 .missiond/intent-pillar-*.lisp" :owner "历史快照, 权威性随 v2 pillar 产出递减")
         ("Forge 源 .missiond/intent-db-*.lisp" :owner "Forge 冲压输入, schema SSOT")])

    (component intent-graph
      :desc "文件间 module-link / cross-ref 有向图, 供可视化与治理"
      :target "forge-daemon/src/intent_graph.rs (external repo ~/Projects/jarvis-forge)")

    (path system-intent-read
      :lifecycle-style on-demand
      (ingress
        :source "mission_intent(read/section/summary/list) / 人工打开 .missiond/v2/*.lisp"
        :entry-components
          ["crates/missiond-daemon/src/handlers/knowledge/intent.rs (handler)"
           "本 pillar 的 intent-files 生态 (文件 source)"])
      (logic-core
        (step s1 "按 project 或系统域定位目标 lisp 文件 (候选: .missiond/intent.lisp / .jarvis/intent.lisp / intent.lisp)")
        (step s2 "fuzzy project lookup (substring match, 单个自解 / 多个 'ambiguous' 返回候选)")
        (step s3 "按 action 分支: read(全文) / section(切 s-expression block) / summary(提 survey-date) / list(返回 {project_id, intent_path, exists}[])")
        (step s4 "保留 file path 与 survey-date 可追溯信息")
        (step s5 "返回 raw lisp / block / summary / file list"))
      (egress
        :writes []
        :reads []
        :file-reads ["<project>/.missiond/intent.lisp" "~/.claude/CLAUDE.md" ".missiond/v2/*.lisp"]
        :memory-cross-ref ["project-management (ProjectRegistry::resolve)"]
        :returns "intent raw content / section / summary / file list"
        :tools-surface "mission_intent (knowledge domain)"))

    (path intent-graph-build
      :lifecycle-style "on-demand / future"
      (ingress
        :source "lisp 文件集合变化 / 可视化治理请求"
        :entry-components ["forge-daemon/src/intent_graph.rs"])
      (logic-core
        (step s1 "扫文件间 module-link / cross-ref / :target / :binds-to")
        (step s2 "构建有向图")
        (step s3 "标出 orphan / cycle / broken-link / duplicate-target")
        (step s4 "供 governance 或可视化消费")
        (step s5 "输出 graph summary + diagnostics"))
      (egress
        :writes []
        :reads []
        :returns "intent-graph / governance diagnostics"
        :status "mostly TBD — forge-daemon 代码实际实现待确认")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.2 Forge Compilation — Lisp → IR → Rust 冲压器
  ;; ══════════════════════════════════════════════════════════
  (section forge-compilation
    :desc "外部冲压器 + governance lint — 把 Lisp 意图转成确定性 Rust 代码"
    :location "外部仓 ~/Projects/jarvis-forge (独立 git repo)"
    :breaks-if ["codegen-pattern-change" "ir-whitelist-change"]

    (component forge
      :desc "Lisp → IR → Rust Generation Gap 隔离冲压器"
      :target "~/Projects/jarvis-forge (外部独立仓)"
      :forge-binary "FORGE_BIN env override, default 'forge' CLI"
      :modes ["strict-codegen" "descriptive" "experimental"])

    (component governance
      :desc "Forge 模式声明 + 命名漂移检查 + broken-target 探测 + 一致性 cross-ref"
      :target "~/Projects/jarvis-forge/forge-daemon/src/governance.rs")

    (path forge-build
      :lifecycle-style on-demand
      (ingress
        :source "mission_forge_build MCP + build-time forge CLI"
        :entry-components
          ["crates/missiond-daemon/src/handlers/compute/forge.rs (missiond 侧 shell bridge)"
           "external jarvis-forge CLI"])
      (logic-core
        (step s1 "ProjectRegistry 查 project_id → 项目根目录")
        (step s2 "读系统或项目 intent lisp 作输入")
        (step s3 "按 governance 模式校验 (strict-codegen / descriptive / experimental)")
        (step s4 "Lisp → IR (外部 forge 进程)")
        (step s5 "IR → Rust 代码输出 (Generation Gap: generated.rs + custom.rs)"))
      (egress
        :writes []
        :reads ["projects"]
        :file-writes ["crates/*/src/**/generated.rs (Forge 输出)"]
        :memory-cross-ref ["project-management"]
        :returns "build result / exit_code / stdout / stderr"
        :tools-surface "mission_forge_build (compute domain)"
        :worker-bridge-crossref "worker pillar :: section worker-side-computation :: path forge-build-bridge")

    (path forge-lint
      :lifecycle-style on-demand
      (ingress
        :source "mission_forge_lint MCP + lisp_survey_worker (post-survey)"
        :entry-components
          ["crates/missiond-daemon/src/handlers/compute/forge.rs"
           "external jarvis-forge CLI (lint command)"])
      (logic-core
        (step s1 "按 project 定位项目根 + intent.lisp")
        (step s2 "执行 governance lint")
        (step s3 "检查模式违规、命名漂移、broken targets、不一致 cross-ref")
        (step s4 "输出 violations_raw + 建议修复项")
        (step s5 "阻止 build 或提示人工批准 (strict 模式)"))
      (egress
        :writes []
        :reads ["projects"]
        :returns "lint report / violations_raw / governance recommendation"
        :tools-surface "mission_forge_lint (compute domain)")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.3 Global CLAUDE.md — 全域总纲指挥层
  ;; ══════════════════════════════════════════════════════════
  (section global-claudemd
    :desc "全域总纲 — 指挥官对 Claude 的跨项目永久指令"
    :path "~/.claude/CLAUDE.md"
    :scope "global-user"
    :format "Markdown + 可选 YAML frontmatter"

    (component global-claudemd-file
      :desc "文件本身 — 全局偏好 / 行为约束 / 宇宙总纲"
      :path "~/.claude/CLAUDE.md"
      :loaded-by "Claude Code 系统启动时自动加载进 system prompt (当前绕 daemon)"
      :writer "用户手动编辑 / Claude Code Edit tool"
      :nature "元层约束 — 非业务记忆 (项目级约束见 memory :: project-management :: helper project-claudemd-manager)"
      :rationale "放本 pillar 而非 memory: 此文件是'系统如何被指挥'的声明, 属元层")

    (component global-claudemd-manager
      :desc "daemon 侧全局 CLAUDE.md 读/写/reload 管理器"
      :status "code-aligned; read/edit full; reload manual"
      :actions ["read" "edit" "reload"]
      :mcp-tool "mission_global_instruction (read/edit/reload)"
      :code ["crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"]
      :readers "Claude Code 每次会话启动 (当前绕 daemon)"
      :writers "mission_global_instruction(action=edit) / 用户手动 / Claude Code Edit tool (文件层)"
      :reload-boundary "daemon_reload_supported=false; Claude Code reads global CLAUDE.md at session bootstrap, so reload returns manual-reload-required"
      :cross-ref "项目级 <project>/CLAUDE.md manager 在 memory :: project-management :: helper project-claudemd-manager (已实现)")

    (path global-claudemd-load
      :lifecycle-style "session-bootstrap"
      (ingress
        :source "Claude Code 会话启动 (每会话一次)"
        :entry-components ["~/.claude/CLAUDE.md (file)"
                           "Claude Code 系统 prompt loader (外部)"])
      (logic-core
        (step s1 "Claude Code 读取 ~/.claude/CLAUDE.md 全文")
        (step s2 "解析 Markdown + 可选 frontmatter")
        (step s3 "把全局偏好、约束、宇宙总纲注入 system prompt")
        (step s4 "与项目级 CLAUDE.md / intent / memory 形成分层 (全局 < 项目 < 会话)")
        (step s5 "未来 manager: 支持 read/edit/reload daemon 侧动作"))
      (egress
        :writes []
        :reads []
        :file-reads ["~/.claude/CLAUDE.md"]
        :returns "global instruction context"
        :status "partial — 当前 Claude Code 自行处理, daemon 侧 manager 待实现")

    (claude-md-sync-crossref
      :note "项目级 CLAUDE.md managed sections 同步由 worker pillar :: section context-assembly :: path claude-md-managed-sync 负责"
      :boundary "本 pillar 只管 global CLAUDE.md; 项目级归 worker context-assembly"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.4 Action-Instruction Specs — directive / plan / workflow 三段 prescription
  ;; ══════════════════════════════════════════════════════════
  (section action-instruction-specs
    :desc "所有描述'应该做什么 / 如何做'的规约 — DB 3 表 + Lisp/YAML 文件"
    :migrated-from "memory pillar :: project-management (4 tables) + non-db-forms (3 variants + 1 form) 于 v0.4.4"
    :rationale "memory 记'是什么'(facts); 本层记'应该做什么'(prescriptions) — 分层原则"
    :v0.4.19-note "DB 表 schema 归 memory :: module directive-layer; 本 section 负责 writer / compiler / sync 逻辑"

    (component directive-spec-db
      :desc "user utterance → lisp 指令编译记录 — 三段式 pipeline 第一段"
      :table "directive"
      :schema-owned-by "memory :: module directive-layer :: plumbing directive-compilation"
      :writer-owned-by "本 pillar (directive-compiler actor v0)"
      :status "store+manager code-aligned + directive-compiler actor v0 code-aligned (mission_directive compiler_mode=sonnet); auto refine / multi-round review 仍 code-alignment pending"
      :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/directive.rs (compile sonnet branch)"
                               "crates/missiond-core/src/db/pg/directive.rs (DirectiveLayerStore impl)"]
      :v0.4.19-rename "原名 intent 表 → directive 表 (避命名歧义: 和 <project>/.missiond/intent.lisp 代码画像文件区分)"
      :vs-per-project-intent "memory :: project-management :: <project>/.missiond/intent.lisp = factual 代码快照; 本表 = 'Jarvis 对用户话的 lisp 指令编译'")

    (component plan-spec-db
      :desc "directive 编译出的执行 DAG — 绑 board_task + 版本 + FSM"
      :table "plan"
      :schema-owned-by "memory :: module directive-layer :: plumbing plan-execution"
      :writer-owned-by "本 pillar (plan-compiler actor v0 + plan-runner v0; 未来 plan-dag-scheduler / plan-runner v1)"
      :status "code-aligned partial — store+manager + plan-compiler v0 (compiler_mode=sonnet) + plan-runner v0 (execute_mode=internal + dispatch_strategy + sidecar, 单节点 dispatch); 自动选 dispatch_strategy/target/target_project + 完整 PLAN DAG scheduler 仍 pending — 详 section action-instruction-actor :: actor plan-dag-scheduler"
      :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs (compile sonnet + execute internal)"]
      :future-implementation-targets ["crates/missiond-daemon/src/intent_layer/plan_runner.rs (新文件 — DAG scheduler 出 plan.rs handler)"
                                      "crates/missiond-daemon/src/intent_layer/plan_dag.rs (新文件 — DAG 结构 + cycle 检测 + ready-set + 节点 FSM)"]
      :pending ["完整 PLAN DAG scheduler — 多节点 dependency 执行 / 并发 dispatch / per-node retry-failure-policy / condition-gate / rollback-compensation / per-node evidence aggregation (architecture-designed; 详 actor plan-dag-scheduler)"
                "PLAN.lisp DAG 自动选 target/dispatch_strategy/target_project (auto-selection v1 仅保守 hint)"
                "supersede-chain 策略 (FSM 迁移已 code-aligned, 自动选最佳 supersede target 仍 caller 决定)"])

    (component workflow-spec-db
      :desc "从成功 plan 蒸馏的可复用模板 — 带 match_rules + 统计"
      :table "workflow"
      :schema-owned-by "memory :: module directive-layer :: plumbing workflow-templates"
      :writer-owned-by "本 pillar (workflow-distiller actor v0 + methodology compiler v0)"
      :status "code-aligned partial — store+manager + workflow-distiller v0 (distill_mode=sonnet) + methodology compiler v0 (compile_mode=deterministic + run_methodology); 高阶 semantic lifting / forge compiler / 自动 record_execution 关联 仍 pending"
      :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs (distill sonnet + compile_methodology deterministic + run_methodology)"]
      :pending ["distiller 高阶 phase/anti-pattern lifting"
                "成功 plan 自动 record_execution + distill 联动"
                "全局 generated flow registry (mission_flow_run 自动发现 .missiond/generated/flows/)"])

    (path directive-plan-workflow-chain
      :lifecycle-style "store+manager + actor v0 code-aligned partial"
      :status "code-aligned partial — store+manager + directive-compiler v0 (sonnet) / plan-compiler v0 (sonnet) / workflow-distiller v0 (sonnet) / methodology compiler v0 (deterministic); 自动 multi-round refine / semantic lifting / 自动 review gate / forge compiler 仍 pending"
      :flow-ref "flow pillar :: F-directive-plan-workflow-compile"
      (ingress
        :source "用户 utterance (来自 Claude Code 对话) / 系统指令 / future actor / future sync worker"
        :entry-components ["directive-spec-db" "plan-spec-db" "workflow-spec-db"]
        :store-api "DirectiveLayerStore trait (directive 6 + plan 6 + workflow 5 methods)")
      (logic-core
        (step s1 "capture: 从用户 utterance / 系统指令 / 未来 MCP 请求收集原文、项目、会话、约束引用")
        (step s2 "directive-compile: LLM-assisted 编译 lisp-level directive sexp, 写 directive_insert(status=draft/refining, compiler_model, references_json)")
        (step s3 "directive-review: 人类或策略 gate approve/refine/archive, 通过 directive_update_status / directive_approve 迁移 FSM")
        (step s4 "plan-compile: 从 approved directive 生成 plan sexp DAG/FSM, 绑定 board_task_id, 计算 sexp_hash, 写 plan_insert(status=draft|awaiting_approval)")
        (step s5 "plan-execute-bridge: approval 后把 plan 适配到 board/flow/skill/pty 执行面, 用 plan_update_status 记录 approved/executing/succeeded/failed/superseded")
        (step s6 "workflow-distill: 从 succeeded plan 抽取 reusable workflow sexp + match_rules, 写 workflow_insert 或 workflow_record_execution")
        (step s7 "workflow-match: 对新 utterance 先 workflow_find_by_match / workflow_list_top_n, 命中后可作为 plan-compile hint 或直接 apply")
        (step s8 "manager-surface: mission_directive/mission_plan/mission_workflow 提供 read/control/manual override; compile/distill remain dry-run until actors exist"))
      (egress
        :writes ["directive" "plan" "workflow (表, 未来)"]
        :reads ["board_tasks" "kb_entries/context" "project registry" "successful plan history"]
        :via-bus []
        :memory-cross-ref ["directive-layer"]
        :returns "compiled directive / executable plan / reusable workflow template"
        :tools-surface "mission_directive / mission_plan / mission_workflow (MCP manager tools, code-aligned partial)")

    (specs-manager-status
      :desc "action/instruction specs 的 read/write/reload manager"
      :actions ["compile" "approve" "list" "get" "supersede" "match" "record-execution" "sync-with-file"]
      :status "manager/tools code-aligned partial — mission_directive / mission_plan / mission_workflow implemented for read/control/draft persistence; LLM compile/distill actors and file sync remain pending"
      :cross-ref "memory :: project-management :: path project-code-snapshot (读 per-project 代码快照 FILE, 职责不同)"
      :future-decision "优先实现本 pillar writer actor + MCP read/control tools; 不建议 drop, 因 store/API 已成型"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.5 Workflows — methodology lisp + executable YAML
  ;; ══════════════════════════════════════════════════════════
  (section workflows
    :desc "统一工作流规约 — 两 kind (人类看 vs 机器跑), 概念一致格式异"
    :unified-in "v0.4.5 (原 workflow-lisp-templates + flow-yaml-templates 合并)"
    :design-rationale "两 kind 形式差异大但概念一致 — 保留各自格式优势, 统一纳管"

    (kind methodology
      :desc "Lisp 方法论模板 — 人类 / agent 参考的 SSOT; 机器执行需先编译成 executable YAML"
      :path ".missiond/workflows/*.lisp"
      :consumers ["human" "mission_intent tool" "agent 参考" "future methodology compiler"]
      :granularity "抽象叙事 — phases / principles / anti-patterns / baseline-numbers / decision-authority"
      :examples ["pillar-refactor.lisp (335 行, 本会话凝结 memory pillar 重构方法论)"
                 "bus-refactor.lisp (11-phase 事件总线重构方法论)"]
      :executability "human-readable source; machine execution via F-methodology-to-executable-compile → generated YAML → mission_flow_run"
      :owner "本 pillar")

    (kind executable
      :desc "YAML 声明式节点编排 — flow-engine-v2 运行时执行"
      :path "$MISSIOND_HOME/flows/*.yaml"
      :loader "crates/missiond-daemon/src/engine/flow/loader.rs (worker pillar 实现)"
      :executor "worker pillar :: section engine-cluster :: subsection flow-engine-v2"
      :parser "serde_yaml::from_str::<FlowDefinition>"
      :granularity "具体机器操作 — 5 node types: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
      :executability "✓ 机器执行"
      :owner "本 pillar (定义 SSOT) + worker pillar (runtime 执行)")

    (path workflow-kinds-split
      :lifecycle-style design-time
      (ingress
        :source "人要看方法论 / 机器要跑 workflow"
        :entry-components [".missiond/workflows/*.lisp" "$MISSIOND_HOME/flows/*.yaml"])
      (logic-core
        (step s1 "methodology-lisp: phases / principles / anti-patterns / authority 边界 (人类消费)")
        (step s2 "executable-yaml: 具体机器节点序列 (flow-engine-v2 消费)")
        (step s3 "两者概念统一为 workflow, 按 受众 / 粒度 / 执行性 拆分")
        (step s4 "允许同名映射 (如 bus-refactor.lisp ↔ bus-refactor.yaml), 不强制 1:1")
        (step s5 "机器执行固定走 F-methodology-to-executable-compile: methodology-lisp → executable-yaml → mission_flow_run"))
      (egress
        :writes []
        :reads []
        :returns "human methodology / machine workflow"
        :consumed-by ["flow pillar (SSOT 索引)" "worker::flow-engine-v2 (执行)" "human / agent review (方法论)"]))

    (path methodology-to-executable-compile
      :lifecycle-style design-time-to-runtime
      :status "code-aligned partial — methodology compiler v0 (compile_mode=deterministic + run_methodology, paren-validate + (step …) 抽取 + executable YAML) + generated flow loader (search: explicit > <project_root>/.missiond/generated/flows > $MISSIOND_HOME/flows); 高阶 semantic lifting / forge compiler / longest-prefix cwd resolver / record_execution-distill 联动 仍 pending"
      :flow-ref "flow pillar :: F-methodology-to-executable-compile"
      :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs (action_compile_methodology + action_run_methodology)"
                               "crates/missiond-daemon/src/engine/flow/loader.rs (generated flow loader: GENERATED_FLOWS_REL + searched_paths)"
                               "crates/missiond-daemon/src/handlers/compute/flow_run.rs (mission_flow_run discoverability surface)"]
      (ingress
        :source ".missiond/workflows/<name>.lisp + params + target_project + dry_run/run mode"
        :entry-components ["mission_workflow(action=compile_methodology|run_methodology)" "future forge compile extension"])
      (logic-core
        (step s1 "load methodology Lisp source and compute source_hash")
        (step s2 "parse phases / steps / gates / anti-patterns / authority boundaries")
        (step s3 "map steps to FlowDefinition node types (LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks)")
        (step s4 "lint generated YAML against flow-engine-v2 schema and tool registry")
        (step s5 "write generated executable YAML with source_lisp/source_hash/compiler_version metadata")
        (step s6 "for run mode, call mission_flow_run; for dry_run, return compiled plan only")
        (step s7 "feed compile/run result to workflow-distiller for future reuse"))
      (egress
        :writes ["$MISSIOND_HOME/flows/<name>.yaml" "optional workflow compile/run feedback"]
        :reads [".missiond/workflows/*.lisp" "tool registry" "flow-engine-v2 schema"]
        :returns "compiled flow path/id + lint result + optional run result"))

    (relationship-between-kinds
      :overlap "都描述'多步工作流'"
      :split-axis "受众 (human vs machine) + 粒度 (抽象 vs 具体) + 执行性"
      :why-not-unify-format "Lisp 富元数据给人看, YAML 轻量 schema 给 flow-engine 消费; 硬统一两边都难用"
      :cross-ref-convention "可约定同名对照, 非强制"
      :future-possibility "已升级为 architecture target: F-methodology-to-executable-compile; 当前代码对齐待实现"))

    (path implementation-alignment-artifacts
      :lifecycle-style "file-first-to-runtime"
      :status "architecture-designed; code-alignment pending — 详细 8 logical-role 视图见 section unified-entry-pipeline"
      :flow-ref "flow pillar :: F-intent-alignment-plan-execution-loop"
      :see-also "section unified-entry-pipeline (本 pillar) — 8 logical roles + double review gate + plan-runner contract"
      (ingress
        :source "你我完成一轮 architecture Lisp 设计后,准备让 MissionD/ClaudeCode 做代码同构"
        :entry-components [".missiond/v2/*.lisp diff" "human instructions" "architecture checker output"])
      (logic-core
        (step s1 "intent-alignment.lisp: 凝结本轮目标、边界、受影响 pillar、禁止事项、验收条件")
        (step s2 "planner LLM: 读取 intent-alignment.lisp + 相关 Lisp snippet + repo summary,生成 PLAN.lisp")
        (step s3 "PLAN.lisp: files/phases/tasks/tests/risks/rollback,供人类/Codex 修订")
        (step s4 "execution: approved PLAN.lisp 进入 mission_execution / mission_task_delegate / mission_flow_run 或人工 ClaudeCode handoff")
        (step s5 "evidence: 收集 tool_calls/event_log/board_tasks/execution log/test output")
        (step s6 "workflow.lisp: 多次成功后由 workflow-distiller 抽象成可复用方法论")
        (step s7 "需要机器执行时,workflow.lisp 再走 F-methodology-to-executable-compile 生成 YAML"))
      (egress
        :file-writes [".missiond/alignment/<topic>/intent-alignment.lisp"
                      ".missiond/plans/<topic>/PLAN.lisp"
                      ".missiond/workflows/<topic>.lisp"]
        :db-writes ["optional directive / plan / workflow mirror rows"]
        :returns "reviewed implementation plan or reusable workflow"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.5b Unified Entry Pipeline — MissionD canonical message → workflow 主线
  ;; ══════════════════════════════════════════════════════════
  (section unified-entry-pipeline
    :desc "MissionD 长期运作 canonical 主线: message → intent-alignment.lisp → review → PLAN.lisp → review → MissionD-internal execution → evidence → workflow.lisp distillation"
    :status "code-aligned partial — directive-compiler v0 / plan-compiler v0 / plan-runner v0 + auto-selection v1 / workflow-distiller v0 / methodology compiler v0 / generated flow loader / unified-entry pipeline v0 internal helper (run_pipeline + 纯函数 plan_pipeline; 7 步 pipeline_stage; 每 response 带 pipeline_stage/next_step/flow_ref/expects_next_inputs/next_call/v0_non_goals[4]; 不新增 MCP tool, tool count 保持 83) / evidence-collector typed EvidenceEntry / plan-dag-scheduler runtime v2 全部 code-aligned partial; wave 14 task 01/02/03/04 升级: file-first writer integration / PlanNodeStateChanged variant + live EventRef / review-gate auto-create v1 (policy enum) / unified-entry pipeline v1 (file-first + review-gate + scheduler args 转发, response 加 artifact_refs) 全部 code-aligned; 高阶 semantic lifting / forge compiler / 完整 11-stage scheduler / 4 项 v0 non-goal 自动化 仍 pending (详 wave-13 + wave-14 anchors)"
    :implementation-target-policy "本 section 各 role 的 :implementation-targets 命名 *current code-aligned entry points*, 不是最终 module boundaries; future code-convergence 可能把现有大 handler 文件按 Lisp 声明拆分成更细的 module / actor"
    :flow-ref "flow pillar :: F-intent-alignment-plan-execution-loop"
    :file-first-ssot ".missiond/alignment/<topic>/intent-alignment.lisp + .missiond/plans/<topic>/PLAN.lisp + .missiond/workflows/<topic>.lisp"
    :db-mirror "memory pillar :: module directive-layer :: directive / plan / workflow tables (可查询镜像 + 状态管理面)"
    :goal "可自动化、可 flow 化、可复用 — 不依赖某个交互 client 私有调度能力"
    :anti-pattern "不允许 LLM 产物 (alignment 或 PLAN) 跳过 review gate 直接进入执行; 不允许 client 直连工位绕过 plan-runner"

    (logical-roles
      :desc "8 个 logical role 串成 unified pipeline; 各 role 的实现状态独立标注"

      (role message-intake-manager
        :desc "汇集来源不一的 message (用户对话 / external MCP client / board task / architecture Lisp delta), 决定走 file-first alignment 还是 directive-mirror"
        :inputs ["user message" "external MCP request" "board_task ingestion" ".missiond/v2/*.lisp diff"]
        :current-surface "mission_directive(action=compile, source=message|architecture_lisp_delta|user_request)"
        :status "code-aligned partial — manager surface via mission_directive; compile is dry-run / persists draft only; directive-compiler actor pending"
        :owner "tools schema 在 tools pillar; intake 决策语义在本 pillar; directive 行 schema 在 memory pillar")

      (role alignment-author
        :desc "把 message + Lisp delta + repo summary 编译成 .missiond/alignment/<topic>/intent-alignment.lisp"
        :modes
          ((mode-A direct-llm
              :desc "本 pillar 直接调 LLM (sonnet / opus / 未来 xjp-router 路由) — 不经过 PTY, 适合无观测需求的纯 LLM 推理"
              :status "code-aligned partial — directive-compiler actor v0 提供 mission_directive(action=compile, compiler_mode=sonnet) 入口 (SonnetGateway interactive lane + sexp validation); 多轮 refine / model alias 切换 仍 code-alignment pending"
              :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/directive.rs (action=compile sonnet branch)"
                                       "crates/missiond-daemon/src/llm/sonnet_gateway.rs"])
           (mode-B resident-claudecode-slot
              :desc "派给常驻 ClaudeCode lisp-architect slot 写 alignment file — 复用已加载的 .missiond/v2/*.lisp 上下文 asset, 减少重新定位成本"
              :preferred-substrate "mission_pty_send / mission_task_delegate 把任务挂到既有 lisp-architect slot; 不为本轮 alignment 重开 fresh slot"
              :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: policy resident-lisp-architect-session"
              :status "operational-practice (人工已用) + architecture-designed; slot 自动 dispatch 仍 code-alignment pending"))
        :status "code-aligned partial — mode A (directive-compiler v0) 已 code-aligned 写 directive sexp + DB draft; file-first .missiond/alignment/<topic>/intent-alignment.lisp 自动写入仍 pending; mode B 仍是人工挂任务到 slot; 自动选择 mode A vs mode B (alignment-author actor 自主路由) 仍 code-alignment pending"
        :default-mode "Lisp 架构改动默认走 mode B (resident-claudecode-slot); 短任务/无上下文需求可走 mode A (direct LLM via mission_directive compiler_mode=sonnet)"
        :architecture-writes [".missiond/alignment/<topic>/intent-alignment.lisp (file-first SSOT, status=draft)" "directive table draft mirror"]
        :code-aligned-writes ["directive sexp + references_json + directive table draft (mission_directive compiler_mode=sonnet, persist=true)"]
        :review-gate-downstream "alignment-review-gate"
        :pending ["file-first .missiond/alignment/<topic>/intent-alignment.lisp 自动写入/与 directive 表双向同步"
                  "alignment-author actor 自主决定 mode + 多轮 refine"
                  "mode B 自动 dispatch (plan-runner 或 alignment-author 主动把任务挂到 lisp-architect slot)"])

      (role alignment-review-gate
        :desc "human/Codex 修改 alignment 直到满意; 状态从 draft/reviewing 流转到 approved/rejected/superseded"
        :gate-rule "未通过 approval gate 不允许生成 PLAN.lisp"
        :owner "human/Codex (人工评审为主); 未来 actor 可发 QuestionEvent 触发人工 review"
        :current-surface "mission_directive(action=approve|archive|version_chain)"
        :status "code-aligned partial — manual gate via mission_directive control actions; auto QuestionEvent + 自动 review prompts pending")

      (role plan-compiler
        :desc "读取 approved intent-alignment.lisp 或 board task context, 调用 LLM planner 生成 .missiond/plans/<topic>/PLAN.lisp"
        :inputs ["approved intent-alignment.lisp / approved directive (source_directive_id) / board task context" "repo state summary" "kb context" "previous plan history"]
        :architecture-writes [".missiond/plans/<topic>/PLAN.lisp (file-first SSOT, status=draft)" "plan table draft/awaiting_approval mirror"]
        :code-aligned-writes ["plan sexp + plan table draft/awaiting_approval row (mission_plan compiler_mode=sonnet, persist=true)"]
        :current-surface "mission_plan(action=compile, compiler_mode=dry_run|sonnet, persist=true)"
        :status "code-aligned partial — plan-compiler v0 写 plan sexp / plan 表 (compiler_mode=sonnet, persist=true 写 awaiting_approval, 不自动 approve, compiled_from=directive/<id>:<version> 或 board_task/<id>); file-first .lisp 自动同步 + 自动 :dispatch-strategy/:target_project 推断 + 多轮 re-compile 仍 pending"
        :model-policy "provider alias configurable (例: OPUS-4.7-class planner / planner-class model); 不硬编码可用性; current code-aligned v0 uses claude-sonnet"
        :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/plan.rs (schema)"
                                 "crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=compile sonnet branch)"]
        :pending ["file-first .missiond/plans/<topic>/PLAN.lisp 自动写入/与 plan 表双向同步"
                  "planner-class model alias 切换 (例: opus planner) — 现仅 sonnet 可用"
                  "alignment 多轮 refine 后增量 re-compile"
                  "PLAN.lisp 节点 :dispatch-strategy/:target_project 字段强制 schema 校验"])

      (role plan-review-gate
        :desc "human/Codex 修改 PLAN 直到满意; approved PLAN 才能执行"
        :gate-rule "未通过 approval gate 不允许进入 execution-runner"
        :current-surface "mission_plan(action=approve|mark|supersede)"
        :status "code-aligned partial — manual gate via mission_plan control actions; auto QuestionEvent gate pending")

      (role plan-runner
        :desc "MissionD 内部 plan 执行调度器 — 不是 client 直接调工位, 而是 plan-runner 内部消费 mission_execution / mission_task_delegate / mission_flow_run / mission_compute_slot / mission_pty_spawn"
        :principle "review gate 已经把 LLM 产物收敛到可执行边界; plan-runner 在 daemon 内部按 PLAN.lisp 节点调度 + 工位策略调度 + execution coordination, 把执行从 client 私有逻辑里抽离"
        :current-surface "mission_plan(action=execute, execute_mode=bridge|internal, dispatch_strategy=..., max_parallel_nodes=...)"
        :status "code-aligned partial — plan-runner v0 + auto-selection v1 (execute_mode=internal dispatch 到 mission_execution/task_delegate/flow_run + 写 sidecar plan_runner_dispatch + 推 plan FSM executing; bridge mode 兼容; dispatch_strategy 进 response/sidecar/companion log meta; auto-selection 从 plan.sexp_text 保守解析 :target/:dispatch-strategy/:parallelism/:target-project/:requested-cwd hints; explicit args 优先, 无法推断返回 MISSING_PARAM; agent-team 注入幂等); plan_dag runtime v2 code-aligned partial (max_parallel_nodes / 6 lifecycle + 3 skip 子分类 / failure-policy fail-fast vs continue / per-node typed evidence; dry_run 返 DAG + concurrency_plan); 完整 11-stage / 任意语义解析 / file-first PLAN.lisp writer 仍 pending — 详 actor plan-dag-scheduler (anchor: intent-layer.plan-dag-runtime-v2)"
        :execution-substrate "mission_execution 12-action manager (open/list/claim/heartbeat/release/deviate/decide/issue/complete/status/audit/repair) — F-execution-log-governance"
        :downstream-tools ["mission_execution" "mission_task_delegate" "mission_flow_run" "mission_compute_slot" "mission_pty_spawn" "mission_pty_send"]
        :workstation-dispatch-responsibility "读取 PLAN.lisp 节点 :dispatch-strategy + :target_project, 按 dispatch-decision-matrix 选 resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback, 再 spawn/reuse 对应 slot"
        :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration / flow pillar :: F-workstation-dispatch-policy"
        :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/plan.rs (execute_mode + dispatch_strategy + auto-selection schema)"
                                 "crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=execute internal + auto-selection v1 sexp hint parser + sidecar + plan FSM)"]
        :pending ["完整 11-stage PLAN DAG scheduler (claim-lease / per-node retry / acceptance / rollback / review-gate paused / mark-plan-final / trigger-record-execution-distill) — runtime v2 仅 dispatch + lifecycle + failure-policy + per-node evidence 子集"
                  "arbitrary PLAN.lisp 语义解释 (超出保守 hints)"
                  "PLAN.lisp 节点 → flow YAML 自动编译 (auto-selection 暂用 :flow-id 命中)"
                  "file-first PLAN.lisp writer/sync"
                  "status update failure 时事务性回滚"]
        :v1-future-cross-ref "section action-instruction-actor :: actor plan-dag-scheduler (code-aligned partial via runtime v2; 完整 11-stage scheduler 仍 architecture-designed) — claim-lease 接入 agent-execution-coordination"
        :anti-pattern "不允许把执行调度逻辑下沉给某个 client (例如 ClaudeCode CLI) 私有的 next_call 解析能力 — 那等于把 MissionD 主线绑死在某个 client")

      (role evidence-collector
        :desc "执行过程证据落盘 — git diff / tests / tool_calls / event_log refs / execution companion log refs / deviations / decisions / completions"
        :reads ["tool_calls" "event_log (DomainEvent stream)" "ExecutionEvent stream" "board_tasks" "execution companion log" "test outputs" "git diff"]
        :writes [".missiond/v2/plans/<plan_id>.evidence.json (sidecar — code-aligned via mission_plan(record_evidence) + plan-runner v0 internal mode 自动追加 plan_runner_dispatch entry + plan_dag runtime v2 每节点 dispatch 写 plan_dag_node_dispatch entry)" "future plan_evidence DB JSONB 列或表"]
        :current-surface "mission_plan(action=record_evidence) + plan-runner internal mode 自动写 + plan_dag runtime v2 自动写每节点 transition"
        :status "code-aligned partial — sidecar via record_evidence; plan-runner v0 internal mode 自动追加 dispatch entry; typed EvidenceEntry helper (handlers/knowledge/evidence_collector.rs) canonical source/kind/schema_version + with_extra flat-top byte-for-byte 兼容 legacy passthrough; plan.rs + plan_dag.rs 已接入; live ExecutionEvent ref 用 EventRef::unavailable 占位; 全自动 evidence-collector actor 仍 pending (anchor: intent-layer.evidence-collector-typed-helper)"
        :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs (typed EvidenceEntry / EventRef / append helper — wave 13 task 01)"
                                 "crates/missiond-daemon/src/handlers/knowledge/plan.rs (record_evidence + plan-runner internal append, typed wrap)"
                                 "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs (每节点 dispatch + 每节点 state transition 写 typed EvidenceEntry)"]
        :pending ["全自动 evidence-collector actor"
                  "升级到 DB plan_evidence JSONB 列或独立表"
                  "EventRef::unavailable → live ExecutionEvent id (等 bus live subscription)"
                  "ExecutionEvent dispatch metadata 入 sidecar (wave 13 task 02 决议: scheduler runtime 与 bus subscription 正交, 不扩 ExecutionEvent variant 直到 bus subscription / 完整 11-stage scheduler 落地)"]
        :purpose "为 workflow-distillation 与 retrospective 提供输入")

      (role workflow-distiller
        :desc "成功多次的 plan 沉淀成 .missiond/workflows/<topic>.lisp; 可继续走 F-methodology-to-executable-compile 编译为 YAML 由 flow-engine-v2 runner 执行"
        :inputs ["succeeded plan + plan evidence sidecar" "repeated success history"]
        :architecture-writes [".missiond/workflows/<topic>.lisp (file-first SSOT)" "workflow table mirror"]
        :code-aligned-writes ["workflow sexp + match_rules JSON + workflow table draft/template (mission_workflow distill_mode=sonnet, persist=true)" "<project_root>/.missiond/generated/flows/<flow_id>.yaml (compile_mode=deterministic, persist=true)"]
        :current-surface "mission_workflow(action=distill|record_execution|compile_methodology|run_methodology) — distill_mode=dry_run|sonnet, compile_mode=dry_run|deterministic"
        :status "code-aligned partial — workflow-distiller v0 (distill_mode=sonnet 写 workflow sexp / match_rules / workflow 表) + methodology compiler v0 (compile_mode=deterministic, paren-validate + (step …) + YAML) + generated flow loader (mission_flow_run discoverability); file-first .missiond/workflows/<topic>.lisp 自动同步 + 高阶 semantic lifting + forge compiler + 自动 record_execution 关联 仍 pending"
        :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/workflow.rs (schema)"
                                 "crates/missiond-daemon/src/handlers/knowledge/workflow.rs (action=distill sonnet + action_compile_methodology deterministic + action_run_methodology)"
                                 "crates/missiond-daemon/src/engine/flow/loader.rs (generated flow loader)"
                                 "crates/missiond-daemon/src/handlers/compute/flow_run.rs (mission_flow_run discoverability)"]
        :pending ["file-first .missiond/workflows/<topic>.lisp 自动写入/与 workflow 表双向同步"
                  "distiller 高阶 semantic lifting (phase / anti-pattern / authority)"
                  "forge compiler 接管 methodology → flow 编译"
                  "成功 plan 自动触发 distill (现需 caller 主动)"
                  "record_execution 与 distill 双向自动联动"]))

    (review-gates
      (alignment-review-gate
        :artifact ".missiond/alignment/<topic>/intent-alignment.lisp"
        :status-lifecycle "draft → reviewing → approved | rejected | superseded"
        :owner "human/Codex"
        :downstream "plan-compiler 仅在 status=approved 时启动")
      (plan-review-gate
        :artifact ".missiond/plans/<topic>/PLAN.lisp"
        :status-lifecycle "draft → reviewing → approved → executing → succeeded | failed | superseded"
        :owner "human/Codex"
        :downstream "plan-runner 仅在 status=approved 时启动"))

    (file-vs-db-contract
      :ssot "file-first — alignment/plan/workflow .lisp 是真正的 review 边界 (人机协作格式), DB 行是可查询镜像 + 状态管理面"
      :alignment-mirror ".missiond/alignment/<topic>/intent-alignment.lisp ↔ memory :: directive-layer :: directive table"
      :plan-mirror ".missiond/plans/<topic>/PLAN.lisp ↔ memory :: directive-layer :: plan table"
      :workflow-mirror ".missiond/workflows/<topic>.lisp ↔ memory :: directive-layer :: workflow table"
      :evidence-mirror ".missiond/v2/plans/<plan_id>.evidence.json (sidecar) — 未来可升级到 plan_evidence DB JSONB")

    (no-new-tool-decision
      :rationale "mission_directive(compile) + mission_plan(execute) + mission_workflow 已是充分管理面; 不新增 mission_message / mission_invoke"
      :future-candidates "见 flow pillar :: future-flows :: unified-entry-future-candidates"
      :tool-count-invariant "保持 83 actual tools"
      :wave-13-task-03-confirmation "wave 13 task 03 (commit 9759675) — unified-entry pipeline v0 internal helper 不新增 tool, 通过 next_call descriptor 驱动 caller 调既有 tool (anchor: intent-layer.unified-entry-pipeline.run-pipeline-helper / .no-new-tool-decision)")

    (v0-non-goals
      :scope "wave 13 task 03 unified-entry pipeline v0 internal helper 明确不实现的 4 项自动化 — caller 必须人工/Codex review"
      :enforcement "internal helper 在每 response meta.v0_non_goals 显式 surface 这 4 项, 防止 caller 误以为系统能自动越过 review gate"
      :items
        [(item-1 :name "auto_approve_directive"
                 :rationale "alignment-review-gate 必须人工/Codex 评审; 不允许 LLM 产物自动越过 mission_directive(action=approve)")
         (item-2 :name "auto_approve_plan"
                 :rationale "plan-review-gate 必须人工/Codex 评审; 不允许 LLM 产物自动越过 mission_plan(action=approve)")
         (item-3 :name "auto_answer_review_question"
                 :rationale "QuestionEvent 必须人工/Codex 答; internal helper 不替人答 review question")
         (item-4 :name "autonomous_workstation_dispatch"
                 :rationale "internal helper 不自动 spawn ClaudeCode 工位执行 plan; mission_plan(action=execute) 是 caller 显式触发, internal helper 仅返回 next_call descriptor")]
      :code-anchor "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :status pending
      :wave "13 task 03 (commit 9759675)")

    (workstation-dispatch-policy
      :status "moved-to-shard (operational-practice + architecture-designed)"
      :file-ref ".missiond/v2/intent-workstation-policy.lisp"
      :shard-section "workstation-dispatch-policy"
      :section-id "intent-layer.unified-entry-pipeline.workstation-dispatch-policy"
      :worker-pillar-cross-ref "worker pillar :: section claudecode-workstation-orchestration (also moved to shard)"
      :flow-pillar-cross-ref "flow pillar :: F-workstation-dispatch-policy + F-intent-alignment-plan-execution-loop :: s2 / s4 / s6 (also moved to shard)"
      :tools-pillar-cross-ref "mission_pty_spawn / mission_pty_send / mission_compute_slot / mission_task_delegate / mission_execution / mission_plan"
      :note "6 principles (alignment-author-mode-selection / plan-runner-dispatch-selection / task-md-self-contained-contract / resident-context-not-substitute-for-checker / spawn-over-prompt-mode / project-root-cwd-contract) live in shard; section-id stable per L2 plan rule-1")

    (canonical-flow-cross-ref "flow pillar :: F-intent-alignment-plan-execution-loop — 8 stage 详细 narrative 在那里 / flow pillar :: F-workstation-dispatch-policy — 工位调度策略 narrative"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.6 Capability Evolution Governance — 工具/flow 使用度治理
  ;; ══════════════════════════════════════════════════════════
  ;; ── section capability-evolution-governance moved to L2 shard ──
  ;; Full content moved to .missiond/v2/intent-capability-governance.lisp
  ;; (wave 15 task 02 L2 shard split per architecture-dsl.lisp ::
  ;;  l2-shard-split-plan :: shard intent-capability-governance).
  (section capability-evolution-governance
    :status "moved-to-shard (policy-designed + code-aligned partial)"
    :file-ref ".missiond/v2/intent-capability-governance.lisp"
    :shard-section "capability-evolution-governance"
    :section-ids
      ["intent-layer.section.capability-evolution-governance"
       "intent-layer.capability-evolution-governance.semantic-evidence-v1"]
    :flow-cross-ref "flow pillar :: F-capability-usage-monitoring (also moved to shard)"
    :memory-cross-ref "memory pillar :: derived-read-model capability-usage-read-model (also moved to shard)"
    :tool-cross-ref "tools pillar :: implemented-surface mission_capability_usage (also moved to shard)"
    :note "section-id stable per L2 plan rule-1; full policy / lifecycle-states / capability-lifecycle-review live in shard")

  ;; ══════════════════════════════════════════════════════════
  ;; 5.7 LispSurvey — 项目代码变化感知器 (dual-owned with worker)
  ;; ══════════════════════════════════════════════════════════
  (section lisp-survey-dual-owned
    :desc "项目代码变化 → <project>/.missiond/intent.lisp 增量更新"
    :worker-cross-ref "worker pillar :: section worker-cluster :: subsection worker-sonnet :: path lisp-survey-update"
    :ownership-split
      (trigger-on-worker "ContextualCommitDetected subscribe + BackgroundWorker impl + self-trigger filter + debounce")
      (semantic-on-this-pillar "survey prompt 模板 + slot registered-task 'lisp_survey' + intent.lisp 写 ownership")

    (path lisp-survey-update-semantic
      :lifecycle-style event-driven
      :added "commit 79a877f"
      (ingress
        :source "worker pillar 的 lisp-survey-update path 触发后, slot 端执行本 path 语义"
        :entry-components
          ["crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs (触发侧, worker 拥有)"
           "slot_orchestrator registered-task 'lisp_survey' (slot-id lisp-surveyor, model sonnet, timeout 900s)"
           "survey prompt (本 pillar 拥有的模板)"])
      (logic-core
        (step s1 "worker 侧已过滤 self-trigger (slot_id != lisp-surveyor) + ProjectRegistry::resolve + 60s debounce")
        (step s2 "本 pillar 组装 survey prompt: 'diff + intent_path + 差量 指令'")
        (step s3 "slot_manager.execute('lisp_survey', prompt) → lisp-surveyor persistent Sonnet slot")
        (step s4 "slot 端执行 LLM 调用 + 解析 response")
        (step s5 "如 NO_CHANGE: 跳过; 否则 slot Edit 工具更新 <project>/.missiond/intent.lisp"))
      (egress
        :writes []
        :reads []
        :file-writes ["<project>/.missiond/intent.lisp (via slot Edit tool)"]
        :via-bus ["SystemEvent::ContextualCommitDetected (consumed on worker side)"]
        :returns "survey result / NO_CHANGE / intent file update"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.8 ArchMaintenance — 架构文档演化器 (dual-owned with worker)
  ;; ══════════════════════════════════════════════════════════
  (section arch-maintenance-dual-owned
    :desc "代码 commit → architecture manifest 增量更新"
    :worker-cross-ref "worker pillar :: section worker-cluster :: subsection worker-sonnet :: path arch-maintenance-worker-cycle"
    :ownership-split
      (trigger-on-worker "ContextualCommitDetected subscribe (commit 65c8b59 替换 git-log 3600s polling)")
      (semantic-on-this-pillar "arch prompt 模板 + slot registered-task 'arch_maintenance' + manifest 文件 ownership")

    (path arch-maintenance-semantic
      :lifecycle-style event-driven
      (ingress
        :source "worker 侧 arch_maintenance_worker 触发后, slot 端执行本 path 语义"
        :entry-components
          ["crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs (触发, worker)"
           "slot_orchestrator registered-task 'arch_maintenance' (slot-id arch-surveyor, model sonnet, timeout 900s)"
           "arch prompt (本 pillar 拥有的模板)"])
      (logic-core
        (step s1 "消费带 conversation/session/slot 上下文的 ContextualCommitDetected (commit 65c8b59 新事件 — 含 conversation_id 桥接)")
        (step s2 "本 pillar 组装 arch maintenance prompt: 'diff + project + context + manifest 位置'")
        (step s3 "SlotManager.execute('arch_maintenance', prompt) → arch-surveyor slot")
        (step s4 "slot 端执行 LLM 调用 + Edit 更新 architecture manifest 文件")
        (step s5 "manifest 文件归本 pillar ownership, 非 memory"))
      (egress
        :writes []
        :reads []
        :file-writes ["project architecture manifest (例: YAML 或 Markdown, 项目自定义)"]
        :via-bus ["SystemEvent::ContextualCommitDetected (consumed on worker side)"]
        :returns "slot dispatch result / arch manifest update"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.9 Learning Engine — 7 sub-engine 认知/推理内核
  ;; ══════════════════════════════════════════════════════════
  (section learning-engine
    :desc "learning-engine 7 sub-engine — 从 worker pillar 迁入 (DC014 决策)"
    :migrated-from "worker pillar :: engine-cluster :: learning-engine"
    :remaining-in-worker "触发点 — BackgroundWorker impl + event subscribe + lifecycle mechanics"
    :primary-ownership-here "认知逻辑 / 推理 pipeline / 分析模型"
    :code-files-location "仍在 crates/missiond-daemon/src/engine/learning_engine/ (物理位置不变, 语义归属迁移)"
    :targets
      ["crates/missiond-daemon/src/engine/learning_engine/mod.rs"
       "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
       "crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs"
       "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
       "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
       "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
       "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs"
       "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
       "crates/missiond-daemon/src/engine/learning_engine/gen_engine.rs (Forge shell)"]
    :support-files ["gen_engine.rs — Forge domain shell, 零业务逻辑"]

    ;; ── 5.8.1 Decision sub ──
    (subsection decision
      :desc "决策识别 + 模式归纳"
      :purpose "识别问题是否可由 KB 直接解 / 需要 LLM 辅助 / 应升级人类; 从历史决策中归纳可复用模式"

      (path decision-cascade
        :lifecycle-style event-driven
        :target "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
        :phase-B-verified "2026-04-21 — 4 tier 全部已实现 (详 phase-B-scan-findings § A.3)"
        (ingress
          :source "agent question / decision need / escalation path"
          :entry-components ["decision_engine.rs"])
        (logic-core
          :cascade
            [(tier-1 kb-lookup
                :status "✓ implemented @ decision_engine.rs:155-180"
                :impl "kb_search_ranked() + dual scoring + confidence ≥0.5 threshold for auto-apply")
             (tier-2 gemini-consult
                :status "✓ implemented @ decision_engine.rs:210-260"
                :impl "call_gemini_for_flow() + JSON contract (answer/reasoning/action) + confidence 0.7")
             (tier-3 decision-slot
                :status "✓ implemented @ decision_engine.rs:290-340"
                :impl "request_execution_slot(slot-decision) + pty.send(120s timeout)")
             (tier-4 human-escalation
                :status "✓ implemented @ decision_engine.rs:360-380"
                :impl "agent_questions (target=master) + board_task priority 升级")])
        (egress
          :writes ["agent_questions" "question_routing_trace"]
          :reads ["agent_questions (pending)" "kb_entries (search_ranked/get_by_id)" "board_tasks" "board_task_notes"]
          :via-bus ["QuestionEvent::Resolved" "QuestionEvent::DecisionResolved" "TaskEvent::Created"]
          :llm-calls "Gemini (tier-2)"
          :slot-calls "slot-decision (tier-3 dispatch via pty.send)"
          :returns "decision result (tier / answer / needs-human)"))

      (path decision-harvest-generalization
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs"
        (ingress
          :source "已发生的 decision / outcome 样本"
          :entry-components ["decision_harvest.rs"])
        (logic-core
          (step s1 "回看历史决策 + 对应 outcome")
          (step s2 "抽取可复用模式 / policy 候选")
          (step s3 "将模式化结果回送给知识 / 策略侧使用"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "harvested decision patterns"
          :need-more-ground-truth "durable sink 未明")))

    ;; ── 5.8.2 Extraction sub ──
    (subsection extraction
      :desc "事件/会话 → 知识提取"
      :state-machine-crossref "extraction-phase FSM (见 5.11)"

      (path extraction-pipeline
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
        :phases-fsm "Idle → Sending → WaitingForIdleness → Complete (引用 5.11)"
        (ingress
          :source "session/event 落地后的知识提取触发"
          :entry-components ["extraction.rs"])
        (logic-core
          (step s1 "从 conversation / event material 中抽取候选知识")
          (step s2 "执行快速提取 (快 path)")
          (step s3 "执行深度提取 (slow path, 通常经 slot)")
          (step s4 "知识候选返回给上层消费链路 (KB writer)"))
        (egress
          :writes []
          :reads ["conversations" "conversation_messages"]
          :via-bus []
          :returns "knowledge extraction candidates"
          :need-more-ground-truth "精确 table sink 待 phase-B"))

      (path historical-scan-backfill
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
        (ingress
          :source "回溯扫描请求 / backlog catch-up"
          :entry-components ["historical_scanner.rs"])
        (logic-core
          (step s1 "按时间窗口或 session backlog 扫历史会话")
          (step s2 "把旧数据补进 extraction / analysis 队列")
          (step s3 "扫描结果交给后续提取器"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "historical scan result"))

      (path idle-explore-trigger
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
        (ingress
          :source "系统或 slot 空闲期触发"
          :entry-components ["idle_explorer.rs"])
        (logic-core
          (step s1 "检测系统/slot 的空闲窗口")
          (step s2 "空闲窗口内触发 exploration / extraction / backfill 任务")
          (step s3 "待探索对象交后续 path, 自己只做时机治理"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "idle exploration triggers")))

    ;; ── 5.8.3 Analysis sub ──
    (subsection analysis
      :desc "意图分析 + 时间轴分析"

      (path intent-analysis
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs"
        :唯一明确写表 "本 sub-engine 是 learning-engine 7 个中唯一在 memory v0.5.2 有明确 writer 声明的"
        :memory-cross-ref-binds-to "memory :: module conversation-logs :: binds-to intent_analyst"
        (ingress
          :source "session / turn 分析触发 + autopilot get_recent_intents 也调"
          :entry-components ["intent_analyst.rs"])
        (logic-core
          (step s1 "按 session 扫 conversation turns")
          (step s2 "用分析模型为一组 turns 识别 intent group")
          (step s3 "把 intent group 与 turn back-reference 写回 conversation 视角"))
        (egress
          :writes ["user_intents" "conversation_turns.intent_group_id"]
          :reads ["conversation_turns"]
          :via-bus []
          :memory-cross-ref ["conversation-logs"]
          :trait-writer "ConversationStore::insert_user_intent + 5 查询方法 (traits.rs:136-150)"
          :returns "intent analysis result"))

      (path timeline-analysis
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
        (ingress
          :source "时间轴 / long-range sequence 分析触发"
          :entry-components ["timeline_analyst.rs"])
        (logic-core
          (step s1 "按时间顺序聚合会话 / 事件 / 任务序列")
          (step s2 "寻找长期模式 / 关键转折点 / 上下文压缩机会")
          (step s3 "分析结果回给策略、记忆或展示侧"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "timeline analysis result"
          :need-more-ground-truth "precise table contract 待 phase-B")))

    (learning-engine-contract-summary
      :phase-B-verified "2026-04-21 — 7 sub R/W 矩阵全 confirmed (详 phase-B-scan-findings § A.1)"
      :writes-full-matrix
        (decision_engine     :writes ["agent_questions (answer/retry_count)" "question_routing_trace"])
        (decision_harvest    :writes ["kb_entries (policy:decision category)" "kb_update (confidence)"])
        (extraction          :writes ["realtime_forwarded_at (watermarks)" "deep_checkpoint" "slot_tasks" "conversation_task_id" "kb_update" "kb_forget"])
        (historical_scanner  :writes ["mark_habit_scanned" "daemon_state (last_habit_scan_at)"])
        (idle_explorer       :writes ["board_tasks (create, auto_execute=true)" "kb_remember" "daemon_state"])
        (intent_analyst      :writes ["user_intents" "conversation_turns.intent_group_id"])
        (timeline_analyst    :writes ["board_tasks (create)" "kb_remember (category=ops:insight)" "daemon_state"])
      :reads-full-matrix
        (decision_engine     :reads ["agent_questions (pending)" "kb_entries" "board_tasks" "board_task_notes"])
        (decision_harvest    :reads ["agent_questions (target=master)"])
        (extraction          :reads ["pending_realtime_messages" "conversation status" "pending_deep_analysis conversations" "conversation_messages" "daemon_state" "kb_list_low_utility"])
        (historical_scanner  :reads ["daemon_state" "conversation counts" "unscanned_conversations"])
        (idle_explorer       :reads ["daemon_state (last_idle_explore_at)" "kb_entries" "board_tasks" "beacons" "snapshots"])
        (intent_analyst      :reads ["conversation_turns (intent_coverage, turns_after)" "caller source"])
        (timeline_analyst    :reads ["timeline_stats (12h)" "timeline_search (errors)" "board_tasks (dedup)"])
      :llm-usage
        (gemini ["decision_engine (tier-2)" "decision_harvest (Few-Shot)" "timeline_analyst (12h insight JSON)"])
        (sonnet ["extraction (KB reflection)" "intent_analyst (intent pattern detection)"])
      :slot-usage
        ["decision_engine → slot-decision (tier-3)"
         "extraction → request_default_slot + request_execution_slot (300-900s)"
         "historical_scanner → MEMORY_SLOW_SLOT_ID"]
      :via-bus-produces
        ["QuestionEvent::Resolved/DecisionResolved (decision)"
         "SlotEvent::TaskDispatched (extraction/scanner)"
         "MemoryEvent::PhaseChanged (extraction)"
         "MemoryEvent::IntentAnalyzed (intent_analyst)"
         "BoardEvent::StatusChanged (idle_explorer)"
         "SystemEvent::InsightGenerated (timeline)"]
      :sub-engine-details
        (idle_explorer :8-categories ["一致性" "陈旧" "信标" "重复" "聚合" "巩固" "状态" "影子回放"])
        (intent_analyst :limits "15 轮批量 (防翻页 bug)" :patterns ["stuck_retry" "architecture_explore" "refactor_shift" "scope_creep"])
        (historical_scanner :interval "4h 周期")
        (timeline_analyst :interval "12h 周期")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.9 Flow-Engine v1 — Project Lifecycle Phases
  ;; ══════════════════════════════════════════════════════════
  (section flow-engine-v1-project-lifecycle
    :desc "flow-engine v1: project lifecycle phases 推进 (从 worker 迁入, DC014)"
    :migrated-from "worker pillar :: engine-cluster :: intent-engine :: board-phase-engine"
    :target "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
    :distinguish-from-v2 "v2 是 YAML declarative general-purpose runtime, 归 worker pillar; v1 是 autopilot-driven project-lifecycle phases, 归本 pillar"
    :state-machine-crossref "engineering-phase FSM (见 5.11)"

    (path board-phase-engine
      :lifecycle-style spawned
      :phase-B-verified "2026-04-21 — 7 phase 全部已实现 + decision_harvest closure loop (详 phase-B-scan-findings § A.4)"
      :completeness "✓ FULL — Investigate/ConsultGemini1/Plan/ConsultGemini2/Execute/Finalize/Done 全 transition 有代码 + artifact 存储"
      (ingress
        :source "autopilot claim 后的 board task phase progression"
        :entry-components
          ["crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
           "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs (worker pillar 触发)"])
      (logic-core
        (step s1 "按 board lifecycle phase 把任务从 Investigate / ConsultGemini1 / Plan / ConsultGemini2 / Execute / Finalize / Done 推进")
        (step s2 "每 phase 转换需对应 mission_submit_phase_result 提交的 artifactType (investigation_report / gemini_advice_1 / execution_plan / gemini_advice_2 / execution_result / commit_hash)")
        (step s3 "ConsultGemini1/2 阶段 daemon 直调 Gemini (非 slot), call_gemini_for_flow + JSON 契约解析")
        (step s4 "Investigate/Plan/Execute/Finalize 阶段走 slot (pty.send)")
        (step s5 "Finalize→Done 自动 trigger decision_harvest → policy:decision KB 条目沉淀")
        (step s6 "re-entry guard 防并发; phase 推进结果与失败状态回写 board_tasks.flow_phase"))
      (transitions-full-implementation
        ((Investigate -> ConsultGemini1 :at "flow_engine.rs:150-200" :artifact "investigation_report")
         (ConsultGemini1 -> Plan         :at "flow_engine.rs:200-250" :artifact "gemini_advice_1")
         (Plan -> ConsultGemini2         :at "flow_engine.rs:250-300" :artifact "execution_plan")
         (ConsultGemini2 -> Execute      :at "flow_engine.rs:300-350" :artifact "gemini_advice_2")
         (Execute -> Finalize            :at "flow_engine.rs:350-400" :artifact "execution_result" :note "with error handling + retry guard")
         (Finalize -> Done               :at "flow_engine.rs:400-450" :artifact "commit_hash + decision_harvest trigger")))
      (egress
        :writes ["board_tasks.status" "board_tasks.flow_phase" "board_tasks.flow_context" "board_tasks.flow_artifacts" "board_task_notes"]
        :reads ["board_task (flow_phase, flow_context, questions, notes)" "flow_template definitions"]
        :via-bus ["TaskEvent::PhaseChanged"]
        :llm-calls "Gemini (ConsultGemini1/2 direct, 非 slot)"
        :slot-calls "pty.send for Investigate/Plan/Execute/Finalize phases"
        :memory-cross-ref ["board"]
        :tools-surface "mission_submit_phase_result (sysinfra domain via misc handler)"
        :returns "board phase progression result"
        :closure-with-learning-engine "Finalize→Done auto-harvests decisions into policy:decision KB 条目 (→ learning-engine decision_harvest 闭环)"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.10 Action-Instruction Actor (Future)
  ;; ══════════════════════════════════════════════════════════
  (section action-instruction-actor
    :desc "directive / plan / workflow 编译器 actor — store API 已就绪, runtime actor 尚未实现"
    :status "manager-surfaces-code-aligned; compiler/distiller actors pending"

    (actor directive-compiler
      :status "architecture-designed; code-alignment pending"
      :desc "把用户 utterance (自然语言) 编译成 lisp-level directive prescription"
      :future-target "crates/missiond-daemon/src/intent_layer/directive_compiler.rs (TBD)"
      :input "用户话 / 系统指令"
      :output "directive record → directive 表 (memory :: directive-layer writer)"
      :dependencies ["LLM (gemini/sonnet chat)" "context assembly" "KB (convention 查询)"]
      (ingress
        :source ["user utterance" "mission_directive(action=compile) dry-run/draft surface" "autopilot directive capture"]
        :required-context ["project/session" "raw utterance" "references_json"])
      (logic-core
        (step s1 "assemble context from project, KB conventions, recent intents, and current board state")
        (step s2 "ask LLM to emit lisp-level directive sexp plus references_json")
        (step s3 "validate sexp reader balance and directive DSL shape")
        (step s4 "insert directive(status=draft/refining, version=1, compiler_model)")
        (step s5 "if confidence/high-trust policy allows, mark approved; otherwise create review question"))
      (egress
        :writes ["directive"]
        :emits ["QuestionEvent::Created when approval needed (future)"]
        :returns "directive id/version/status"))

    (actor plan-compiler
      :status "architecture-designed; code-alignment pending"
      :desc "从 directive 生成可执行 DAG / FSM / supersede chain"
      :future-target "crates/missiond-daemon/src/intent_layer/plan_compiler.rs (TBD)"
      :input "directive record"
      :output "plan record → plan 表"
      :dependencies ["directive-compiler 输出" "board_tasks 模板"]
      (ingress
        :source ["directive status=approved" "mission_plan(action=compile) dry-run/draft surface" "board task requiring plan"]
        :required-context ["directive id/version" "board_task_id or task draft" "available execution surfaces"])
      (logic-core
        (step s1 "load approved directive and optional board task")
        (step s2 "choose target execution surface: board lifecycle, flow-engine-v2 YAML, skill workflow, pty slot, or mixed DAG")
        (step s3 "compile executable plan sexp and compute sexp_hash")
        (step s4 "if an older active plan exists for board_task_id, create new version and mark old plan superseded")
        (step s5 "insert plan(status=draft/awaiting_approval, compiled_from=directive sexp_hash)")
        (step s6 "approval gate moves plan to approved; execution adapter later updates executing/succeeded/failed"))
      (egress
        :writes ["plan" "board_tasks.source_directive_id when bound"]
        :reads ["directive" "board_tasks" "workflow templates as hints"]
        :returns "plan id/version/status/execution target"))

    ;; ── actor plan-dag-scheduler moved to L2 shard ──
    ;; Full content moved to .missiond/v2/intent-plan-dag.lisp
    ;; (wave 15 task 02 L2 shard split per architecture-dsl.lisp ::
    ;;  l2-shard-split-plan :: shard intent-plan-dag).
    (actor plan-dag-scheduler
      :status code-aligned-partial
      :file-ref ".missiond/v2/intent-plan-dag.lisp"
      :shard-section "actor plan-dag-scheduler"
      :section-ids
        ["intent-layer.actor.plan-dag-scheduler"
         "intent-layer.plan-dag-runtime-v2"
         "intent-layer.plan-dag-runtime-v2.node-lifecycle"
         "intent-layer.plan-dag-runtime-v2.failure-policy"
         "intent-layer.plan-dag-runtime-v2.execution-event-decision"
         "intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"]
      :flow-cross-ref "flow pillar :: F-intent-alignment-plan-execution-loop :: s6 :: dag-scheduler (also moved to shard)"
      :note "minimal-node-schema / node-fsm-enum / claim-lease-protocol-binding / 11-stage logic-core / anti-patterns / open-design-questions live in shard; section-ids stable per L2 plan rule-1")

    (actor workflow-distiller
      :status "architecture-designed; code-alignment pending"
      :desc "从成功 plan 蒸馏出可复用 workflow 模板"
      :future-target "crates/missiond-daemon/src/intent_layer/workflow_distiller.rs (TBD)"
      :input "已完成的 plan + outcome"
      :output "workflow record → workflow 表"
      :dependencies ["plan 历史" "match_rules 算法"]
      (matching-policy
        :status "architecture-designed; code-alignment pending"
        :match-rules-shape
          ((intent-signature "normalized utterance intent + verbs + object type + affected pillars")
           (scope-signature "project_id/global + repo language/framework + domain tags")
           (execution-shape "preferred surface: board / flow-engine-v2 / skill / pty / mission_execution")
           (constraints "risk level, required approvals, max runtime, allowed tools")
           (negative-rules "anti-patterns / do-not-use surfaces / known failure contexts"))
        :scoring
          ((semantic-similarity 0.40)
           (scope-match 0.20)
           (success-rate 0.15)
           (recency 0.10)
           (cost-fit 0.10)
           (human-pin 0.05))
        :thresholds
          ((auto-apply "score >= 0.86 and success_rate >= 0.8 and no approval-required constraint")
           (suggest-only "0.62 <= score < 0.86")
           (no-match "score < 0.62 or negative-rule hit"))
        :lru-policy "evict or down-rank workflows with low score, old last_used_at, high avg_cost_usd, or repeated failure; never evict human-pinned workflows automatically")
        :feedback-loop "record_execution updates executions/success_count/failure_count/avg_cost_usd/last_used_at and can adjust match_rules after human approval")
      (ingress
        :source ["plan status=succeeded" "mission_workflow(action=distill) dry-run/draft surface" "periodic top successful plan scan"]
        :required-context ["plan sexp" "directive utterance" "execution outcome/cost"])
      (logic-core
        (step s1 "load succeeded plan, source directive, and observed outcome/cost")
        (step s2 "normalize plan into reusable workflow sexp by removing task-specific constants")
        (step s3 "derive match_rules: intent-signature, scope-signature, execution-shape, constraints, negative-rules")
        (step s4 "score against existing workflows; upsert new template, merge into existing, or record execution")
        (step s5 "apply LRU/down-rank policy after updating executions/success_count/failure_count/avg_cost_usd/last_used_at")
        (step s6 "emit review question if automatic rule mutation would broaden a workflow's applicability"))
      (egress
        :writes ["workflow"]
        :reads ["plan" "directive"]
        :returns "workflow id/name/match_rules/stats"))

    (mcp-tools-manager-surface
      :mission_directive
        (:status "code-aligned partial; compile dry-run, list/get/approve/archive/version_chain full"
         :actions ["compile" "list" "get" "approve" "archive" "version_chain"]
         :owner "tools pillar shell + intent-layer directive-compiler actor")
      :mission_plan
        (:status "code-aligned partial — compile dry-run/sonnet, list/get/by_task/approve/mark/supersede full, execute bridge + internal v0 单节点 + record_evidence sidecar; internal v1 完整 11-stage scheduler pending — 详 actor plan-dag-scheduler"
         :actions ["compile" "list" "get" "by_task" "approve" "mark" "supersede" "execute" "record_evidence"]
         :owner "tools pillar shell + intent-layer plan-compiler v0 / plan-runner v0 (单节点) / 未来 plan-dag-scheduler (多节点)")
      :mission_workflow
        (:status "code-aligned partial; list/get/match/record_execution full, apply read-only, distill/compile_methodology dry-run, run_methodology not_implemented"
         :actions ["list" "get" "match" "apply" "distill" "record_execution" "compile_methodology" "run_methodology"]
         :owner "tools pillar shell + intent-layer workflow-distiller actor"))

    (mcp-tools-existing-crossref
      :mission_execution "worker v0.5 I007 相关 — 12 actions handler/tool 已实现 (agent-execution-coordination v0.5.2 manager-interface)"
      :cross-ref-memory "memory v0.5.2 helper agent-execution-coordination"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.11 State Machines Owned — engineering-phase + extraction-phase
  ;; ══════════════════════════════════════════════════════════
  (section state-machines-owned
    :desc "归本 pillar 的 FSM — 元层语义"
    :authority "intent-pillar-state-machines.lisp 老图"

    (state-machine engineering-phase
      :target "crates/missiond-core/src/types/board.rs (enum EngineeringPhase)"
      :owner-path "本 pillar :: flow-engine-v1-project-lifecycle"
      :consumed-by ["flow-engine v1 :: board-phase-engine" "mission_submit_phase_result"]

      (states
        (Investigate "调查阶段 — agent 研究代码 / 需求")
        (Consult     "审查阶段 — Gemini / 专家审核调查结果")
        (Plan        "计划阶段 — 生成执行计划")
        (Execute     "执行阶段 — 实际动工")
        (Finalize    "收尾阶段 — commit / review / 复盘"))

      (transitions
        (Investigate -> Consult   :trigger "context-gathered")
        (Consult     -> Plan      :trigger "review-complete")
        (Plan        -> Execute   :trigger "plan-approved")
        (Execute     -> Finalize  :trigger "implementation-done")
        (Finalize    -> Investigate :trigger "issues-found")))

    (state-machine extraction-phase
      :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
      :owner-path "本 pillar :: learning-engine :: subsection extraction :: path extraction-pipeline"

      (states
        (Idle              "空闲等待")
        (Sending           "发送 extraction 请求 (通常派 slot)")
        (WaitingForIdleness "等待 slot 回到 idle")
        (Complete          "提取完成"))

      (transitions
        (Idle              -> Sending            :trigger "extraction-triggered")
        (Sending           -> WaitingForIdleness :trigger "content-sent")
        (WaitingForIdleness -> Complete          :trigger "slot-idle")
        (WaitingForIdleness -> Idle              :trigger "timeout")
        (Complete          -> Idle               :trigger "reset")))

    (related-fsm-other-pillars
      :note "其他 FSM 归属非本 pillar"
      (pty-session         :归属 "worker pillar :: section pty :: subsection pty-state-machine (8 states + 14 transitions)")
      (board-task          :归属 "memory pillar :: module board (BoardTaskStatus enum)")
      (task                :归属 "memory pillar :: module system-support")
      (question            :归属 "memory pillar :: module system-support")))

  ;; ══════════════════════════════════════════════════════════
  ;; Design Principles
  ;; ══════════════════════════════════════════════════════════
  (design-principles
    (principle-1 "intent-layer 管 prescriptive: 系统应该怎么做")
    (principle-2 "memory 管 factual: 系统现在记得什么 / 代码真实状态")
    (principle-3 "worker 管 runtime: 系统在跑什么 (timer / dispatch / event)")
    (principle-4 "execution-log 属 operational state (memory v0.5.2 helper), 不和 methodology 混写")
    (principle-5 "forge + mission_intent = 本 pillar 对外两大出口")
    (principle-6 "双重归属 = 触发/语义 拆分 (lisp_survey / arch_maintenance 模式)")
    (principle-7 "认知/推理逻辑 归本 pillar; 运行时/调度 机制 归 worker pillar"))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (IL-T001 …)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (IL-T001 :status RESOLVED :resolved-at "2026-04-21"
             :finding "7 sub R/W 矩阵全 confirmed. 详 phase-B-scan-findings-2026-04-21.md § A.1 + learning-engine-contract-summary 已补 full matrix")
    (IL-T002 :status "architecture-designed-code-alignment-pending"
             :note "3 actor (directive-compiler / plan-compiler / workflow-distiller) 架构已设计并挂到 F-directive-plan-workflow-compile; 真实实现排期进入代码对齐阶段")
    (IL-T003 :status "code-aligned-manager-surfaces; actors-pending"
             :note "mission_directive / mission_plan / mission_workflow 已挂到 F-directive-plan-workflow-compile; MCP surface/handler 已实现 read/control/draft persistence/execute bridge, LLM compiler/distiller actor 仍待代码对齐")
    (IL-T004 :status "code-aligned"
             :note "global-claudemd-manager + mission_global_instruction(read/edit/reload) 已代码同构; read/edit full, reload manual-reload-required")
    (IL-T005 :status "code-aligned"
             :note "mission_execution (agent-execution-coordination v0.5.2, 12 actions) 已由 4ab7994 代码对齐; ExecutionEvent 域与发射也已代码对齐")
    (IL-T006 :status "pending-external-scan"
             :note "forge-daemon/src/intent_graph.rs 在外部仓 ~/Projects/jarvis-forge, 本次 phase-B 未扫")
    (IL-T007 :status RESOLVED :resolved-at "2026-04-21"
             :finding "4 tier 全部已实现 (kb-lookup/gemini-consult/decision-slot/human-escalation). 详 § A.3 + decision-cascade path 已补 :status implemented")
    (IL-T008 :status "architecture-designed-code-alignment-pending"
             :note "workflow-distiller match_rules + LRU 策略已补 matching-policy: scoring/threshold/LRU/feedback loop; actor 实现待代码对齐")
    (IL-T009 :status RESOLVED :resolved-at "2026-04-21"
             :finding "flow-engine v1 EngineeringPhase 7 phase 全部实现 + 到 Finalize→Done 自动 trigger decision_harvest 形成闭环. 详 § A.4 + board-phase-engine path 已补 transitions-full-implementation 表")
    (IL-T010 :status "architecture-designed-code-alignment-pending"
             :note "methodology-lisp → executable-yaml 已定为 F-methodology-to-executable-compile: Lisp SSOT, 生成 YAML 副本, 再由 mission_flow_run 执行; compiler/tool 实现待代码对齐")
    (IL-T011 :status "architecture-decided"
             :note "当前继续嵌入 missiond-daemon,但按 intent_layer/* clean module boundary 实现; 只有当 forge/多进程复用成为现实需求时再抽独立 crate"))
  ;; close sections 5.2-5.11 (6 unclosed) + close pillar
  ))))))
