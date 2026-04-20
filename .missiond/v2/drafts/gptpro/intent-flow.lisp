;; ══════════════════════════════════════════════════════
;; MissionD — Flow Pillar (Detailed Draft)
;; 目标: 把跨 pillar 的 narrative 用 ingress/core/egress 结构固定下来
;; ══════════════════════════════════════════════════════

(pillar flow
  :status "草稿 v0.1 — 以 board-centric 为主, 外加 conversation / context / project 关键 flow"
  :primary-sources ["v2/intent.lisp :: pillar flow" "intent-flows.lisp"]
  :secondary-sources ["intent-pillar-engines.lisp :: autopilot / flow-engine-v2"]

  (purpose "跨 pillar 的动作前后流程 — 把 memory 静态与 worker 计算串成叙事")

  (principle
    :memory "记状态"
    :worker "做计算"
    :flow   "串顺序与阶段")

  ;; ─────────────────────────────────────────────────────
  ;; F1 任务主生命周期
  ;; ─────────────────────────────────────────────────────
  (flow board-task-main-lifecycle
    (ingress
      :trigger "mission_board_create / decomposed child / auto_execute 扫描"
      :entry-point "memory::board::mcp-board-lifecycle")
    (logic-core
      (stage s1 "create: 写 board_tasks status=open")
      (stage s2 "scan-decide: autopilot 扫 open + auto_execute=1 任务")
      (stage s3 "atomic-claim: SQL CAS open→running + executor + lease")
      (stage s4 "execute: 派给 PTY slot / worker / flow-engine-v2")
      (stage s5 "report-completion: status=done/failed + release claim")
      (stage s6 "downstream-cascade: 检查依赖链, unblock 或 retry-cascade")
      (alt lease-recovery "lease 过期时 recover_stale_running_tasks → status=open"))
    (egress
      :writes "board_tasks / prompt_snapshots"
      :emits  "BoardEvent"
      :result "任务完成或失败, 并对后继任务造成状态影响"))

  ;; ─────────────────────────────────────────────────────
  ;; F2 任务拆解
  ;; ─────────────────────────────────────────────────────
  (flow board-task-decompose
    (ingress
      :trigger "mission_board_decompose(task_id, slot_id, hints)"
      :entry-point "knowledge::board handler")
    (logic-core
      (stage s1 "request: 组装拆解请求并指定 slot")
      (stage s2 "analyze: slot 上的 LLM 产出结构化 subtask plan")
      (stage s3 "write-dag: 写多个 child board_tasks + depends_on / parent_id")
      (stage s4 "emit: 对每个子任务发 BoardTaskCreated"))
    (egress
      :writes "board_tasks DAG"
      :result "父任务被拆成可执行子任务图"))

  ;; ─────────────────────────────────────────────────────
  ;; F3 Agent 提问阻塞/恢复
  ;; ─────────────────────────────────────────────────────
  (flow agent-question-block-resume
    (ingress
      :trigger "mission_question create with task_id"
      :entry-point "comm::question handler / memory::board::agent-questions")
    (logic-core
      (stage s1 "question-create: 写 agent_questions status=pending")
      (stage s2 "block-task: 同事务把关联 board_task 置 blocked")
      (stage s3 "human-or-agent-answer: answer_agent_question() 写 answered/dismissed")
      (stage s4 "check-pending: 若该 task 已无 pending 问题, 则 blocked→open")
      (stage s5 "emit resolved event"))
    (egress
      :writes "agent_questions / board_tasks"
      :emits  "QuestionEvent::Resolved"
      :result "任务自动从阻塞恢复"))

  ;; ─────────────────────────────────────────────────────
  ;; F4 Autopilot Tick
  ;; ─────────────────────────────────────────────────────
  (flow autopilot-tick-pipeline
    (ingress
      :trigger "autopilot timer 5-10s"
      :entry-point "worker::autopilot")
    (logic-core
      (stage s1 "memory-scheduler: 扫提醒 / 内部待推进状态")
      (stage s2 "extraction-check: 看 extraction / learning 执行情况")
      (stage s3 "board-task-dispatch: 复用 F1 的 s2-s4")
      (stage s4 "flow-progression: 推进 flow_template 任务节点")
      (stage s5 "supervision-check: stale lease / zombie slot / stale task 回收"))
    (egress
      :writes "board / scheduler / supervision 相关状态"
      :result "一次完整的系统推进 tick"))

  ;; ─────────────────────────────────────────────────────
  ;; F5 YAML Flow Node 执行
  ;; ─────────────────────────────────────────────────────
  (flow flow-engine-v2-node-execution
    (ingress
      :trigger "board_task running 且 flow_template 非空 / mission_flow_run"
      :entry-point "worker::flow-engine-v2")
    (logic-core
      (stage s1 "load-yaml: 从 $MISSIOND_HOME/flows/<id>.yaml 载入 FlowDefinition")
      (stage s2 "execute-node: 按 LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks 分派")
      (stage s3 "persist-context: 每节点结果都写 board_tasks.flow_context")
      (stage s4 "advance-or-complete: 更新 flow_phase, 或转回 F1 的 report-completion")
      (stage s5 "on-error: 保留已完成上下文并返回失败"))
    (egress
      :writes "board_tasks.flow_context / flow_phase / status"
      :result "可恢复的工作流执行状态"))

  ;; ─────────────────────────────────────────────────────
  ;; F6 PTY JSONL → 会话记忆
  ;; ─────────────────────────────────────────────────────
  (flow conversation-jsonl-ingest
    (ingress
      :trigger "PTY 写出新 JSONL 记录 / daemon 启动 backfill"
      :entry-point "worker::conversation-ingestion")
    (logic-core
      (stage s1 "events_sync 读取增量或补历史")
      (stage s2 "conversation-logger 持久化 conversations / messages")
      (stage s3 "conversation-organizer 做会话整理与 turn/tool 聚合")
      (stage s4 "tagger-chunker / experience-harvester / embedding-worker 继续派生处理")
      (stage s5 "必要时触发 retro / survey / reconcile"))
    (egress
      :writes "conversation-logs + kb-manager 派生数据"
      :emits  "MessageEvent / SessionEvent / MemoryEvent"
      :result "原始终端输出被纳入长期记忆链"))

  ;; ─────────────────────────────────────────────────────
  ;; F7 LLM 调用前上下文拼装
  ;; ─────────────────────────────────────────────────────
  (flow context-assembly
    (ingress
      :trigger "任何上游要发起一次 LLM 调用"
      :entry-point "worker::context-pipeline")
    (logic-core
      (stage s1 "读取 slot-env / project / role / cwd")
      (stage s2 "按优先级拉 skill / KB / conversation-history / topology / CLAUDE.md")
      (stage s3 "调用 retrieval 做检索与排序")
      (stage s4 "context_budget 分配 token")
      (stage s5 "生成 prompt bundle 给 llm_gateway"))
    (egress
      :result "assembled prompt + source trace"))

  ;; ─────────────────────────────────────────────────────
  ;; F8 retrospective → memory
  ;; ─────────────────────────────────────────────────────
  (flow retrospective-to-memory
    (ingress
      :trigger "session end / conversation complete"
      :entry-point "retro-worker")
    (logic-core
      (stage s1 "挑出已结束会话")
      (stage s2 "retro-worker 做总结 / pattern analysis")
      (stage s3 "写 retrospective_results")
      (stage s4 "必要时提炼为 KB 条目或经验条目")
      (stage s5 "供后续 insight / memory recall 消费"))
    (egress
      :writes "retrospective_results / 部分 KB"
      :result "会话经验沉淀"))

  ;; ─────────────────────────────────────────────────────
  ;; F9 项目注册
  ;; ─────────────────────────────────────────────────────
  (flow project-init
    (ingress
      :trigger "mission_project init(path, id?, slots?)"
      :entry-point "knowledge::project handler")
    (logic-core
      (stage s1 "canonicalize path")
      (stage s2 "derive id from dir name")
      (stage s3 "git remote get-url origin → github_url")
      (stage s4 "scan candidate intent.lisp paths")
      (stage s5 "upsert project")
      (stage s6 "backfill project_id to historical conversations")
      (stage s7 "reload SharedProjectRegistry"))
    (egress
      :writes "projects + historical backfill"
      :result "registered project context"))

  (future-flows
    (knowledge-mutation-to-index "knowledge 写入 → embedding → HNSW ready")
    (incident-reaction          "IncidentEvent → aiops / remediation")
    (execution-log-governance   "mission_execution claim/deviate/complete → board linkage")))
