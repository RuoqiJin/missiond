;; ═════════════════════════════════════════════════════════════
;; MissionD — Flow Pillar (phase-A first-draft v0.1)
;; 目标: 跨 pillar 的 narrative — 串 memory 状态 + worker 计算 + tools 端点 + intent-layer 元层
;; 底稿: gptpro intent-flow.lisp (179 行) + v2/intent.lisp 详细 flows catalog
;;       + intent-flows.lisp 老图 10 简洁 flow
;; 定位: 本 pillar 无代码 ownership, 只有 narrative — 描述"什么时候什么顺序做什么"
;; ═════════════════════════════════════════════════════════════

(pillar flow
  :version "v0.1"
  :status "phase-A first-draft 2026-04-21 — 本会话主驾"
  :predecessor "drafts/gptpro/intent-flow.lisp (179 行 starter) + v2/intent.lisp flow pillar 详细 catalog"
  :target-path ".missiond/v2/intent-flow.lisp"

  :actual-state-sources
    [".missiond/v2/intent.lisp :: pillar flow (最详细, 已有 catalog + stages)"
     ".missiond/intent-flows.lisp (v1 老图 10 简洁 flow 定义)"
     ".missiond/intent-pillar-engines.lisp (autopilot tick + flow-engine-v2 runtime)"
     ".missiond/v2/intent-memory.lisp v0.5.1 (board state-machine + 各 module lifecycle)"
     ".missiond/v2/intent-worker.lisp v0.3 (触发点 + 执行 mechanics + pty FSM)"
     ".missiond/v2/intent-tools.lisp v0.1 (78 tools :flow-ref pending)"
     ".missiond/v2/intent-intent-layer.lisp v0.1 (元层 ownership + 认知推理)"]

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
      :question "78 tools 是否每个都有对应 flow?"
      :decision "不 — 多数 single-step read/write 不值得独立 flow. 仅'有显著多 stage 跨 pillar 语义'的 tool 配 flow"
      :estimated-count "约 15-20 flow 覆盖核心 tool. 其余 tool 的 :flow-ref 指向 'trivial-single-step' 或共享已有 flow"
      :effect "本文件 flow 数量 ~20 而非 78")

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
      :effect "tools v0.2 可按本 index 填 :flow-ref 具体值"))

  (purpose "跨 pillar 编排 — 把 memory 状态 + worker 计算 + tools 端点 + intent-layer 元层 串成 end-to-end narrative")

  (pillar-ingress
    (entry-1 "tools pillar 调用 → 启动 flow 的 trigger")
    (entry-2 "event-bus 事件 → 订阅式 flow 启动")
    (entry-3 "timer / autopilot tick → 周期 flow")
    (entry-4 "外部 (用户 / agent) 手动触发"))

  (pillar-core
    (core-1 "flow = 多 stage 的 narrative, 每 stage :at 跨 pillar 跳点")
    (core-2 "flow 无代码, 只有 lisp 描述 + YAML (executable kind)")
    (core-3 "tools 的 :flow-ref 是反向指向 — tool → flow → 跨 pillar stages")
    (core-4 "flow-engine-v2 是 executable kind 的 runtime (worker pillar 实现)")
    (core-5 "methodology-lisp (intent-layer :: workflows :: methodology) 是人类方法论 flow, 不机器执行"))

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
      :desc "父任务 AI 分析 → 子任务 DAG"
      :triggers ["mission_board_decompose(task_id, slot_id, hints)"]
      :stages
        ((s1 request
            :at "memory pillar :: module board :: mcp-board-lifecycle"
            :action "组装拆解请求并指定 slot"
            :tools-consumed ["mission_board_decompose"])
         (s2 analyze
            :at "worker pillar :: section pty :: subsection slot-orchestrator"
            :action "slot LLM 执行结构化 subtask plan 产出")
         (s3 write-dag
            :at "memory pillar :: module board"
            :writes "多个 child board_tasks rows (parent_id + depends_on JSONB)"
            :emits "BoardTaskCreated (每子任务一次)"))
      :result "父任务 → DAG of children with dependency links"
      :tools-backref ["mission_board_decompose"])

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
      :stages
        ((s1 submit
            :at "memory pillar :: module board"
            :writes "board_tasks.flow_phase 推进 (按 engineering-phase FSM)"
            :tools-consumed ["mission_submit_phase_result"])
         (s2 decision-engine-gate
            :at "intent-layer pillar :: learning-engine :: decision :: decision-cascade"
            :optional "requiresMasterDecision 字段存在时触发"
            :action "Decision Engine 审核 → 通过或升级指挥官")
         (s3 advance-phase
            :at "intent-layer pillar :: flow-engine-v1 :: board-phase-engine"
            :writes "board_tasks.flow_phase 下一 state (Investigate→Consult→Plan→Execute→Finalize→Done)"
            :fsm "engineering-phase FSM"))
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
      :triggers ["EmbeddingTask MPSC (from F6-s6 或 backfill)"]
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
            :pending "xjp_router_client 尚未实现, 临时仍用 sonnet_gateway (I006)")
         (s4 vector-upsert
            :at "worker pillar :: embedding-worker-loop"
            :writes "kb_embeddings / ast_embeddings / turn_topics"
            :memory-module "embedding-support")
         (s5 index-ready
            :at "memory pillar :: module kb-manager (FTS5 + HNSW 索引)"
            :action "检索可见性释放"))
      :tools-backref ["mission_embedding_ops"])

    (flow F8-retrospective-to-memory
      :desc "会话结束 → 复盘 → 沉淀到 memory + KB"
      :triggers ["SessionCompleted 事件"]
      :stages
        ((s1 session-end-detection
            :at "worker pillar :: worker-local :: pty_event_worker"
            :emits "SessionCompleted (inferred 或 direct)")
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
      :tools-backref []))

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
      :tools-backref "all 78 tools (meta-flow)"))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.6 Category: Workflow-Runtime Flows (flow-engine-v2)
  ;; ══════════════════════════════════════════════════════════
  (category workflow-runtime-flows
    :desc "flow-engine-v2 YAML declarative node 执行 — 唯一真正 flow orchestration 的 flow"

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
      :note "仅此 flow 是真正的 tool→flow→worker→memory 完整 5 跳链路. 其他 tools 当前 3 跳 (tool→handler→memory/worker) 无 flow 抽象"))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.7 Category: Forge Flows
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
      :tools-backref ["mission_forge_lint" "mission_cascade_lint (借用 forge lint 模式)"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 7.8 Tool-Backed Flows Index — tools v0.1 :flow-ref 映射
  ;; ══════════════════════════════════════════════════════════
  (section tool-backed-flows-index
    :desc "tools v0.1 的 78 个 :flow-ref pending 应填入的 flow 引用"
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
      (mission_kb_query            :flow "F10-context-assembly :: s3 (indirect)")
      (mission_kb_remember         :flow "trivial-single-step + 触发 F7-embedding-pipeline")
      (mission_kb_mutate           :flow "trivial-single-step")
      (mission_kb_ops              :flow "F7 related (analyze/compact 可含 flow)")
      (mission_embedding_ops       :flow "F7-embedding-pipeline")
      (mission_code_search         :flow "F10 s3 retrieval-fusion")
      (mission_beacon              :flow "trivial 或 F6 (ast_sync 间接)")
      (mission_skill_context       :flow "F10-context-assembly")
      (mission_skill_exec          :flow "F5-flow-engine-v2-node-execution (skill workflow 类似 flow)")

      ;; PTY & Slot
      (mission_pty_spawn           :flow "sole-spawn-bottleneck invariant (non-flow) + F-daemon-bootstrap 若启动时")
      (mission_pty_send            :flow "trivial-single-step")
      (mission_pty_read            :flow "trivial-single-step")
      (mission_pty_status          :flow "trivial-single-step")
      (mission_pty_signal          :flow "trivial-single-step")
      (mission_pty_confirm         :flow "F-learned-permission (手动 confirm 路径)")
      (mission_pty_screenshot      :flow "trivial-single-step")
      (mission_compute_slot        :flow "F-daemon-bootstrap (slot spawn 机制)")
      (mission_slots               :flow "trivial-single-step")
      (mission_slot_history        :flow "trivial-single-step")

      ;; Task & Flow & Forge
      (mission_task_submit         :flow "与 F1 或 skill workflow 相关")
      (mission_task_query          :flow "trivial-single-step")
      (mission_task_cancel         :flow "F1 中断")
      (mission_task_delegate       :flow "F1 包装 (自主选 slot)")
      (mission_flow_run            :flow "F5-flow-engine-v2-node-execution (primary)")
      (mission_forge_build         :flow "F-forge-build")
      (mission_forge_lint          :flow "F-forge-lint")

      ;; Worker & Control
      (mission_worker              :flow "trivial-single-step (control-tree 查询)")
      (mission_control             :flow "trivial-single-step (cascade 治理)")
      (mission_pause               :flow "trivial-single-step (global kill-switch)")

      ;; Project
      (mission_project             :flow "F9-project-init (init action); 其他 trivial")
      (mission_intent              :flow "trivial-single-step (文件读)")

      ;; Router & LLM
      (mission_router_chat         :flow "F-strategy-analysis 类似 (gemini chat 模式)")
      (mission_router_chat_manage  :flow "trivial-single-step")
      (mission_sonnet_process      :flow "trivial-single-step (单 LLM 调用)")
      (mission_minimax_process     :flow "trivial-single-step (deprecated)")

      ;; Cascade
      (mission_cc_query            :flow "trivial-single-step")
      (mission_cc_swarm            :flow "F5-flow-engine-v2 ParallelSlotTasks 模式")
      (mission_universe_graph      :flow "trivial-single-step (memory 读)")
      (mission_cascade_plan        :flow "待 cascade 具体 flow 设计")
      (mission_cascade_trigger     :flow "待 cascade 具体 flow 设计")
      (mission_cascade_lint        :flow "F-forge-lint 模式")

      ;; Sysinfra
      (mission_sys_logs            :flow "trivial-single-step")
      (mission_sys_config          :flow "trivial-single-step")
      (mission_daemon_update       :flow "F-daemon-bootstrap 重启类")
      (mission_infra_query         :flow "trivial-single-step")
      (mission_infra_ops           :flow "trivial-single-step (health check)")
      (mission_power_control       :flow "trivial-single-step")
      (mission_inbox               :flow "trivial-single-step")
      (mission_incident            :flow "trivial 或 incident-reaction flow (future)")
      (mission_gemini_auth         :flow "trivial-single-step")
      (mission_permission_query    :flow "trivial-single-step")
      (mission_permission_mutate   :flow "F-learned-permission 部分 step")
      (mission_memory              :flow "trivial 或 F-extraction-pipeline (pending action)")
      (mission_insight             :flow "trivial-single-step")
      (mission_audit               :flow "trivial-single-step")
      (mission_llm_trace           :flow "trivial-single-step")
      (mission_timeline            :flow "trivial-single-step (event-bus pillar 读)")
      (mission_job_poll            :flow "trivial-single-step")
      (mission_agent               :flow "F-daemon-bootstrap 类 (spawn)")
      (mission_codex_ops           :flow "trivial-single-step (读 codex_ingestion 产出)"))

    (index-summary
      :total-tools 78
      :non-trivial-flow-backed "约 20 tools (有独立或共享的 multi-stage flow)"
      :trivial-single-step "约 58 tools (单 step, 无 flow 抽象价值)"
      :future-cascade-flows "mission_cascade_plan/trigger 需独立 flow 设计 (未包含在 v0.1)"))

  ;; ══════════════════════════════════════════════════════════
  ;; Future Flows (未来补充)
  ;; ══════════════════════════════════════════════════════════
  (future-flows
    (knowledge-mutation-to-index
      :desc "knowledge 写入 → embedding → HNSW ready"
      :partial-covered-by "F7-embedding-pipeline"
      :future "补完整 index refresh + search availability")

    (incident-reaction
      :desc "IncidentEvent → aiops / remediation"
      :trigger "worker pillar :: infra :: aiops 产 Incident"
      :future "标准 remediation playbook")

    (execution-log-governance
      :desc "mission_execution claim/deviate/complete → board linkage"
      :protocol "agent-execution-coordination v0.5.1 (memory pillar)"
      :future "12 actions handler 实现后 (worker I007 / IL-T005)")

    (directive-plan-workflow-compile
      :desc "user utterance → directive → plan → workflow"
      :stages
        "intent-layer directive-compiler → plan-compiler → workflow-distiller"
      :status "schema-ready-pending-implementation (intent-layer 5.10 actor 全 TBD)")

    (cascade-execution
      :desc "mission_cascade_plan / trigger → 多 agent / 多 session 并发执行"
      :cross-ref "worker pillar :: cascade-events (CascadeTriggered / CascadeCompleted)"
      :future "完整 cascade orchestration 设计"))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (F-T001…)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (F-T001 :status "future-design"
            :note "cascade flow (mission_cascade_plan/trigger) 具体 staging — 待 cascade 整体设计")
    (F-T002 :status "awaiting-decision"
            :note "mission_task_submit/query/cancel 与 skill_exec 可能重叠 — 职责分工决策")
    (F-T003 :status "future-implementation"
            :note "flow-engine-v2 ParallelSlotTasks Phase-2 reflow (当前 fire-and-forget POC)")
    (F-T004 :status "partial-resolved"
            :phase-B-finding "aiops 自动 remediation 已实现 (详 phase-B-scan-findings § C.4): health 恢复自动 close Board task + 加 recovery note, health 失败建 Board task + incident, PtySlot incident 派 Opus slot. incident-reaction 作为完整 flow 的独立 narrative 仍待整理"
            :remaining "把现有 remediation 逻辑整理为 flow narrative")
    (F-T005 :status "awaiting-decision"
            :note "methodology lisp → executable YAML 自动转换 pipeline — 未来可 forge 冲压")
    (F-T006 :status RESOLVED :resolved-at "2026-04-21"
            :finding "autopilot.rs 60s tick (worker v0.3 path autopilot-tick 已确认, phase-B A.2 补: 60s 主编排脉搏 / 双内存槽管理 / 故障隔离). CAS claim 具体 @ memory pillar board state-machine")
    (F-T007 :status "partial-resolved"
            :phase-B-finding "SessionCompleted (及 NarrationSessionCompleted) 由 bus/v2_subscribers 路径 emit (phase-B B.3 发现 experience_harvester 经此路径激活). 完整 emit 机制待补"
            :remaining "SessionCompleted emit 点完整清单 (pty_event_worker 还是别处?)")
    (F-T008 :status "pending-phase-C"
            :note "xjp-router 接入后 F7 embedding-pipeline 变化 — 需 xjp_router_client 实现后补 flow 迁移设计 (同 worker I006)")
    (F-T009 :status "future-validation"
            :note "F-learned-permission 100% 覆盖率验证 — 需 code audit 所有 ConfirmRequired 路径"))
)
