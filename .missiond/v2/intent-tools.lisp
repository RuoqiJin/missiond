;; ═════════════════════════════════════════════════════════════
;; MissionD — Tools Pillar (phase-A first-draft v0.1)
;; 目标: 按 4 domain (compute/knowledge/comm/sysinfra) 列全 78 MCP 工具
;;       每工具含 :dispatches-to-worker / :memory-cross-ref / :flow-ref (预留) /
;;                :called-by / :necessity-pending-review
;; 底稿: intent-mcp-defs.lisp (schema SSOT, 40KB) + intent-pillar-mcp-dispatch.lisp
;;       (handler 映射) + v0.3 intent-worker.lisp :mcp-surface-to-tools (worker 侧)
;; 架构原则: tools 是"对外服务端点", 不是汇合点 — 单向上暴露, 单向下派发
;; ═════════════════════════════════════════════════════════════

(pillar tools
  :version "v0.1"
  :status "phase-A first-draft 2026-04-21 — 本会话主驾 (gptpro 无法查代码)"
  :predecessor "drafts/gptpro/intent-tools.lisp (239 行 starter)"
  :target-path ".missiond/v2/intent-tools.lisp"

  :actual-state-sources
    [".missiond/intent-mcp-defs.lisp (schema SSOT, 24 mcp-module, 详细 input/returns)"
     ".missiond/intent-pillar-mcp-dispatch.lisp (tool → handler 映射权威)"
     ".missiond/intent-rpc-gateway.lisp (JSON-RPC gateway 层)"
     "crates/missiond-mcp/src/ (MCP 壳 — 13+9+7+5 = 34 文件)"
     "crates/missiond-daemon/src/handlers/ (daemon 实际 handler 逻辑)"
     ".missiond/v2/intent-worker.lisp v0.3 :mcp-surface-to-tools (worker 侧对齐)"]

  :design-correction-sources
    [".missiond/v2/intent.lisp :: pillar tools (占位)"
     ".missiond/v2/intent-memory.lisp v0.5.1 frozen (每 module :mcp-surface)"
     "drafts/gptpro/intent-tools.lisp (gptpro draft — 架构方向已纠正, 本 v0.1 不继承其 'tools 是汇合点' 的误解)"]

  :historical-footprint-sources
    ["mcp-defs 头部 '67 tools' 是旧数, 实际 78 (含 batch_set_project / embedding_ops / universe_graph / cascade_* / incident / gemini_auth 等新增)"
     "mission_minimax_process 已 Deprecated → mission_sonnet_process (mcp-defs 显式标注)"]

  ;; ══════════════════════════════════════════════════════════
  ;; phase-A-decisions — 架构纠正
  ;; ══════════════════════════════════════════════════════════
  (phase-A-decisions
    (Q-T1
      :question "tools pillar 是否为 pillar 汇合点 (gptpro v0.1 draft 这样画)?"
      :decision "reject — tools 不是汇合点, 是对外服务端点 (surface + dispatch)"
      :rationale "指挥官澄清: tools 向上对 intent-layer 暴露接口, 向下通过 flow pillar 派发到 worker, 单向不汇合"
      :effect "本 lisp 不画 '四角汇合' 图, 只画单向调用链")

    (Q-T2
      :question "每个 tool 背后是否都有 flow orchestration?"
      :decision "理想态 YES, 当前态 NO — 仅 mission_flow_run 走完整 flow"
      :rationale "其余 77 tools 当前是 single-step 派发 (MCP 壳 → daemon handler → 结果). 未来每 tool 背后对应 flow pillar 的 flow id"
      :effect "每 tool 预留 :flow-ref 字段. 当前值 'pending-flow-pillar — single-step direct-dispatch'. 等 flow pillar 设计后逐条填")

    (Q-T3
      :question "tools 的上游调用者是谁?"
      :decision "主: intent-layer (意识层决策); 辅: external MCP client (Claude Code / Gemini CLI / xjpcode) + board-frontend"
      :rationale "理想架构是 intent-layer 唯一上游, 但现实有外部 agent/UI 直接调. board-frontend 的 /api/* 路由也转调 MCP"
      :anti-pattern "worker 不反向调 tools (循环); tools 不调 intent-layer (越界)")

    (Q-T4
      :question "MCP 壳位置 vs handler 位置不一致的 tool 怎么处理?"
      :decision "每 tool 两个字段分开: :mcp-file (tools/{group}/x.rs 壳) + :handler-file (handlers/{group}/y.rs 逻辑)"
      :examples
        ["mission_pause: 壳 tools/compute/slot.rs (mcp module 'slot'), handler sysinfra::misc"
         "mission_slot_history: 壳 tools/compute/slot.rs, handler comm::timeline"
         "mission_beacon: 壳 tools/knowledge/kb.rs (mcp module 'kb'), handler comm::audit"
         "mission_codex_ops: 壳 tools/comm/codex_ops.rs, handler comm::codex_ops"]
      :rationale "MCP schema 归组 ≠ handler 归组 (历史原因). 本 lisp 按 handler 归组 (mcp-dispatch 老图为准)")

    (Q-T5
      :question "mission_minimax_process 已 deprecated, 本 lisp 如何处理?"
      :decision "保留列项, 标 :status 'deprecated-migrated-to-sonnet', :necessity-pending-review true — 候删"
      :rationale "feedback_drop_sqlite 类似, 弃用不即删, 保档等评审"))

  (purpose "通过 MCP JSON-RPC 对外暴露 78 个服务端点 — intent-layer 主调用者, flow pillar 下游 orchestration, 单向不汇合")

  (pillar-ingress
    (entry-1
      :source "intent-layer pillar 决策 → 工具调用 (主)"
      :frequency "多数 tool 调用"
      :mechanism "intent-layer 通过 decision-engine / autopilot / learning-engine 产生 tool 调用请求"
      :authority "架构理想: intent-layer 是唯一真正 '目的调用者'")

    (entry-2
      :source "external MCP client JSON-RPC stdio"
      :frequency "少但关键"
      :examples ["Claude Code CLI (主要消费者)"
                 "Gemini CLI slot"
                 "xjpcode CLI"
                 "Cursor / 其他 MCP client"])

    (entry-3
      :source "board-frontend Next.js /api/* routes"
      :frequency "UI 触发"
      :examples ["/api/pty/spawn → mission_pty_spawn"
                 "/api/pty/screen → mission_pty_read(screen)"
                 "/api/pty/confirm → mission_pty_confirm"
                 "/api/slots → mission_slots"
                 "/api/tasks → mission_board_query(list)"
                 "/api/conversations → mission_conversation_query"
                 "/api/kb → mission_kb_query/mutate"
                 "/api/projects → mission_project(list)"
                 "/api/questions → mission_question(list)"
                 "/api/timeline/events → mission_timeline"
                 "/api/architecture → (WIP)"
                 "/api/system/health → aggregate"
                 "/api/system/llm-traces → mission_llm_trace"])

    (entry-4
      :source "missiond 内部反向 mcp-client 调用"
      :frequency "少"
      :mechanism "crates/missiond-daemon/src/infra/mcp_client.rs — daemon 内某 handler/flow/worker 反向调 MCP 工具"
      :use-case "flow-engine-v2 的 McpTool node type 通过 dispatch_tool 进入"))

  (pillar-core
    (core-1 "MCP JSON-RPC stdio 服务器 (crates/missiond-mcp/src/bin/mission-mcp.rs + server.rs + gateway_impl.rs + gen_gateway.rs)")
    (core-2 "4 domain, 78 tools 总: compute 21 / knowledge 29 / comm 14 / sysinfra 14")
    (core-3 "两层架构: MCP 壳 (schema validation + action dispatch) → Daemon handler (实际逻辑)")
    (core-4 "tool-registry + gen_gateway (Forge 冲压路由, tool_name → handler 数据驱动)")
    (core-5 "每 tool 预留 :flow-ref (待 flow pillar 设计) + :necessity-pending-review (指挥官评审)")
    (core-6 "schema SSOT 在 .missiond/intent-mcp-defs.lisp (24 mcp-module + 详细 input/returns)")
    (core-7 "handler mapping SSOT 在 .missiond/intent-pillar-mcp-dispatch.lisp (tool → handler)")
    (core-8 "审计: 所有 tool 调用写 tool_calls 表 (memory :: conversation-logs)"))

  (pillar-egress
    (egress-1 "→ flow pillar: 每 tool 背后对应 flow (待 flow pillar 设计; 当前仅 mission_flow_run 是显式 flow)")
    (egress-2 "→ worker pillar: 14 compute + 4 sysinfra = 18 tools 映射 worker path (v0.3 intent-worker.lisp :mcp-surface-to-tools 已详述)")
    (egress-3 "→ memory pillar: knowledge + comm = 43 tools 读写 memory module schema")
    (egress-4 "→ intent-layer pillar: mission_intent + mission_forge_{build,lint} + mission_project 操作 lisp 文件与 forge 冲压")
    (egress-5 "→ system-layer pillar: sysinfra tools 读写 control_tree / config / logs / power")
    (egress-6 "返回 JSON-RPC result / error (error-codes 见下) + 写 tool_calls 审计表")

    (cross-pillar-calling-chain
      :principle "单向调用链 (不是汇合)"
      :ideal-chain "intent-layer → tools → flow pillar → worker pillar → memory pillar (5 跳)"
      :current-reality "多数 tool 是 3 跳 (caller → MCP 壳 → daemon handler → 结果); 仅 mission_flow_run 走完整 5 跳"
      :future-state "flow pillar 设计后, 多数 tool 会抽象出 flow, 走完整链"
      :anti-patterns
        ["tools 调 intent-layer → 循环, 禁"
         "worker 直调 tools → worker 应只被 flow 编排"
         "memory 调 tools → memory = 库, 纯被调用方, 禁"])

    (error-codes
      :authority "intent-pillar-mcp-dispatch.lisp + intent-rpc-gateway.lisp"
      :list ["UNKNOWN_TOOL" "UNKNOWN_ACTION" "MISSING_PARAM" "INVALID_PARAM"
             "NOT_FOUND" "PERMISSION_DENIED" "IPC_TIMEOUT" "SPAWN_FAILED" "DB_ERROR"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.1 RPC Gateway — JSON-RPC 入口
  ;; ══════════════════════════════════════════════════════════
  (section rpc-gateway
    :desc "MCP 协议 JSON-RPC stdio 服务器 — 所有 tool 调用的入口"
    :targets
      ["crates/missiond-mcp/src/bin/mission-mcp.rs (binary entry)"
       "crates/missiond-mcp/src/server.rs (JSON-RPC loop)"
       "crates/missiond-mcp/src/gateway_impl.rs (dispatch)"
       "crates/missiond-mcp/src/gen_gateway.rs (Forge 生成路由表)"
       "crates/missiond-mcp/src/protocol.rs (MCP protocol types)"
       "crates/missiond-mcp/src/lib.rs"]
    :methods ["initialize" "notifications/initialized" "tools/list" "tools/call" "ping"]
    :dispatch-rule "数据驱动: tool_name → handler 映射, 非硬编码 match"
    :role "纯 plumbing — schema 在 tools pillar, handler 在其他 pillar, gateway 只做路由")

  ;; ══════════════════════════════════════════════════════════
  ;; 3.2 Compute Domain — 21 tools
  ;; ══════════════════════════════════════════════════════════
  (domain compute
    :desc "PTY / slot / worker / task / flow / forge / process — 计算执行相关"
    :count 21
    :mcp-shell-dir "crates/missiond-mcp/src/tools/compute/ (13 .rs)"
    :handler-dir "crates/missiond-daemon/src/handlers/compute/"

    ;; ── PTY module (7 tools) ──
    (module pty
      :mcp-file "crates/missiond-mcp/src/tools/compute/pty.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/pty.rs"
      :tool-count 7

      (tool mission_pty_spawn
        :desc "启动 PTY 交互式会话"
        :schema-ref "intent-mcp-defs.lisp :: mcp-module pty :: mission_pty_spawn"
        :required ["slotId"]
        :optional ["waitForIdle" "timeoutSecs" "autoRestart" "mcpConfigPath"]
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: invariant sole-spawn-bottleneck (spawn_tracked_slot)"
        :memory-cross-ref ["slot-support"]
        :event-emits ["SlotSessionChanged" "ManagerEvent::TextComplete" "PtyStateChanged"]
        :flow-ref "pending-flow-pillar — single-step"
        :called-by ["intent-layer" "board-frontend/api/pty/spawn" "external MCP client"]
        :necessity-pending-review true)

      (tool mission_pty_send
        :desc "向 PTY 发送消息, 默认 fire-and-forget"
        :required ["slotId" "message"]
        :optional ["waitForResponse" "timeoutMs"]
        :dispatches-to-worker "section pty :: subsection pty-transport :: path pty-session-lifecycle"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend" "external MCP client" "flow-engine-v2 SlotTask"]
        :necessity-pending-review true)

      (tool mission_pty_read
        :desc "读取 PTY 内容"
        :actions ["screen" "history" "logs"]
        :required ["action" "slotId"]
        :dispatches-to-worker "section pty :: subsection pty-transport :: path pty-signal-extraction"
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/pty/screen" "external MCP client"]
        :necessity-pending-review true)

      (tool mission_pty_screenshot
        :desc "截取 PTY 终端 PNG"
        :required ["slotId"]
        :dispatches-to-worker "section pty :: subsection pty-transport :: screenshot.rs"
        :flow-ref "pending-flow-pillar"
        :called-by ["board-frontend" "intent-layer (debug)"]
        :necessity-pending-review true)

      (tool mission_pty_status
        :desc "获取 PTY 会话状态 (FSM 当前 state)"
        :optional ["slotId"]
        :dispatches-to-worker "section pty :: subsection pty-state-machine (查询 8-state FSM)"
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend" "external MCP client"]
        :necessity-pending-review true)

      (tool mission_pty_signal
        :desc "向 PTY 发送信号 kill/interrupt"
        :actions ["kill" "interrupt"]
        :required ["action" "slotId"]
        :dispatches-to-worker "section pty :: subsection pty-transport :: path pty-session-lifecycle"
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review true)

      (tool mission_pty_confirm
        :desc "发送确认响应 (auto-approve 的手动 MCP 对偶)"
        :required ["slotId" "response"]
        :dispatches-to-worker "section pty :: subsection learned-permissions (confirm flow 手动路径)"
        :event-emits ["learn permission if opt2"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/pty/confirm"]
        :necessity-pending-review true)

      (contract-summary-for-pty-module
        :memory-write ["slot_sessions" "learned_permissions.yaml (file)"]
        :events ["ManagerEvent::*" "PtyStateChanged" "SlotSessionChanged"]))

    ;; ── Task module (3 tools) ──
    (module task
      :mcp-file "crates/missiond-mcp/src/tools/compute/task.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/task.rs"

      (tool mission_task_submit
        :desc "提交任务给专家 Agent, async 或 sync"
        :actions ["async" "sync"]
        :required ["role"]
        :optional ["action" "prompt" "question" "slotId" "timeoutMs"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["system-support (tasks 表)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review true)

      (tool mission_task_query
        :desc "任务状态查询 status/list/ack/track"
        :actions ["status" "list" "ack" "track"]
        :required ["action"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["system-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true)

      (tool mission_task_cancel
        :desc "取消任务"
        :required ["taskId"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["system-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true))

    ;; ── Task Delegate (1 tool) ──
    (module task_delegate
      :mcp-file "crates/missiond-mcp/src/tools/compute/task_delegate.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"

      (tool mission_task_delegate
        :desc "声明式任务委派 — 描述目标, daemon 自主选 slot/执行/回报"
        :required ["objective"]
        :optional ["intent" "cwd" "timeout_secs" "priority" "depends_on" "context_hints"]
        :dispatches-to-worker "section engine-cluster (autopilot 选 slot) + section pty (slot-orchestrator 派发)"
        :memory-cross-ref ["board (创建 board_task)" "system-support"]
        :flow-ref "pending-flow-pillar (高度适合 flow-engine-v2 包装)"
        :called-by ["intent-layer (主)"]
        :necessity-pending-review true))

    ;; ── Process (3 tools: agent/slots/inbox, 注意 inbox 在此 handler) ──
    (module process
      :mcp-file "crates/missiond-mcp/src/tools/compute/process.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/process.rs"

      (tool mission_agent
        :desc "Agent 进程管理 spawn/kill/restart/list"
        :note "注意: handler 'process' 但 mcp-dispatch 老图标 handler 为 compute::cc_tasks — mcp 壳在 process.rs, handler 可能是 cc_tasks. 两者分离"
        :actions ["spawn" "kill" "restart" "list"]
        :required ["action"]
        :optional ["slotId" "visible" "autoRestart"]
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path claude-slot-dispatch (cc_tasks)"
        :memory-cross-ref ["slot-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review true)

      (tool mission_slots
        :desc "列出所有工位配置"
        :required []
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path slot-manager-runtime-authority"
        :memory-cross-ref ["slot-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/slots"]
        :necessity-pending-review true)

      (tool mission_inbox
        :desc "获取收件箱消息 (跨 domain, 此 handler 可能归 sysinfra::misc)"
        :optional ["unreadOnly" "limit"]
        :note "mcp-dispatch 标 handler 为 sysinfra::misc — 本 tool 实际属 sysinfra 但 mcp 壳放 process.rs"
        :dispatches-to-worker "N/A — 纯 memory 读"
        :memory-cross-ref ["system-support (inbox_messages)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review true))

    ;; ── CC Tasks (2 tools) ──
    (module cc_tasks
      :mcp-file "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/cc_tasks.rs"

      (tool mission_cc_query
        :desc "Claude Code 任务监控 sessions/tasks/overview/in_progress"
        :actions ["sessions" "tasks" "overview" "in_progress"]
        :required ["action"]
        :optional ["sessionId" "projectPath" "activeOnly"]
        :dispatches-to-worker "N/A — 纯 memory 读 (handlers/knowledge/cascade.rs)"
        :note "mcp-dispatch 标 handler 为 knowledge::cascade — 跨 mcp/handler 归组"
        :memory-cross-ref ["conversation-logs" "board"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review true)

      (tool mission_cc_swarm
        :desc "通过 PTY 触发 Claude Code Swarm 模式并行执行"
        :required ["slotId" "tasks"]
        :optional ["teammateCount" "timeoutMs"]
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path claude-slot-dispatch"
        :memory-cross-ref ["conversation-logs" "slot-support"]
        :flow-ref "pending-flow-pillar (非常适合包 ParallelSlotTasks node)"
        :called-by ["intent-layer"]
        :necessity-pending-review true))

    ;; ── Minimax / Sonnet (2 tools, minimax deprecated) ──
    (module minimax-and-sonnet
      :mcp-file "crates/missiond-mcp/src/tools/compute/minimax.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/{minimax,process}.rs"

      (tool mission_sonnet_process
        :desc "调 Claude Sonnet 处理文本 (HTTP 调用无 PTY 开销)"
        :actions ["summarize" "translate" "custom"]
        :required ["text" "task"]
        :optional ["prompt" "targetLang" "maxChars"]
        :dispatches-to-worker "section llm-gateways :: path sonnet-priority-gateway"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "internal worker"]
        :necessity-pending-review true)

      (tool mission_minimax_process
        :desc "[DEPRECATED] 调 MiniMax 处理文本, 已迁移到 Sonnet"
        :status "deprecated-migrated-to-sonnet"
        :actions ["summarize" "translate" "custom"]
        :dispatches-to-worker "section llm-gateways :: path minimax-legacy-gateway"
        :flow-ref "pending-flow-pillar"
        :called-by ["legacy callers"]
        :necessity-pending-review true
        :removal-candidate "yes — 候评审删除"))

    ;; ── Worker / Control (2 tools) ──
    (module worker
      :mcp-file "crates/missiond-mcp/src/tools/compute/worker.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/worker.rs"

      (tool mission_worker
        :desc "后台 Worker + LLM 闸口管理 list/control"
        :actions ["list" "control"]
        :required ["action"]
        :optional ["target" "control_action"]
        :dispatches-to-worker "section orchestration-governance :: path pause-resume-cascade"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend" "external MCP client (debug)"]
        :necessity-pending-review true)

      (tool mission_control
        :desc "统一调控闸口 (级联机制: 关 provider 自动暂停依赖 worker)"
        :required ["target_type" "action"]
        :optional ["target_name"]
        :target-types ["global" "provider" "domain" "worker" "slot_role" "project"]
        :dispatches-to-worker "section orchestration-governance :: path pause-resume-cascade (含 set_project P2+P3 commit 50a5296)"
        :file-writes ["control_tree.json"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend (system control UI)"]
        :necessity-pending-review true))

    ;; ── Slot (2 tools: slot_history + pause, 注意两者 handler 跨组) ──
    (module slot
      :mcp-file "crates/missiond-mcp/src/tools/compute/slot.rs"
      :note "两个 tool 的 handler 都不在 compute/slot 下"

      (tool mission_slot_history
        :desc "查询工位任务历史 (realtime_extract/deep_analysis/kb_gc 等)"
        :optional ["slotId" "taskType" "status" "limit" "stats"]
        :mcp-shell-file "crates/missiond-mcp/src/tools/compute/slot.rs"
        :handler-file "crates/missiond-daemon/src/handlers/comm/timeline.rs"
        :dispatches-to-worker "N/A — 纯 memory 读"
        :memory-cross-ref ["slot-support (slot_tasks)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review true)

      (tool mission_pause
        :desc "全局暂停/恢复所有工位的工作分派"
        :actions ["pause" "resume" "status"]
        :optional ["action"]
        :mcp-shell-file "crates/missiond-mcp/src/tools/compute/slot.rs"
        :handler-file "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
        :dispatches-to-worker "section orchestration-governance :: path pause-resume-cascade (global kill-switch)"
        :file-writes ["control_tree.json"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend" "external MCP client"]
        :necessity-pending-review true))

    ;; ── Compute Slot (1 tool) ──
    (module compute_slot
      :mcp-file "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"

      (tool mission_compute_slot
        :desc "动态计算工位管理 create/terminate/extend/list (TTL 生命周期, 上限 5 活跃 · 8h)"
        :actions ["create" "terminate" "extend" "list"]
        :required ["action"]
        :optional ["template" "objective" "cwd" "max_ttl" "slot_id" "additional_seconds" "status"]
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path {claude,gemini}-slot-dispatch (经 sole-spawn-bottleneck)"
        :memory-cross-ref ["slot-support (dynamic_slots, slot_sessions)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (autopilot 派发)" "board-frontend"]
        :necessity-pending-review true))

    ;; ── Job (1 tool) ──
    (module job
      :mcp-file "crates/missiond-mcp/src/tools/compute/job.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/job.rs"

      (tool mission_job_poll
        :desc "轮询异步 Job 状态 poll/list/cancel"
        :actions ["poll" "list" "cancel"]
        :required ["job_id"]
        :optional ["action"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["system-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true))

    ;; ── Flow Run (1 tool — 唯一已有 flow orchestration 的 tool) ──
    (module flow_run
      :mcp-file "crates/missiond-mcp/src/tools/compute/flow_run.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/flow_run.rs"

      (tool mission_flow_run
        :desc "Flow Engine v2 declarative YAML → node-sequence 执行器"
        :actions ["run" "list" "status"]
        :required ["flow_id"]
        :optional ["params" "action" "task_id"]
        :dispatches-to-worker "section engine-cluster :: subsection flow-engine-v2 (3 path: load/dispatch/persist)"
        :memory-cross-ref ["board (board_tasks.flow_context / flow_phase / status)"]
        :flow-ref "self — 唯一已自带 flow 的 tool"
        :called-by ["intent-layer" "external MCP client" "board-frontend"]
        :necessity-pending-review false
        :note "此 tool 是 tools → flow → worker 完整 5 跳链路的唯一模板, 其他 77 tools 将来借鉴其模式"))

    ;; ── Forge (2 tools) ──
    (module forge
      :mcp-file "crates/missiond-mcp/src/tools/compute/forge.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/forge.rs"
      :added "commit 34167db"

      (tool mission_forge_build
        :desc "shell out 'forge build <root>' — lisp → IR → rust 冲压"
        :required ["project"]
        :optional ["dry_run" "output_dir"]
        :dispatches-to-worker "section worker-side-computation :: path forge-build-bridge"
        :cross-ref-intent-layer "forge 本体 (lisp→IR→rust 冲压器) 归 intent-layer pillar"
        :memory-cross-ref ["project-management"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_forge_lint
        :desc "shell out 'forge lint <root>' — governance lint on intent.lisp"
        :required ["project"]
        :dispatches-to-worker "section worker-side-computation :: path forge-build-bridge"
        :cross-ref-intent-layer "governance lint 归 intent-layer pillar :: governance component"
        :memory-cross-ref ["project-management"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "lisp_survey_worker (post-survey lint)"]
        :necessity-pending-review false))

    (compute-contract-summary
      :writes-memory ["slot_sessions" "dynamic_slots" "slot_tasks" "board_tasks" "tasks"]
      :file-writes ["control_tree.json" "terminal PNG (ephemeral)" "learned_permissions.yaml"]
      :event-emits ["SlotSessionChanged" "ManagerEvent::*" "BoardEvent" "WorkerStatusChanged"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.3 Knowledge Domain — 29 tools
  ;; ══════════════════════════════════════════════════════════
  (domain knowledge
    :desc "KB / board / skill / memory / insight / project / intent / cascade — 知识与规划相关"
    :count 29
    :mcp-shell-dir "crates/missiond-mcp/src/tools/knowledge/ (9 .rs)"
    :handler-dir "crates/missiond-daemon/src/handlers/knowledge/"

    ;; ── KB module (7 tools) ──
    (module kb
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/kb.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/kb.rs"
      :tool-count 7

      (tool mission_kb_query
        :desc "知识库查询 search/get/list (FTS5 + Embedding 混合 RRF)"
        :actions ["search" "get" "list"]
        :optional ["action" "query" "category" "limit" "offset" "search_mode" "key"]
        :dispatches-to-worker "section worker-side-computation :: path retrieval-fusion (4 路并发检索)"
        :memory-cross-ref ["kb-manager (kb_entries, kb_embeddings)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (context assembly)" "board-frontend/api/kb" "external MCP client"]
        :necessity-pending-review false
        :note "最密集的搜索消费者 — 每次 LLM 调用都可能触发")

      (tool mission_kb_remember
        :desc "记录知识到长期记忆 (已存在则更新)"
        :required ["category" "key" "summary"]
        :optional ["detail" "source" "confidence"]
        :categories-enum "preference / memory / memory:architecture / memory:bugfix / memory:debug / memory:ops / memory:feature / memory:decision / memory:platform / project / architecture / architecture:summary / decision / policy:decision / feature / infra / procedure"
        :dispatches-to-worker "N/A — memory 直写 (触发 embedding_worker via EmbeddingTask)"
        :memory-cross-ref ["kb-manager"]
        :event-emits ["KbEntryCreated / KbEntryUpdated"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (主动记忆)" "external MCP client (agent 自主)"]
        :necessity-pending-review false)

      (tool mission_kb_mutate
        :desc "KB 写操作 forget/update/import"
        :actions ["forget" "update" "import"]
        :required ["action"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/kb DELETE/PATCH"]
        :necessity-pending-review false)

      (tool mission_kb_ops
        :desc "KB 运维 gc/analyze/discover/queue_status/execute_plan/compact"
        :actions ["gc" "analyze" "discover" "queue_status" "execute_plan" "compact"]
        :required ["action"]
        :dispatches-to-worker "varies (analyze 走 sonnet gateway; discover 走 SSH; compact 走 DB)"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (定期运维)" "manual MCP debug"]
        :necessity-pending-review true
        :note "较杂 — 运维类 action 可能未来拆成独立 tool")

      (tool mission_kb_batch_set_project
        :desc "批量设置 KB 条目项目归属"
        :required ["assignments"]
        :added "commit 3c10d21"
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager" "project-management"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend (/api/kb PATCH)"]
        :necessity-pending-review false)

      (tool mission_embedding_ops
        :desc "Embedding 操作 stats/backfill"
        :actions ["stats" "backfill"]
        :required ["action"]
        :dispatches-to-worker "section xjp-router-gateway :: path xjp-router-embedding (v0.3 新) + workers/sonnet/embedding_worker"
        :memory-cross-ref ["embedding-support" "kb-manager"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_code_search
        :desc "代码结构 L3 搜索 (AST 索引)"
        :required ["query"]
        :optional ["repo" "file_path" "node_type" "limit"]
        :dispatches-to-worker "section worker-side-computation :: path retrieval-fusion (code_prefetch 主导)"
        :memory-cross-ref ["kb-manager (ast_nodes, ast_files, ast_search_hits)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend" "external MCP client"]
        :necessity-pending-review false))

    ;; ── Memory Ops (1 tool) ──
    (module memory_ops
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/memory.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/memory.rs"

      (tool mission_memory
        :desc "记忆与 Token 管理 pending/pause/token_stats"
        :actions ["pending" "pause" "token_stats"]
        :required ["action"]
        :optional ["paused" "sessionId" "slotId" "since" "groupBy"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: memory-scheduler-queue (pending); section orchestration-governance :: pause (pause)"
        :memory-cross-ref ["conversation-logs" "llm-support (token_usage_ledger)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true
        :note "3 action 职责差异大 — 可能要拆"))

    ;; ── Board (8 tools) ──
    (module board
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/board.rs"

      (tool mission_board_query
        :desc "任务板统一查询 list/get/search/summary/clear_done"
        :actions ["list" "get" "search" "summary" "clear_done"]
        :optional ["action" "status" "includeHidden" "id" "ids" "includeChildren" "query" "project" "category" "parentId" "limit" "since"]
        :dispatches-to-worker "N/A — memory 读"
        :memory-cross-ref ["board"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/tasks" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_board_create
        :desc "创建任务 (支持 parentId / DAG dependsOn / Flow flowTemplate)"
        :required ["title"]
        :optional ["description" "priority" "category" "project" "server" "dueDate" "parentId" "assignee" "autoExecute" "promptTemplate" "hidden" "flowTemplate" "dependsOn"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick (dispatch 时触发)"
        :memory-cross-ref ["board"]
        :event-emits ["BoardTaskCreated"]
        :flow-ref "pending-flow-pillar (自己就是 flow 的起点 — flowTemplate 字段)"
        :called-by ["intent-layer" "board-frontend" "worker (conversation_logger 创建 memory-hook)"]
        :necessity-pending-review false)

      (tool mission_board_update
        :desc "更新任务 (单个 id 或批量 ids)"
        :optional ["id" "ids" "title" "description" "status" "priority" "category" "project" "server" "dueDate" "parentId" "assignee" "autoExecute" "promptTemplate" "hidden" "flowPhase" "flowTemplate" "dependsOn"]
        :flow-phases ["investigate" "consult_gemini_1" "plan" "consult_gemini_2" "execute" "finalize" "done"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["board"]
        :event-emits ["BoardTaskUpdated"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "autopilot (flow-progression)" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_delete
        :desc "删除任务 (级联子任务)"
        :required ["id"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["board"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_claim
        :desc "原子认领任务 (仅 open 且未认领时成功)"
        :required ["taskId"]
        :optional ["executorId" "executorType"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick (CAS claim)"
        :memory-cross-ref ["board"]
        :event-emits ["BoardTaskClaimed"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "autopilot" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_board_retry
        :desc "重试失败/阻塞任务 (reset 状态, 可同步 reset 下游)"
        :required ["taskId"]
        :optional ["resetDownstream"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick"
        :memory-cross-ref ["board"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_note_add
        :desc "为任务添加进度笔记"
        :required ["taskId" "content"]
        :optional ["noteType" "author"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["board (notes)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "worker" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_decompose
        :desc "一键拆分任务 (派 slot 调查后自动建 DAG 子任务)"
        :required ["taskId"]
        :optional ["slotId" "hints"]
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path claude-slot-dispatch"
        :memory-cross-ref ["board"]
        :flow-ref "pending-flow-pillar (高度适合 flow 包装)"
        :called-by ["intent-layer"]
        :necessity-pending-review true))

    ;; ── Cascade (6 tools: cc_query/cc_swarm 其实按 handler 归 cascade) ──
    (module cascade
      :mcp-file "varies — cc_query/cc_swarm mcp 壳在 tools/compute/cc_tasks.rs; 其他在 knowledge/cascade.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
      :note "cascade handler 同时服务 compute 域的 cc_tasks 与 knowledge 域的 cascade_*, 按 mcp-dispatch 分类全列 knowledge"

      (tool mission_universe_graph
        :desc "跨项目 KB 索引 → 实体/关系图"
        :dispatches-to-worker "N/A — memory 读"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "external MCP client"]
        :necessity-pending-review true)

      (tool mission_cascade_plan
        :desc "cascade 规划"
        :dispatches-to-worker "section engine-cluster :: intent-engine"
        :memory-cross-ref ["board"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true)

      (tool mission_cascade_trigger
        :desc "cascade 触发"
        :dispatches-to-worker "section engine-cluster :: intent-engine"
        :event-emits ["CascadeTriggered"]
        :memory-cross-ref ["board"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true)

      (tool mission_cascade_lint
        :desc "cascade lint"
        :dispatches-to-worker "section worker-side-computation :: path forge-build-bridge (lint 借用 forge 模式)"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer"]
        :necessity-pending-review true))

    ;; ── Skill (4 tools) ──
    (module skill
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/skill.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/skill.rs"

      (tool mission_skill_query
        :desc "Skill 查询 list/search/topics/actions/stats"
        :actions ["list" "search" "topics" "actions" "stats"]
        :required ["action"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager (skill 相关表)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "external MCP client (Claude Code skill discovery)"]
        :necessity-pending-review false)

      (tool mission_skill_context
        :desc "Skill 上下文构建 build/resolve (含 requires 依赖)"
        :actions ["build" "resolve"]
        :required ["action" "query"]
        :optional ["skill" "include_board"]
        :dispatches-to-worker "section context-assembly :: path context-bundle-assembly"
        :memory-cross-ref ["kb-manager" "board (若 include_board)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (LLM 前置 context)" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_skill_mutate
        :desc "Skill 写 upsert/record/render/rollback"
        :actions ["upsert" "record" "render" "rollback"]
        :required ["action"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "skill_exec (内部 record)"]
        :necessity-pending-review false)

      (tool mission_skill_exec
        :desc "执行 Skill workflow (顺序 MCP 工具步骤)"
        :required ["skill" "action"]
        :optional ["dry_run" "params"]
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "pending-flow-pillar (与 flow-engine-v2 并存的 skill workflow 机制)"
        :called-by ["intent-layer"]
        :necessity-pending-review true
        :note "与 flow-engine-v2 职责可能重叠, 评审是否合并"))

    ;; ── Insight (1 tool) ──
    (module insight
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/insight.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/insight.rs"

      (tool mission_insight
        :desc "查看 MissionD 战略认知 (开发轨迹/协作模式/反面模式/摩擦点)"
        :optional ["section"]
        :sections ["all" "profile" "trajectory" "patterns" "proposals" "friction"]
        :dispatches-to-worker "N/A — 纯读"
        :memory-cross-ref ["kb-manager (insight 类 kb_entries)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (自省)" "指挥官查看"]
        :necessity-pending-review false))

    ;; ── Project (1 tool, 多 action) ──
    (module project
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/project.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/project.rs"
      :added "commit 76900d1 + 84ac1a6 + 8438a7d"

      (tool mission_project
        :desc "项目管理 list/get/set_active/sync/init/context/memories"
        :actions ["list" "get" "set_active" "sync" "init" "context" "memories"]
        :required ["action"]
        :special-action-init "canonicalize path → derive id → git remote → scan intent.lisp → upsert → backfill → reload SharedProjectRegistry"
        :dispatches-to-worker "section orchestration-governance :: path daemon-bootstrap (ProjectRegistry reload)"
        :memory-cross-ref ["project-management" "conversation-logs (stats)" "kb-manager (stats)" "slot-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/projects"]
        :necessity-pending-review false))

    ;; ── Intent (1 tool) ──
    (module intent
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/intent.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/intent.rs"
      :added "commit ec269d7"

      (tool mission_intent
        :desc "读 per-project intent.lisp (read/section/summary/list)"
        :actions ["read" "section" "summary" "list"]
        :required ["action"]
        :candidates-paths [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
        :dispatches-to-worker "N/A — file 读"
        :cross-ref-intent-layer "读的是 intent-layer pillar 拥有的 lisp 文件, handler 从 file 读"
        :memory-cross-ref ["project-management"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (agent 调查自身)" "external MCP client (Claude Code 导航)" "主 Claude (jarvis-manual 工作流)"]
        :necessity-pending-review false))

    (knowledge-contract-summary
      :memory-cross-ref "覆盖 memory pillar 的 7 module 中的 5 个 (kb-manager/board/project-management/conversation-logs/slot-support)"
      :event-emits ["KbEntryCreated/Updated" "BoardTaskCreated/Updated/Claimed" "CascadeTriggered/Completed"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.4 Comm Domain — 14 tools
  ;; ══════════════════════════════════════════════════════════
  (domain comm
    :desc "conversation / question / router_chat / timeline / audit / retrospective / codex_ops — 通信/观测/对话管理"
    :count 14
    :mcp-shell-dir "crates/missiond-mcp/src/tools/comm/ (7 .rs)"
    :handler-dir "crates/missiond-daemon/src/handlers/comm/"

    ;; ── Conversation (3 tools) ──
    (module conversation
      :mcp-file "crates/missiond-mcp/src/tools/comm/conversation.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/conversation.rs"

      (tool mission_conversation_query
        :desc "对话统一查询 list/get/search/message_search/context/events"
        :actions ["list" "get" "search" "message_search" "context" "events"]
        :optional ["action" "status" "conversationType" "taskId" "sessionId" "tail" "sinceId" "includeRaw" "query" "queryMode" "timeRange" "project" "excludeSessionId" "offset" "role" "toolName" "messageId" "before" "after" "eventType" "limit" "since" "until"]
        :dispatches-to-worker "section worker-side-computation :: path retrieval-fusion (search hybrid mode)"
        :memory-cross-ref ["conversation-logs"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (历史回看)" "board-frontend/api/conversations" "external MCP client (auto-memory)"]
        :necessity-pending-review false
        :note "用户在 CLAUDE.md 指定: 读历史会话 → mission_conversation_query(action=get); 复盘 → mission_retrospective")

      (tool mission_conversation_analyze
        :desc "对话分析 retrospective/trajectory/activity"
        :actions ["retrospective" "trajectory" "activity"]
        :required ["action"]
        :optional ["sessionId" "depth" "toolUseId" "since" "until" "limit"]
        :dispatches-to-worker "section engine-cluster :: subsection learning-engine (intent-layer 主 ownership) + retro_worker"
        :memory-cross-ref ["conversation-logs" "system-support (deep_analysis)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "指挥官"]
        :necessity-pending-review false)

      (tool mission_conversation_reconcile
        :desc "JSONL-DB 对账 (不传 sessionId 全量扫)"
        :optional ["sessionId"]
        :dispatches-to-worker "section worker-cluster :: worker-local :: cli-ingestion functional-group (reconcile_worker / gemini_reconcile_worker)"
        :memory-cross-ref ["conversation-logs" "system-support (reconcile_watermarks)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (运维)" "手动 debug"]
        :necessity-pending-review false))

    ;; ── Question (1 tool) + LLM trace / Decision stats / Gemini auth / Incident ──
    ;; 这 5 个 tool 都在 tools/comm/question.rs 的 mcp 壳, handler 散在 comm + sysinfra
    (module question
      :mcp-file "crates/missiond-mcp/src/tools/comm/question.rs"

      (tool mission_question
        :desc "Agent 待决策问题管理 create/list/get/answer/dismiss"
        :actions ["create" "list" "get" "answer" "dismiss"]
        :required ["action"]
        :optional ["id" "question" "context" "taskId" "slotId" "sessionId" "target" "options" "decisionType" "answer" "status" "limit"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs"
        :dispatches-to-worker "N/A — 可能触发 decision-engine (intent-layer primary)"
        :memory-cross-ref ["system-support (agent_questions)" "board (可能 blocked)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "worker (生成提问)" "board-frontend (回答 UI)"]
        :necessity-pending-review false)

      (tool mission_llm_trace
        :desc "LLM 调用链路追踪 gemini_trace/stats/watch/auth/jarvis_logs/jarvis_trace"
        :actions ["gemini_trace" "gemini_stats" "gemini_watch" "gemini_auth" "jarvis_logs" "jarvis_trace"]
        :required ["action"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/audit.rs"
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["llm-support (gemini_requests)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (自省 LLM 行为)" "board-frontend/api/system/llm-traces"]
        :necessity-pending-review false)

      (tool mission_decision_stats
        :desc "Decision Engine 统计"
        :optional ["hours"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/conversation.rs"
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["system-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "指挥官"]
        :necessity-pending-review true
        :note "归属: decision-engine 逻辑本身属 intent-layer pillar, 此 tool 是对外查询面")

      (tool mission_gemini_auth
        :desc "Gemini CLI 认证模式切换"
        :optional ["mode"]
        :modes ["apikey" "google" "status"]
        :handler-file "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
        :dispatches-to-worker "N/A — 配置修改"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官" "setup 脚本"]
        :necessity-pending-review true
        :note "mcp 壳放 comm/question.rs, handler 在 sysinfra/misc — 位置别扭, 评审是否挪位")

      (tool mission_incident
        :desc "AIOps Incident 管理 test/list"
        :actions ["test" "list"]
        :required ["action"]
        :optional ["severity" "title" "source" "server_id" "limit"]
        :handler-file "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
        :dispatches-to-worker "N/A (但 worker::infra::aiops 会产 incidents)"
        :memory-cross-ref ["system-support (incidents)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "aiops 健康扫描 worker (产)" "board-frontend"]
        :necessity-pending-review true
        :note "mcp 壳放 comm/question.rs, handler 在 sysinfra — 位置别扭"))

    ;; ── Router Chat (2 tools) ──
    (module router_chat
      :mcp-file "crates/missiond-mcp/src/tools/comm/router_chat.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/router_chat.rs"

      (tool mission_router_chat
        :desc "通过 AI 路由器与 Gemini 等模型多轮对话"
        :optional ["messages" "message" "task_id" "context" "model" "max_tokens" "search" "files" "idle_timeout" "channel" "api_key_alias"]
        :dispatches-to-worker "section llm-gateways :: path gemini-unified-gateway (目前); section xjp-router-gateway :: path xjp-router-chat-future (未来)"
        :external-service "XJP Router (HTTP 代理 Gemini/Sonnet/Minimax)"
        :memory-cross-ref ["conversation-logs (router 历史)" "llm-support"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "gemini-router slot (registered-tasks)" "external MCP client"]
        :necessity-pending-review false
        :note "`interactive caller` 豁免 llm_gate (REQUEST_CALLER='router_chat', check_interactive_exempt)")

      (tool mission_router_chat_manage
        :desc "Gemini 对话管理 history/list/delete/clear/delete_message/restore/stats/compress"
        :actions ["history" "list" "delete" "clear" "delete_message" "restore" "stats" "compress"]
        :required ["action"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["conversation-logs"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false))

    ;; ── Timeline (1 tool) ──
    (module timeline
      :mcp-file "crates/missiond-mcp/src/tools/comm/timeline.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/timeline.rs"

      (tool mission_timeline
        :desc "系统时间轴 query/trace/stats/search"
        :actions ["query" "trace" "stats" "search"]
        :required ["action"]
        :optional ["eventType" "traceId" "since" "until" "limit" "offset" "keyword"]
        :dispatches-to-worker "N/A — 读 event-bus pillar (跨 pillar)"
        :cross-ref-pillar-four "event-bus pillar :: event_log 表"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "board-frontend/api/timeline/events"]
        :necessity-pending-review false))

    ;; ── Audit (1 tool) ──
    (module audit
      :mcp-file "crates/missiond-mcp/src/tools/comm/audit.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/audit.rs"

      (tool mission_audit
        :desc "对话工具调用审计 trace/detail/stats/export"
        :actions ["trace" "detail" "stats" "export"]
        :required ["action"]
        :optional ["sessionId" "toolId" "taskId" "toolFilter" "includeReasoning" "includeMessages"]
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["conversation-logs (tool_calls)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (自省)" "指挥官 debug"]
        :necessity-pending-review false))

    ;; ── Retrospective (1 tool) ──
    (module retrospective
      :mcp-file "crates/missiond-mcp/src/tools/comm/conversation.rs (共用 conversation.rs 模块)"
      :handler-file "crates/missiond-daemon/src/handlers/comm/retrospective.rs"

      (tool mission_retrospective_manage
        :desc "复盘管理 list/backfill"
        :actions ["list" "backfill"]
        :required ["action"]
        :optional ["limit" "since"]
        :dispatches-to-worker "section worker-cluster :: worker-sonnet :: path retro-worker-cycle"
        :memory-cross-ref ["conversation-logs (retrospectives)" "system-support (deep_analysis)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "指挥官 '会话复盘'"]
        :necessity-pending-review false
        :note "CLAUDE.md 指定: 会话复盘 → mission_retrospective"))

    ;; ── Beacon (1 tool) ──
    (module beacon
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/kb.rs (mcp 壳在 kb module)"
      :handler-file "crates/missiond-daemon/src/handlers/comm/audit.rs"
      :note "mcp 归 knowledge, handler 归 comm — 跨域 tool"

      (tool mission_beacon
        :desc "代码信标操作 list/map/upsert"
        :actions ["list" "map" "upsert"]
        :required ["action"]
        :optional ["name" "file_path" "symbol" "feature" "annotation"]
        :dispatches-to-worker "section worker-cluster :: worker-local :: path ast-sync-worker-cycle (间接)"
        :memory-cross-ref ["kb-manager (beacon_nodes)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "ast_sync_worker (间接)"]
        :necessity-pending-review false))

    ;; ── Codex Ops (1 tool) ──
    (module codex_ops
      :mcp-file "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/codex_ops.rs"
      :added "commit ec269d7"

      (tool mission_codex_ops
        :desc "查询 Codex CLI 操作历史 (from ~/.codex/state_5.sqlite via codex_ingestion_worker)"
        :actions ["query" "history" "stats"]
        :required ["action"]
        :dispatches-to-worker "N/A — 读 codex_ingestion_worker 已摄入的 conversations"
        :memory-cross-ref ["conversation-logs"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer (查 Codex 历史)"]
        :necessity-pending-review false))

    (comm-contract-summary
      :memory-cross-ref ["conversation-logs" "system-support" "llm-support" "kb-manager"]
      :cross-pillar ["event-bus (timeline)" "worker pillar (retro / reconcile 驱动)"]
      :event-consumption []))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.5 Sysinfra Domain — 14 tools
  ;; ══════════════════════════════════════════════════════════
  (domain sysinfra
    :desc "system / infra / permission / power / misc — 基础设施与权限管理"
    :count 14
    :mcp-shell-dir "crates/missiond-mcp/src/tools/sysinfra/ (5 .rs)"
    :handler-dir "crates/missiond-daemon/src/handlers/sysinfra/"

    ;; ── System (4 tools, all in system.rs) ──
    (module system
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/system.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/system.rs"

      (tool mission_sys_logs
        :desc "读 MissionD daemon 运行日志尾部"
        :optional ["lines" "level" "grep"]
        :dispatches-to-worker "N/A — 读文件"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官 debug" "intent-layer"]
        :necessity-pending-review false)

      (tool mission_sys_config
        :desc "读/写 daemon 配置"
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["system-support (daemon_state)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官 setup" "autopilot (动态配置)"]
        :necessity-pending-review false)

      (tool mission_daemon_update
        :desc "daemon 自更新"
        :dispatches-to-worker "N/A — 外部流程"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官"]
        :necessity-pending-review true)

      ;; mission_control 已在 compute/worker.rs 定义 - 不重复)
      )

    ;; ── Infra (2 tools) ──
    (module infra
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"

      (tool mission_infra_query
        :desc "基础设施查询 list/get"
        :actions ["list" "get"]
        :required ["action"]
        :optional ["id" "role" "provider"]
        :dispatches-to-worker "N/A — 读 infra/servers.yaml 或 DB"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官" "intent-layer (ops 任务)"]
        :necessity-pending-review false)

      (tool mission_infra_ops
        :desc "基础设施运维 health/reachability/diagnose"
        :actions ["health" "reachability" "diagnose"]
        :required ["action"]
        :optional ["target" "channels" "checks"]
        :dispatches-to-worker "section worker-cluster :: worker-local (aiops 跨 pillar 到 system pillar infra/aiops.rs)"
        :memory-cross-ref ["system-support (incidents)"]
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官" "aiops worker"]
        :necessity-pending-review false))

    ;; ── Permission (2 tools) ──
    (module permission
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
      :added "Phase 1-5 upgrade 2026-04-12"

      (tool mission_permission_query
        :desc "权限查询 get/learned_list (含 merged_for_slot debug 视图)"
        :actions ["get" "learned_list"]
        :required ["action"]
        :optional ["scopeType" "scopeId"]
        :dispatches-to-worker "section pty :: subsection learned-permissions :: mcp-merged-view"
        :memory-cross-ref []
        :file-reads ["learned_permissions.yaml"]
        :flow-ref "pending-flow-pillar"
        :called-by ["intent-layer" "指挥官 audit"]
        :necessity-pending-review false)

      (tool mission_permission_mutate
        :desc "权限写 set_role/set_slot/auto_allow/reload/revoke"
        :actions ["set_role" "set_slot" "auto_allow" "reload" "revoke"]
        :required ["action"]
        :dispatches-to-worker "section pty :: subsection learned-permissions :: path learned-permission-read (reload)"
        :memory-cross-ref []
        :file-writes ["learned_permissions.yaml"]
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官 (手动配置)" "intent-layer"]
        :necessity-pending-review false))

    ;; ── Power (1 tool) ──
    (module power
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/power.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/power.rs"

      (tool mission_power_control
        :desc "物理服务器电源管控 wake(WoL/gcloud)/suspend/status"
        :required ["target" "action"]
        :dispatches-to-worker "N/A — 外部 WoL / gcloud API"
        :memory-cross-ref []
        :flow-ref "pending-flow-pillar"
        :called-by ["指挥官" "intent-layer (大任务前唤醒 GPU 机)"]
        :necessity-pending-review false))

    ;; ── Misc (5 tools — 杂项, 多个 tool 共用 misc handler) ──
    (module misc
      :mcp-file "varies — pause 在 tools/compute/slot.rs; inbox/incident/gemini_auth 在 tools/comm/question.rs; submit_phase_result 在 tools/knowledge/board.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
      :note "misc handler 聚合多个跨 domain 的杂项 tool"

      ;; mission_pause 已在 compute/slot 列, 不重复
      ;; mission_inbox 已在 compute/process 列
      ;; mission_incident 已在 comm/question 列
      ;; mission_gemini_auth 已在 comm/question 列

      (tool mission_submit_phase_result
        :desc "Flow 任务阶段产出物提交 (系统自动推进到下一阶段)"
        :required ["taskId" "artifactType" "content"]
        :optional ["requiresMasterDecision"]
        :artifact-types ["investigation_report" "execution_plan" "execution_result" "commit_hash"]
        :mcp-shell-file "crates/missiond-mcp/src/tools/knowledge/board.rs (mcp 壳在 board)"
        :handler-file "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick (flow-progression 推进 board_task 的 flow_phase)"
        :memory-cross-ref ["board (board_tasks.flow_phase / flow_context)"]
        :flow-ref "pending-flow-pillar (自身就是 flow 推进工具)"
        :called-by ["intent-layer (agent 阶段完成)" "external MCP client"]
        :necessity-pending-review false))

    (sysinfra-contract-summary
      :memory-cross-ref ["system-support" "board"]
      :file-writes ["control_tree.json" "learned_permissions.yaml"]
      :cross-pillar ["system-layer (config/log)" "worker pillar (infra/aiops)"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.6 Tool Governance — schema / audit / reload
  ;; ══════════════════════════════════════════════════════════
  (section tool-governance

    (schema-source-of-truth
      :file ".missiond/intent-mcp-defs.lisp"
      :format "(mcp-module X :target ... (tool mission_X :description ... (input ...) :returns ... :dispatch-on action))"
      :invariant "tool schema 变更必须先改本 lisp, 由 Forge 冲压 gen_gateway.rs")

    (handler-mapping-source-of-truth
      :file ".missiond/intent-pillar-mcp-dispatch.lisp"
      :format "(tool mission_X :handler 'handlers::group::module')"
      :invariant "handler 迁移必须同步改本 lisp (mcp-dispatch 老图)")

    (tool-call-log
      :desc "所有 tool 调用的执行审计轨迹"
      :table "tool_calls"
      :memory-module "conversation-logs (v0.5.1 frozen)"
      :writer "gen_gateway.rs 路由入口 (每 tools/call 前后)")

    (tool-registry-runtime
      :target "crates/missiond-mcp/src/gen_gateway.rs"
      :mechanism "tool_name → handler_fn 数据驱动 dispatch, Forge 冲压生成"))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (T001-T010)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (T001 "实际 tool 总数确认 — mcp-defs 头说 67, 实际按 mcp-dispatch + 磁盘应是 78. 已按 78 计数")
    (T002 "每 tool :flow-ref 具体值 — 全部 'pending-flow-pillar', 待 flow pillar 设计后逐条填")
    (T003 "每 tool :necessity-pending-review — 指挥官逐条评审, 评审后改 false 或标 'remove-candidate'")
    (T004 "跨 mcp 壳 vs handler 位置的 tool (pause / slot_history / beacon / inbox / incident / gemini_auth / submit_phase_result) — 是历史还是设计? 是否要挪位?")
    (T005 "mission_minimax_process deprecated — 评审何时真删")
    (T006 "mission_memory 3 action (pending/pause/token_stats) 职责差异大 — 是否要拆 3 个 tool?")
    (T007 "mission_skill_exec vs flow-engine-v2 职责重叠 — 评审是否合并")
    (T008 "mission_kb_ops 6 action (gc/analyze/discover/queue_status/execute_plan/compact) 较杂 — 是否拆")
    (T009 "mcp-defs 40KB 我只读了约一半 (头 800 行), 剩余部分需回看 — sys_config / daemon_update / power_control / infra_query / permission 等 schema 具体")
    (T010 "flow pillar 设计时, 需评估每 tool 的 flow 应该是 single-node 还是 multi-node — 若 single-node, 是否真有必要包装 flow?"))
)
