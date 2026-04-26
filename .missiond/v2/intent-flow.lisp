;; ═════════════════════════════════════════════════════════════
;; MissionD — Flow Pillar (phase-C recursive-contract v0.7)
;; 目标: 跨 pillar 的 narrative — 串 memory 状态 + worker 计算 + tools 端点 + intent-layer 元层
;; 底稿: gptpro intent-flow.lisp (179 行) + v2/intent.lisp 详细 flows catalog
;;       + intent-flows.lisp 老图 10 简洁 flow
;; 定位: 本 pillar 无代码 ownership, 只有 narrative — 描述"什么时候什么顺序做什么"
;; ═════════════════════════════════════════════════════════════

(pillar flow
  :version "v0.7"
  :status "phase-C recursive architecture contract 2026-04-26 — trigger/state → ordered cross-pillar stages → egress; directive/incident/methodology/capability-usage flows + project-root spawn cwd contract designed; F-intent-alignment-plan-execution-loop + F-execution-log-governance + F-scoped-commit-handoff + F-workstation-dispatch-policy + F-methodology-to-executable-compile + F-capability-usage-monitoring + actor v0 + PLAN DAG runtime v2 + unified-entry pipeline v0 internal helper + evidence-collector typed helper 全部 code-aligned partial (详 wave-13 anchors); wave 14 task 01/02/03/04 (commits 00cbc1d/2e7789a/96842cd/338a3fb): file-first writer integration / PlanNodeStateChanged variant + live ref / review-gate auto-create v1 (policy enum manual|emit_question|off) / unified-entry pipeline v1 (file-first + review-gate + scheduler args 转发) 全部 code-aligned (详 wave-14 anchors via intent-pillar-source-index.lisp); 完整 11-stage PLAN DAG / semantic compiler / scoped commit daemon enforce / event bus live subscription / 4 项 v0 non-goal 自动化 仍 pending"
  :predecessor "drafts/gptpro/intent-flow.lisp (179 行 starter) + v2/intent.lisp flow pillar 详细 catalog"
  :target-path ".missiond/v2/intent-flow.lisp"

  :actual-state-sources
    [".missiond/v2/intent.lisp :: pillar flow (最详细, 已有 catalog + stages)"
     ".missiond/intent-flows.lisp (v1 老图 10 简洁 flow 定义)"
     ".missiond/intent-pillar-engines.lisp (autopilot tick + flow-engine-v2 runtime)"
     ".missiond/v2/intent-memory.lisp v0.5.5 (board state-machine + directive artifacts + execution protocol + capability usage read-model + directive manager surfaces)"
     ".missiond/v2/intent-worker.lisp v0.5 (执行 mechanics + pty FSM + xjp-router/mission_execution + project-root spawn cwd)"
     ".missiond/v2/intent-tools.lisp v0.7 (83 actual tools classified + execution/capability usage/directive-plan-workflow/global-instruction surfaces + project-root spawn cwd)"
     ".missiond/v2/intent-intent-layer.lisp v0.4 (元层 ownership + methodology compile + capability governance)"
     "2026-04-25 code scan: handlers/compute/{task_delegate,compute_slot,flow_run}.rs + handlers/knowledge/{kb,cascade,skill}.rs + handlers/comm/conversation.rs::mission_embedding_ops + engine/{flow, intent_engine/workflow_executor}.rs"]

  :design-correction-sources
    ["gptpro intent-flow.lisp 9 flows + 3 future — 吸收为骨架"
     "v2/intent.lisp flow pillar 详细 stages :at / :writes / :emits — 采纳 stage 标记模式"]

  :principle
    (:memory "状态 (snapshot) — 记什么是什么")
    (:worker "机制 (engine) — 做怎么做")
    (:tools  "端点 (surface) — 谁能调什么")
    (:intent-layer "元层 (prescription) — 应该怎么做")
    (:flow   "编排 (choreography) — 串什么时候什么顺序做什么")

  :naming-convention
    (:stage-id "s1 / s2 / ...")
    (:at-target "pillar-X :: section/module :: path/component (跨 pillar 跳点)")
    (:writes    "产生什么数据变动")
    (:emits     "产生什么 DomainEvent (可选)")
    (:tools-consumed "mission_X (若此 stage 由某 MCP tool 触发或输出)")

  ;; ══════════════════════════════════════════════════════════
  ;; phase-A-decisions
  ;; ══════════════════════════════════════════════════════════
  (phase-A-decisions
    (Q-F1
      :question "flow pillar 是否拥有代码?"
      :decision "不拥有 — 本 pillar 是 narrative 层, 代码 ownership 在其他 4 pillar"
      :effect "每 stage 的 :at 跨 pillar 引用, 本文件纯是'跨 pillar 故事线'")

    (Q-F2
      :question "83 tools 是否每个都有对应 flow?"
      :decision "不 — 多数 single-step read/write 不值得独立 flow. 仅'有显著多 stage 跨 pillar 语义'的 tool 配 flow"
      :estimated-count "约 15-20 flow 覆盖核心 tool. 其余 tool 的 :flow-ref 指向 'trivial-single-step' 或共享已有 flow"
      :effect "本文件 flow 数量 ~20 而非 83")

    (Q-F3
      :question "flow 如何分类?"
      :decision "按 domain 分 6 category: board / conversation / cognition / context / infrastructure / workflow-runtime"
      :rationale "便于 tools pillar 的 :flow-ref 索引")

    (Q-F4
      :question "flow-engine-v2 与本 pillar 关系?"
      :decision "flow-engine-v2 是 runtime (worker pillar); 本 pillar 是 definition (narrative + YAML spec). 两者分层"
      :cross-ref "intent-layer pillar :: section workflows :: kind executable — 文件 SSOT"
      :cross-ref "worker pillar :: section engine-cluster :: subsection flow-engine-v2 — runtime")

    (Q-F5
      :question "对 tools v0.1 的 :flow-ref pending 字段, 本 v0.1 提供什么?"
      :decision "提供 flow catalog + tool→flow 映射表 (section tool-backed-flows-index)"
      :effect "tools v0.2 可按本 index 填 :flow-ref 具体值")

    (Q-F6
      :question "从 tools 反推 flow 时, 第一批应该补哪些?"
      :decision "先补真实代码已存在但 v2 flow 仍标 pending/future 的主干: task_delegate auto-provision / skill workflow executor / cascade execution"
      :rationale "三者都不是 trivial-single-step: 它们跨 tool schema、daemon handler、worker runtime、memory state、event-bus/audit, 且已有代码真相可锚定"
      :effect "本 v0.2 不扩大到 83 tool 全量重写, 只把高价值 flow 从 pending 提升为 named flow"))

  (purpose "跨 pillar 编排 — 把 memory 状态 + worker 计算 + tools 端点 + intent-layer 元层 串成 end-to-end narrative")

  (recursive-architecture-contract
    :shape "pillar = ingress → logic-core → egress; flow = ingress(trigger/state) → logic-core(ordered cross-pillar steps) → egress(writes/emits/returns/next-flow)"
    :unit "flow 是跨 pillar 的分子; stage 是 flow 内的原子; stage 内不展开 owner pillar 的私有实现细节"
    :rule-1 "所有 flow 必须按执行顺序写 step, 每个 step 标 :at owner pillar"
    :rule-2 "flow 只描述 choreography, 不拥有 worker runtime / memory schema / tool schema"
    :rule-3 "从 tools 反推 flow 时, 先判断 named-flow / shared-flow / trivial-single-step, 禁止 83 tools 机械生成 83 flows"
    :rule-4 "一个 flow 的 egress 必须列 writes / emits / returns / downstream 至少一种")

  (pillar-ingress
    (entry-1 "tools pillar 调用 → 启动 flow 的 trigger")
    (entry-2 "event-bus 事件 → 订阅式 flow 启动")
    (entry-3 "timer / autopilot tick → 周期 flow")
    (entry-4 "外部 (用户 / agent) 手动触发"))

  (pillar-core
    :contract "flow catalog 只做跨 pillar 顺序叙事: trigger/state 进入, ordered stage 串联, 产物从 egress 指向 owner pillar"

    (function flow-authoring-contract
      (ingress
        :source ["tools :flow-ref pending" "event-bus event" "timer/autopilot" "human methodology"])
      (logic-core
        (step s1 "确认 trigger 与 preconditions")
        (step s2 "按真实执行顺序列 stages")
        (step s3 "每个 stage 标 owner pillar 与读写/事件/工具")
        (step s4 "把 egress 回填到 tools / worker / memory / intent-layer 对应 cross-ref"))
      (egress
        :writes "本 lisp 的 named flow spec"
        :updates ["tools pillar :flow-ref" "worker pillar cross-ref" "intent-layer workflow ownership"]))

    (function tool-to-flow-classification
      (ingress
        :source "tools pillar 83 endpoints")
      (logic-core
        (step s1 "若 tool 跨多个 pillar 且有顺序状态推进 → named-flow")
        (step s2 "若多个 tool 共用同一链路 → shared-flow")
        (step s3 "若单纯读写/配置查询 → trivial-single-step")
        (step s4 "若代码真相未查清 → pending-with-ground-truth-question"))
      (egress
        :to "section tool-backed-flows-index"
        :review "need-more-ground-truth"))

    (function executable-flow-bridge
      (ingress
        :source "$MISSIOND_HOME/flows/*.yaml / mission_flow_run")
      (logic-core
        (step s1 "intent-layer owns executable YAML definition")
        (step s2 "worker pillar flow-engine-v2 loads and runs YAML")
        (step s3 "flow pillar narrates node order and cross-pillar effects"))
      (egress
        :to-worker "F5-flow-engine-v2-node-execution"
        :to-intent-layer "workflows :: executable"))

    (function methodology-flow-bridge
      (ingress
        :source ".missiond/workflows/*.lisp")
      (logic-core
        (step s1 "intent-layer owns human-readable methodology")
        (step s2 "flow pillar references methodology as narrative source")
        (step s3 "future forge path may compile methodology lisp → executable YAML"))
      (egress
        :to-intent-layer "workflows :: methodology"
        :future "methodology lisp → executable YAML compiler"))

    (core-invariants
      (core-1 "flow = 多 stage narrative, 每 stage :at 跨 pillar 跳点")
      (core-2 "flow 无代码, 只有 lisp 描述 + YAML/executable 引用")
      (core-3 "tools 的 :flow-ref 是反向指向 — tool → flow → 跨 pillar stages")
      (core-4 "flow-engine-v2 是 executable kind 的 runtime (worker pillar 实现)")
      (core-5 "methodology-lisp 是人类方法论 flow, 不直接机器执行")))

  (pillar-egress
    (egress-1 "→ tools pillar: 提供 flow-backed tool 的 :flow-ref 具体值 (供 tools v0.2 填)")
    (egress-2 "→ worker::flow-engine-v2: 提供 executable YAML flow 定义 ($MISSIOND_HOME/flows/*.yaml)")
    (egress-3 "→ intent-layer :: workflows :: methodology: 提供人类方法论 flow lisp (.missiond/workflows/*.lisp)")
    (egress-4 "→ human review: 本文件本身是跨 pillar action 可读 narrative")

    (cross-pillar-notes
      (memory
        :usage "每 stage 的 :writes / :reads 映射到 memory pillar module"
        :no-code-owned "本 pillar 不拥有 memory schema")
      (worker
        :usage "每 stage 的 :at (执行点) 可能指向 worker pillar 具体 path"
        :flow-engine-v2-runtime "executable kind 由 worker::flow-engine-v2 执行")
      (tools
        :relationship "tool 是 flow 的 entry trigger 或 stage 出口"
        :backref "tools v0.1 的 :flow-ref pending 待本 catalog 填充")
      (intent-layer
        :ownership-split "methodology lisp 文件归 intent-layer; executable YAML 定义也归 intent-layer (两 kind workflows)"
        :本-pillar-角色 "narrative 索引与跨 pillar 链路描述")))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.1 Category: Board Flows (任务生命周期)
  ;; ══════════════════════════════════════════════════════════
  (category board-flows
    :desc "board_tasks 为中心的任务流"

    (flow F1-board-task-main-lifecycle
      :desc "任务创建 → autopilot claim → slot 派发 → 完成"
      :triggers ["mission_board_create" "mission_board_decompose (decomposed child)" "auto_execute=1 定时扫描"]
      :stages
        ((s1 create
            :at "memory pillar :: module board :: mcp-board-lifecycle"
            :writes "board_tasks status=open"
            :emits "BoardTaskCreated"
            :tools-consumed ["mission_board_create"])
         (s2 scan-decide
            :at "worker pillar :: section engine-cluster :: intent-engine :: autopilot-tick"
            :reads "board_tasks WHERE auto_execute=1 AND status=open"
            :decides "是否 claim + 派给哪个 slot/worker")
         (s3 atomic-claim
            :at "memory pillar :: module board (SQL CAS)"
            :writes "status=running + claim_executor_id + lease_expires_at"
            :atomicity "SQL CAS open→running"
            :emits "BoardTaskClaimed + BoardTaskStatusChanged"
            :tools-consumed ["mission_board_claim (手动 claim 路径)"])
         (s4 execute
            :at "worker pillar :: section pty :: subsection slot-orchestrator / 或 flow-engine-v2"
            :action "实际执行 — 若 flow_template 非空走 flow-engine-v2 (→ F5)"
            :side-effects "autopilot.save_prompt_snapshot → prompt_snapshots"
            :flow-ref "若走 flow-engine-v2 → F5-flow-engine-v2-node-execution")
         (s5 report-completion
            :at "memory pillar :: module board"
            :writes "status=done/failed + claim_executor_id=NULL + lease=released"
            :emits "BoardTaskStatusChanged")
         (s6 downstream-cascade
            :at "worker pillar :: section engine-cluster :: autopilot-tick"
            :action "检查 depends_on 的下游 → unblock 或 retry-cascade"
            :optional true))
      :alternative-paths
        ((lease-recovery
            :trigger "autopilot tick 发现 lease_expires_at < now() 且 status=running"
            :at "worker pillar :: autopilot"
            :action "调 BoardStore::recover_stale_running_tasks → status=open + claim 清除"
            :rationale "executor 崩溃不留僵尸任务"))
      :tools-backref ["mission_board_create" "mission_board_claim" "mission_board_update" "mission_board_retry"])

    (flow F2-board-task-decompose
      :desc "父任务校验 → decompose prompt → 指定 slot 执行 → slot 回写子任务 DAG"
      :triggers ["mission_board_decompose(task_id, slot_id, hints)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/knowledge/board.rs :: mission_board_decompose"
      :stages
        ((s1 validate-parent
            :at "tools/memory boundary :: board handler"
            :reads "board_tasks by task_id + child count"
            :guards ["parent status == open" "parent has no existing subtasks"]
            :tools-consumed ["mission_board_decompose"])
         (s2 assemble-decompose-prompt
            :at "board handler"
            :reads ["parent title/description/category/priority/project" "state.skills.build_context(task.title)" "optional hints"]
            :action "生成要求 slot 调 mission_board_create + mission_board_note_add 的结构化 prompt")
         (s3 create-submit-task
            :at "crate::state::submit_task(role=coder)"
            :writes "legacy tasks queue entry for decompose execution")
         (s4 bind-target-slot
            :at "task store"
            :writes "submit task slot_id")
         (s5 immediate-dispatch-if-idle
            :at "worker pillar :: section pty :: subsection slot-orchestrator"
            :action "若目标 slot Idle, state.pty.send_fire_and_forget(decompose_prompt)"
            :emits "SlotEvent::TaskDispatched{purpose=decompose}")
         (s6 parent-progress-note
            :at "memory pillar :: board notes"
            :writes "父任务 progress note: decompose submit_task_id + slot_id")
         (s7 slot-writes-child-dag
            :at "memory pillar :: module board"
            :action "slot 按 prompt 调 mission_board_create / mission_board_note_add"
            :writes "多个 child board_tasks rows (parent_id + depends_on JSONB)"
            :emits "BoardTaskCreated (每子任务一次)"))
      :result "父任务 → DAG of children with dependency links"
      :tools-backref ["mission_board_decompose"]
      :open-questions
        ["daemon 当前不验证 slot 产出的 child DAG 是否覆盖父任务全部需求"
         "默认 slot_id=slot-coder-1 是否应改为动态 slot selection / task_delegate 复用"])

    (flow F3-agent-question-block-resume
      :desc "Agent 卡住 → 提问 → task 被 block → 回答后 auto-unblock"
      :triggers ["mission_question create with task_id"]
      :stages
        ((s1 question-create
            :at "memory pillar :: module system-support :: agent-questions"
            :writes "agent_questions status=pending"
            :side-effect "CAS UPDATE board_tasks SET status=blocked WHERE id=task_id"
            :tools-consumed ["mission_question(action=create)"])
         (s2 human-or-agent-answer
            :at "用户手动 / Claude Code / 其他 agent"
            :writes "agent_questions status=answered + answer text"
            :tools-consumed ["mission_question(action=answer)"])
         (s3 auto-unblock
            :at "memory pillar :: module board (same txn as s2)"
            :trigger-check "task 所有 pending 问题是否全部 answered/dismissed"
            :writes "board_tasks status=blocked→open (仅当最后一个问题解决时)"
            :emits "QuestionEvent::Resolved"))
      :status "✓ auto-unblock 已实现 — v0.4.12 修正 (非 gap)"
      :tools-backref ["mission_question"])

    (flow F4-autopilot-tick-pipeline
      :desc "autopilot 每 60s 的完整 tick — 多子流程依次跑"
      :triggers ["autopilot timer (60s, main.rs:1076-1096)"]
      :stages
        ((s1 memory-scheduler
            :at "worker pillar :: section engine-cluster :: intent-engine :: memory-scheduler-queue"
            :action "扫 pending / reminder / 内部待推进状态")
         (s2 extraction-check
            :at "intent-layer pillar :: section learning-engine :: subsection extraction (检查)"
            :action "检查 extraction / learning 相关执行态")
         (s3 board-task-dispatch
            :at "worker pillar :: autopilot-tick"
            :action "复用 F1 s2-s4 — list open tasks → CAS claim → 选 slot/worker")
         (s4 flow-progression
            :at "worker pillar :: autopilot (推进) + intent-layer :: flow-engine-v1 (board-phase-engine)"
            :action "推进 board_tasks 挂的 flow 状态 (v1 lifecycle phases)")
         (s5 supervision-check
            :at "worker pillar :: section pty :: subsection slot-orchestrator :: slot-manager-runtime-authority"
            :action "lease recovery / stale task / zombie slot 回收"))
      :tools-backref [])

    (flow F-board-submit-phase
      :desc "Flow 任务阶段产出物提交 → 下一阶段推进"
      :triggers ["mission_submit_phase_result"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/sysinfra/misc.rs :: mission_submit_phase_result"
      :stages
        ((s1 load-and-validate-flow-task
            :at "sysinfra misc handler + memory pillar :: board"
            :reads "board_tasks by task_id"
            :guards ["flow_phase is not null" "artifact_type matches current EngineeringPhase"]
            :tools-consumed ["mission_submit_phase_result"])
         (s2 persist-artifact
            :at "memory pillar :: board"
            :writes "board_tasks.flow_context.{investigation_report|execution_plan|execution_result|commit_hash}")
         (s3 advance-phase
            :at "memory pillar :: board + intent-layer engineering-phase FSM"
            :writes "board_tasks.flow_phase 下一 state")
         (s4 progress-note
            :at "memory pillar :: board notes"
            :writes "phase completed progress note")
         (s5 hard-plan-execute-intercept
            :at "intent-layer decision gate + event-bus question stream"
            :trigger "phase == Plan"
            :writes "agent_questions(decision_type=risk)"
            :emits "QuestionEvent::Created")
         (s6 soft-uncertainty-intercept
            :at "intent-layer decision gate + event-bus question stream"
            :trigger "requiresMasterDecision present"
            :writes "agent_questions(decision_type=implementation)"
            :emits "QuestionEvent::Created"))
      :tools-backref ["mission_submit_phase_result"]
      :fsm-ref "intent-layer pillar :: state-machines-owned :: engineering-phase"))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.2 Category: Conversation Flows (会话摄取 / 复盘 / 对账)
  ;; ══════════════════════════════════════════════════════════
  (category conversation-flows
    :desc "外部 CLI JSONL → conversation 表 → 认知管道 → 派生表"

    (flow F6-conversation-jsonl-ingest
      :desc "PTY JSONL / Codex SQLite / Gemini tmp → conversations 表 → 后续派生"
      :triggers
        ["WatcherEvent::NewMessages (Claude Code)"
         "Codex SQLite poll (10s interval)"
         "Gemini tmp poll (10s interval)"
         "Claude projects JSONL poll (reconcile, 10s)"]
      :stages
        ((s1 external-state-scan
            :at "worker pillar :: section worker-cluster :: worker-local :: functional-group cli-ingestion"
            :action "4 worker 扫各自外部 state (conversation_logger / codex_ingestion / gemini_reconcile / reconcile)")
         (s2 ingestion-route
            :at "worker pillar :: section pty :: cross-pillar-notes::system-infra :: ingestion-router"
            :action "infra::ingestion_router 按类型分类消息")
         (s3 normalize-and-write
            :at "system-layer pillar :: infra :: message-handler (data-plane 穿越)"
            :writes "conversations + conversation_messages (memory :: conversation-logs)"
            :emits "JsonlMessageIngested / MessagePersisted / ConversationMessageLogged")
         (s4 organize
            :at "worker pillar :: worker-local :: functional-group 认知管道 :: conversation_organizer"
            :action "compaction link + orphan parent fix + 5s debounce"
            :emits "SessionEvent::Organized")
         (s5 tag-and-chunk
            :at "worker pillar :: worker-local :: tagger_chunker (stage2 analyze + stage3 chunk)"
            :writes "message_labels + turns + commit detection"
            :emits "ContextualCommitDetected (on commit detected)")
         (s6 embedding-trigger
            :at "worker pillar :: worker-local :: ast_sync / conversation triggers EmbeddingTask"
            :via-channel "embedding_tx MPSC"
            :downstream "→ F7-embedding-pipeline"))
      :tools-backref ["mission_conversation_reconcile"])

    (flow F7-embedding-pipeline
      :desc "新内容 → embedding 生成 → 向量存储 → 索引就绪"
      :triggers ["EmbeddingTask MPSC (from F6-s6 / KB mutation / Skill mutation / explicit backfill)"]
      :stages
        ((s1 consume-task
            :at "worker pillar :: worker-sonnet :: embedding-worker-loop"
            :reads "conversations / ast_nodes / kb_entries / compaction_fragments")
         (s2 dedup-check
            :at "embedding-worker-loop"
            :action "查既有 embedding 决定增量 upsert 或 backfill")
         (s3 llm-call
            :at "worker pillar :: section xjp-router-gateway :: xjp-router-embedding (v0.3 新)"
            :action "HTTP → Windows 12900KF QWEN3"
            :fail-fast "禁止 fallback"
            :code-alignment "code-aligned: xjp_router_client.rs + embedding_worker provider selection; sonnet embedding lane removed")
         (s4 vector-upsert
            :at "worker pillar :: embedding-worker-loop"
            :writes "kb_embeddings / ast_embeddings / turn_topics"
            :memory-module "embedding-support")
         (s5 index-ready
            :at "memory pillar :: module kb-manager (FTS5 + HNSW 索引)"
            :action "检索可见性释放"))
      :tools-backref ["mission_embedding_ops" "mission_kb_remember" "mission_kb_mutate" "mission_skill_mutate"])

    (flow F-kb-mutation-to-index
      :desc "KB 写入/更新/删除/导入/项目归属 → memory state + graph links + embedding/index refresh"
      :triggers ["mission_kb_remember" "mission_kb_mutate(action=forget|update|import)" "mission_kb_batch_set_project"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/knowledge/kb.rs"
      :stages
        ((s1 parse-and-quality-guard
            :at "tools pillar :: knowledge/kb schema + kb handler"
            :action "remember/update 先过 content quality guard; mutate 先按 action 归一到 legacy handler")
         (s2 write-kb-state
            :at "memory pillar :: kb-manager"
            :writes ["kb_entries upsert/update/delete/import" "project_id metadata"])
         (s3 maintain-graph-links
            :at "memory pillar :: kb graph + AST links"
            :action "remember consolidated_from → supersedes edges; symbol/file_hint → ast link; delete 清 edges/ast_links")
         (s4 trigger-embedding-refresh
            :at "worker pillar :: embedding-worker-loop"
            :condition "created/updated/content_changed/imported entries"
            :action "embedding_tx.try_send(EmbeddingTask::ProcessKBEntry)")
         (s5 emit-memory-event
            :at "event-bus pillar :: MemoryEvent"
            :emits "KBBatchMutated{count,categories,action}")
         (s6 conflict-detection
            :at "knowledge/kb handler + embedding cache"
            :condition "new remember only"
            :action "semantic conflict check; optional confidence downweight; contradicts edge")
         (s7 return-result
            :returns "remember/mutate/batch project json; downstream F7 makes embedding/index visible"))
      :tools-backref ["mission_kb_remember" "mission_kb_mutate" "mission_kb_batch_set_project"]
      :downstream ["F7-embedding-pipeline when content changes" "F10-context-assembly retrieval sees refreshed KB after index ready"])

    (flow F-kb-governance-ops
      :desc "KB 运维治理 — gc/compact/analyze/discover/plan queue/execute plan"
      :triggers ["mission_kb_ops(action=gc|compact|analyze|discover|queue_status|execute_plan)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/knowledge/kb.rs"
      :stages
        ((s1 action-dispatch
            :at "tools pillar :: mission_kb_ops"
            :action "compact 直接进 handle_kb_compact; analyze/discover/queue_status/execute_plan/gc 归一到 legacy handler")
         (s2 gc-or-compact
            :at "memory pillar :: kb-manager"
            :actions ["gc stats/stale/duplicates/clean_stale/clean_duplicates" "compact rule-based dryRun/delete"])
         (s3 analyze-kb
            :at "worker pillar :: llm-gateways :: gemini gateway"
            :action "分页读取 KB, 可注入 board context, 构造 overview/consolidation/custom prompt")
         (s4 save-consolidation-plan
            :at "memory pillar :: kb operation queue"
            :condition "mode=consolidation_plan and save_plan=true"
            :writes "kb_ops queue")
         (s5 queue-status
            :at "memory pillar :: kb operation queue"
            :reads "plan operations + optional summary")
         (s6 execute-plan
            :at "knowledge/kb handler + memory/task"
            :action "expire stale ops, mark running, apply delete/update directly; merge/distill dispatch legacy memory task")
         (s7 discover-infra
            :at "system-layer/worker boundary :: SSH probe"
            :action "resolve infra key/credentials, probe remote host, remember infra KB entry")
         (s8 return-and-notify
            :emits "TaskEvent::Created when execute_plan dispatches merge/distill task"
            :returns "governance stats/analysis/plan/queue/execution/discovery result"))
      :tools-backref ["mission_kb_ops"]
      :downstream ["F-kb-mutation-to-index for remembered infra/update/delete effects" "F-task-submit-dispatch when merge/distill dispatches legacy memory task"])

    (flow F-session-completion-event-chain
      :desc "slot/session terminal signal → SessionEvent::Completed → retro/strategy/experience consumers"
      :status "architecture-designed; emit-point audit/code-alignment pending"
      :triggers ["stable PTY idle after task completion" "conversation/session close" "flow-engine terminal state" "manual backfill"]
      :event-bus-contract "event-bus v1.3.2 :: session-completion-contract"
      :stages
        ((s1 detect-terminal-condition
            :at "worker pillar :: pty_event_worker / conversation organizer / flow-engine-v2"
            :action "detect task/session terminal boundary, not every transient Idle")
         (s2 collect-session-identity
            :at "memory pillar :: conversation-logs + slot-support"
            :reads ["session_id" "project_id" "slot_id" "conversation_id" "last_message_seq" "flow_id/board_task_id if any"])
         (s3 build-completed-event
            :at "event-bus producer boundary"
            :event "SessionEvent::Completed"
            :payload ["session_id" "project_id?" "slot_id?" "conversation_id?" "completion_source" "ended_at" "summary_ref?" "dedupe_key"])
         (s4 append-with-dedupe
            :at "event-bus pillar :: log.append"
            :dedupe "session_id + completion_source + ended_at_window")
         (s5 retro-consume
            :at "worker pillar :: retro_worker"
            :downstream "F8-retrospective-to-memory")
         (s6 strategy-consume
            :at "worker pillar :: strategy_worker / experience_harvester"
            :downstream "F-strategy-analysis")
         (s7 projection
            :at "event-bus ws_bridge + timeline readers"
            :action "surface session completion to timeline/UI without duplicating analysis"))
      :egress
        (writes ["event_log(Session::Completed)" "retrospectives/deep_analysis via downstream consumers"]
         emits ["SessionEvent::Completed"]
         returns "session completion fan-out status")
      :tools-backref ["mission_timeline" "mission_conversation_analyze" "mission_retrospective_manage"])

    (flow F8-retrospective-to-memory
      :desc "会话结束 → 复盘 → 沉淀到 memory + KB"
      :triggers ["SessionCompleted 事件"]
      :stages
        ((s1 session-end-detection
            :at "event-bus pillar :: SessionEvent::Completed"
            :via "F-session-completion-event-chain")
         (s2 retro-analysis
            :at "worker pillar :: worker-sonnet :: retro-worker-cycle"
            :reads "conversations (session window)")
         (s3 llm-retro
            :at "worker pillar :: llm-gateways :: sonnet-priority-gateway"
            :action "生成 session retro / deep analysis")
         (s4 persist
            :at "memory pillar :: conversation-logs (retrospectives) + system-support (deep_analysis)"
            :writes ["retrospectives" "deep_analysis"])
         (s5 kb-upsert
            :at "memory pillar :: kb-manager"
            :optional "必要时提炼为 KB 条目"
            :writes ["kb_entries (若有)"]))
      :tools-backref ["mission_retrospective_manage" "mission_conversation_analyze(action=retrospective)"])

    (flow F-strategy-analysis
      :desc "SessionCompleted → Gemini 策略分析 → inbox/kb/deep_analysis"
      :triggers ["SessionCompleted"]
      :stages
        ((s1 strategic-prompt-build
            :at "worker pillar :: worker-gemini :: strategy-worker-cycle"
            :reads ["conversations" "kb_entries (strategic-state)" "daemon_state"])
         (s2 gemini-call
            :at "worker pillar :: llm-gateways :: gemini-unified-gateway"
            :action "Gemini driver/cli/client 路径")
         (s3 persist-strategy
            :at "memory pillar :: system-support + kb-manager"
            :writes ["inbox_messages" "kb_entries" "deep_analysis"]))
      :tools-backref [])

    (flow F-translation-cycle
      :desc "thinking message → Sonnet 翻译 → message_translations"
      :triggers ["MessageEvent::thinking_message"]
      :stages
        ((s1 filter-and-prep
            :at "worker pillar :: worker-sonnet :: translation-worker-cycle"
            :reads "conversation_messages")
         (s2 sonnet-translate
            :at "worker pillar :: llm-gateways :: sonnet-priority-gateway")
         (s3 persist
            :at "memory pillar :: conversation-logs"
            :writes ["message_translations"]))
      :tools-backref [])

    (flow F-router-chat-session
      :desc "router chat 请求 → 上下文/附件装配 → Gemini/Router 调用 → 可选历史保存与压缩"
      :triggers ["mission_router_chat" "mission_router_chat_manage(action=history|list|delete|clear|delete_message|restore|stats|compress)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/comm/router_chat.rs"
      :stages
        ((s1 ingress-normalize
            :at "tools pillar :: mission_router_chat"
            :action "message shorthand 归一为 messages[]; 解析 model/context/search/files/channel/api_key_alias/task_id")
         (s2 task-chat-context
            :at "memory pillar :: conversation-logs/router_chat"
            :condition "task_id present"
            :reads ["router_chat_conversations" "router_chat_summary" "active unsummarized router_chat_messages"]
            :action "summary + active history prepend为 system/history context; new_messages 单独保留用于保存")
         (s3 optional-domain-context
            :at "memory pillar :: kb-manager + board"
            :condition "context=kb|board|both"
            :reads ["kb_entries excluding credentials" "board_tasks"]
            :action "注入第一条 user message; 概念上复用 F10 context assembly 的 retrieval 输入面")
         (s4 file-attachment-prep
            :at "system-layer boundary :: file access + Gemini File API"
            :action "canonicalize file paths, denylist sensitive paths, inline text/truncated text, binary via Gemini File API when available")
         (s5 budget-and-llm-call
            :at "worker pillar :: llm-gateways :: gemini-unified-gateway"
            :action "附件模式拒绝截断; 否则 apply context budget; multimodal 走 direct Gemini, normal path 走 Router/GeminiClient")
         (s6 persist-session
            :at "memory pillar :: conversation-logs/router_chat"
            :condition "task_id present"
            :writes ["new user messages" "assistant response" "conversation_id"])
         (s7 manage-history
            :at "tools pillar :: mission_router_chat_manage"
            :actions ["history/load by task" "list" "delete/clear/delete_message with archive" "restore" "stats"])
         (s8 compress-history
            :at "worker pillar :: llm-gateways + memory pillar"
            :condition "action=compress"
            :action "load compressible old messages, call Gemini summarizer, optimistic-lock update rolling summary cursor"))
      :tools-backref ["mission_router_chat" "mission_router_chat_manage"]
      :downstream ["F10-context-assembly (conceptual dependency when context injects KB/board)" "LLM trace/audit read models observe downstream effects"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.3 Category: Cognition Flows (认知 / 决策 / 学习)
  ;; ══════════════════════════════════════════════════════════
  (category cognition-flows
    :desc "认知推理链 — 归 intent-layer 主 ownership, 本处是 narrative 跨 pillar staging"

    (flow F9-decision-cascade
      :desc "agent question → 多 tier 级联决策 → 回答"
      :triggers ["decision 需求 (agent question / escalation / autopilot 调用)"]
      :stages
        ((s1 question-or-decision-arrive
            :at "intent-layer pillar :: learning-engine :: decision :: decision-cascade"
            :optional-tool ["mission_question" "mission_task_submit"])
         (s2 tier-1-kb-lookup
            :at "worker pillar :: worker-side-computation :: retrieval-fusion"
            :reads "kb_entries"
            :decision "若命中 → 返回 KB 答案; 否则 → tier-2")
         (s3 tier-2-gemini-consult
            :at "worker pillar :: llm-gateways :: gemini-unified-gateway"
            :decision "Gemini 确定 → 返回; 不定 → tier-3")
         (s4 tier-3-decision-slot
            :at "worker pillar :: section pty :: subsection slot-orchestrator"
            :action "派 decision slot 深度分析"
            :decision "slot 确定 → 返回; 不定 → tier-4")
         (s5 tier-4-human-escalation
            :at "指挥官手动 / Claude Code 交互"
            :action "升级人类决策"))
      :tools-backref ["mission_decision_stats"])

    (flow F-extraction-pipeline
      :desc "session 落地 → 两阶段提取 → 知识候选"
      :triggers ["extraction 触发 (autopilot extraction-check / SessionOrganized 后)"]
      :stages
        ((s1 session-ready
            :at "memory pillar :: conversation-logs"
            :reads ["conversations" "conversation_messages"])
         (s2 fast-extract
            :at "intent-layer pillar :: learning-engine :: extraction :: extraction-pipeline"
            :fsm-state "Idle → Sending (extraction-phase FSM)"
            :action "快速提取")
         (s3 deep-extract-via-slot
            :at "worker pillar :: section pty :: subsection slot-orchestrator"
            :fsm-state "Sending → WaitingForIdleness"
            :action "派 slot 做深度提取")
         (s4 slot-idle-complete
            :at "worker pillar :: pty_event_worker 触发 SlotBecameIdle"
            :fsm-state "WaitingForIdleness → Complete")
         (s5 persist-knowledge
            :at "memory pillar :: kb-manager (via mission_kb_remember 或直接 trait)"
            :writes ["kb_entries" "kb_embeddings (downstream F7)"]))
      :fsm-ref "intent-layer pillar :: state-machines-owned :: extraction-phase"
      :tools-backref [])

    (flow F-intent-analysis
      :desc "turn 分析 → user_intents + intent_group 回写"
      :triggers ["autopilot idle-explore 或 backfill"]
      :stages
        ((s1 scan-turns
            :at "intent-layer pillar :: learning-engine :: analysis :: intent-analysis"
            :reads "conversation_turns")
         (s2 llm-analyze
            :at "worker pillar :: llm-gateways (sonnet 或 gemini, 取决于实现)"
            :action "识别 intent group")
         (s3 persist-intent
            :at "memory pillar :: conversation-logs"
            :writes ["user_intents" "conversation_turns.intent_group_id"]
            :trait-writer "ConversationStore::insert_user_intent"))
      :tools-backref [])

    (flow F-historical-backfill
      :desc "backlog catch-up → extraction queue"
      :triggers ["autopilot backfill 调度 或 手动 mission_conversation_reconcile"]
      :stages
        ((s1 scan-backlog
            :at "intent-layer pillar :: learning-engine :: extraction :: historical-scan-backfill"
            :action "按时间窗口扫历史 conversations")
         (s2 queue-into-extraction
            :at "intent-layer pillar :: extraction-pipeline"
            :action "旧数据进 extraction 队列")
         (s3 downstream-extraction
            :flow-ref "F-extraction-pipeline"))
      :tools-backref ["mission_conversation_reconcile"])

    (flow F-idle-explore
      :desc "系统空闲 → 触发探索任务"
      :triggers ["autopilot supervision-check 发现 slot idle"]
      :stages
        ((s1 idle-detect
            :at "intent-layer pillar :: learning-engine :: extraction :: idle-explore-trigger"
            :reads "slot_sessions runtime state")
         (s2 trigger-exploration
            :at "本 flow 的 s3 / F-extraction-pipeline / experience_harvester (若 activated)"
            :action "触发 exploration / extraction / backfill"))
      :tools-backref [])

    (flow F-lisp-survey-update
      :desc "commit 检测 → 派 lisp-surveyor slot → 更新项目 intent.lisp"
      :triggers ["SystemEvent::ContextualCommitDetected"]
      :stages
        ((s1 commit-detected
            :at "worker pillar :: worker-local :: tagger_chunker (commit detection in stage2)"
            :emits "ContextualCommitDetected{conv_id, session_id, slot_id, commit_hash, message}")
         (s2 worker-side-filter
            :at "worker pillar :: worker-sonnet :: lisp-survey-update"
            :action "self-trigger 过滤 + ProjectRegistry 查 intent_path + 60s debounce per project")
         (s3 prompt-assembly
            :at "intent-layer pillar :: section lisp-survey-dual-owned :: lisp-survey-update-semantic"
            :action "组装 survey prompt (diff + intent_path)")
         (s4 slot-dispatch
            :at "worker pillar :: section pty :: subsection slot-orchestrator"
            :registered-task "lisp_survey (slot-id lisp-surveyor, model sonnet, timeout 900s)")
         (s5 slot-edit-intent
            :at "lisp-surveyor slot 执行"
            :action "parse response: NO_CHANGE → skip; otherwise Edit tool 更新 intent.lisp"
            :file-writes ["<project>/.missiond/intent.lisp"]))
      :tools-backref [])

    (flow F-arch-maintenance
      :desc "commit → 派 arch-surveyor → 更新 architecture manifest"
      :triggers ["SystemEvent::ContextualCommitDetected"]
      :stages
        ((s1 commit-detected :flow-ref "F-lisp-survey-update s1")
         (s2 arch-worker-trigger
            :at "worker pillar :: worker-sonnet :: arch-maintenance-worker-cycle")
         (s3 prompt-and-dispatch
            :at "intent-layer pillar :: section arch-maintenance-dual-owned :: arch-maintenance-semantic")
         (s4 slot-edit-manifest
            :at "arch-surveyor slot"
            :file-writes ["project architecture manifest (YAML/MD)"]))
      :tools-backref []))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.4 Category: Context Flows (LLM 调用前上下文装配)
  ;; ══════════════════════════════════════════════════════════
  (category context-flows
    :desc "slot 激活 / LLM 调用前的 prompt + context 装配"

    (flow F10-context-assembly
      :desc "slot 激活 / 即将发起 LLM 调用 → 按 source-priority 装配 prompt"
      :triggers ["slot 激活" "LLM 调用前置"]
      :stages
        ((s1 slot-env-build
            :at "worker pillar :: section context-assembly :: slot-env-build"
            :action "收集 role / cwd / project / session tracking / secret")
         (s2 source-ranking
            :at "worker pillar :: section context-assembly :: context-bundle-assembly"
            :source-priority "slot-env → skill-context → kb → conversation-history → topology-map → claude-md")
         (s3 retrieval-fusion
            :at "worker pillar :: section worker-side-computation :: retrieval-fusion"
            :action "4 路并发: vector / fulltext / fuzzy / tag")
         (s4 budget-allocate
            :at "worker pillar :: context-assembly :: context_budget + pure_budget"
            :action "token 估算 + 6MB 上限 + 源间分配衰减")
         (s5 assemble-bundle
            :at "worker pillar :: context-pipeline"
            :returns "prompt bundle + source trace"
            :downstream "→ llm_gateway / slot dispatch"))
      :tools-backref ["mission_skill_context" "mission_kb_query (indirect)" "mission_code_search (indirect)"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.5 Category: Infrastructure Flows (启动 / 项目注册 / 权限)
  ;; ══════════════════════════════════════════════════════════
  (category infrastructure-flows
    :desc "daemon bootstrap / project registration / learned permissions"

    (flow F-daemon-bootstrap
      :desc "daemon 启动 6 phase 顺序"
      :triggers ["binary main 启动"]
      :stages
        ((s1 infrastructure
            :at "worker pillar :: section orchestration-governance :: daemon-bootstrap-spawn-order"
            :init "db → embed_model → event_bus")
         (s2 project-registry
            :at "Phase-1.5 (commit e18d0bf, 必须早于 slot_manager)"
            :action "store.list_projects() → ProjectRegistry::new → SharedProjectRegistry")
         (s3 core-modules
            :init "pty_manager → slot_manager → mission_control")
         (s4 gateways
            :init "gemini_gateway → sonnet_gateway → llm_gateway (future: xjp_router_client)")
         (s5 pipelines
            :init "context_pipeline → worker_registry → control_tree")
         (s6 workers
            :init "17 BackgroundWorker spawn (见 workers/ 子目录)")
         (s7 engines-io
            :init "autopilot → ipc-handler → ws-server"))
      :tools-backref [])

    (flow F-daemon-update-restart
      :desc "daemon 自更新 — cargo build → 原子替换当前二进制 → codesign → 延迟重启"
      :triggers ["mission_daemon_update(skip_build?)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/sysinfra/system.rs"
      :stages
        ((s1 resolve-runtime-paths
            :at "tools pillar :: sysinfra/system :: mission_daemon_update"
            :action "resolve current_exe as binary_dest; derive project_root/build_target from CARGO_MANIFEST_DIR")
         (s2 optional-build
            :condition "skip_build=false"
            :action "run cargo build --release --package missiond-daemon from project_root")
         (s3 atomic-replace
            :action "copy target/release/missiond to temp path, chmod 755, rename over current binary")
         (s4 codesign-macos
            :condition "target_os=macos"
            :action "codesign -s - --force new binary")
         (s5 restart-selection
            :action "check launchctl gui/<uid>/com.missiond.daemon")
         (s6 launchd-restart
            :condition "launchd service exists"
            :action "spawn delayed launchctl kickstart -k after response is sent")
         (s7 script-fallback
            :condition "launchd service missing"
            :action "write temp restart script, terminate old PID, remove socket, nohup new binary"))
      :tools-backref ["mission_daemon_update"]
      :risks ["self-update kills current MCP connection after response" "cargo build/code signing can fail"])

    (flow F-infra-diagnostics
      :desc "infra registry/health/reachability/diagnose — 本地状态 + 网络通道 + 远端 SSH 检查"
      :triggers ["mission_infra_query" "mission_infra_ops(action=health|reachability|diagnose)" "mission_power_control(status)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/sysinfra/infra.rs + misc.rs"
      :stages
        ((s1 registry-read
            :at "system-layer pillar :: infra registry"
            :reads ["servers.yaml runtime registry" "state.infra"])
         (s2 health-snapshot
            :at "sysinfra/misc :: mission_health"
            :condition "action=health"
            :reads ["PTY slot status" "ControlTree memory pause" "extraction states" "stats snapshot"])
         (s3 reachability-probes
            :at "sysinfra/infra :: mission_reachability"
            :condition "action=reachability"
            :action "run selected lan_ping/public_ping/tailscale/ssh/deploy_agent probes in parallel")
         (s4 diagnose-target
            :at "sysinfra/infra :: mission_os_diagnose"
            :condition "action=diagnose"
            :action "resolve SSH targets from registry or direct target")
         (s5 credential-fallback
            :at "memory pillar :: kb-manager"
            :condition "diagnose needs password fallback"
            :reads "credential KB search by target")
         (s6 remote-probe
            :at "system-layer boundary :: SSH process"
            :action "run selected shell checks: system/crashes/top_cpu/temperatures/journal_errors/docker/network/gpu")
         (s7 severity-return
            :action "parse probe output, compute green/yellow/red severity, return structured diagnostics"))
      :tools-backref ["mission_infra_query" "mission_infra_ops" "mission_power_control(status)"])

    (flow F-incident-reaction
      :desc "incident candidate → persist/dedupe → classify → board remediation → optional worker dispatch → observe resolution"
      :status "code-aligned; mission_incident get/remediate/status/close + aiops triage helpers implemented"
      :triggers
        ["worker pillar :: infra::aiops incident candidate"
         "mission_incident(action=test|list|get|remediate|status|close)"
         "mission_infra_ops(action=health|diagnose) red/yellow result"
         "event-bus :: IncidentEvent"]
      :ingress
        (entry "incident source with severity/title/source/server_id/evidence")
      :stages
        ((s1 incident-source
            :at "system-layer/worker boundary :: infra aiops or sysinfra tool"
            :action "normalize health failure, pty-slot incident, synthetic test incident, or diagnostic red result")
         (s2 persist-and-dedupe
            :at "memory pillar :: system-support :: incidents"
            :action "write incident row, dedupe by source/server/title/window, attach evidence and first_seen/last_seen")
         (s3 classify-remediation
            :at "intent-layer pillar :: aiops policy (future) + worker existing health scan"
            :action "classify severity, remediation playbook, auto-close eligibility, and escalation target")
         (s4 board-task-link
            :at "memory pillar :: board"
            :action "create or update remediation board_task; add recovery note when health recovers")
         (s5 dispatch-remediation
            :at "worker pillar :: slot/task dispatch"
            :condition "playbook actionable and safe"
            :action "delegate remediation to Opus/ops slot or task_delegate; otherwise leave board task for human")
         (s6 observe-resolution
            :at "worker pillar :: aiops periodic scan + tools mission_incident(list)"
            :action "observe recovered/degraded/still-failing state; close board task or escalate")
         (s7 audit-return
            :at "event-bus + tools"
            :emits ["IncidentEvent::Reported / IncidentEvent::Resolved where implemented" "BoardEvent::StatusChanged when board linked"]
            :returns "incident id/status/remediation task/ref"))
      :egress
        (writes ["incidents" "board_tasks" "board notes" "optional slot_tasks"]
         reads ["health snapshot" "infra registry" "recent incidents"]
         returns "incident reaction receipt / list / remediation status")
      :tools-backref ["mission_incident" "mission_infra_ops" "mission_board_query" "mission_task_delegate"])

    ;; ── F-capability-usage-monitoring moved to L2 shard ──
    ;; Full content moved to .missiond/v2/intent-capability-governance.lisp
    ;; Trailing `))` reproduces the original block's depth-balance footprint
    ;; (the original ended with one extra `)` closing the parent category form).
    (flow F-capability-usage-monitoring
      :status "moved-to-shard (code-aligned partial)"
      :file-ref ".missiond/v2/intent-capability-governance.lisp"
      :shard-section "F-capability-usage-monitoring"
      :section-id "flow.capability-usage-monitoring"
      :role "narrative — see shard for 9-step + ingress/egress/triggers; section-id stable per L2 plan rule-1"
      :tools-backref ["mission_audit" "mission_timeline" "mission_codex_ops" "mission_capability_usage"]))

    (flow F9-project-init
      :desc "一步注册新项目 — path → 完整元数据 → DB + 历史回填 + 注册表热重载"
      :triggers ["mission_project(action=init, path, id?, slots?)"]
      :added "commit 84ac1a6"
      :stages
        ((s1 canonicalize-path
            :at "tools pillar :: knowledge :: project :: mission_project")
         (s2 derive-id :at "handler" :action "from dir name 或 param")
         (s3 git-remote
            :action "git remote get-url origin → github_url"
            :may-fail "无 remote 时 skip")
         (s4 scan-intent-lisp
            :paths [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
            :at "intent-layer pillar :: system-intent-read (文件查找)")
         (s5 upsert-project
            :at "memory pillar :: project-management"
            :writes "projects")
         (s6 backfill-conversations
            :at "memory pillar :: conversation-logs"
            :action "backfill_project_id(path%) + backfill_project_id(claude-encoded%)"
            :writes "conversations project_id 字段")
         (s7 reload-registry
            :at "worker pillar :: daemon-bootstrap (热重载 SharedProjectRegistry)"
            :action "注册表更新"))
      :tools-backref ["mission_project"]
      :return "{id, path, githubUrl, intentPath, backfilledConversations, status='registered'}")

    (flow F-learned-permission
      :desc "auto-approve confirm dialog → 学 permission → 注入 settings.local.json"
      :triggers ["ManagerEvent::ConfirmRequired (99% 自动) 或 mission_pty_confirm (1% 手动)"]
      :coverage-contract
        (sources ["pty_event_worker ConfirmRequired" "manual mission_pty_confirm" "future CLI-specific confirm parser branches"]
         invariant "任何会导致自动确认/手动确认的路径,都必须先经过 pattern extraction 或显式标记 no-learn"
         validation "code-alignment 阶段 grep all ConfirmRequired / ConfirmResponse / trust-dialog branches,逐条标 covered/no-learn")
      :stages
        ((s1 detect-confirm
            :at "worker pillar :: worker-local :: pty_event_worker :: handle_confirm_required"
            :trigger "auto-approve keyword check (trust/always/不再)")
         (s2 extract-pattern
            :at "worker pillar :: section pty :: subsection learned-permissions :: permission_extract"
            :output "ExtractedConfirm{pattern, project_path}")
         (s3 learn-role-scope
            :at "worker pillar :: learned-permissions :: path learned-permission-write"
            :file-writes "learned_permissions.yaml (role scope)"
            :always true)
         (s4 learn-project-scope
            :optional "if project_path Some → ProjectRegistry::resolve"
            :file-writes "learned_permissions.yaml (project scope)")
         (s5 send-response
            :at "worker pillar :: pty_event_worker"
            :action "ConfirmResponse::Option(2) 写 PTY (digit + Enter, 80ms apart)")
         (s6 next-spawn-inject
            :at "worker pillar :: section pty :: subsection slot-orchestrator :: perm_injector"
            :file-writes "<cwd>/.claude/settings.local.json (merged, idempotent)"))
      :tools-backref ["mission_pty_confirm" "mission_permission_query" "mission_permission_mutate"])

    (flow F-mcp-request-dispatch
      :desc "external MCP call → daemon handler → response"
      :triggers ["JSON-RPC stdio from external MCP client"]
      :stages
        ((s1 stdio-receive
            :at "tools pillar :: section rpc-gateway :: server.rs (JSON-RPC loop)")
         (s2 parse-envelope
            :at "tools pillar :: gateway_impl.rs")
         (s3 tools-list-or-call
            :at "tools pillar :: gen_gateway.rs"
            :action "tool_name → handler_fn dispatch")
         (s4 ipc-bridge-or-direct
            :at "system-layer pillar :: infra :: ipc_handler (若跨进程)")
         (s5 handler-dispatch
            :at "对应 pillar 的 handler (compute/knowledge/comm/sysinfra)")
         (s6 db-query-or-action
            :at "memory pillar 或 worker pillar 等")
         (s7 response
            :at "tools pillar :: server.rs (JSON-RPC response)"
            :writes "tool_calls (audit 表)"))
      :tools-backref "all 83 tools (meta-flow)"))

    (flow F-runtime-control-governance
      :desc "运行时控制面 — ControlTree / worker registry / legacy gates / global pause"
      :triggers ["mission_control(target_type, action)" "mission_worker(action=control)" "mission_pause(action)"]
      :phase-C-verified "2026-04-25 — handlers/compute/worker.rs + handlers/sysinfra/misc.rs"
      :stages
        ((s1 parse-control-intent
            :at "tools pillar :: compute/worker or sysinfra/misc handler"
            :action "mission_control 读取 target_type/action/target_name; mission_worker remap control_action; mission_pause 读取 action")
         (s2 status-read
            :at "worker pillar :: control_manager + worker_registry + llm_gate"
            :branch "status/list"
            :reads ["ControlTree status_summary" "worker_registry list" "llm_gate status" "global_paused atomics"])
         (s3 mutate-control-tree
            :at "worker pillar :: control_manager"
            :branch "mission_control pause/resume"
            :action "set_global/provider/domain/worker/slot_role/project paused state")
         (s4 sync-legacy-state
            :at "worker pillar :: legacy compatibility gates"
            :action "sync global_paused atomics, llm_gate provider state, Codex disable flag, worker_registry state")
         (s5 active-slot-role-enforcement
            :at "worker pillar :: PTY manager"
            :condition "mission_control target_type=slot_role and action=pause"
            :action "kill running PTY sessions whose slot role matches target_name")
         (s6 global-pause-flag
            :at "system-layer filesystem"
            :branch "mission_pause"
            :file-writes ["$MISSIOND_HOME/global_paused on pause"]
            :file-deletes ["$MISSIOND_HOME/global_paused on resume"])
         (s7 return-control-state
            :returns "updated control_tree/status text/list json"))
      :tools-backref ["mission_control" "mission_worker" "mission_pause"]
      :ownership
        (tools "MCP schema + action/target surface")
        (worker "ControlTree, WorkerRegistry, LLM gates, PTY kill enforcement")
        (system-layer "global pause flag file"))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.6 Category: Workflow-Runtime Flows (flow-engine-v2)
  ;; ══════════════════════════════════════════════════════════
  (category workflow-runtime-flows
    :desc "flow-engine-v2 YAML declarative node 执行 — 唯一真正 flow orchestration 的 flow"

    (flow F-directive-plan-workflow-compile
      :desc "user utterance / directive request → directive record → approved plan → optional execution bridge → reusable workflow template"
      :status "code-aligned partial — manager surfaces + directive-compiler v0 (compiler_mode=sonnet) + plan-compiler v0 (sonnet) + plan-runner v0 (execute_mode=internal) + workflow-distiller v0 (distill_mode=sonnet); 自动 mode 选择 / auto QuestionEvent gates 仍 pending"
      :triggers
        ["mission_directive(action=compile|approve|list|get|archive|version_chain)"
         "mission_plan(action=compile|approve|mark|supersede|execute|record_evidence)"
         "mission_workflow(action=match|apply|distill|record_execution|compile_methodology|run_methodology)"
         "intent-layer actor capture from user utterance"]
      :ingress
        (entry "utterance/source directive, optional conversation_id/project_id/board_task_id, approval/manual override intent")
      :stages
        ((s1 capture-source
            :at "intent-layer pillar :: directive-plan-workflow-chain"
            :action "collect utterance/system instruction/MCP request with project, conversation, constraints, and references")
         (s2 directive-compile
            :at "intent-layer pillar :: actor directive-compiler v0"
            :action "compiler_mode=sonnet 调 SonnetGateway interactive lane 编译 directive sexp; 校验 fenced block / parens / allowed top-level head; 写 compiler_model + references_json"
            :code-alignment "actor v0 code-aligned; compiler_mode=dry_run 默认仍是 preview; compiler_mode=sonnet 真实编译; persist=true 写 draft row 待 review")
         (s3 directive-store
            :at "memory pillar :: directive-layer :: directive table"
            :writes "directive_insert(status=draft/refining, compiler_model, references_json, sexp_hash)")
         (s4 directive-review-gate
            :at "intent-layer + tools manager surface"
            :action "approve/refine/archive through policy or human gate; future QuestionEvent when confirmation needed")
         (s5 plan-compile
            :at "intent-layer pillar :: actor plan-compiler v0"
            :condition "directive status=approved 或 board task context 提供"
            :action "compiler_mode=sonnet 从 approved directive / board task 编译 plan sexp DAG/FSM; compiled_from = directive/<id>:<version> 或 board_task/<id>"
            :code-alignment "actor v0 code-aligned; compiler_mode=dry_run 默认 preview; compiler_mode=sonnet 真实编译; persist=true 写 awaiting_approval (不自动 approve)")
         (s6 plan-store-and-bind
            :at "memory pillar :: directive-layer + board"
            :writes ["plan table with status=draft/awaiting_approval" "optional board_tasks.source_directive_id / plan binding"])
         (s7 plan-approve-or-supersede
            :at "memory pillar :: DirectiveLayerStore"
            :action "approve plan, mark executing/succeeded/failed, or supersede old active plan for same board_task")
         (s8 execution-bridge
            :at "flow pillar dispatch boundary / plan-runner v0 internal mode"
            :action "route approved plan via execute_mode=bridge (next_call descriptor) 或 execute_mode=internal (plan-runner 直接 dispatch 到 mission_execution / mission_task_delegate / mission_flow_run; downstream 落到 F1 board lifecycle / F5 flow-engine-v2 / F-skill-workflow-execution / F-workflow-slot-full-lifecycle / F-execution-log-governance)"
            :code-alignment "execute_mode=bridge 仍返回 next_call (向后兼容); execute_mode=internal code-aligned: plan-runner 内部 dispatch + 写 evidence sidecar plan_runner_dispatch entry + 推 plan 状态 executing (status update failure 暴露 partial); dispatch_strategy 进入 response + sidecar + (target=mission_execution) 转发到 companion log meta; 'PLAN.lisp DAG 自动选 target / dispatch_strategy / target_project' 仍 code-alignment pending")
         (s9 workflow-distill
            :at "intent-layer pillar :: actor workflow-distiller v0"
            :condition "plan status=succeeded or successful execution evidence provided"
            :action "distill_mode=sonnet 读 succeeded plan + evidence sidecar 生成 workflow sexp + match_rules JSON; persist=true 写 draft/template"
            :code-alignment "actor v0 code-aligned; distill_mode=dry_run 默认 preview; distill_mode=sonnet 真实蒸馏; 高阶 semantic lifting / 自动 record_execution 关联仍 code-alignment pending")
         (s10 workflow-match-apply
            :at "memory pillar :: directive-layer :: workflow table"
            :action "workflow_find_by_match / workflow_list_top_n provides hints or reusable plan candidates for future directives")
         (s11 manager-surface-return
            :at "tools pillar :: mission_directive / mission_plan / mission_workflow"
            :returns "directive/plan/workflow id, status, version chain, match/apply/distill result"))
      :egress
        (writes ["directive" "plan" "workflow" "optional board_tasks binding" "optional agent_questions"]
         reads ["conversation/user utterance" "project registry" "kb context" "board_tasks" "successful plan history"]
         downstream ["F1-board-task-main-lifecycle" "F5-flow-engine-v2-node-execution" "F-skill-workflow-execution" "F-workflow-slot-full-lifecycle"]
         returns "directive/plan/workflow artifacts and execution bridge target")
      :tools-backref ["mission_directive" "mission_plan" "mission_workflow"])

    (flow F-intent-alignment-plan-execution-loop
      :desc "MissionD 统一入口 canonical pipeline: message → intent-alignment.lisp → review → PLAN.lisp → review → MissionD-internal execution → evidence → workflow.lisp distillation"
      :status "code-aligned partial — directive-compiler v0 / plan-compiler v0 / plan-runner v0 + auto-selection v1 (sexp hint parsing 单节点) / workflow-distiller v0 / methodology compiler v0 / generated flow loader 全部 code-aligned; s6 dag-scheduler runtime v2 + unified-entry run_pipeline + evidence-collector typed 全部 code-aligned partial (详 wave-13 anchors via intent-pillar-source-index.lisp :: intent-layer.plan-dag-runtime-v2 / .unified-entry-pipeline.run-pipeline-helper / .evidence-collector-typed-helper); 完整 11-stage 协议 / arbitrary semantic interpretation / file-first .lisp writer-sync / alignment+plan auto QuestionEvent gates / methodology semantic lifting / forge compiler / ExecutionEvent dispatch metadata / autonomous workstation dispatch (4 项 v0 non-goal) 仍 pending"
      :role "MissionD 长期运作的主线 — 不是 client 直连工位, 而是文件优先 + DB 镜像 + 双 review gate + MissionD plan-runner 内部调度 + 证据收集 + 沉淀复用"
      :rationale "把当前'你我改 Lisp,再交给 ClaudeCode 实现'的人工闭环,升级成 MissionD 内部可自动化、可 flow 化、可复用的 directive/plan/workflow pipeline; 不依赖某个交互 client 私有调度能力"
      :triggers
        ["用户 message (来自任何 client / 会话)"
         "external MCP client request (mission_directive/action=compile)"
         "board task ingest (auto_execute=1 或人工指派)"
         "architecture Lisp change set ready for implementation"
         "mission_directive(action=compile, source=message|architecture_lisp_delta|user_request)"]
      :ingress
        (entry "user message / external MCP client / board task / architecture Lisp delta + project_id/conversation_id?/board_task_id?/topic? + approval/manual override intent")
      :artifacts
        ((intent-alignment-lisp
            :path ".missiond/alignment/<topic>/intent-alignment.lisp"
            :purpose "本轮 message/Lisp 差异凝结的对齐输入: objective / scope / affected pillars / implemented-vs-pending / non-goals / acceptance tests"
            :review-gate "alignment-review-gate (human/Codex)"
            :status-lifecycle "draft → reviewing → approved | rejected | superseded"
            :ssot "file-first (DB directive row 是可查询镜像)")
         (plan-lisp
            :path ".missiond/plans/<topic>/PLAN.lisp"
            :purpose "LLM 规划 + human/Codex review 后的可执行计划: files / phases / tasks / tests / risks / rollback"
            :review-gate "plan-review-gate (human/Codex)"
            :status-lifecycle "draft → reviewing → approved → executing → succeeded | failed | superseded"
            :ssot "file-first (DB plan row 是可查询镜像)")
         (plan-evidence-sidecar
            :path ".missiond/v2/plans/<plan_id>.evidence.json"
            :purpose "已执行 plan 的证据落盘: tool_calls / event_log refs / execution companion log refs / deviations / decisions / completions / test outputs / git diffs"
            :writer "mission_plan(action=record_evidence) — code-aligned"
            :future "可升级为 DB JSONB 列或专用 plan_evidence 表; 也是 workflow-distillation 的输入")
         (workflow-lisp
            :path ".missiond/workflows/<topic>.lisp"
            :purpose "多次成功运行后的方法论沉淀; human/agent SSOT, 可由 F-methodology-to-executable-compile 生成 YAML"
            :status-lifecycle "draft → published → deprecated"))
      :stages
        ((s1 message-intake
            :at "tools pillar :: mission_directive (action=compile, source=message|architecture_lisp_delta|user_request) + intent-layer pillar :: message-intake-manager"
            :reads ["user message / external MCP request / board_task ingestion" ".missiond/v2/*.lisp diff (when source=architecture_lisp_delta)" "project registry" "kb context"]
            :writes ["directive draft row (when persist=true)" "file-first alignment request stub"]
            :code-alignment "code-aligned partial — manager surface via mission_directive(action=compile); compiler_mode=dry_run 默认 preview; compiler_mode=sonnet 走 directive-compiler v0 (SonnetGateway interactive lane + sexp validation); persist=true 写 draft 等待 review"
            :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/directive.rs (schema)"
                                     "crates/missiond-daemon/src/handlers/knowledge/directive.rs (handler — action=compile dispatch + compiler_mode 校验)"]
            :pending ["设计的 message-intake-manager 自主路由 — 当前由 caller 显式给 source/topic"
                      "directive-compiler 自动多轮 review/refine 收敛 — 仅 v0 单次编译"]
            :decision "选择 alignment topic + project_id; 决定走 file-first 还是 directive-mirror"
            :no-new-tool "本阶段不引入 mission_message / mission_invoke; 统一入口由 mission_directive 充当管理面")
         (s2 intent-alignment-authoring
            :at "intent-layer pillar :: alignment-author (mode A: direct LLM via mission_directive compiler_mode=sonnet, mode B: resident ClaudeCode lisp-architect slot)"
            :reads ["directive draft" "user message" ".missiond/v2/*.lisp diff" "kb hints" "previous alignment topics"]
            :architecture-writes [".missiond/alignment/<topic>/intent-alignment.lisp (file-first SSOT, status=draft)" "directive table draft mirror"]
            :code-aligned-writes ["directive sexp + references_json + directive table draft (mission_directive compiler_mode=sonnet, persist=true)"]
            :status "code-aligned partial — directive-compiler v0 写 directive sexp/references_json/DB draft; file-first .missiond/alignment/<topic>/intent-alignment.lisp 自动同步 + mode B 自动 dispatch 仍 pending"
            :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/directive.rs (action=compile sonnet branch — SonnetGateway interactive call + sexp validation)"
                                     "crates/missiond-daemon/src/llm/sonnet_gateway.rs (LLM call substrate)"]
            :pending ["file-first .missiond/alignment/<topic>/intent-alignment.lisp 自动写入 / 与 directive 表双向同步 — 当前由人工产出"
                      "mode B (resident ClaudeCode lisp-architect slot) 自动 dispatch — 当前需人工把任务挂到既有 slot"
                      "alignment-author actor 自主决定 mode A vs mode B 与多轮收敛"]
            :note "compiler_mode=dry_run 默认行为不调 LLM, 仅返回 preview; compiler_mode=sonnet 真实调用 SonnetGateway, 失败时 suggestion 明确说明可退回 dry_run 或启动 sonnet gateway"
            :workstation-preference "Lisp 架构改动优先复用常驻 ClaudeCode lisp-architect slot (resident-lisp-architect-session policy); 上下文 asset 已经预热, 不为单轮 alignment 重开 fresh session"
            :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: policy resident-lisp-architect-session")
         (s3 alignment-review-gate
            :at "human/Codex review loop + intent-layer pillar :: alignment-review-gate"
            :reads [".missiond/alignment/<topic>/intent-alignment.lisp" "directive row mirror"]
            :writes [".missiond/alignment/<topic>/intent-alignment.lisp (status: draft → reviewing → approved | rejected | superseded)" "directive table status update"]
            :gate-rule "未通过 approval gate 不允许进入 plan-authoring; LLM 产物不能直接进 plan"
            :code-alignment "manager surface code-aligned via mission_directive(action=approve|archive|version_chain); auto QuestionEvent 触发 仍 code-alignment pending"
            :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/directive.rs (approve/archive/version_chain branches)"
                                     "crates/missiond-core/src/db/pg/directive.rs (DirectiveLayerStore impl)"]
            :pending ["自动 QuestionEvent 把人工 review 转入事件循环"
                      "review history 与 multi-reviewer aggregation"])
         (s4 plan-authoring
            :at "intent-layer pillar :: plan-compiler (LLM planner via mission_plan compiler_mode=sonnet, 或 resident planning slot)"
            :condition "intent-alignment.lisp status=approved"
            :reads [".missiond/alignment/<topic>/intent-alignment.lisp (approved)" "approved directive (source_directive_id) / board task context" "repo state summary" "relevant Lisp snippets" "kb context"]
            :architecture-writes [".missiond/plans/<topic>/PLAN.lisp (file-first SSOT, status=draft)" "plan table draft/awaiting_approval mirror"]
            :code-aligned-writes ["plan sexp + plan table draft/awaiting_approval row (mission_plan compiler_mode=sonnet, persist=true)"]
            :model-policy "provider alias configurable (例: OPUS-4.7-class planner / planner-class model); 不硬编码可用性; current code-aligned v0 uses claude-sonnet"
            :status "code-aligned partial — plan-compiler v0 写 plan sexp / plan 表 (mission_plan compile sonnet, persist=true 写 awaiting_approval, 不自动 approve, compiled_from=directive/<id>:<version> 或 board_task/<id>); file-first .lisp 自动同步 + 自动 :dispatch-strategy 推断 仍 pending"
            :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/plan.rs (schema)"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=compile sonnet branch)"
                                     "crates/missiond-daemon/src/llm/sonnet_gateway.rs"]
            :pending ["file-first .missiond/plans/<topic>/PLAN.lisp 自动写入/与 plan 表双向同步"
                      "plan-compiler 自动从 PLAN.lisp DAG 推断 :dispatch-strategy / :target_project 字段 — 当前由 LLM 自由生成, 仍需人工审核"
                      "alignment 多轮 refine 后的增量 re-compile"
                      "planner-class model alias 切换 (例: opus planner) — 现仅 sonnet 可用"]
            :workstation-preference "若 PLAN 跨多文件且需要 cumulative repo 知识 → resident planning slot; 若单一目标项目可直接 fresh slot 草拟"
            :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration"
            :plan-must-record "PLAN.lisp 节点应显式标记 :dispatch-strategy 字段 ∈ {resident-lisp / fresh-code-alignment / agent-team / prompt-fallback / mixed | unknown} 与 :target_project, 供 s6 plan-runner 直接消费; 当前 sonnet 编译产物的字段写入由 LLM 产生, 仍需人工审核")
         (s5 plan-review-gate
            :at "human/Codex review loop + intent-layer pillar :: plan-review-gate"
            :reads [".missiond/plans/<topic>/PLAN.lisp" "plan row mirror"]
            :writes [".missiond/plans/<topic>/PLAN.lisp (status: draft → reviewing → approved | rejected | superseded)" "plan table status update via mission_plan(action=approve|mark|supersede)"]
            :gate-rule "未通过 approval gate 不允许进入 execution-runner"
            :code-alignment "manager surface code-aligned via mission_plan; auto QuestionEvent gate 仍 code-alignment pending"
            :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=approve|mark|supersede branches)"
                                     "crates/missiond-core/src/db/pg/directive.rs (plan FSM transitions)"]
            :pending ["自动 QuestionEvent 触发 (人工 review 转入事件循环)"
                      "review history 留痕"])
         (s6 execution-runner
            :at "MissionD plan-runner v0 + auto-selection v1 + plan_dag runtime v2 :: 内部消费 mission_execution / mission_task_delegate / mission_flow_run / mission_compute_slot / mission_pty_spawn"
            :condition "PLAN.lisp status=approved"
            :principle "不是 client 直接调工位, 而是 MissionD plan-runner 内部调度 — alignment 与 plan 的 review gate 已经把 LLM 产物收敛到可执行边界"
            :current-implementation "execute_mode=bridge (默认, 返 next_call descriptor 向后兼容) / execute_mode=internal (plan-runner v0 直接 dispatch + 写 evidence sidecar + 推 plan 到 executing; dispatch_strategy 进 response/sidecar/companion log meta); auto-selection v1 从 plan.sexp_text 保守解析 :target/:target-tool/:tool/:flow-id/:dispatch-strategy/:parallelism/:target-project/:requested-cwd/:objective/:summary; plan_dag runtime v2 (max_parallel_nodes default=1=v1 sequential / tokio::JoinSet wave-based / 6 lifecycle + 3 skip 子分类 / failure-policy fail-fast vs continue / per-node evidence typed 串行化) — 详 anchor intent-layer.plan-dag-runtime-v2"
            :code-alignment "code-aligned partial — execute_mode=internal + dispatch_strategy + auto-selection v1 (单节点 dispatch) + plan_dag runtime v2 (concurrency + lifecycle + failure-policy + per-node evidence) 已落; explicit args 仍优先, 无法安全推断返回 MISSING_PARAM; 完整 11-stage scheduler / claim-lease / per-node retry / acceptance evaluator / rollback / review-gate paused / mark-plan-final 仍 pending — 详 sub-section dag-scheduler"
            :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/plan.rs (schema: execute_mode + dispatch_strategy + auto-selection 描述)"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=execute internal branch + auto-selection v1 sexp hint parser + dispatch + sidecar + plan FSM)"
                                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (mission_execution forwarding)"
                                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs (task_delegate forwarding)"]
            :auto-selection-precedence ["explicit_arg" "plan_hint (:target / :dispatch-strategy / :parallelism mapping)" "default 'unknown' (dispatch_strategy) / MISSING_PARAM (target)"]
            :auto-selection-response-fields ["target_source ∈ {explicit_arg, plan_hint, missing}" "dispatch_strategy_source ∈ {explicit_arg, plan_hint, default}" "plan_hint_summary"]
            :agent-team-substrate-injection "auto-selection v1: 当 dispatch_strategy 解析为 agent-team 且 target=mission_task_delegate 时, runner 向 objective 注入字面提示 '使用 agent-team提高效率' (幂等, 已注入则不重复)"
            :pending ["完整 11-stage PLAN DAG scheduler (claim-lease / per-node retry / acceptance / rollback / review-gate paused / mark-plan-final / trigger-record-execution-distill) — sub-section dag-scheduler 协议正文; runtime v2 仅 concurrency + lifecycle + failure-policy + per-node evidence 子集"
                      "arbitrary PLAN.lisp 语义解释 (超出保守 key/value hints)"
                      "auto QuestionEvent gates (alignment / plan)"
                      "ExecutionEvent 扩展 dispatch metadata + PlanNodeStateChanged (anchor: intent-layer.plan-dag-runtime-v2.execution-event-decision)"
                      "file-first PLAN.lisp writer/sync"]
            ;; ── dag-scheduler sub-section moved to L2 shard ──
            ;; Full content moved to .missiond/v2/intent-plan-dag.lisp
            (dag-scheduler
              :status code-aligned-partial
              :file-ref ".missiond/v2/intent-plan-dag.lisp"
              :shard-section "dag-scheduler (flow s6 execution-runner sub-block)"
              :section-id "flow.execution-runner-dag-scheduler"
              :scope "完整 PLAN DAG scheduler 11-stage 协议正文 — 见 shard"
              :runtime-v2-ref "intent-layer.plan-dag-runtime-v2 (also in shard)"
              :note "section-id stable per L2 plan rule-1; ingress / 11-stage logic-core / egress / dispatch-strategy / file-vs-db-contract / pending / implementation-targets / checker-contract live in shard")
            :substrate "execution substrate = mission_execution 12-action manager (open/list/claim/heartbeat/release/deviate/decide/issue/complete/status/audit/repair, F-execution-log-governance)"
            :options-before-runner ["bridge mode 仍可用 (caller 自行执行 next_call)" "internal mode 由 plan-runner 直接 dispatch + 写 evidence + 推 plan FSM"]
            :dispatch-strategy
              ((case lisp-only-architecture-task
                  :strategy "resident-lisp-architect-session"
                  :substrate "复用常驻 ClaudeCode lisp-architect slot (mission_pty_send / mission_task_delegate)"
                  :rationale "保留架构上下文; 不为单次 Lisp 改动重开 fresh session"
                  :status "operational-practice")
               (case code-alignment-implementation-task
                  :strategy "fresh-code-alignment-session"
                  :substrate "mission_pty_spawn 新 slot, project-root cwd; 或 mission_compute_slot create dynamic slot"
                  :rationale "任务 .md 自包含 + 隔离度好 + 可并发多个 slot"
                  :status "operational-practice")
               (case broad-independent-scan-or-refactor
                  :strategy "fresh session + agent-team-hint"
                  :substrate "在派给 ClaudeCode 的任务文字中明确写'使用 agent-team 提高效率'"
                  :guardrail "并行子 agent 只读不写; 写入仍由主 agent 单点落笔"
                  :status "operational-practice; plan-runner 自动加 hint 待 code-alignment")
               (case project-bound-coding
                  :strategy "spawn in target project root"
                  :substrate "mission_pty_spawn / mission_compute_slot 显式 project_root, 进 sole-spawn-bottleneck"
                  :rationale "project memory / JSONL / tool path / conversation.project_id 全靠 cwd 落地"
                  :status "code-aligned for spawn cwd; 自动选路 pending")
               (fallback prompt-mode
                  :strategy "claude -p"
                  :status "fallback / non-preferred"
                  :rationale "无 PTY session / 无 evidence / 无 capability-usage 闭环; 仅在 daemon 不可用 / 真正 throwaway 查询时退化使用"
                  :enforcement-future "plan-runner 默认禁止; 显式 dispatch_strategy=prompt-fallback 才允许"))
            :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration / flow pillar :: F-workstation-dispatch-policy / intent-layer pillar :: section unified-entry-pipeline :: workstation-dispatch-policy")
         (s7 evidence-collection
            :at "intent-layer pillar :: evidence-collector + memory/event-bus pillars"
            :reads ["tool_calls" "event_log (DomainEvent stream)" "board_tasks" "execution companion log" "ExecutionEvent stream" "git diff" "test outputs"]
            :writes [".missiond/v2/plans/<plan_id>.evidence.json (mission_plan action=record_evidence + plan-runner v0 internal mode 自动追加 plan_runner_dispatch entry, 含 dispatch_strategy/target_project/requested_cwd/target/inner_result)" "future plan_evidence DB JSONB"]
            :status "code-aligned partial — plan-runner v0 internal mode 自动落 sidecar; 全自动 evidence-collector actor (跨 plan-runner 以外路径) 仍 pending"
            :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs (record_evidence action + plan-runner internal mode auto-append)"
                                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (companion log meta dispatch_strategy)"]
            :pending ["全自动 evidence-collector actor (聚合 git diff / event_log / ExecutionEvent / test outputs 到 sidecar)"
                      "evidence sidecar 升级到 plan_evidence DB JSONB"
                      "ExecutionEvent dispatch metadata 字段 — 当前仅 companion log durable"]
            :purpose "为 workflow-distillation 与 retrospective 提供输入")
         (s8 workflow-distillation
            :at "intent-layer pillar :: workflow-distiller via mission_workflow(action=distill, distill_mode=sonnet)"
            :condition "plan status=succeeded 多次或 human 显式标 reusable"
            :reads ["plan row + plan evidence sidecar (.missiond/v2/plans/<plan_id>.evidence.json)" "successful plan history"]
            :architecture-writes [".missiond/workflows/<topic>.lisp (file-first SSOT)" "workflow table draft/template mirror"]
            :code-aligned-writes ["workflow sexp + match_rules JSON + workflow table draft/template (mission_workflow distill_mode=sonnet, persist=true)"]
            :status "code-aligned partial — workflow-distiller v0 写 workflow sexp / match_rules / workflow 表 (distill_mode=sonnet 读 succeeded plan + evidence sidecar); file-first .lisp 自动同步 / 高阶 semantic lifting / 自动 record_execution 关联 仍 pending"
            :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/workflow.rs (schema: distill_mode)"
                                     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs (action=distill sonnet branch + match_rules JSON 生成)"]
            :pending ["file-first .missiond/workflows/<topic>.lisp 自动写入/与 workflow 表双向同步"
                      "distiller 高阶 semantic lifting (phase / anti-pattern / authority 边界) — 当前只产 sexp + 简单 match_rules"
                      "成功 plan 自动触发 distill (无需 caller 主动调) — 当前手动"
                      "record_execution 与 distill 双向自动联动"]
            :downstream "machine execution 走 F-methodology-to-executable-compile (compile_mode=deterministic + run_methodology) → compiled YAML → flow-engine-v2 runner path; mission_flow_run discoverability / generated flow loader 是后续独立 code-alignment"))
      :egress
        (writes [".missiond/alignment/<topic>/intent-alignment.lisp"
                 ".missiond/plans/<topic>/PLAN.lisp"
                 ".missiond/v2/plans/<plan_id>.evidence.json"
                 ".missiond/workflows/<topic>.lisp"
                 "optional directive / plan / workflow DB mirror rows"
                 "optional ExecutionEvent + execution companion log entries"]
         reads ["user message" "architecture Lisp diffs" "repo state" "run evidence"]
         downstream ["F-directive-plan-workflow-compile" "F-execution-log-governance" "F-capability-usage-monitoring" "F-methodology-to-executable-compile"]
         returns "reviewed alignment + reviewed plan + execution evidence + distilled reusable workflow")
      :review-gates ["alignment-review-gate (s3)" "plan-review-gate (s5)"]
      :file-vs-db-contract "file-first SSOT — alignment/plan/workflow .lisp 是 human/agent 真正 review 边界; directive/plan/workflow DB 行是可查询镜像 + 状态管理面"
      :file-first-writer-status "code-alignment pending — directive/plan/workflow sexp + DB row 已 code-aligned; 但自动同步到 .missiond/alignment/<topic>/intent-alignment.lisp / .missiond/plans/<topic>/PLAN.lisp / .missiond/workflows/<topic>.lisp 文件 仍未实现, 当前由人工产出"
      :implementation-target-policy "本 flow 各 stage 的 :implementation-targets 命名 *current code-aligned entry points*, 不是最终 module boundaries; future code-convergence 可能把现有大 handler 文件按 Lisp 声明拆分成更细的 module / actor"
      :no-new-tool-decision "当前不新增 mission_message / mission_invoke; mission_directive(action=compile) 已是充分管理入口, 详 future-flows :: unified-entry-future-candidates"
      :tools-backref ["mission_directive (message intake + directive-compiler v0 via compiler_mode=sonnet)"
                      "mission_plan (PLAN.lisp 管理面 + plan-compiler v0 via compiler_mode=sonnet + plan-runner v0 via execute_mode=internal + dispatch_strategy + record_evidence)"
                      "mission_workflow (distiller v0 via distill_mode=sonnet + methodology compiler v0 via compile_mode=deterministic + run_methodology)"
                      "mission_execution (execution substrate, 12-action coordination, dispatch_strategy/target_project/requested_cwd 持久化到 companion log meta)"])

    (flow F-workstation-dispatch-policy
      :status "moved-to-shard (operational-practice + code-aligned partial)"
      :file-ref ".missiond/v2/intent-workstation-policy.lisp"
      :shard-section "F-workstation-dispatch-policy"
      :section-id "flow.workstation-dispatch-policy"
      :role "narrative — see shard for full 5-stage spec; section-id stable per L2 plan rule-1"
      :worker-cross-ref "worker pillar :: section claudecode-workstation-orchestration (also moved to shard)"
      :intent-layer-cross-ref "intent-layer pillar :: section unified-entry-pipeline :: workstation-dispatch-policy (also moved to shard)"
      :tools-backref ["mission_pty_spawn" "mission_pty_send" "mission_pty_read"
                      "mission_compute_slot" "mission_task_delegate"
                      "mission_execution" "mission_plan"])

    (flow F-methodology-to-executable-compile
      :desc "methodology Lisp SSOT → executable YAML artifact → flow-engine-v2 run"
      :status "code-aligned partial — methodology compiler v0 (compile_mode=deterministic + run_methodology, paren-validate + (step …) 提取 + YAML 生成 + 复用 flow-engine-v2 runner) + generated flow loader (search order: explicit > <project_root>/.missiond/generated/flows > $MISSIOND_HOME/flows; mission_flow_run 暴露 flow_source/flow_path/searched_paths/project_root_status); 高阶 forge compiler / semantic lifting / longest-prefix cwd resolver / record_execution-distill 联动 仍 pending"
      :architecture-decision "不先新增 direct mission_workflow_execute; methodology Lisp 保持人类/agent SSOT, 机器执行先编译为 YAML 再走既有 mission_flow_run"
      :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/workflow.rs (schema: compile_mode + run_methodology params)"
                               "crates/missiond-daemon/src/handlers/knowledge/workflow.rs (action_compile_methodology deterministic + action_run_methodology)"
                               "crates/missiond-daemon/src/engine/flow/loader.rs (generated flow loader: GENERATED_FLOWS_REL + searched_paths)"
                               "crates/missiond-daemon/src/handlers/compute/flow_run.rs (mission_flow_run discoverability: flow_source / project_root_status)"
                               "crates/missiond-mcp/src/tools/compute/flow_run.rs (schema: project/target_project/cwd + list dedupe)"]
      :pending ["forge compiler / 高阶 semantic lifting (phases / anti-patterns / authority) — v0 仅做 paren-validate + (step …) 抽取"
                "richer project-root resolution via longest-prefix cwd resolver"
                "automatic record_execution / distill feedback link (s7 feedback-and-distill 仍未自动联动)"]
      :triggers
        ["mission_workflow(action=compile_methodology|run_methodology)"
         "mission_forge_build / mission_forge_lint extension"
         "human requests to execute .missiond/workflows/<name>.lisp"]
      :ingress
        (entry "workflow_path or workflow_name + params + target_project + dry_run/run mode")
      :stages
        ((s1 load-methodology-lisp
            :at "intent-layer pillar :: workflows :: kind methodology"
            :source ".missiond/workflows/<name>.lisp"
            :action "read methodology source, resolve project/global workflow search path, compute source_hash")
         (s2 parse-methodology-contract
            :at "intent-layer pillar :: workflow compiler v0 (deterministic) — phases / anti-pattern semantic lifting still future"
            :action "compile_mode=deterministic 校验括号 + 提取 (step ...); 高阶 phases/gates/anti-patterns/authority lifting 仍 code-alignment pending"
            :code-alignment "deterministic v0 code-aligned; semantic lifting pending")
         (s3 map-to-flow-definition
            :at "intent-layer + worker flow schema boundary"
            :action "map methodology steps to FlowDefinition nodes: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
            :code-alignment "v0 仅生成 (step …) 对应的简单节点; 全 5 类型映射 / 变量引用校验仍 pending")
         (s4 lint-generated-flow
            :at "intent-layer forge lint + worker flow loader"
            :action "validate node schema, tool names, params, variable references, slot requirements, and unsafe operations"
            :code-alignment "v0 暂不做完整 lint, 由后续 forge_lint extension 补; pending")
         (s5 write-executable-yaml
            :at "memory/filesystem boundary :: <project_root>/.missiond/generated/flows"
            :writes "<project_root>/.missiond/generated/flows/<flow_id>.yaml (默认 flow_id=methodology-<stem>-v0)"
            :metadata ["source_lisp" "source_hash" "compiler_version" "generated_at" "manual_review_required?"]
            :code-alignment "compile_mode=deterministic + persist=true 已写; mission_flow_run 已 code-aligned partial 自动发现 (search order: explicit flow_path > <project_root>/.missiond/generated/flows/<flow_id>.yaml > $MISSIOND_HOME/flows/<flow_id>.yaml; action=list 合并 generated + core, generated 优先去重; response 暴露 flow_source / searched_paths / project_root_status); longest-prefix cwd resolver 仍 pending")
         (s6 dry-run-or-run
            :at "tools pillar :: mission_workflow(action=run_methodology) → worker pillar :: flow-engine-v2 runner path"
            :action "dry-run returns compiled plan; run 复用 flow-engine-v2 runner 执行 compiled YAML"
            :code-alignment "run_methodology 解析 flow_id|flow_path|name 找 compiled YAML; dry_run=true 返 would_run; dry_run=false 复用 flow-engine-v2 runner path 执行; mission_flow_run discoverability / generated-flow-loader 是后续独立 code-alignment 任务")
         (s7 feedback-and-distill
            :at "intent-layer pillar :: workflow-distiller"
            :action "record compile/run feedback for future workflow template improvement"))
      :egress
        (writes ["generated executable YAML" "optional workflow compile record" "optional board/flow run state"]
         reads ["methodology Lisp" "flow-engine-v2 schema" "tool registry"]
         returns "compiled flow_id/path, lint result, optional mission_flow_run result")
      :tools-backref ["mission_forge_build" "mission_forge_lint" "mission_flow_run" "mission_workflow"])

    (flow F5-flow-engine-v2-node-execution
      :desc "YAML flow 定义 → node-sequence 执行 → board_tasks.flow_context 持久化"
      :triggers
        ["mission_flow_run (action=run)"
         "board_task running 且 flow_template 非空 (F1 s4 触发)"]
      :added "commit 49bd316 (2026-04-14)"
      :stages
        ((s1 load-yaml
            :at "worker pillar :: section engine-cluster :: flow-engine-v2 :: flow-definition-load"
            :source "$MISSIOND_HOME/flows/<flow_id>.yaml"
            :parser "serde_yaml::from_str::<FlowDefinition>")
         (s2 init-context
            :at "worker pillar :: flow-engine-v2 runner"
            :action "FlowContext{vars, current_node=0, completed_nodes=[]}")
         (s3 execute-node-loop
            :at "worker pillar :: flow-engine-v2 :: flow-node-handler-dispatch"
            :loop "iterate flow.nodes skip(ctx.current_node)"
            :node-types-supported
              ["LlmCall → llm_gateway (gemini/sonnet)"
               "SlotTask → pty.send_fire_and_forget (non-excluded running slot)"
               "McpTool → handlers::dispatch_tool"
               "DaemonAction → read_intent_lisp / close_flow 等"
               "ParallelSlotTasks → JoinSet + Arc<Semaphore>(effective) round-robin"])
         (s3b parallel-slot-tasks-phase2
            :at "worker pillar :: flow-engine-v2 :: ParallelSlotTasks handler"
            :status "architecture-designed; code-alignment pending"
            :ingress "ParallelSlotTasks node with task list, slot_selector, max_concurrency, join_policy, save_as"
            :logic-core
              ((step p1 "resolve eligible running non-excluded slots by role/project/capability; fail-fast if none")
               (step p2 "expand each parallel item into SlotTaskDispatch{slot_id, prompt, vars, timeout, idempotency_key}")
               (step p3 "schedule dispatches via JoinSet guarded by Arc<Semaphore>(max_concurrency); default round-robin over eligible slots")
               (step p4 "each child writes child_result{task_id, slot_id, status, output_ref, error, started_at, finished_at}")
               (step p5 "aggregate results by join_policy: all_success / best_effort / first_success / quorum(n)")
               (step p6 "persist partial results after each child completion to board_tasks.flow_context[save_as], not only at final join")
               (step p7 "on cancellation/timeout, interrupt only dispatched child tasks owned by this node; leave slots alive")
               (step p8 "emit optional TaskEvent::Completed / IncidentEvent::Reported for failed child aggregate when event-bus implementation is wired"))
            :egress
              (writes ["board_tasks.flow_context.<save_as>.children" "completed_nodes when aggregate policy passes" "ctx.last_error when policy fails"]
               emits ["future TaskEvent child result events" "IncidentEvent for systematic fan-out failure"]
               returns "ParallelAggregate{children, success_count, failure_count, selected_output}"))
         (s4 per-node-save-and-persist
            :at "worker pillar :: flow-engine-v2 :: flow-runner-persist"
            :action "save_as → ctx.vars; completed_nodes.push; persist_context(ctx → board_tasks.flow_context)"
            :retry "ErrorPolicy::Retry(N) → 2^attempt exp backoff")
         (s5 advance-or-complete
            :at "worker pillar :: flow-engine-v2 + memory :: board"
            :writes ["board_tasks.flow_context" "board_tasks.flow_phase"
                     "board_tasks.status (completed/failed)"])
         (s6 on-error
            :at "flow-engine-v2 runner"
            :writes "ctx.last_error = Some(e.to_string()); persist"
            :policy "ErrorPolicy::Stop → propagate / Skip → warn + continue / Retry → retry"))
      :fail-fast-invariants
        ["Slot not running → immediate err"
         "Unknown LLM provider → immediate err"
         "Unknown daemon action → immediate err"
         "Flow YAML not found → immediate err"
         "ParallelSlotTasks empty tasks → immediate err"
         "ParallelSlotTasks no running non-excluded slots → immediate err"
         "Async recursion cycle broken via Box::pin"]
      :tools-backref ["mission_flow_run"]
      :note "仅此 flow 是真正的 tool→flow→worker→memory 完整 5 跳链路. 其他 tools 当前 3 跳 (tool→handler→memory/worker) 无 flow 抽象")

    (flow F-dynamic-slot-lifecycle
      :desc "动态计算工位生命周期 — create/terminate/extend/list + async job + spawn_tracked_slot"
      :triggers ["mission_compute_slot(action=create|terminate|extend|list)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
      :stages
        ((s1 action-dispatch
            :at "tools pillar :: compute_slot handler"
            :action "create/terminate/extend/list")
         (s2 create-validate
            :branch "create"
            :at "worker pillar :: dynamic slot handler"
            :guards ["template in coder/ops/researcher" "active dynamic slots < 5" "cwd resolves to registered target_project_root" "spawn cwd must equal project root" "TTL 5m..8h"])
         (s3 persist-and-register
            :branch "create"
            :at "memory + worker"
            :writes ["dynamic_slots active row" "SlotManager runtime dynamic slot"])
         (s4 create-async-job
            :branch "create"
            :at "worker pillar :: job_store"
            :writes "AsyncJob running mission_compute_slot:create")
                 (s5 spawn-background-pty
                    :branch "create"
                    :at "worker pillar :: slot_orchestrator::spawner::spawn_tracked_slot"
                    :action "init PTY slot with process cwd=target_project_root, inject permissions via bottleneck, wait_for_idle=60s, initial_prompt=objective")
         (s6 complete-or-fail-job
            :branch "create"
            :at "job_store + memory"
            :action "complete job with slot metadata or terminate DB row + unregister dynamic slot on spawn failure")
         (s7 terminate
            :branch "terminate"
            :guards "slot_id must start slot-dyn-"
            :action "kill PTY, mark dynamic_slots terminated, unregister SlotManager slot")
         (s8 extend
            :branch "extend"
            :guards "additional_seconds <= 3600 and max extension count not exceeded"
            :writes "dynamic_slots.expires_at")
         (s9 list
            :branch "list"
            :reads ["dynamic_slots by optional status" "static SlotManager slots"]
            :returns "static_slots + dynamic_slots + active count/limit"))
      :tools-backref ["mission_compute_slot" "mission_job_poll"]
      :downstream ["F-workflow-slot-full-lifecycle s2/s7" "F-task-delegate-autoprovision s3 when task_delegate auto-provisions"])

    (flow F-task-delegate-autoprovision
      :desc "声明式任务委派 → slot 选择/动态开槽 → board_task 入队 → 立即触发 dispatch"
      :triggers ["mission_task_delegate(objective, intent, cwd, timeout_secs, priority, depends_on, context_hints)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
              :stages
                ((s1 validate-and-classify
                    :at "tools pillar :: compute/task_delegate + daemon handler"
                    :action "校验 objective 非空; intent ∈ code/ops/research/general; timeout clamp 60..7200s"
                    :maps "intent → template: code=coder / ops=ops / research=researcher / general=coder")
                 (s1b resolve-target-project-root
                    :at "memory pillar :: project-management :: project-registry"
                    :action "cwd/project_id/board context → target_project_root; requested subdir retained only as prompt/context"
                    :fail-fast "unresolved project root or cwd outside registered project")
                 (s2 reserve-idle-slot
                    :at "worker pillar :: slot_dispatch::SlotAcquireGuard + PTY status"
                    :action "遍历非 excluded roles, try_acquire_guard 后要求 SessionState::Idle and slot.project_root == target_project_root"
                    :excludes ["jarvis" "memory" "supervisor" "decision"])
         (s3 auto-provision-if-needed
            :at "worker pillar :: handlers/compute/compute_slot.rs"
            :action "无 idle slot 且非 ops 时, 委托 mission_compute_slot(action=create)"
            :constraints "dynamic_slots active < 5; ttl=max(timeout+300,3600); spawn async job; autopilot 后续 pickup"
            :flow-ref "F-workflow-slot-full-lifecycle :: s2-slot-provision")
         (s4 context-hints
            :at "worker-side computation + memory readers"
            :reads ["kb_search(first 3)" "skills.search(first 3)"]
            :limits "500 chars per entry, 2000 chars context block, 16000 chars final description")
         (s5 create-board-task
            :at "memory pillar :: module board"
            :writes "board_tasks(title, description, priority, assignee?, auto_execute=true, depends_on?, timeout_secs, context_intent)"
            :flow-ref "F1-board-task-main-lifecycle :: s1")
         (s6 notify-dispatch
            :at "worker pillar :: board_dispatch_notify"
            :action "notify_one(), 不等 60s autopilot tick"
            :downstream "F1-board-task-main-lifecycle :: s2 scan-decide / s3 claim / s4 execute"))
      :tools-backref ["mission_task_delegate" "mission_compute_slot" "mission_board_query"]
      :ownership
        (tools "schema + MCP endpoint")
        (worker "slot selection, guard, auto-provision, dispatch notify")
        (memory "board_tasks + dynamic_slots/slot_sessions")
        (flow "把 delegate 从一条 tool call 展开成 task lifecycle narrative"))

    (flow F-task-submit-dispatch
      :desc "legacy task submit → tasks queue → optional immediate PTY dispatch / auto-spawn → fallback queued"
      :triggers ["mission_task_submit(action=async|sync, role, prompt|question, slotId?, timeoutMs?)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/compute/task.rs :: handle_submit/handle_ask"
      :stages
        ((s1 action-dispatch
            :at "tools pillar :: compute/task handler"
            :action "action async → handle_submit; action sync → handle_ask (当前只建 task, 不阻塞等结果)"
            :tools-consumed ["mission_task_submit"])
         (s2 create-legacy-task
            :at "memory pillar :: system-support legacy tasks"
            :action "crate::state::submit_task(role, prompt|question)"
            :writes "tasks row queued")
         (s3 bind-target-slot
            :at "task store"
            :optional true
            :action "若 slotId 指定, 写入 task.slot_id 作为 autopilot fallback")
         (s4 select-candidates
            :at "worker pillar :: slot registry"
            :action "slotId 指定则单候选; 否则按 slot.config.role == role 过滤")
         (s5 idle-immediate-dispatch
            :at "worker pillar :: pty + slot_dispatch guard"
            :action "若候选 slot Idle, send_fire_and_forget(prompt), 更新 tasks status=running/slot_id/session_id/started_at"
            :emits "SlotEvent::TaskDispatched{purpose=submit}")
         (s6 auto-spawn-exited-slot
            :at "worker pillar :: slot_orchestrator::spawner::spawn_tracked_slot"
            :action "若无 idle dispatch, 对 Exited/None 状态候选 wait_for_idle spawn 后再发 prompt"
            :emits "SlotEvent::TaskDispatched{purpose=submit}")
         (s7 fallback-queued
            :at "event-bus pillar :: TaskEvent"
            :action "若仍未 dispatch, 返回 dispatched=false 并发布 TaskEvent::Created"
            :emits "TaskEvent::Created")
         (s8 return-receipt
            :at "tools pillar"
            :returns "ToolResult json {taskId, dispatched, slotId?|hint?}"))
      :tools-backref ["mission_task_submit"]
      :ownership
        (tools "schema + action dispatch")
        (worker "slot guard / PTY send / auto-spawn mechanics")
        (memory "legacy tasks queue")
        (event-bus "TaskEvent::Created / SlotEvent::TaskDispatched")
        (flow "把 legacy task queue 的 submit 分支同 board_task delegate 分开描述"))

    (flow F-task-legacy-queue-control
      :desc "legacy tasks queue 查询/追踪/确认/取消 — 非 board_tasks lifecycle"
      :triggers ["mission_task_query(action=status|list|ack|track)" "mission_task_cancel(taskId)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/compute/task.rs :: query/cancel/track"
      :stages
        ((s1 action-dispatch
            :at "tools pillar :: compute/task handler"
            :action "query: status/list/ack/track; cancel: handle_cancel"
            :tools-consumed ["mission_task_query" "mission_task_cancel"])
         (s2 status-or-list
            :at "memory pillar :: system-support legacy tasks"
            :branch "mission_task_query status/list"
            :reads "get_task / get_tasks_by_status / get_all_tasks")
         (s3 ack-completed
            :at "memory pillar :: system-support legacy tasks"
            :branch "mission_task_query ack"
            :action "ack_completed_tasks(since)")
         (s4 track-aggregate
            :at "worker + memory"
            :branch "mission_task_query track"
            :reads ["task row" "PTY slot status" "slot_session" "conversation jsonl metadata" "slot_progress" "slot_last_responses"])
         (s5 cancel-guarded-update
            :at "memory pillar :: system-support legacy tasks"
            :branch "mission_task_cancel"
            :guards "only Queued or Running"
            :writes "status=Cancelled + finished_at=now")
         (s6 return-result
            :at "tools pillar"
            :returns "ToolResult json/json_pretty/error"))
      :tools-backref ["mission_task_query" "mission_task_cancel"]
      :ownership
        (tools "schema + action dispatch")
        (worker "PTY/progress aggregation for track")
        (memory "legacy tasks queue state")
        (flow "把 query/cancel 合并为 legacy queue control shared-flow"))

    ;; ── F-execution-log-governance moved to L2 shard ──
    ;; Full content moved to .missiond/v2/intent-execution-governance.lisp
    (flow F-execution-log-governance
      :status "moved-to-shard (code-aligned partial)"
      :file-ref ".missiond/v2/intent-execution-governance.lisp"
      :shard-section "F-execution-log-governance"
      :section-id "flow.execution-log-governance"
      :tools-backref ["mission_execution"]
      :note "8 stages + 12-action manager + cross-pillar + scoped commit handoff cross-ref live in shard; section-id stable per L2 plan rule-1")

    ;; ── F-scoped-commit-handoff moved to L2 shard ──
    ;; Full content moved to .missiond/v2/intent-execution-governance.lisp
    (flow F-scoped-commit-handoff
      :status "moved-to-shard (architecture-designed + task-file operational-practice)"
      :file-ref ".missiond/v2/intent-execution-governance.lisp"
      :shard-section "F-scoped-commit-handoff"
      :section-id "flow.scoped-commit-handoff"
      :tools-backref ["mission_execution"]
      :note "7 stages + commit-policy + failure-modes + cross-pillar live in shard; section-id stable per L2 plan rule-1")

    (flow F-skill-knowledge-lifecycle
      :desc "Skill registry/query/context/mutation → skill tables/files + embeddings + context bundle"
      :triggers ["mission_skill_query(action=list|search|topics|actions|stats)" "mission_skill_context(action=build|resolve)" "mission_skill_mutate(action=upsert|record|render|rollback)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/knowledge/skill.rs"
      :stages
        ((s1 action-dispatch
            :at "tools pillar :: knowledge/skill handler"
            :action "query/context/mutate 三个 consolidated tool 分派到 legacy inner handlers")
         (s2 query-skill-hub
            :at "memory pillar :: skill store + in-memory skill index"
            :actions ["list skill metadata" "topics list" "actions parse workflow metadata" "stats read skill_executions"])
         (s3 search-skills
            :at "worker/memory retrieval"
            :branch "mission_skill_query action=search"
            :action "name/aka bonus + skill FTS + skill embedding cosine + RRF; record top topic hits")
         (s4 build-context
            :at "worker pillar :: context assembly"
            :branch "mission_skill_context action=build"
            :action "state.skills.build_context(query) + KB search budget block")
         (s5 resolve-context
            :at "worker pillar :: context assembly"
            :branch "mission_skill_context action=resolve"
            :action "primary skill selection, requires_json dependency expansion to 2 layers, infra/kb aggregation, optional board search")
         (s6 mutate-skill-content
            :at "memory pillar :: skill topic/block store + filesystem"
            :branch "mission_skill_mutate upsert/record"
            :writes ["skill_topics when auto-created" "skill_blocks" "SKILL.md materialized"])
         (s7 refresh-skill-embedding
            :at "worker pillar :: embedding-worker-loop"
            :condition "upsert/record"
            :action "embedding_tx.try_send(EmbeddingTask::ProcessSkillTopic)")
         (s8 render-or-rollback
            :at "filesystem + skill ingestion"
            :branch "mission_skill_mutate render/rollback"
            :action "materialize one/all topics; or restore version content and re-ingest skill directory")
         (s9 return-result
            :returns "query/context/mutation result; downstream F7 refreshes searchable skill embeddings"))
      :tools-backref ["mission_skill_query" "mission_skill_context" "mission_skill_mutate"]
      :downstream ["F7-embedding-pipeline when skill content changes" "F10-context-assembly consumes resolved skill context"])

    (flow F-skill-workflow-execution
      :desc "Skill action workflow → 顺序 MCP tool steps → skill_executions 审计"
      :triggers ["mission_skill_exec(skill, action, dry_run?, params?)"]
      :phase-C-verified "2026-04-25 — engine/intent_engine/workflow_executor.rs + handlers/knowledge/skill.rs"
      :distinguish-from-flow-v2 "不是 flow-engine-v2 的别名; 它读取 skill 文件中的 ```workflow block, 以 MCP tool step 顺序执行"
      :long-term-boundary "保留独立 runtime: skill_exec 是 skill-local action macro executor; flow-engine-v2 是 durable YAML orchestration. 未来只做 definition adapter, 不强行合并 executor"
      :stages
        ((s1 load-skill-topic
            :at "memory pillar :: project-management/skill store + filesystem"
            :reads "skill_topic_get(skill) → topic.file_path → read_to_string")
         (s2 parse-workflow-block
            :at "missiond_core::skill::parse_workflow_blocks"
            :action "找 workflow.id == action; 解析 steps(tool, params, save_as, on_error)")
         (s3 approval-or-preview
            :at "workflow_executor"
            :action "requires_approval 且非 dry_run → PendingApproval; dry_run → WorkflowStepPreview")
         (s4 create-execution-log
            :at "memory pillar :: project-management skill_executions"
            :writes "skill_execution_insert(exec_id, skill, action, steps_total, triggered_by=manual)")
         (s5 context-hooks
            :at "worker pillar :: workflow_executor"
            :action "执行 context_hooks, 每 hook 10s timeout, 输出 save_as 到 context")
         (s6 run-steps
            :at "worker pillar :: AppState::call_tool → handlers::dispatch_tool"
            :action "逐 step resolve ${var}; call MCP tool with 30s timeout; save_as 写 context"
            :error-policy "stop / skip / retry(exponential 1,2,4...) / fallback:step_id; MAX_STEP_VISITS=5; MAX_DEPTH=3")
         (s7 persist-result
            :at "memory pillar :: skill_executions"
            :writes "status running/success/failed + steps_completed + context_json + error + duration_ms"))
      :tools-backref ["mission_skill_exec"]
      :design-decision "当前保留为 skill-local workflow runtime; 与 flow-engine-v2 的合并是未来产品决策, 不是代码真相")

    (flow F-cc-swarm-pty-prompt
      :desc "Claude Code teammate swarm prompt wrapper — 单 slot 内部并行, 非 daemon-owned ParallelSlotTasks"
      :triggers ["mission_cc_swarm(slotId, tasks[], teammateCount?, timeoutMs?)"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/compute/cc_tasks.rs :: mission_cc_trigger_swarm"
      :distinguish-from-flow-v2 "F5 的 ParallelSlotTasks 是 daemon flow node fan-out; mission_cc_swarm 只是构造 prompt 并调用 state.pty.send(slot_id, prompt, timeout_ms)"
      :stages
        ((s1 parse-args
            :at "tools pillar :: cc_tasks handler"
            :reads "slot_id, tasks[], teammate_count(default 3), timeout_ms(default 600000)"
            :tools-consumed ["mission_cc_swarm"])
         (s2 build-swarm-prompt
            :at "cc_tasks handler"
            :action "把 tasks[] 编号后拼成 'Plan 模式 + teammate 并行执行' prompt")
         (s3 send-to-slot
            :at "worker pillar :: pty transport"
            :action "state.pty.send(slot_id, prompt, timeout_ms), 阻塞等待响应")
         (s4 observe-via-cc-tasks
            :at "worker/local conversation ingestion + cc_tasks watcher"
            :optional true
            :tools-consumed ["mission_cc_query"]
            :reads "Claude Code session/task JSONL derived state"))
      :tools-backref ["mission_cc_swarm" "mission_cc_query"]
      :ownership
        (tools "schema + consolidated cc_tasks action")
        (worker "PTY send + cc_tasks watcher observation")
        (flow "标明这是 prompt-level swarm, 不要误并入 flow-engine-v2 ParallelSlotTasks"))

    (flow F-workflow-slot-full-lifecycle
      :desc "通用 slot 全生命周期 E2E — 开工位 → 工位就绪 → 指挥执行 → 监控 → 完成检测 → 回收 → 审计. 覆盖 4 类 workflow 源"
      :triggers
        ["指挥官或 intent-layer 发起 workflow 执行"
         "autopilot board_task dispatch (带 flow_template)"
         "mission_task_delegate (声明式委派)"
         "skill workflow 执行 (mission_skill_exec)"]
      :purpose "回答'按现在设计能否开工位/指挥/回收 workflow'的完整 flow; 做 MCP/worker/memory 覆盖率检查"

      (workflow-kinds-supported
        :desc "4 类 workflow 源, 各类对应不同 s4 dispatch tool"
        (methodology-lisp
          :path ".missiond/workflows/*.lisp (intent-layer :: workflows :: methodology)"
          :example "pillar-refactor.lisp / bus-refactor.lisp"
          :dispatch-status "architecture-designed: F-methodology-to-executable-compile → mission_flow_run; code-alignment pending")
        (executable-yaml
          :path "$MISSIOND_HOME/flows/*.yaml (intent-layer :: workflows :: executable)"
          :dispatch "✓ mission_flow_run (flow-engine-v2) — 完整路径")
        (skill-workflow
          :path "kb_entries skill topic action block"
          :dispatch "✓ mission_skill_exec — 30s/step + MAX_DEPTH=3"
          :flow-ref "F-skill-workflow-execution")
        (free-task
          :path "自然语言 prompt (ad-hoc)"
          :dispatch "✓ mission_task_submit / mission_task_delegate / mission_pty_send"))

      :stages

        ;; ── Stage 1: 载入 workflow 定义 ──
                ((s1-workflow-definition-load
            :at "tools pillar + intent-layer"
            :tools-alternatives
              ((methodology  "architecture target: mission_workflow/forge compiler reads .missiond/workflows/*.lisp → generates executable YAML; current mission_workflow compile_methodology is dry-run preview")
               (executable   "✓ mission_flow_run(action=list|status) → loader.rs 从 $MISSIOND_HOME/flows/<id>.yaml 载入")
               (skill        "✓ mission_skill_query(action=list|get) + mission_skill_context(action=resolve)")
               (free         "无需定义载入"))
            :reads ["<project>/.missiond/intent.lisp (intent tool)" "$MISSIOND_HOME/flows/*.yaml (loader.rs)" "kb_entries (skill)"]
                    :writes [])

                 ;; ── Stage 1.5: Resolve Target Project Root ──
                 (s1b-target-project-root
                    :at "memory pillar :: project-management :: project-registry"
                    :action "resolve project_id/cwd/task context to canonical target_project_root before any slot spawn or slot reuse"
                    :rules ["process cwd for ClaudeCode/Gemini/Codex must be target_project_root"
                            "requested cwd below project root is prompt/context metadata, not spawn cwd"
                            "existing slot reuse requires slot.project_root == target_project_root"
                            "Gemini/Codex hard-fail when target root is unresolved"])

                 ;; ── Stage 2: Slot Provision (5 种路径) ──
                 (s2-slot-provision
                    :at "worker pillar :: section pty :: subsection slot-orchestrator"
                    :worker-path "invariant sole-spawn-bottleneck + project-root-spawn-cwd (spawner.rs::spawn_tracked_slot — ALL callers 经此, process cwd=target_project_root)"
            :tools-alternatives
              ((option-A-复用-persistent
                  :desc "复用 slots.yaml 里 auto_start=true 的 registered slot (4 种): arch-surveyor / strategy / gemini-router / lisp-surveyor"
                  :tool "无需开, mission_slots(list) 查现有 state, 直接 s4 dispatch"
                  :constraint "只适用 task_type 匹配 registered-task 的场景 (arch_maintenance / strategy_analyst / gemini_router / lisp_survey)")
                       (option-B-开固定-slotId
                          :desc "按固定 slot_id 开 PTY session (如手动 spawn claude-code-opus)"
                          :tool "mission_pty_spawn(slotId, waitForIdle?, timeoutSecs?, autoRestart?, mcpConfigPath?)"
                          :worker-path "spawn_tracked_slot 经 project-root resolution → perm-inject → tracking-env → pty-spawn(project_root) → uuid-capture → initial-prompt")
               (option-C-动态-TTL
                          :desc "动态 compute_slot (5 active 上限, 4h default / 8h max TTL)"
                          :tool "mission_compute_slot(action=create, template=coder|ops, objective, cwd, max_ttl)"
                          :writes ["dynamic_slots (new row)" "slot_sessions"]
                          :constraint "cwd must resolve to target_project_root; max 5 active; TTL 可 extend (action=extend, +3600s max 每次)")
               (option-D-声明式-委派
                  :desc "daemon 自主选 slot — 给 objective 让它挑"
                  :tool "mission_task_delegate(objective, intent=code|ops|research|general, cwd, timeout_secs≤7200, priority, depends_on)"
                  :worker-path "autopilot + slot selection heuristics")
               (option-E-隐式-board
                  :desc "创建 board_task 带 flow_template, autopilot 扫 + claim + 派发"
                  :tool "mission_board_create(title, flowTemplate, ...) + autopilot-tick claim"
                  :flow-ref "F1-board-task-main-lifecycle s1→s3"))
                    :writes ["slot_sessions" "dynamic_slots (若 option C)" "board_tasks (若 option E)"]
                    :via-bus ["SlotSessionChanged"])

         ;; ── Stage 3: Slot Readiness (等 Idle) ──
         (s3-slot-readiness
            :desc "等 slot 到 FSM Idle state 方可发 prompt"
            :at "worker pillar :: section pty :: subsection pty-state-machine + semantic-parser"
            :fsm-path "pty-session FSM: Starting → Idle (trigger prompt-detected)"
            :mechanisms
              ((阻塞式 "mission_pty_spawn(waitForIdle=true, timeoutSecs=60) — 一步到位, 开 + 等 Idle")
               (轮询式 "mission_pty_status 看 FSM 当前 state, 循环到 Idle")
               (事件式 "subscribe ManagerEvent::StateChange 到 Idle (semantic-parser 识别 prompt)"))
            :semantic-parser-role "CC detection-order: trust-dialog → confirm-dialog → idle-or-slash → processing → responding → error; Gemini: error → thinking → responding → tool-running → idle → idle-placeholder → idle-transitional"
            :gap-check "semantic-parser 需准确识别 CC/Gemini/Codex 3 种 CLI 的 Idle 签名 — 已有实现 (missiond-core/src/semantic_parsing/ Forge 冲压)")

         ;; ── Stage 4: Dispatch Workflow (最核心) ──
         (s4-dispatch-workflow
            :at "tools pillar + worker pillar 执行点"
            :tools-alternatives
              ((methodology-lisp
                  :desc "architecture target: methodology Lisp 作为 SSOT, 先编译为 executable YAML, 再走 mission_flow_run"
                  :current-workaround "人工 Read .missiond/workflows/<name>.lisp → 塞内容到 mission_task_delegate 或 mission_pty_send"
                  :target-flow "F-methodology-to-executable-compile"
                  :code-alignment "mission_workflow compile_methodology dry-run surface code-aligned; forge extension / compiler actor pending")
               (executable-yaml
                  :desc "✓ 完整路径: 载入 YAML → run flow-engine-v2"
                  :tool "mission_flow_run(flow_id, params, action=run)"
                  :flow-ref "F5-flow-engine-v2-node-execution")
               (skill-workflow
                  :desc "✓ skill topic 的 action block 执行 (类似小 orchestrator)"
                  :tool "mission_skill_exec(skill, action, dry_run?, params?)"
                  :worker-path "section engine-cluster :: intent-engine :: workflow-executor-runtime (30s/step MCP dispatch + MAX_DEPTH=3)"
                  :flow-ref "F-skill-workflow-execution")
               (free-simple
                  :desc "直接塞 prompt, 阻塞等回复"
                  :tool "mission_pty_send(slotId, message, waitForResponse=true, timeoutMs?)")
               (free-async
                  :desc "异步任务 — 提交后轮询"
                  :tool "mission_task_submit(role, prompt, action=async) → mission_task_query(action=status|track) / mission_job_poll(job_id)")
               (multi-agent-swarm
                  :desc "多 agent 并发 (适合大任务拆分)"
                  :tool "mission_cc_swarm(slotId, tasks[], teammateCount=3, timeoutMs?)"
                  :flow-ref "F-cc-swarm-pty-prompt"
                  :cross-ref "与 flow-engine-v2 ParallelSlotTasks 是两个不同实现路径; 当前 tool 只向单个 PTY slot 发 teammate prompt"))
                    :worker-paths
                      ["section pty :: subsection slot-orchestrator :: claude-slot-dispatch (CC 类)"
                       "section pty :: subsection slot-orchestrator :: gemini-slot-dispatch (Gemini 类)"
                       "section engine-cluster :: flow-engine-v2 :: flow-node-handler-dispatch (YAML)"
                       "section engine-cluster :: intent-engine :: workflow-executor-runtime (skill)"]
                    :slot-reuse-invariant "all project-bound dispatch must use a slot whose project_root equals target_project_root; otherwise provision a new slot in that root"
                    :writes ["board_tasks (若走 flow_run 或 task_delegate)" "slot_tasks" "conversation_messages (prompt 进 CC JSONL)"]
            :via-bus ["ManagerEvent::TextComplete (on reply)" "LlmEvent::* (若走 LLM gateway)"])

         ;; ── Stage 5: Monitor Execution ──
         (s5-monitor-execution
            :at "worker pillar :: section pty :: subsection pty-state-machine + pty-transport"
            :tools-alternatives
              ((read-screen "mission_pty_read(action=screen|history|logs, slotId, lines?)")
               (status      "mission_pty_status(slotId?) — 返回 FSM 当前 state")
               (screenshot  "mission_pty_screenshot(slotId) — PNG 截图")
               (task-track  "mission_task_query(action=status|track, taskId)")
               (job-poll    "mission_job_poll(job_id, action=poll|list|cancel)")
               (watch-events "subscribe SlotBecameIdle / SlotStuck / ManagerEvent::* (适合 daemon 内部订阅)"))
            :fsm-transitions-observed "Idle → Thinking (spinner) → Responding (output) → ToolRunning → [Confirming?] → Idle"
            :anomaly-detection "missiond-pty/anomaly.rs 被动监控 stuck / parser 信心低 / anchor 缺失")

         ;; ── Stage 6: Completion Detection ──
         (s6-completion-detection
            :at "worker + memory pillar + event-bus"
            :mechanisms
              ((event-trigger
                  :event "SlotBecameIdle + ManagerEvent::TextComplete"
                  :适用 "所有 slot dispatch"
                  :subscriber "daemon 内部 (worker / flow-engine-v2)")
               (task-poll
                  :tool "mission_task_query(taskId, status=done/failed)"
                  :适用 "option-D 声明式委派 / option-E board task")
               (job-poll
                  :tool "mission_job_poll(job_id, status=completed/failed)"
                  :适用 "异步 task_submit")
               (status-check
                  :tool "mission_pty_status 看 FSM 回 Idle"
                  :适用 "手动监控")
               (autopilot-report-completion
                  :at "F1 s5 report-completion"
                  :适用 "board_task 自动推进"))
            :writes ["board_tasks.status=done/failed (flow/board)" "slot_tasks.status" "skill_execution (若 skill)"]
            :via-bus ["SlotBecameIdle" "SessionCompleted (可能触发)" "BoardEvent::StatusChanged"])

         ;; ── Stage 7: Teardown ──
         (s7-teardown
            :at "worker pillar :: section pty :: subsection slot-orchestrator :: slot-manager-runtime-authority"
            :tools-alternatives
              ((persistent-保留
                  :desc "registered-tasks 的 slot 保留不杀, 等下次复用"
                  :action "仅 release claim (autopilot report-completion 自动做)"
                  :writes "board_tasks.claim_executor_id=NULL")
               (dynamic-terminate
                  :tool "mission_compute_slot(action=terminate, slot_id)"
                  :适用 "option-C 动态 slot 主动清")
               (force-kill
                  :tool "mission_pty_signal(action=kill, slotId)"
                  :适用 "冻死 / 手动强杀")
               (interrupt
                  :tool "mission_pty_signal(action=interrupt, slotId)"
                  :适用 "软中断当前任务但保留 slot")
               (auto-reap-supervisor
                  :at "supervisor.rs (599 行)"
                  :阈值 "Graceful: context < 15% → 标记 Idle 时 restart; Emergency: context < 3% → 强 kill"
                  :recovery "requeue running tasks + release Board claims + sleep 3s + respawn via ensure_memory_slot_by_id"))
            :worker-path "supervision-check (autopilot s5 的 lease recovery / stale task / zombie slot)"
            :writes ["slot_sessions (close)" "dynamic_slots (若 terminate)" "board_tasks (claim 清除)"]
            :via-bus ["SlotSessionChanged (on close)"])

         ;; ── Stage 8: Audit & Retrospective ──
         (s8-audit
            :at "memory pillar"
            :automatic
              ["tool_calls 表 (gen_gateway 每 tool 调用前后自动写)"
               "slot_tasks 历史 (slot_orchestrator 管)"
               "conversation_messages (JSONL 经 conversation_logger 摄入)"]
            :retrospective-trigger
              ["SessionCompleted → retro_worker (sonnet) → deep_analysis + retrospectives"
               "flow-ref: F8-retrospective-to-memory"]
            :tools-for-query
              ["mission_audit(action=trace|detail|stats|export) — 按 sessionId/toolId/taskId 查审计"
               "mission_slot_history(slotId?) — 工位历史"
               "mission_llm_trace(gemini_trace/stats) — LLM 调用链路"])
        )

      ;; ── 覆盖率检查 ──
      :tools-coverage-check
        :spawn "✓ 5 options (persistent/fixed/dynamic/delegate/board) 齐备"
        :dispatch "✓ 6 paths architecturally covered (methodology compile target / executable / skill / free-simple / free-async / cc-swarm) — methodology code-alignment pending"
        :monitor "✓ 6 tools (pty_read/status/screenshot/task_query/job_poll/ManagerEvent) 齐备"
        :teardown "✓ 5 paths (persistent-保留/dynamic-terminate/force-kill/interrupt/auto-reap) 齐备"
        :audit "✓ 自动写 + 3 query tool 齐备"
        :overall "architecture complete for 6 dispatch paths; methodology compile implementation pending"

      :worker-coverage-check
        :spawn-bottleneck "✓ spawner::spawn_tracked_slot 10 callers 统一 (perm-inject + tracking-env + pty-spawn + uuid-capture + initial-prompt 5 stage)"
        :fsm "✓ pty-session FSM 8 states + 14 transitions 完整"
        :semantic-parser "✓ CC+Gemini+Codex 3 种 CLI Idle/Thinking/etc 识别 (Forge 冲压)"
        :supervisor "✓ 599 行 graceful/emergency restart + extraction-phase FSM + recovery 3s"
        :registered-tasks "✓ 4 persistent slot (arch-surveyor/strategy/gemini-router/lisp-surveyor)"
        :dynamic-pool "✓ compute_slot 5 active + 8h TTL"
        :control-tree "✓ 6 层 cascade (global/provider/domain/worker/slot_role/project) + push-based watch"
        :overall "100% 齐备 — worker 侧无缺"

      :memory-coverage-check
        :slot-lifecycle-tables "✓ slot_sessions + slot_tasks + dynamic_slots"
        :board-coordination "✓ board_tasks (含 flow_phase / flow_context / flow_template / claim_executor_id / lease)"
        :audit "✓ tool_calls + conversation_messages + retrospectives + deep_analysis"
        :fsm-persistence "✓ BoardTaskStatus + EngineeringPhase + TaskStatus + SlotTrait 枚举"
        :overall "100% 齐备 — memory 侧 schema 完备"

      ;; ── GAP 清单 ──
      :gaps-identified
        ((GAP-1 (methodology-lisp-execution)
            :severity "medium"
            :status "architecture-designed-code-alignment-pending"
            :desc "'.missiond/workflows/*.lisp' (pillar-refactor / bus-refactor 等人类方法论) 无直接执行 runtime. 架构决定: Lisp 保持 SSOT, 编译为 executable YAML, 再由 mission_flow_run 执行"
            :selected-path "F-methodology-to-executable-compile"
            :rejected-path "不优先做 direct mission_workflow_execute; 避免绕开已有 flow-engine-v2"
            :implementation-needed ["methodology parser/compiler actor" "generated YAML metadata/source_hash" "mission_workflow run_methodology execution or forge compile surface"]
            :affects ["intent-layer :: workflows :: kind methodology 的 executability 字段"
                      "tools pillar :: mission_workflow"
                      "flow pillar :: F-methodology-to-executable-compile"])
         (GAP-2 (orchestration-path-overlap)
            :severity "low-medium"
            :desc "3 条 orchestration 路径并存, 每类 workflow 该走哪条不明确: skill_exec (30s MCP dispatch) vs flow_run (YAML node sequence) vs task_delegate (自然语言)"
            :cross-ref "tools v0.1 T007 决策项"
            :recommendation "指挥官评审: 保留 3 条 / 合并 skill_exec 到 flow_run / 明确分工规则")
         (GAP-3 (long-running-workflow-ttl)
            :severity "low"
            :desc "动态 slot 8h max TTL vs registered persistent slot 永久. 长任务 (>8h) 必须用 persistent, 但 persistent 是 role-based 路由, 灵活度不够"
            :recommendation "未来若需要: 补 'long-running dynamic slot' 概念 或 persistent slot 加 task-based 路由")
         (GAP-4 (slot-failure-retry-flow)
            :severity "low"
            :desc "slot 失败 retry 限于 board_tasks (mission_board_retry), 不针对 slot 本身. 若 dispatch 成功但 slot 内任务失败 (非 board 路径), 无标准 retry 机制"
            :recommendation "对 option-D (task_delegate) / option-F (free-async) 补 slot-level retry 字段")
         (GAP-5 (slot-pool-abstraction-gap)
            :severity "low"
            :desc "动态 slot (compute_slot) vs 持久 slot (registered) 无统一 'slot pool' 抽象. MCP 调用方要自己决定 (用 pty_spawn 还是 compute_slot)"
            :recommendation "未来可补 mission_slot_pool (unified) 抽象; 当前问题不大"))

      :tools-backref-full-list
        ["mission_intent (s1 methodology, ⚠)"
         "mission_flow_run (s1+s4 executable)"
         "mission_skill_query + mission_skill_context + mission_skill_exec (s1+s4 skill)"
         "mission_slots (s2 persistent list)"
         "mission_pty_spawn (s2 固定 slot, s3 waitForIdle)"
         "mission_compute_slot (s2 动态 create + s7 terminate)"
         "mission_task_delegate (s2 声明式 + s4 free-async)"
         "mission_task_submit (s4 free-async)"
         "mission_board_create/claim (s2 隐式 board + F1 s1-s3)"
         "mission_pty_send (s4 free-simple / methodology workaround)"
         "mission_pty_read (s5 monitor screen)"
         "mission_pty_status (s3 poll Idle + s5 monitor)"
         "mission_pty_screenshot (s5 monitor)"
         "mission_task_query (s5+s6 poll done)"
         "mission_job_poll (s5+s6 async poll)"
         "mission_cc_swarm (s4 multi-agent)"
         "mission_pty_signal (s7 kill/interrupt)"
         "mission_audit + mission_slot_history + mission_llm_trace (s8 audit query)"
         "mission_pause + mission_worker (orthogonal control-tree pause/resume)"]

      :overall-conclusion
        "工具面 + worker 执行 + memory 表 架构上已闭合; methodology Lisp 执行已选定 compile-to-YAML → mission_flow_run 路径, 代码对齐待实现. 其他 4 gap 是 low-severity 优化项."))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.7 Category: Cascade Flows
  ;; ══════════════════════════════════════════════════════════
  (category cascade-flows
    :desc "Universe graph / cascade repair — 跨 service 依赖分析与级联修复"

    (flow F-cascade-execution
      :desc "manifest → universe graph → blast radius plan → optional repair execution → task events"
      :triggers ["mission_universe_graph" "mission_cascade_plan" "mission_cascade_trigger" "mission_cascade_lint"]
      :phase-C-verified "2026-04-25 — crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
      :stages
        ((s1 resolve-manifest
            :at "tools pillar :: knowledge/cascade handler"
            :action "manifest_path 或 UNIVERSE_MANIFEST; canonicalize; 限制在 UNIVERSE_ROOT 或 /Users/jinchen/Projects")
         (s2 build-universe-graph
            :at "external forge_core::universe_graph"
            :action "resolve_universe_graph(manifest_path)"
            :tools-consumed ["mission_universe_graph"])
         (s3 plan-blast-radius
            :at "external forge_core::cascade"
            :action "create_plan(graph, ServiceDelta{service, changed}, dry_run=true)"
            :returns "phases + upstream_map"
            :tools-consumed ["mission_cascade_plan"])
         (s4 trigger-guard
            :at "cascade handler"
            :action "CASCADE_TRIGGER_ENABLED kill-switch + path whitelist"
            :tools-consumed ["mission_cascade_trigger"])
         (s5 emit-start
            :at "event-bus pillar :: TaskEvent"
            :emits "TaskEvent::CascadeTriggered{service, changed}")
         (s6 execute-repair
            :at "tokio::task::spawn_blocking + forge_core::cascade::execute_plan"
            :action "外部命令式修复, max_repair_cycles 来自 max_cycles")
         (s7 emit-complete
            :at "event-bus pillar :: TaskEvent"
            :emits "TaskEvent::CascadeCompleted{services_repaired, services_failed, hard_halted, duration_ms}")
         (s8 lint-only
            :at "forge_core::universe_graph::validate_universe_integrity"
            :action "lint 不执行修复, 返回 clean/warnings/failed"
            :tools-consumed ["mission_cascade_lint"]))
      :tools-backref ["mission_universe_graph" "mission_cascade_plan" "mission_cascade_trigger" "mission_cascade_lint"]
      :ownership
        (tools "MCP schema + external trigger")
        (worker "blocking execution wrapper + runtime guard")
        (event-bus "CascadeTriggered / CascadeCompleted")
        (intent-layer "未来可从 directive/plan 生成 cascade trigger")
        (flow "把 forge_core 的 plan/execute/lint 串成系统级 narrative")
      :open-questions
        ["cascade 执行产物是否需要写 board_tasks 或 audit detail, 当前只返回 ToolResult + 发 TaskEvent"
         "CASCADE_TRIGGER_ENABLED 当前 dev-mode 默认 allow, 生产策略待 system-layer/control-tree 决策"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.8 Category: Forge Flows
  ;; ══════════════════════════════════════════════════════════
  (category forge-flows
    :desc "forge build / lint — intent-layer 编译器对外流"

    (flow F-forge-build
      :desc "lisp → IR → rust 冲压"
      :triggers ["mission_forge_build(project, dry_run?, output_dir?)"]
      :stages
        ((s1 resolve-project
            :at "worker pillar :: section worker-side-computation :: forge-build-bridge"
            :action "ProjectRegistry 查 project_id → 项目根")
         (s2 shell-forge
            :at "external ~/Projects/jarvis-forge CLI"
            :command "forge build <root>"
            :env "FORGE_BIN override 可选")
         (s3 forge-internal-pipeline
            :at "intent-layer pillar :: section forge-compilation :: forge-build"
            :action "Lisp → IR → Rust Generation Gap")
         (s4 capture-output
            :at "worker pillar :: forge-build-bridge"
            :captures "stdout / stderr / exit_code / violations"))
      :tools-backref ["mission_forge_build"])

    (flow F-forge-lint
      :desc "governance lint → intent.lisp 合规检查"
      :triggers ["mission_forge_lint(project)" "lisp_survey_worker post-survey (若接入)"]
      :stages
        ((s1 resolve-project :at "forge-build-bridge")
         (s2 shell-forge-lint :command "forge lint <root>")
         (s3 governance-check
            :at "intent-layer pillar :: forge-compilation :: forge-lint"
            :action "strict-codegen / descriptive / experimental 模式检查")
         (s4 violations-return
            :returns "violations_raw + 修复建议"))
      :tools-backref ["mission_forge_lint"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.9 Tool-Backed Flows Index — tools v0.1 :flow-ref 映射
  ;; ══════════════════════════════════════════════════════════
  (section tool-backed-flows-index
    :desc "tools v0.7 的 83 个 :flow-ref 应填入的 flow 引用"
    :coverage "部分 tool 直接映射到单 flow; 多数 read/write tool 是 trivial-single-step (无 flow 抽象价值)"

    (flow-mapping
      ;; Board 相关
      (mission_board_create        :flow "F1-board-task-main-lifecycle :: s1")
      (mission_board_claim         :flow "F1-board-task-main-lifecycle :: s3")
      (mission_board_update        :flow "F1-board-task-main-lifecycle :: s5 (status 更新)")
      (mission_board_retry         :flow "F1-board-task-main-lifecycle :: reset")
      (mission_board_decompose     :flow "F2-board-task-decompose")
      (mission_submit_phase_result :flow "F-board-submit-phase")
      (mission_board_query         :flow "trivial-single-step (纯 memory 读)")
      (mission_board_delete        :flow "trivial-single-step")
      (mission_board_note_add      :flow "trivial-single-step")

      ;; Question & Decision
      (mission_question            :flow "F3-agent-question-block-resume")
      (mission_decision_stats      :flow "F9-decision-cascade (查看统计)")

      ;; Conversation & Retro
      (mission_conversation_reconcile :flow "F6-conversation-jsonl-ingest (手动 reconcile)")
      (mission_conversation_analyze   :flow "F8-retrospective-to-memory (retrospective action)")
      (mission_retrospective_manage   :flow "F8-retrospective-to-memory")
      (mission_conversation_query     :flow "trivial-single-step")

      ;; KB & Embedding
      (mission_kb_query            :flow "F10-context-assembly :: s3 retrieval-fusion (search); trivial-single-step get/list")
      (mission_kb_remember         :flow "F-kb-mutation-to-index + F7-embedding-pipeline when content changes")
      (mission_kb_mutate           :flow "F-kb-mutation-to-index")
      (mission_kb_ops              :flow "F-kb-governance-ops")
      (mission_kb_batch_set_project :flow "F-kb-mutation-to-index :: s2 metadata update")
      (mission_embedding_ops       :flow "F7-embedding-pipeline :: stats/backfill")
      (mission_code_search         :flow "F10-context-assembly :: s3 retrieval-fusion (AST/code)")
      (mission_beacon              :flow "architecture-designed; code-alignment pending: consolidated mission_beacon should map list/map/upsert to legacy mission_beacon_*; ast_sync indirect")
      (mission_skill_query         :flow "F-skill-knowledge-lifecycle :: s2/s3/s9")
      (mission_skill_context       :flow "F-skill-knowledge-lifecycle :: s4/s5 + F10-context-assembly")
      (mission_skill_mutate        :flow "F-skill-knowledge-lifecycle :: s6/s7/s8 + F7-embedding-pipeline when content changes")
      (mission_skill_exec          :flow "F-skill-workflow-execution (独立 skill workflow runtime)")

      ;; PTY & Slot
      (mission_pty_spawn           :flow "F-workflow-slot-full-lifecycle :: s2-slot-provision + s3-slot-readiness (via sole-spawn-bottleneck) / F-workstation-dispatch-policy :: s2 fresh-code-alignment substrate (preferred spawn over claude -p)")
      (mission_pty_send            :flow "F-workflow-slot-full-lifecycle :: s4-dispatch-workflow / F-workstation-dispatch-policy :: s5 resident-lisp continuation")
      (mission_pty_read            :flow "F-workflow-slot-full-lifecycle :: s5-monitor-execution / F-workstation-dispatch-policy :: s5 monitor")
      (mission_pty_status          :flow "F-workflow-slot-full-lifecycle :: s3-slot-readiness + s5-monitor-execution + s6-completion-detection")
      (mission_pty_signal          :flow "F-workflow-slot-full-lifecycle :: s7-teardown")
      (mission_pty_confirm         :flow "F-learned-permission (manual confirm branch) + F-workflow-slot-full-lifecycle :: s5-monitor-execution")
      (mission_pty_screenshot      :flow "F-workflow-slot-full-lifecycle :: s5-monitor-execution")
      (mission_compute_slot        :flow "F-dynamic-slot-lifecycle + F-workflow-slot-full-lifecycle :: s2/s7 + F-task-delegate-autoprovision :: s3 / F-workstation-dispatch-policy :: s2 fresh-code-alignment substrate (dynamic slot variant)")
      (mission_slots               :flow "trivial-single-step")
      (mission_slot_history        :flow "trivial-single-step")

      ;; Task & Flow & Forge
      (mission_task_submit         :flow "F-task-submit-dispatch")
      (mission_task_query          :flow "F-task-legacy-queue-control :: status/list/ack/track")
      (mission_task_cancel         :flow "F-task-legacy-queue-control :: cancel")
      (mission_task_delegate       :flow "F-task-delegate-autoprovision / F-workstation-dispatch-policy :: s2 resident-lisp resume substrate (把任务挂到既有 slot)")
      (mission_flow_run            :flow "F5-flow-engine-v2-node-execution (primary) / F-methodology-to-executable-compile :: s5+s6 (generated flow loader code-aligned partial — search <project_root>/.missiond/generated/flows + $MISSIOND_HOME/flows; flow_source / searched_paths 暴露)")
      (mission_forge_build         :flow "F-forge-build")
      (mission_forge_lint          :flow "F-forge-lint")

      ;; Worker & Control
      (mission_worker              :flow "F-runtime-control-governance :: s2/s4")
      (mission_control             :flow "F-runtime-control-governance")
      (mission_pause               :flow "F-runtime-control-governance :: s1/s2/s6/s7")

      ;; Project
      (mission_project             :flow "F9-project-init (init); registry/context/memories/vault/import/survey are direct project-management ops")
      (mission_intent              :flow "trivial-single-step (project intent file read/path scan)")

      ;; Router & LLM
      (mission_router_chat         :flow "F-router-chat-session :: s1-s6")
      (mission_router_chat_manage  :flow "F-router-chat-session :: s7/s8")
      (mission_sonnet_process      :flow "trivial-single-step (单 LLM 调用)")
      (mission_minimax_process     :flow "trivial-single-step (deprecated)")

      ;; Cascade
      (mission_cc_query            :flow "trivial-single-step")
      (mission_cc_swarm            :flow "F-cc-swarm-pty-prompt (非 F5 ParallelSlotTasks)")
      (mission_universe_graph      :flow "F-cascade-execution :: s1/s2 (graph-only)")
      (mission_cascade_plan        :flow "F-cascade-execution :: s1-s3")
      (mission_cascade_trigger     :flow "F-cascade-execution :: s1-s7")
      (mission_cascade_lint        :flow "F-cascade-execution :: s1/s2/s8")

      ;; Sysinfra
      (mission_sys_logs            :flow "trivial-single-step")
      (mission_sys_config          :flow "trivial-single-step")
      (mission_daemon_update       :flow "F-daemon-update-restart")
      (mission_infra_query         :flow "trivial-single-step")
      (mission_infra_ops           :flow "F-infra-diagnostics")
      (mission_power_control       :flow "trivial-single-step MVP wake/suspend request; status overlaps F-infra-diagnostics TCP probe")
      (mission_inbox               :flow "trivial-single-step")
      (mission_incident            :flow "F-incident-reaction (test/list/get/remediate/status/close code-aligned)")
      (mission_gemini_auth         :flow "trivial-single-step")
      (mission_permission_query    :flow "F-learned-permission :: read/debug views + trivial static config read")
      (mission_permission_mutate   :flow "F-learned-permission :: manual set/reload/revoke")
      (mission_memory              :flow "F-extraction-pipeline :: pending read; F-runtime-control-governance :: memory pause; token_stats read model")
      (mission_insight             :flow "trivial-single-step (strategic-state KB read model)")
      (mission_audit               :flow "trivial-single-step")
      (mission_llm_trace           :flow "trivial-single-step")
      (mission_timeline            :flow "trivial-single-step (event-bus pillar 读)")
      (mission_job_poll            :flow "trivial-single-step")
      (mission_agent               :flow "F-daemon-bootstrap 类 (spawn)")
      (mission_codex_ops           :flow "trivial-single-step (recent/thread/tool_stats read model over codex_ingestion 产出)")
      (mission_execution           :flow "F-execution-log-governance (open/list/claim/heartbeat/release/deviate/decide/issue/complete/status/audit/repair; code-aligned + dispatch_strategy/target_project/requested_cwd 已写入 companion log meta) — unified pipeline 的 execution substrate (s6 execution-runner)")
      (mission_capability_usage    :flow "F-capability-usage-monitoring (snapshot/report/candidates/mark/ack; code-aligned partial — semantic evidence v1: 5 sources + lisp hint merge-candidate)")
      (mission_directive           :flow "F-intent-alignment-plan-execution-loop :: s1 message-intake + s3 alignment-review-gate (statement intake + alignment 管理面; directive-compiler v0 via compiler_mode=sonnet code-aligned, persist=true 写 draft) / F-directive-plan-workflow-compile :: directive branch")
      (mission_plan                :flow "F-intent-alignment-plan-execution-loop :: s4 plan-authoring (plan-compiler v0 via compiler_mode=sonnet code-aligned) + s5 plan-review-gate + s6 execution-runner (plan-runner v0 via execute_mode=internal + dispatch_strategy code-aligned, bridge mode 仍向后兼容) + s7 evidence sidecar / F-directive-plan-workflow-compile :: plan branch")
      (mission_workflow            :flow "F-intent-alignment-plan-execution-loop :: s8 workflow-distillation (workflow-distiller v0 via distill_mode=sonnet code-aligned) / F-directive-plan-workflow-compile :: workflow branch + F-methodology-to-executable-compile (methodology compiler v0 via compile_mode=deterministic + run_methodology code-aligned partial)")
      (mission_global_instruction  :flow "trivial-single-step read/edit/manual-reload (code-aligned; reload returns manual-reload-required)"))

    (future-flow-mapping
      :status "future surfaces backlog currently empty; current 83 tools all indexed; unified-entry-pipeline 不引入新 tool — 详 future-flows :: unified-entry-future-candidates")

    (index-summary
      :total-tools 83
      :non-trivial-flow-backed "约 25 tools (multi-stage flow; mission_execution / mission_capability_usage / mission_directive/plan/workflow / mission_global_instruction 已分类)"
      :trivial-single-step "约 55-58 tools (单 step, 无 flow 抽象价值)"
      :wave-history "v0.2 (task_delegate/skill_exec/universe_graph/cascade_*/board_decompose/submit_phase_result/cc_swarm) → v0.3 (pty + kb-mutation/governance + skill-knowledge) → v0.4 (router_chat/codex_ops/beacon) → v0.5 (incident + methodology compile) → v0.6 (capability usage monitoring) → v0.7 (directive/plan/workflow surfaces + F-intent-alignment-plan-execution-loop canonical pipeline + F-workstation-dispatch-policy) — 不新增 tool"
      :wave-13-status-backfill "wave 13 task 04 (commits 0a3ffe0 / 46a9453 / 3ed14fc + 88568a9 / 8bb6110 / 9759675): directive/plan/workflow/methodology compiler v0 + plan-runner v0 + auto-selection v1 + generated flow loader + capability_usage semantic evidence v1 + mission_execution dispatch_strategy companion log + PLAN DAG runtime v2 + unified-entry pipeline v0 + evidence-collector typed 全部 code-aligned partial; 完整 11-stage PLAN DAG / file-first .lisp writer / auto QuestionEvent / semantic lifting / forge compiler / ExecutionEvent dispatch metadata 仍 pending")

  ;; ══════════════════════════════════════════════════════════
  ;; Future Flows (未来补充)
  ;; ══════════════════════════════════════════════════════════
  (future-flows
    (knowledge-index-hardening
      :desc "knowledge 写入 → embedding → HNSW ready 的生产级可观测性/可用性保障"
      :now-covered-by "F-kb-mutation-to-index + F7-embedding-pipeline"
      :future "补 index refresh 完成回执、search availability SLA、失败重试/告警")

    (incident-reaction
      :desc "IncidentEvent → aiops / remediation"
      :trigger "worker pillar :: infra :: aiops 产 Incident"
      :status "moved-to named flow"
      :moved-to "category infrastructure-flows :: F-incident-reaction"
      :code-alignment "code-aligned: get/remediate/status/close + board task linkage + close guard")

    (methodology-to-executable-compile
      :desc "methodology Lisp SSOT → generated executable YAML → mission_flow_run"
      :status "moved-to named flow"
      :moved-to "category workflow-runtime-flows :: F-methodology-to-executable-compile"
      :code-alignment "mission_workflow compile_methodology dry-run/read surface code-aligned; YAML emitter actor / forge extension / run_methodology execution still pending")

    (execution-log-governance
      :desc "mission_execution claim/deviate/complete → board linkage"
      :protocol "agent-execution-coordination v0.5.2 (memory pillar)"
      :status "moved-to named flow"
      :moved-to "category workflow-runtime-flows :: F-execution-log-governance"
      :code-alignment "code-aligned: 12 actions handler/tool + ExecutionEvent emission")

    (directive-plan-workflow-compile
      :desc "user utterance → directive → plan → workflow"
      :stages
        "intent-layer directive-compiler → plan-compiler → workflow-distiller"
      :status "moved-to named flow"
      :moved-to "category workflow-runtime-flows :: F-directive-plan-workflow-compile"
      :code-alignment "DirectiveLayerStore + mission_directive/mission_plan/mission_workflow MCP manager surfaces code-aligned; directive-compiler / plan-compiler / workflow-distiller actors still pending")

    (cascade-execution
      :desc "mission_cascade_plan / trigger → 多 agent / 多 session 并发执行"
      :cross-ref "worker pillar :: cascade-events (CascadeTriggered / CascadeCompleted)"
      :status "resolved-by-v0.2"
      :moved-to "category cascade-flows :: F-cascade-execution"
      :note "2026-04-25 code scan confirmed handler stages; remaining question is durable audit detail, not flow existence")

    (unified-entry-future-candidates
      :desc "未来若需要让 message intake 与 plan-runner 拥有专属 MCP surface 可考虑的候选; 当前 83 tools 内不计入"
      :status "future-candidate-only; not counted in current 83 tools"
      :rationale "当前 mission_directive(action=compile) 已能承载 message intake 管理面, mission_plan(action=execute) 已能 bridge 执行; 因此优先打磨现有 surface 的 actor/runner 实现, 而不是新增 tool 入口"
      (candidate mission_message
        :purpose "若未来希望统一 client 把 user message 直接送进 MissionD 入口, 而不通过 mission_directive(action=compile, source=message)"
        :why-not-now "mission_directive 已是充分管理面; 引入新 tool 会与现有 directive 管理 surface 重复, 增加 actor 路由复杂度"
        :counted-in-83 "no")
      (candidate mission_invoke
        :purpose "若未来希望统一 client 触发已 approved PLAN.lisp 的内部执行 (而非 mission_plan(action=execute) 返回 next_call descriptor 由 caller 自行执行)"
        :why-not-now "等 plan-runner 自动调度落地后, mission_plan(action=execute) 自然演化成内部 dispatch, 不需要再开 tool 入口"
        :counted-in-83 "no")))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (F-T001…)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (F-T001 :status RESOLVED :resolved-at "2026-04-25"
            :finding "mission_cascade_plan/trigger/lint 真实 staging 已由 code scan 确认并落为 F-cascade-execution; trigger 会发 TaskEvent::CascadeTriggered/CascadeCompleted")
    (F-T002 :status "architecture-decided"
            :finding "mission_skill_exec 不是 flow-engine-v2 别名, 而是独立 skill workflow executor: parse skill workflow block → 30s MCP step dispatch → skill_executions 审计"
            :decision "长期保留两个 runtime: skill_exec 负责 skill-local action macro; flow-engine-v2 负责 durable YAML orchestration. 未来可做 definition adapter, 不合并 executor")
    (F-T003 :status "architecture-designed-code-alignment-pending"
            :note "flow-engine-v2 ParallelSlotTasks Phase-2 已补 s3b parallel-slot-tasks-phase2: slot selection, semaphore fan-out, per-child persistence, join_policy, cancellation, aggregate result. 注意 mission_cc_swarm 仍是 PTY prompt wrapper, 不是该节点的 tool 入口")
    (F-T004 :status "architecture-designed-code-alignment-pending"
            :phase-B-finding "aiops 自动 remediation 已实现 (详 phase-B-scan-findings § C.4): health 恢复自动 close Board task + 加 recovery note, health 失败建 Board task + incident, PtySlot incident 派 Opus slot. incident-reaction 作为完整 flow 的独立 narrative 仍待整理"
            :resolved-by "F-incident-reaction"
            :remaining "代码对齐阶段补 full remediation playbook surface/event")
    (F-T005 :status "architecture-designed-code-alignment-pending"
            :note "methodology lisp → executable YAML 自动转换 pipeline 已定为 F-methodology-to-executable-compile; compiler/tool 实现待代码对齐")
    (F-T006 :status RESOLVED :resolved-at "2026-04-21"
            :finding "autopilot.rs 60s tick (worker v0.4 path autopilot-tick 已确认, phase-B A.2 补: 60s 主编排脉搏 / 双内存槽管理 / 故障隔离). CAS claim 具体 @ memory pillar board state-machine")
    (F-T007 :status "architecture-designed-code-alignment-pending"
            :phase-B-finding "SessionCompleted (及 NarrationSessionCompleted) 由 bus/v2_subscribers 路径 emit (phase-B B.3 发现 experience_harvester 经此路径激活)"
            :resolved-by "F-session-completion-event-chain + event-bus v1.3.2 session-completion-contract"
            :remaining "代码对齐阶段 grep/verify 所有 emit 点并迁到 SessionEvent::Completed contract")
    (F-T008 :status "architecture-designed-code-alignment-pending"
            :note "xjp-router 接入后 F7 embedding-pipeline 变化已补为 xjp-router target; 代码对齐仍等 worker I006")
    (F-T009 :status "architecture-designed-code-alignment-pending"
            :note "F-learned-permission 已补 coverage-contract; 代码对齐阶段 grep all ConfirmRequired / ConfirmResponse / trust-dialog branches,逐条标 covered/no-learn"))
)
