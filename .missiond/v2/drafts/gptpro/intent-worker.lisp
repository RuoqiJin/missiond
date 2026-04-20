;; ══════════════════════════════════════════════════════
;; MissionD — Worker Pillar (Detailed Draft)
;; 目标: 按“入点 / 逻辑核心 / 出点”重整 worker pillar
;; 底稿: 旧地图真实代码切面 + v2 intent.lisp 的新边界
;; ══════════════════════════════════════════════════════

(pillar worker
  :status "草稿 v0.1 — 以旧地图真实状态为底稿, 按 v2 哲学重整"
  :actual-state-sources
    ["intent-pillar-event-workers.lisp"
     "intent-pillar-llm-context.lisp"
     "intent-pillar-engines.lisp"
     "intent-pillar-semantic-parser.lisp"
     "intent-domain-engines.lisp"
     "intent-pillar-transport-bootstrap.lisp"]
  :design-correction-sources
    ["v2/intent.lisp :: pillar worker"
     "intent-memory-execution.lisp :: D006 (worker footprint drift 提醒)"]

  (purpose "系统如何把计算派出去 — PTY / API / 本地 worker / 编排引擎")

  (pillar-ingress
    (entry-1 "来自外部调用的任务: MCP handler / board task / flow node / daemon action")
    (entry-2 "来自终端的实时输入: PTY 输出 / JSONL / semantic parser state")
    (entry-3 "来自事件层的触发: Slot / Message / Board / Session / Memory / Incident 事件")
    (entry-4 "来自定时器的触发: autopilot tick / worker interval / cron 类扫表")
    (entry-5 "来自上下文层的请求: LLM 调用前的 context assembly / retrieval"))

  (pillar-core
    (core-1 "PTY 作为一等计算介质")
    (core-2 "LLM gateway 作为多模型统一门面")
    (core-3 "19 个后台 worker 作为计算租户")
    (core-4 "worker-registry + control-tree + autopilot + flow-engine-v2 作为统一编排核心")
    (core-5 "search-engines / forge 归 worker, 因为它们是 computation 不是 memory"))

  (pillar-egress
    (egress-1 "写回 memory pillar: conversations / board / KB / slots / observability")
    (egress-2 "向 event-bus 发射 domain events")
    (egress-3 "返回工具调用结果 / flow node 结果 / slot dispatch receipt")
    (egress-4 "驱动下一轮 worker / subscriber / autopilot 动作"))

  ;; ─────────────────────────────────────────────────────
  ;; 2.1 PTY 介质层
  ;; ─────────────────────────────────────────────────────
  (section pty
    :targets ["crates/missiond-pty/src/*"
              "crates/missiond-daemon/src/slot_manager/"
              "crates/missiond-daemon/src/slot_orchestrator/"]

    (path pty-session-lifecycle
      (ingress
        :source "mission_pty_* 工具 / slot_manager 自动启动 / flow-engine SlotTask 节点"
        :entry-components ["PTYManager" "SlotManager" "slot_orchestrator"])
      (logic-core
        (step s1 "slot_orchestrator 决定使用哪类控制器 (cc / gemini) 与哪个 slot")
        (step s2 "PTYManager 创建或附着 session, session.rs 接管读写 / 截屏 / 增量提取 / 异常检测")
        (step s3 "semantic-terminal 将终端文本转成 idle / thinking / responding / tool / confirm / title 等结构化状态")
        (step s4 "pty_event_worker 监听状态变化, 处理 auto-approve 规则, 并发出 SlotBecameIdle / SlotStuck")
        (step s5 "slot_manager 更新 slot 生命周期权威状态"))
      (egress
        :writes "slot runtime state / slot_sessions"
        :emits  "SlotEvent / SessionEvent"
        :returns "session handle / slot dispatch status"))

    (path conversation-jsonl-ingestion
      (ingress
        :source "Claude/Gemini/Codex PTY 写出的 JSONL"
        :entry-components ["events_sync.rs" "conversation-logger worker"])
      (logic-core
        (step s1 "events_sync 读取 JSONL 增量, 调 handle_new_events 或启动时 backfill")
        (step s2 "extract_visible_text / extract_tool_names 等 helper 将原始条目压成可写记录")
        (step s3 "conversation-logger 把会话和消息持久化")
        (step s4 "后续 organizer / tagger / embedding / reconcile worker 继续消费"))
      (egress
        :writes "conversations / conversation_messages / 辅助事件记录"
        :emits  "MessageEvent / SessionEvent"
        :returns "ingestion progress"))

    (path slot-dispatch
      (ingress
        :source "board/autopilot / flow-engine-v2 / mission_agent / mission_compute_slot"
        :entry-components ["slot_orchestrator::agent.rs" "cc_controller.rs" "gemini_controller.rs"])
      (logic-core
        (step s1 "按角色与 engine 选择目标 slot")
        (step s2 "校验 slot 是否 running, 必要时启动或拒绝")
        (step s3 "把 prompt / request 发入对应 PTY 控制器")
        (step s4 "等待后续 idle / message / tool 事件回流"))
      (egress
        :writes "slot_sessions / slot_tasks(若是受管 AI 任务)"
        :emits  "SlotEvent"
        :returns "dispatch receipt")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.2 LLM Gateway 层
  ;; ─────────────────────────────────────────────────────
  (section llm-gateways
    :targets ["crates/missiond-daemon/src/llm/*"
              "crates/missiond-daemon/src/context/*"]

    (path llm-request-routing
      (ingress
        :source "worker / handler / flow-node 的 LLM 请求"
        :entry-components ["LlmGateway" "llm_gate"])
      (logic-core
        (step s1 "llm_gate 做并发 / rate-limit / 429 backoff 检查")
        (step s2 "interactive caller 可按策略绕过部分 gate, 后台 worker 不豁免")
        (step s3 "LlmGateway 按 model/provider 路由到 sonnet / gemini / codex / minimax(legacy)")
        (step s4 "provider-specific client 执行 API 或 PTY 调用")
        (step s5 "日志 / 度量 / 失败语义返回上层"))
      (egress
        :writes "llm traces / 部分 observability"
        :emits  "LlmEvent / WorkerEvent"
        :returns "model output / error"))

    (path context-assembly
      (ingress
        :source "任何一次即将发起的 LLM 调用"
        :entry-components ["context_pipeline" "context_budget" "slot_env"])
      (logic-core
        (step s1 "slot_env 注入当前 slot / project / role / cwd 上下文")
        (step s2 "按优先级拼 sources: slot-env → skill-context → kb → conversation-history → topology-map → claude-md")
        (step s3 "context_budget 估 token, 做预算分配与裁剪")
        (step s4 "生成最终 prompt bundle 给 llm_gateway")
        (step s5 "必要时走 retrieval / hybrid search 取补充上下文"))
      (egress
        :writes "无 durable write; 只生成调用上下文"
        :returns "assembled prompt + source trace"))

    (path embedding-routing
      (ingress
        :source "embedding-worker 发起 embedding 请求"
        :entry-components ["sonnet-gateway"])
      (logic-core
        (step s1 "统一经 sonnet_gateway 路由到 qwen3 embedding provider")
        (step s2 "禁止 fallback, 失败直接报错")
        (step s3 "向上游 worker 返回向量结果"))
      (egress
        :writes "由上游 worker 决定写到哪张表的 embedding 列"
        :returns "embedding vec")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.3 Worker Cluster
  ;; ─────────────────────────────────────────────────────
  (section worker-cluster
    (active-roster
      (sonnet 5  "embedding / translation / arch-maintenance / retro / lisp-survey")
      (codex  1  "vision")
      (gemini 1  "strategy")
      (local  12 "conversation-logger / conversation-organizer / pty-event / tagger-chunker / experience-harvester / reconcile / gemini-reconcile / ast-sync / code-prefetch / codex-ingestion / gemini-logger / xjpcode-briefing"))

    (known-footprint-drift
      :note "旧代码足迹里仍可能看到 briefing_worker / step_narrator 等历史文件; 设计上按 active roster 管, zombie 文件列入 pillar 二清理清单")

    (path event-driven-worker-cycle
      (ingress
        :source "event-bus subscribe / watch receiver / internal channel"
        :entry-components ["v2 subscribers" "worker-registry spawn handles"])
      (logic-core
        (step s1 "worker 注册 name / kind / dependency")
        (step s2 "收到事件后先过 control-tree 的 provider/domain/worker pause 判定")
        (step s3 "执行业务逻辑: 提取 / 打标 / 翻译 / 归档 / survey / organize")
        (step s4 "必要时调用 LLM gateway / slot orchestrator / memory store")
        (step s5 "成功后 ack / 失败后交由 failure policy 或下轮重试"))
      (egress
        :writes "conversation / KB / observability / project intent 文件等"
        :emits  "MemoryEvent / WorkerEvent / MessageEvent"
        :returns "worker progress / logs"))

    (path timer-worker-cycle
      (ingress
        :source "interval / cron / periodic sweep"
        :entry-components ["translation-worker" "strategy-worker" "reconcile / retention 类 worker"])
      (logic-core
        (step s1 "按周期唤醒")
        (step s2 "扫待处理数据或外部状态")
        (step s3 "做局部计算或补偿")
        (step s4 "写回 memory 或发事件"))
      (egress
        :writes "各自负责的数据域"
        :emits  "对应 domain events"))

    (path lisp-survey-update
      (ingress
        :source "ContextualCommitDetected"
        :entry-components ["lisp-survey-worker"])
      (logic-core
        (step s1 "过滤 self-trigger: lisp-surveyor 自己产生的 commit 不再递归 survey")
        (step s2 "ProjectRegistry 解析 project_id 与 intent_path")
        (step s3 "按 project 做 60s debounce")
        (step s4 "组装 diff + intent path prompt, 交给 persistent survey slot")
        (step s5 "若返回 NO_CHANGE 则跳过, 否则更新项目 intent.lisp"))
      (egress
        :writes "<project>/.missiond/intent.lisp 或等价 intent 文件"
        :emits  "Project/Memory side effects"
        :returns "survey result")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.4 编排与治理
  ;; ─────────────────────────────────────────────────────
  (section orchestration
    (path worker-lifecycle-governance
      (ingress
        :source "daemon bootstrap / worker spawn / mission_worker / mission_pause"
        :entry-components ["worker-registry" "control-tree"])
      (logic-core
        (step s1 "worker-registry 注册所有 BackgroundWorker 实现")
        (step s2 "spawn 时按 KIND 自动注入 provider dependency")
        (step s3 "ControlManager 通过 watch channel 广播 pause/resume 状态")
        (step s4 "worker select! 等待 changed().await, 零轮询地响应治理变化")
        (step s5 "按 global > provider/domain > worker override 的优先级生效"))
      (egress
        :writes "control_tree.json"
        :returns "worker health / pause summary"))

    (path autopilot-tick
      (ingress
        :source "autopilot timer (5-10s)"
        :entry-components ["autopilot"])
      (logic-core
        (step s1 "memory-scheduler: 扫提醒 / 内部待推进状态")
        (step s2 "extraction-check: 检查 extraction/learning 相关执行态")
        (step s3 "board-task-dispatch: 扫 board_tasks, 原子 claim, 选择 slot/worker")
        (step s4 "flow-progression: 对 flow_template 任务推进一节点")
        (step s5 "supervision-check: lease recovery / 僵尸 slot / stale task 回收"))
      (egress
        :writes "board_tasks / prompt_snapshots / 一些 scheduler 状态"
        :emits  "BoardEvent / WorkerEvent"
        :returns "tick summary"))

    (path flow-engine-v2-node-execution
      (ingress
        :source "running board_task 且 flow_template 非空 / mission_flow_run"
        :entry-components ["flow/loader.rs" "flow/runner.rs" "flow/handlers.rs"])
      (logic-core
        (step s1 "loader 从 $MISSIOND_HOME/flows/*.yaml 载入 FlowDefinition")
        (step s2 "runner 逐节点执行, 并做变量插值")
        (step s3 "handlers 按节点类型分派: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks")
        (step s4 "每节点完成后 persist_context 到 board_tasks.flow_context")
        (step s5 "advance-or-complete: 推进 flow_phase 或上报完成/失败"))
      (egress
        :writes "board_tasks.flow_context / status / flow_phase"
        :returns "node result / flow status")))

  ;; ─────────────────────────────────────────────────────
  ;; 2.5 Code Generation / Search Engines
  ;; ─────────────────────────────────────────────────────
  (section worker-side-computation
    (path forge-build
      (ingress
        :source "mission_forge_build / mission_forge_lint"
        :entry-components ["forge handler" "外部 forge 仓库"])
      (logic-core
        (step s1 "按 project_id 查 ProjectRegistry 定位项目根目录")
        (step s2 "shell out 到 forge build 或 forge lint")
        (step s3 "捕获 stdout/stderr/exit_code/violations")
        (step s4 "回传给 tools pillar")
        (step s5 "必要时触发 survey / intent-layer follow-up"))
      (egress
        :returns "forge execution result"))

    (path retrieval-fusion
      (ingress
        :source "mission_kb_search / mission_memory / context assembly / code search"
        :entry-components ["search engines" "fusion ranker"])
      (logic-core
        (step s1 "四路检索并发: vector-hnsw / fulltext-gin / fuzzy-trigram / tag-exact")
        (step s2 "叠加 project_id OR NULL 的作用域过滤")
        (step s3 "融合打分, 产出 ranked candidates")
        (step s4 "交给工具调用或 context pipeline 消费")
        (step s5 "由调用方决定是否补充 explanation / snippet / prompt bundle"))
      (egress
        :returns "retrieval results"
        :writes "无专属 durable write; 索引更新由 memory 写入自然触发"))))
