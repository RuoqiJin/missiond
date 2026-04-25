;; ═════════════════════════════════════════════════════════════
;; MissionD — Tools Pillar (phase-C recursive-contract v0.7)
;; 目标: 按 4 domain (compute/knowledge/comm/sysinfra) 列全 83 MCP 工具
;;       每工具含 :dispatches-to-worker / :memory-cross-ref / :flow-ref (预留) /
;;                :called-by / :necessity-pending-review
;;       future surfaces 单独列, 当前 backlog 清零
;; 底稿: intent-mcp-defs.lisp (schema SSOT, 40KB) + intent-pillar-mcp-dispatch.lisp
;;       (handler 映射) + v0.4 intent-worker.lisp :mcp-surface-to-tools (worker 侧)
;; 架构原则: tools 是"对外服务端点", 不是汇合点 — 单向上暴露, 单向下派发
;; ═════════════════════════════════════════════════════════════

(pillar tools
  :version "v0.7"
  :status "phase-C recursive architecture contract 2026-04-25 — 83 actual endpoints; execution/capability usage/directive-plan-workflow/global-instruction surfaces code-aligned; project-root spawn cwd contract aligned; mission_pty_spawn / mission_pty_send / mission_compute_slot / mission_task_delegate marked as preferred workstation dispatch substrate for unified-entry pipeline (resident-lisp / fresh-code-alignment / agent-team / spawn-over-prompt); 不新增 tool"
  :predecessor "drafts/gptpro/intent-tools.lisp (239 行 starter)"
  :target-path ".missiond/v2/intent-tools.lisp"

  :actual-state-sources
    [".missiond/intent-mcp-defs.lisp (schema SSOT, 24 mcp-module, 详细 input/returns)"
     ".missiond/intent-pillar-mcp-dispatch.lisp (tool → handler 映射权威)"
     ".missiond/intent-rpc-gateway.lisp (JSON-RPC gateway 层)"
     "crates/missiond-mcp/src/ (MCP 壳 — 13+9+7+5 = 34 文件)"
     "crates/missiond-daemon/src/handlers/ (daemon 实际 handler 逻辑)"
     ".missiond/v2/intent-worker.lisp v0.5 :mcp-surface-to-tools + project-root spawn cwd (worker 侧对齐)"
     ".missiond/v2/intent-memory.lisp v0.5.4 :: agent-execution-coordination + directive artifacts + capability usage read-model"
     ".missiond/v2/intent-intent-layer.lisp v0.4 :: mcp-tools-future + methodology compile + capability governance"
     ".missiond/v2/intent-flow.lisp v0.7 :: F-incident-reaction + F-methodology-to-executable-compile + F-capability-usage-monitoring + project-root spawn cwd"]

  :design-correction-sources
    [".missiond/v2/intent.lisp :: pillar tools (占位)"
     ".missiond/v2/intent-memory.lisp v0.5.4 (每 module :mcp-surface + execution protocol + directive artifacts + capability usage read-model)"
     "drafts/gptpro/intent-tools.lisp (gptpro draft — 架构方向已纠正, 本 v0.1 不继承其 'tools 是汇合点' 的误解)"]

  :historical-footprint-sources
    ["mcp-defs 头部 '67 tools' 是旧数, 当前实际 83 (含 mission_execution / mission_capability_usage / mission_directive / mission_plan / mission_workflow / mission_global_instruction 等新增)"
     "mission_minimax_process 已 Deprecated → mission_sonnet_process (mcp-defs 显式标注)"
     "future surfaces backlog cleared in this batch"
     "promoted surfaces: mission_execution (4ab7994) / mission_capability_usage (c55fd61) / mission_directive / mission_plan / mission_workflow / mission_global_instruction 已进入当前 MCP registry"]

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
         "mission_beacon: 壳 tools/knowledge/kb.rs (mcp module 'kb'), intended handler knowledge::kb; architecture requires consolidated name dispatch, legacy mission_beacon_* 仍兼容"
         "mission_codex_ops: 壳 tools/comm/codex_ops.rs, handler comm::codex_ops"]
      :rationale "MCP schema 归组 ≠ handler 归组 (历史原因). 本 lisp 按 handler 归组 (mcp-dispatch 老图为准)")

    (Q-T5
      :question "mission_minimax_process 已 deprecated, 本 lisp 如何处理?"
      :decision "保留列项, 标 :status 'deprecated-migrated-to-sonnet'; 下一次 breaking MCP schema cleanup 再删"
      :rationale "弃用不即删, 避免 legacy caller 断裂; 架构上不再把它视为战略能力面"))

  (purpose "通过 MCP JSON-RPC 对外暴露 83 个当前服务端点 + future manager surfaces — intent-layer 主调用者, flow pillar 下游 orchestration, 单向不汇合")

  (recursive-architecture-contract
    :shape "pillar = ingress → logic-core → egress; function/tool-family/tool = ingress → logic-core(ordered steps) → egress"
    :unit "tool endpoint 是本 pillar 的最小原子; capability-family 是工具分子; MCP gateway 是 pillar 级外壳"
    :rule-1 "tools pillar 只拥有 schema / validation / dispatch / error normalization / audit, 不拥有业务语义"
    :rule-2 "每个非 trivial tool 的 logic-core 只写到 owner pillar 边界, 真实业务步骤写在 flow/worker/memory/intent-layer 对应 pillar"
    :rule-3 "每个 tool 必须有 :flow-ref; 值只能是 named-flow / shared-flow / trivial-single-step / pending-with-reason"
    :rule-4 "功能继续梳理时, 优先按 capability-family 分组, 再下钻到单 tool")

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
    :contract "对外 call 进入 tools 后, 只经过 endpoint-shell → registry-dispatch → owner-pillar-boundary → audit/result 四层"

    (function mcp-json-rpc-ingress
      (ingress
        :source "stdio JSON-RPC tools/call"
        :target "crates/missiond-mcp/src/bin/mission-mcp.rs + server.rs")
      (logic-core
        (step s1 "解析 JSON-RPC request, 提取 tool_name + args")
        (step s2 "按 mcp-defs schema 做参数 validation / action normalization")
        (step s3 "把合法调用交给 gen_gateway 数据驱动路由"))
      (egress
        :to "function tool-registry-dispatch"
        :error "统一 JSON-RPC error shape"))

    (function tool-registry-dispatch
      (ingress
        :source "validated tool_name + args"
        :registry "crates/missiond-mcp/src/gen_gateway.rs")
      (logic-core
        (step s1 "tool_name → handler domain (compute/knowledge/comm/sysinfra)")
        (step s2 "调用 gateway_impl / daemon handler")
        (step s3 "保持 MCP 壳与 daemon handler 的文件归组差异显式可见"))
      (egress
        :to-owner-pillar ["worker" "memory" "intent-layer" "system-layer" "flow"]
        :returns "ToolResult"))

    (function capability-family-contract
      (ingress
        :source "83 tool endpoints grouped by capability-family"
        :families ["pty" "task" "slot" "board" "kb" "skill" "cascade" "conversation" "llm" "system" "permission"])
      (logic-core
        (step s1 "每个 family 先判断 shared-flow / named-flow / trivial-single-step")
        (step s2 "family 下每个 tool 记录 :dispatches-to-worker / :memory-cross-ref / :flow-ref")
        (step s3 "非 trivial family 反推 flow pillar 的 ordered steps"))
      (egress
        :to-flow "tool-backed-flows-index"
        :to-review "necessity-pending-review 清单"))

    (function tool-audit-and-result
      (ingress
        :source "owner handler result or error")
      (logic-core
        (step s1 "标准化 ToolResult::text/json/error")
        (step s2 "写 tool_calls audit (memory :: conversation-logs)")
        (step s3 "返回 JSON-RPC response 给外部 caller"))
      (egress
        :returns "JSON-RPC result / error"
        :writes ["tool_calls"]))

    (core-invariants
      (core-1 "4 domain, 83 tools 总: compute 27 / knowledge 31 / comm 16 / sysinfra 9 (按 crates/missiond-mcp/src/tools 当前 registry)")
      (core-2 "schema SSOT 在 .missiond/intent-mcp-defs.lisp (24 mcp-module + 详细 input/returns)")
      (core-3 "handler mapping SSOT 在 .missiond/intent-pillar-mcp-dispatch.lisp (tool → handler)")
      (core-4 "tool-registry + gen_gateway 是唯一路由入口, 禁止散落手写 bypass")
      (core-5 "审计: 所有 tool 调用写 tool_calls 表 (memory :: conversation-logs)")
      (core-6 "future-mcp-surfaces 不计入当前 83; 实现时必须先进入 mcp-defs + dispatch SSOT, 再变更 count")))

  (pillar-egress
    (egress-1 "→ flow pillar: 每 tool 背后对应 flow (待 flow pillar 设计; 当前仅 mission_flow_run 是显式 flow)")
    (egress-2 "→ worker pillar: 14 compute + 4 sysinfra = 18 tools 映射 worker path (v0.5 intent-worker.lisp :mcp-surface-to-tools + project-root spawn cwd 已详述)")
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
      :capability-family "pty-session-control"

      (capability-family pty-session-control
        (ingress
          :tools ["mission_pty_spawn" "mission_pty_send" "mission_pty_read" "mission_pty_screenshot" "mission_pty_status" "mission_pty_signal" "mission_pty_confirm"]
          :callers ["intent-layer" "board-frontend" "external MCP client" "flow-engine-v2 SlotTask"])
        (logic-core
          (step s1 "spawn path provisions a slot through spawn_tracked_slot and optional waitForIdle readiness")
          (step s2 "send path dispatches prompt text into an existing PTY session, either blocking or fire-and-forget")
          (step s3 "read/status/screenshot paths observe screen, history, logs, FSM state, progress, last response, or PNG terminal image")
          (step s4 "signal path interrupts or kills a session; kill also requeues running tasks and releases board claims")
          (step s5 "confirm path answers pending permission dialogs and may persist learned role/project permissions"))
        (egress
          :flows ["F-workflow-slot-full-lifecycle :: s2/s3/s4/s5/s7" "F-learned-permission :: manual confirm branch"]
          :memory-cross-ref ["slot-support slot_sessions/slot_tasks" "board board_tasks claims" "learned_permissions.yaml"]
          :events ["SlotSessionChanged" "ManagerEvent::*" "PtyStateChanged"]))

      (tool mission_pty_spawn
        :desc "启动 PTY 交互式会话"
        :schema-ref "intent-mcp-defs.lisp :: mcp-module pty :: mission_pty_spawn"
        :required ["slotId"]
        :optional ["waitForIdle" "timeoutSecs" "autoRestart" "mcpConfigPath" "cwd" "projectId"]
        (ingress
          :schema "slotId required; waitForIdle/timeoutSecs/autoRestart/mcpConfigPath optional; cwd/projectId resolve target_project_root for project-bound CLI spawn"
          :callers ["intent-layer" "board-frontend/api/pty/spawn" "external MCP client"])
        (logic-core
          (step s1 "parse PTYSpawnArgs and find matching slot from mission slot registry")
          (step s2 "resolve target_project_root from projectId/cwd/slot default; reject project-bound spawn if unresolved or cwd outside registered project")
          (step s3 "build PTYSlot and resolve mcp_config_path from arg before slot default")
          (step s4 "call slot_orchestrator::spawner::spawn_tracked_slot with process cwd=target_project_root, the sole spawn bottleneck")
          (step s5 "if waitForIdle is true, wait for readiness within timeoutSecs")
          (step s6 "return spawned PTY session info"))
        (egress
          :writes ["slot_sessions" "runtime PTY manager session"]
          :emits ["SlotSessionChanged" "ManagerEvent::TextComplete" "PtyStateChanged"]
          :returns "PTY session info json")
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: invariant sole-spawn-bottleneck (spawn_tracked_slot)"
        :memory-cross-ref ["slot-support"]
        :event-emits ["SlotSessionChanged" "ManagerEvent::TextComplete" "PtyStateChanged"]
        :flow-ref "F-workflow-slot-full-lifecycle :: s2-slot-provision + s3-slot-readiness / F-workstation-dispatch-policy :: s2 fresh-code-alignment substrate"
        :called-by ["intent-layer" "board-frontend/api/pty/spawn" "external MCP client" "unified entry pipeline plan-runner (preferred over claude -p)"]
        :workstation-dispatch-role "preferred spawn substrate for unified-entry pipeline — fresh code-alignment session 与 resident lisp-architect session 都从这里 sole-spawn-bottleneck 落地; 不允许 plan-runner 默认走 claude -p"
        :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: policy spawn-over-prompt-mode + project-root-cwd-contract"
        :necessity-pending-review false)

      (tool mission_pty_send
        :desc "向 PTY 发送消息, 默认 fire-and-forget"
        :required ["slotId" "message"]
        :optional ["waitForResponse" "timeoutMs"]
        (ingress
          :schema "slotId and message required; waitForResponse selects blocking mode; timeoutMs applies to blocking send"
          :callers ["intent-layer" "board-frontend" "external MCP client" "flow-engine-v2 SlotTask"])
        (logic-core
          (step s1 "parse slotId/message/waitForResponse/timeoutMs")
          (step s2 "if waitForResponse=true, call state.pty.send(slot_id, message, timeout)")
          (step s3 "if waitForResponse=false, call send_fire_and_forget")
          (step s4 "return delivery metadata plus response/duration when blocking"))
        (egress
          :writes ["conversation stream indirectly through PTY/CLI JSONL ingestion"]
          :emits ["ManagerEvent::TextComplete when response completes"]
          :returns "delivered/mode/response?/duration?/hint")
        :dispatches-to-worker "section pty :: subsection pty-transport :: path pty-session-lifecycle"
        :memory-cross-ref []
        :flow-ref "F-workflow-slot-full-lifecycle :: s4-dispatch-workflow"
        :called-by ["intent-layer" "board-frontend" "external MCP client" "flow-engine-v2 SlotTask"]
        :necessity-pending-review false)

      (tool mission_pty_read
        :desc "读取 PTY 内容"
        :actions ["screen" "history" "logs"]
        :required ["action" "slotId"]
        (ingress
          :schema "action and slotId required; action in screen/history/logs; screen accepts optional lines"
          :callers ["intent-layer" "board-frontend/api/pty/screen" "external MCP client"])
        (logic-core
          (step s1 "dispatch consolidated mission_pty_read by action")
          (step s2 "screen returns current terminal screen or tail lines")
          (step s3 "history returns PTY history buffer")
          (step s4 "logs resolves JSONL/log file path and tail command hint"))
        (egress
          :reads ["PTY screen buffer" "PTY history buffer" "slot_sessions/conversations for log path"]
          :returns "screen/history/log metadata")
        :dispatches-to-worker "section pty :: subsection pty-transport :: path pty-signal-extraction"
        :flow-ref "F-workflow-slot-full-lifecycle :: s5-monitor-execution"
        :called-by ["intent-layer" "board-frontend/api/pty/screen" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_pty_screenshot
        :desc "截取 PTY 终端 PNG"
        :required ["slotId"]
        (ingress
          :schema "slotId required"
          :callers ["board-frontend" "intent-layer (debug)"])
        (logic-core
          (step s1 "create screenshots directory under mission home")
          (step s2 "request browser xterm.js canvas screenshot via screenshot_broker")
          (step s3 "if browser screenshot times out/fails, fallback to backend alacritty grid rendering")
          (step s4 "write PNG and return path/source"))
        (egress
          :file-writes ["$MISSIOND_HOME/screenshots/<slotId>-<timestamp>.png"]
          :returns "{path, source, hint}")
        :dispatches-to-worker "section pty :: subsection pty-transport :: screenshot.rs"
        :flow-ref "F-workflow-slot-full-lifecycle :: s5-monitor-execution"
        :called-by ["board-frontend" "intent-layer (debug)"]
        :necessity-pending-review false)

      (tool mission_pty_status
        :desc "获取 PTY 会话状态 (FSM 当前 state)"
        :optional ["slotId"]
        (ingress
          :schema "slotId optional; omitted means all PTY status"
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "if slotId provided, read one PTY status; otherwise read all status")
          (step s2 "for one slot, enrich with slot session uuid and JSONL lastActivitySecsAgo")
          (step s3 "attach slot_progress if available")
          (step s4 "attach truncated lastResponse to avoid blank-screen misjudgment")
          (step s5 "return enriched status or null"))
        (egress
          :reads ["PTY manager status" "slot_sessions" "conversations" "slot_progress" "slot_last_responses"]
          :returns "single enriched status / all status / null")
        :dispatches-to-worker "section pty :: subsection pty-state-machine (查询 8-state FSM)"
        :flow-ref "F-workflow-slot-full-lifecycle :: s3-slot-readiness + s5-monitor-execution + s6-completion-detection"
        :called-by ["intent-layer" "board-frontend" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_pty_signal
        :desc "向 PTY 发送信号 kill/interrupt"
        :actions ["kill" "interrupt"]
        :required ["action" "slotId"]
        (ingress
          :schema "action and slotId required; action in kill/interrupt"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "dispatch consolidated mission_pty_signal by action")
          (step s2 "interrupt writes interrupt signal and keeps slot session alive")
          (step s3 "kill terminates PTY session")
          (step s4 "after kill, clear pending compact restart, requeue running tasks, and release board claims"))
        (egress
          :writes ["tasks requeued when kill" "board_tasks claims released when kill" "runtime PTY session terminated/interrupted"]
          :returns "success plus requeuedTasks/claimsReleased for kill")
        :dispatches-to-worker "section pty :: subsection pty-transport :: path pty-session-lifecycle"
        :flow-ref "F-workflow-slot-full-lifecycle :: s7-teardown"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_pty_confirm
        :desc "发送确认响应 (auto-approve 的手动 MCP 对偶)"
        :required ["slotId" "response"]
        (ingress
          :schema "slotId and response required; response may be boolean, number, or string"
          :callers ["intent-layer" "board-frontend/api/pty/confirm"])
        (logic-core
          (step s1 "capture pending confirm info before responding")
          (step s2 "normalize response into ConfirmResponse Yes/No/Option(n), or raw write fallback")
          (step s3 "send confirmation to PTY session")
          (step s4 "if approval can be learned, extract param pattern and optional project path")
          (step s5 "persist learned permission at role scope and project scope when resolvable"))
        (egress
          :writes ["learned_permissions.yaml role scope" "learned_permissions.yaml project scope when project resolves"]
          :returns "{success, slotId, response}")
        :dispatches-to-worker "section pty :: subsection learned-permissions (confirm flow 手动路径)"
        :event-emits ["learn permission if opt2"]
        :flow-ref "F-learned-permission :: manual confirm branch + F-workflow-slot-full-lifecycle :: s5-monitor-execution (Confirming)"
        :called-by ["intent-layer" "board-frontend/api/pty/confirm"]
        :necessity-pending-review false)

      (contract-summary-for-pty-module
        :memory-write ["slot_sessions" "learned_permissions.yaml (file)"]
        :events ["ManagerEvent::*" "PtyStateChanged" "SlotSessionChanged"]))

    ;; ── Task module (3 tools) ──
    (module task
      :mcp-file "crates/missiond-mcp/src/tools/compute/task.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/task.rs"
      :capability-family "task-execution"

      (capability-family task-execution
        (ingress
          :tools ["mission_task_submit" "mission_task_query" "mission_task_cancel" "mission_task_delegate"]
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "task_submit 建 legacy tasks queue entry, 尝试即时 PTY dispatch 或 auto-spawn")
          (step s2 "task_query 读取/追踪/ack legacy tasks queue")
          (step s3 "task_cancel 对 queued/running legacy task 做 guarded cancel")
          (step s4 "task_delegate 走 board_task declarative lifecycle, 与 legacy tasks queue 分离"))
        (egress
          :flows ["F-task-submit-dispatch" "F-task-legacy-queue-control" "F-task-delegate-autoprovision"]
          :memory-cross-ref ["system-support legacy tasks" "board board_tasks" "slot-support slot_sessions/dynamic_slots"]
          :events ["SlotEvent::TaskDispatched" "TaskEvent::Created"]))

      (tool mission_task_submit
        :desc "提交任务给专家 Agent, async 或 sync"
        :actions ["async" "sync"]
        :required ["role"]
        :optional ["action" "prompt" "question" "slotId" "timeoutMs"]
        (ingress
          :schema "role required; async uses prompt/question; sync uses question; optional slotId targets one slot"
          :default-action "async"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "action dispatch: async → handle_submit; sync → handle_ask")
          (step s2 "submit_task(role, prompt|question) writes legacy tasks row")
          (step s3 "optional slotId writes task.slot_id for autopilot fallback")
          (step s4 "build candidate slots: slotId or role-matched slots")
          (step s5 "try slot_dispatch guard + Idle send_fire_and_forget")
          (step s6 "if no idle dispatch, auto-spawn Exited/None candidate via spawn_tracked_slot")
          (step s7 "on dispatch update task running fields + emit SlotEvent::TaskDispatched; otherwise emit TaskEvent::Created"))
        (egress
          :writes ["tasks row" "task.status/slot_id/session_id/started_at when dispatched"]
          :emits ["SlotEvent::TaskDispatched" "TaskEvent::Created"]
          :returns "{taskId, dispatched, slotId?|hint?}")
        :dispatches-to-worker "handlers/compute/task.rs → legacy tasks queue + slot_dispatch guard + optional spawn_tracked_slot"
        :memory-cross-ref ["system-support (tasks 表)"]
        :flow-ref "F-task-submit-dispatch"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false
        :note "action=sync 当前并不等待 PTY 结果, 只是建 task 后返回 hint; 命名需后续产品评审")

      (tool mission_task_query
        :desc "任务状态查询 status/list/ack/track"
        :actions ["status" "list" "ack" "track"]
        :required ["action"]
        :optional ["taskId" "status" "limit" "since"]
        (ingress
          :schema "action required; status/track require taskId; list accepts status/limit; ack accepts since")
        (logic-core
          (step s1 "action dispatch: status/list/ack/track")
          (step s2 "status reads one task; list reads by status or limit")
          (step s3 "ack calls ack_completed_tasks(since)")
          (step s4 "track aggregates task row + PTY slot status + slot_session + conversation jsonl metadata + progress + last response"))
        (egress
          :reads ["tasks" "slot_sessions" "conversations" "slot_progress" "slot_last_responses"]
          :writes ["ack marker/state via ack_completed_tasks when action=ack"]
          :returns "task json / task list / ack list / pretty tracking object")
        :dispatches-to-worker "handlers/compute/task.rs → legacy tasks read/control + PTY/progress aggregation for track"
        :memory-cross-ref ["system-support (tasks)" "slot-support (slot_sessions)" "conversation-logs (conversations)"]
        :flow-ref "F-task-legacy-queue-control :: status/list/ack/track"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_task_cancel
        :desc "取消任务"
        :required ["taskId"]
        (ingress
          :schema "taskId required"
          :callers ["intent-layer" "external MCP client"])
        (logic-core
          (step s1 "load task by taskId")
          (step s2 "guard: only Queued or Running can cancel")
          (step s3 "update status=Cancelled and finished_at=now")
          (step s4 "return {cancelled: bool}"))
        (egress
          :writes ["tasks.status=Cancelled" "tasks.finished_at"]
          :returns "{cancelled}")
        :dispatches-to-worker "handlers/compute/task.rs → guarded legacy tasks status update"
        :memory-cross-ref ["system-support (tasks)"]
        :flow-ref "F-task-legacy-queue-control :: cancel"
        :called-by ["intent-layer"]
        :necessity-pending-review false))

    ;; ── Task Delegate (1 tool) ──
    (module task_delegate
      :mcp-file "crates/missiond-mcp/src/tools/compute/task_delegate.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"

      (tool mission_task_delegate
        :desc "声明式任务委派 — 描述目标, daemon 自主选 slot/执行/回报"
        :required ["objective"]
        :optional ["intent" "cwd" "projectId" "timeout_secs" "priority" "depends_on" "context_hints"]
        (ingress
          :schema "objective required; intent defaults/general maps to slot template; cwd/projectId resolve target_project_root; timeout clamped by handler"
          :callers ["intent-layer (主)"])
        (logic-core
          (step s1 "validate objective/intent/cwd/timeout/priority")
          (step s2 "resolve target_project_root via ProjectRegistry; requested subdir is context only, not spawn cwd")
          (step s3 "select idle slot using SlotAcquireGuard, excluding protected roles, only when slot.project_root == target_project_root")
          (step s4 "if no idle slot and intent != ops, auto-provision via compute_slot with spawn cwd=target_project_root")
          (step s5 "build context hints from KB/skills")
          (step s6 "create board_task auto_execute=true with project binding")
          (step s7 "notify board_dispatch_notify for immediate dispatch"))
        (egress
          :writes ["board_tasks" "dynamic_slots/slot_sessions indirectly when auto-provisioned"]
          :returns "created board task / dispatch receipt"
          :downstream "F1-board-task-main-lifecycle")
        :dispatches-to-worker "handlers/compute/task_delegate.rs: validate → idle slot guard → optional compute_slot auto-provision → board_dispatch_notify"
        :memory-cross-ref ["board (创建 auto_execute board_task)" "slot-support (动态 slot 间接)" "kb-manager/project-management (context_hints 读 KB/Skill)"]
        :flow-ref "F-task-delegate-autoprovision / F-workstation-dispatch-policy :: s2 resident-lisp resume substrate"
        :called-by ["intent-layer (主)" "unified entry pipeline plan-runner (resident-lisp slot 复用入口)"]
        :workstation-dispatch-role "preferred substrate for resident-lisp-architect-session 复用 — 把任务挂到既有 slot, 复用已加载的 .missiond/v2/*.lisp 上下文; 不为单次 Lisp 改动重开 fresh slot"
        :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: policy resident-lisp-architect-session"
        :necessity-pending-review false
        :note "不是 workflow_executor_runtime 的简单别名; 它是 board_task lifecycle 的 declarative entry"))

    ;; ── Process (3 tools: agent/slots/inbox, 注意 inbox 在此 handler) ──
    (module process
      :mcp-file "crates/missiond-mcp/src/tools/compute/process.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/process.rs"
      :capability-family "process-runtime-surface"

      (capability-family process-runtime-surface
        (ingress
          :tools ["mission_agent" "mission_slots" "mission_inbox"]
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "mission_agent controls or lists configured PTY-backed agent processes")
          (step s2 "mission_slots exposes static slot configuration inventory")
          (step s3 "mission_inbox reads pending inbox messages from system memory"))
        (egress
          :flows ["F-workflow-slot-full-lifecycle for agent spawn/restart/kill" "trivial-single-step inventory/read tools"]
          :memory-cross-ref ["slot-support" "system-support inbox_messages"]))

      (tool mission_agent
        :desc "Agent 进程管理 spawn/kill/restart/list"
        :note "注意: handler 'process' 但 mcp-dispatch 老图标 handler 为 compute::cc_tasks — mcp 壳在 process.rs, handler 可能是 cc_tasks. 两者分离"
        :actions ["spawn" "kill" "restart" "list"]
        :required ["action"]
        :optional ["slotId" "visible" "autoRestart" "cwd" "projectId"]
        (ingress
          :schema "action required; spawn/kill/restart require slotId; spawn/restart accept cwd/projectId to resolve target_project_root; autoRestart optional"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "route consolidated action to mission_spawn/mission_kill/mission_restart/mission_agents")
          (step s2 "spawn: load slot config, resolve target_project_root, build PTYSlot, resolve mcp_config")
          (step s3 "spawn: call spawn_tracked_slot with process cwd=target_project_root and wait_for_idle=30s")
          (step s4 "kill: state.pty.kill(slotId)")
          (step s5 "restart: kill existing PTY then spawn_tracked_slot with same slot config")
          (step s6 "list: iterate mission slots and merge PTY status or no_session placeholder"))
        (egress
          :writes ["runtime PTY session for spawn/restart/kill" "slot_sessions via spawn_tracked_slot"]
          :returns "agent session info / success / agent status list")
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path claude-slot-dispatch (cc_tasks)"
        :memory-cross-ref ["slot-support"]
        :flow-ref "F-workflow-slot-full-lifecycle :: s2/s3/s7; F-daemon-bootstrap 类 process control"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_slots
        :desc "列出所有工位配置"
        :required []
        (ingress
          :schema "no input"
          :callers ["intent-layer" "board-frontend/api/slots"])
        (logic-core
          (step s1 "read state.mission.list_slots()")
          (step s2 "return configured slot inventory"))
        (egress
          :reads ["SlotManager configured slots"]
          :returns "slot config list")
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path slot-manager-runtime-authority"
        :memory-cross-ref ["slot-support"]
        :flow-ref "trivial-single-step (slot inventory read)"
        :called-by ["intent-layer" "board-frontend/api/slots"]
        :necessity-pending-review false)

      (tool mission_inbox
        :desc "获取收件箱消息 (跨 domain, 此 handler 可能归 sysinfra::misc)"
        :optional ["unreadOnly" "limit"]
        :note "mcp-dispatch 标 handler 为 sysinfra::misc — 本 tool 实际属 sysinfra 但 mcp 壳放 process.rs"
        (ingress
          :schema "unreadOnly defaults true; limit defaults 10"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "parse unreadOnly/limit with lenient bool")
          (step s2 "read inbox_messages filtered by unread flag and limit")
          (step s3 "return messages"))
        (egress
          :reads ["system-support inbox_messages"]
          :returns "inbox message list")
        :dispatches-to-worker "N/A — 纯 memory 读"
        :memory-cross-ref ["system-support (inbox_messages)"]
        :flow-ref "trivial-single-step (inbox memory read)"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false))

    ;; ── CC Tasks (2 tools) ──
    (module cc_tasks
      :mcp-file "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/cc_tasks.rs"
      :capability-family "cc-task-observation-and-swarm"

      (capability-family cc-task-observation-and-swarm
        (ingress
          :tools ["mission_cc_query" "mission_cc_swarm"]
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "mission_cc_query reads Claude Code sessions/tasks/overview/in_progress from derived watcher state")
          (step s2 "mission_cc_swarm builds teammate prompt and sends it through one PTY slot"))
        (egress
          :flows ["F-cc-swarm-pty-prompt" "trivial-single-step cc task read model"]
          :memory-cross-ref ["conversation-logs" "slot-support"]))

      (tool mission_cc_query
        :desc "Claude Code 任务监控 sessions/tasks/overview/in_progress"
        :actions ["sessions" "tasks" "overview" "in_progress"]
        :required ["action"]
        :optional ["sessionId" "projectPath" "activeOnly"]
        (ingress
          :schema "action required; sessions accepts projectPath/activeOnly; tasks accepts sessionId or projectPath"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "route action to sessions/tasks/overview/in_progress")
          (step s2 "sessions: read active/all sessions and optional project filter, summarize task counts")
          (step s3 "tasks: return tasks by sessionId or sessions by projectPath")
          (step s4 "overview: return watcher overview")
          (step s5 "in_progress: return active task forms across sessions"))
        (egress
          :reads ["cc_tasks derived watcher state"]
          :returns "sessions/tasks/overview/in_progress json")
        :dispatches-to-worker "N/A — 纯 memory 读 (handlers/knowledge/cascade.rs)"
        :note "mcp-dispatch 标 handler 为 knowledge::cascade — 跨 mcp/handler 归组"
        :memory-cross-ref ["conversation-logs" "board"]
        :flow-ref "trivial-single-step (Claude Code derived read model)"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_cc_swarm
        :desc "通过 PTY 触发 Claude Code Swarm 模式并行执行"
        :required ["slotId" "tasks"]
        :optional ["teammateCount" "timeoutMs"]
        (ingress
          :schema "slotId and tasks required; teammateCount defaults 3; timeoutMs defaults 600000"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "parse slotId/tasks/teammateCount/timeoutMs")
          (step s2 "build Plan-mode prompt listing numbered tasks and teammate count")
          (step s3 "send prompt to one PTY slot with state.pty.send")
          (step s4 "return PTY response text"))
        (egress
          :writes ["PTY conversation stream indirectly"]
          :returns "swarm response text")
        :dispatches-to-worker "handlers/compute/cc_tasks.rs → mission_cc_trigger_swarm → state.pty.send(slot_id, teammate prompt, timeout_ms)"
        :memory-cross-ref ["conversation-logs" "slot-support"]
        :flow-ref "F-cc-swarm-pty-prompt"
        :note "不是 flow-engine-v2 ParallelSlotTasks; daemon 不拥有 teammate fan-out, 只向单 slot 发 prompt"
        :called-by ["intent-layer"]
        :necessity-pending-review false))

    ;; ── Minimax / Sonnet (2 tools, minimax deprecated) ──
    (module minimax-and-sonnet
      :mcp-file "crates/missiond-mcp/src/tools/compute/minimax.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/{minimax,process}.rs"
      :capability-family "llm-text-processing"

      (capability-family llm-text-processing
        (ingress
          :tools ["mission_sonnet_process" "mission_minimax_process"]
          :callers ["intent-layer" "internal worker" "legacy callers"])
        (logic-core
          (step s1 "both tool names route to the Sonnet gateway handler")
          (step s2 "task selects summarize/translate/custom prompt template")
          (step s3 "Sonnet gateway call_interactive handles rate limiting and response"))
        (egress
          :flows ["trivial-single-step LLM text transform"]
          :worker-cross-ref ["llm-gateways :: sonnet-priority-gateway"]))

      (tool mission_sonnet_process
        :desc "调 Claude Sonnet 处理文本 (HTTP 调用无 PTY 开销)"
        :actions ["summarize" "translate" "custom"]
        :required ["text" "task"]
        :optional ["prompt" "targetLang" "maxChars"]
        (ingress
          :schema "text/task required; summarize uses maxChars; translate uses targetLang; custom requires prompt"
          :callers ["intent-layer" "internal worker"])
        (logic-core
          (step s1 "guard Sonnet gateway availability and non-empty text")
          (step s2 "summarize builds capped summary prompt")
          (step s3 "translate builds target language prompt")
          (step s4 "custom concatenates prompt and text")
          (step s5 "call sonnet.call_interactive and return content or error"))
        (egress
          :external-calls ["Sonnet gateway"]
          :returns "processed text")
        :dispatches-to-worker "section llm-gateways :: path sonnet-priority-gateway"
        :memory-cross-ref []
        :flow-ref "trivial-single-step (Sonnet text transform)"
        :called-by ["intent-layer" "internal worker"]
        :necessity-pending-review false)

      (tool mission_minimax_process
        :desc "[DEPRECATED] 调 MiniMax 处理文本, 已迁移到 Sonnet"
        :status "deprecated-migrated-to-sonnet"
        :actions ["summarize" "translate" "custom"]
        (ingress
          :schema "legacy alias; same input as mission_sonnet_process"
          :callers ["legacy callers"])
        (logic-core
          (step s1 "route legacy name to same Sonnet handler")
          (step s2 "execute summarize/translate/custom as Sonnet process"))
        (egress
          :returns "processed text"
          :note "deprecated alias")
        :dispatches-to-worker "section llm-gateways :: path minimax-legacy-gateway"
        :flow-ref "trivial-single-step (deprecated alias to Sonnet)"
        :called-by ["legacy callers"]
        :necessity-pending-review false
        :removal-policy "keep as legacy alias until next breaking MCP schema cleanup; do not count as strategic surface"))

    ;; ── Worker / Control (2 tools) ──
    (module worker
      :mcp-file "crates/missiond-mcp/src/tools/compute/worker.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/worker.rs"
      :capability-family "runtime-control-governance"

      (capability-family runtime-control-governance
        (ingress
          :tools ["mission_worker" "mission_control" "mission_pause"]
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "mission_worker lists worker registry and legacy LLM gates, or controls one worker/provider")
          (step s2 "mission_control mutates ControlTree by global/provider/domain/worker/slot_role/project")
          (step s3 "mission_pause preserves legacy global pause flag semantics"))
        (egress
          :flows ["F-runtime-control-governance"]
          :file-writes ["$MISSIOND_HOME/global_paused via mission_pause"]
          :worker-cross-ref ["ControlTree" "WorkerRegistry" "LLM gates" "PTY kill enforcement"]))

      (tool mission_worker
        :desc "后台 Worker + LLM 闸口管理 list/control"
        :actions ["list" "control"]
        :required ["action"]
        :optional ["target" "control_action"]
        (ingress
          :schema "action required; control requires target and control_action"
          :callers ["intent-layer" "board-frontend" "external MCP client (debug)"])
        (logic-core
          (step s1 "route action=list to list_workers or action=control to worker_control")
          (step s2 "list returns worker_registry.list_all and llm_gate::all_status")
          (step s3 "control remaps control_action into inner action")
          (step s4 "if target is LLM provider, pause/resume/status legacy gate; codex also toggles vision_worker")
          (step s5 "if target is worker, set WorkerState Paused/Running"))
        (egress
          :reads ["worker_registry" "llm_gate status"]
          :writes ["worker state" "LLM gate state" "Codex disabled flag"]
          :returns "worker/gate list, text receipt, or gate status")
        :dispatches-to-worker "section orchestration-governance :: path pause-resume-cascade"
        :memory-cross-ref []
        :flow-ref "F-runtime-control-governance :: s2/s4/s7"
        :called-by ["intent-layer" "board-frontend" "external MCP client (debug)"]
        :necessity-pending-review false)

      (tool mission_control
        :desc "统一调控闸口 (级联机制: 关 provider 自动暂停依赖 worker)"
        :required ["target_type" "action"]
        :optional ["target_name"]
        :target-types ["global" "provider" "domain" "worker" "slot_role" "project"]
        (ingress
          :schema "target_type/action required; target_name required except global/status"
          :callers ["intent-layer" "board-frontend (system control UI)"])
        (logic-core
          (step s1 "if action=status, return control_tree.status_summary")
          (step s2 "pause/resume maps to paused boolean")
          (step s3 "global: set global paused in ControlTree and legacy atomics")
          (step s4 "provider: set provider and sync legacy llm_gate")
          (step s5 "domain/worker/slot_role/project: set matching ControlTree branch")
          (step s6 "worker: also sync WorkerRegistry state")
          (step s7 "slot_role pause: kill running PTY sessions for matching role")
          (step s8 "return updated control tree summary"))
        (egress
          :writes ["ControlTree state" "legacy global pause atomics" "LLM gate state" "WorkerRegistry state" "runtime PTY sessions killed for slot_role pause"]
          :returns "updated control tree summary")
        :dispatches-to-worker "section orchestration-governance :: path pause-resume-cascade (含 set_project P2+P3 commit 50a5296)"
        :file-writes ["control_tree.json"]
        :flow-ref "F-runtime-control-governance"
        :called-by ["intent-layer" "board-frontend (system control UI)"]
        :necessity-pending-review false))

    ;; ── Slot (2 tools: slot_history + pause, 注意两者 handler 跨组) ──
    (module slot
      :mcp-file "crates/missiond-mcp/src/tools/compute/slot.rs"
      :note "两个 tool 的 handler 都不在 compute/slot 下"
      :capability-family "slot-observation-and-pause"

      (capability-family slot-observation-and-pause
        (ingress
          :tools ["mission_slot_history" "mission_pause"]
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "slot_history reads slot task history or aggregate stats")
          (step s2 "pause toggles legacy global dispatch pause flag"))
        (egress
          :flows ["F-runtime-control-governance for pause" "trivial-single-step slot task history read"]
          :memory-cross-ref ["slot_tasks"]))

      (tool mission_slot_history
        :desc "查询工位任务历史 (realtime_extract/deep_analysis/kb_gc 等)"
        :optional ["slotId" "taskType" "status" "limit" "stats"]
        :mcp-shell-file "crates/missiond-mcp/src/tools/compute/slot.rs"
        :handler-file "crates/missiond-daemon/src/handlers/comm/timeline.rs"
        (ingress
          :schema "slotId/taskType/status/limit filters; stats=true returns aggregate stats"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "parse filters with camelCase args")
          (step s2 "if stats=true, read slot_task_stats(slotId)")
          (step s3 "otherwise list_slot_tasks(slotId, taskType, status, limit)")
          (step s4 "return pretty JSON"))
        (egress
          :reads ["slot_tasks"]
          :returns "slot task stats or task list")
        :dispatches-to-worker "N/A — 纯 memory 读"
        :memory-cross-ref ["slot-support (slot_tasks)"]
        :flow-ref "trivial-single-step (slot task history read)"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_pause
        :desc "全局暂停/恢复所有工位的工作分派"
        :actions ["pause" "resume" "status"]
        :optional ["action"]
        :mcp-shell-file "crates/missiond-mcp/src/tools/compute/slot.rs"
        :handler-file "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
        (ingress
          :schema "action optional; defaults status"
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "status reads ControlTree global_paused and global_paused_at")
          (step s2 "pause sets global_paused atomics and writes $MISSIOND_HOME/global_paused")
          (step s3 "resume clears global_paused atomics and removes pause flag file")
          (step s4 "return status/receipt text"))
        (egress
          :writes ["global_paused atomics" "$MISSIOND_HOME/global_paused flag file"]
          :returns "pause/resume/status text")
        :dispatches-to-worker "section orchestration-governance :: path pause-resume-cascade (global kill-switch)"
        :file-writes ["control_tree.json"]
        :flow-ref "F-runtime-control-governance :: s1/s2/s6/s7"
        :called-by ["intent-layer" "board-frontend" "external MCP client"]
        :necessity-pending-review false))

    ;; ── Compute Slot (1 tool) ──
    (module compute_slot
      :mcp-file "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
      :capability-family "dynamic-slot-lifecycle"

      (capability-family dynamic-slot-lifecycle
        (ingress
          :tools ["mission_compute_slot" "mission_job_poll"]
          :callers ["intent-layer" "autopilot/task_delegate" "board-frontend"])
        (logic-core
          (step s1 "create validates template/cwd/TTL/limit, writes dynamic slot row, registers runtime slot, and starts async PTY spawn job")
          (step s2 "terminate kills PTY, marks dynamic slot terminated, unregisters runtime slot")
          (step s3 "extend adjusts active dynamic slot expiry within extension limits")
          (step s4 "list returns dynamic and static slot inventory")
          (step s5 "job_poll observes create job completion/failure"))
        (egress
          :flows ["F-dynamic-slot-lifecycle" "F-workflow-slot-full-lifecycle :: s2/s7"]
          :memory-cross-ref ["dynamic_slots" "slot_sessions" "job_store"]
          :events ["SlotSessionChanged via spawn_tracked_slot"]))

      (tool mission_compute_slot
        :desc "动态计算工位管理 create/terminate/extend/list (TTL 生命周期, 上限 5 活跃 · 8h)"
        :actions ["create" "terminate" "extend" "list"]
        :required ["action"]
        :optional ["template" "objective" "cwd" "projectId" "max_ttl" "slot_id" "additional_seconds" "status"]
        (ingress
          :schema "action required; create requires template; create accepts cwd/projectId and must resolve target_project_root; terminate/extend require slot_id; list accepts status"
          :callers ["intent-layer (autopilot 派发)" "board-frontend" "task_delegate auto-provision"])
        (logic-core
          (step s1 "dispatch action create/terminate/extend/list")
          (step s2 "create: validate template, active limit 5, resolve cwd/projectId to target_project_root, TTL min/max, objective")
          (step s3 "create: build SlotConfig(project_root, requested_cwd) and DynamicSlot row, persist to DB, register in SlotManager")
          (step s4 "create: create AsyncJob and spawn background PTY task via spawn_tracked_slot with process cwd=target_project_root")
          (step s5 "create background: complete job or mark dynamic slot spawn_failed and unregister")
          (step s6 "terminate: require slot-dyn-* id, kill PTY, terminate DB row, unregister slot")
          (step s7 "extend: validate additional_seconds <= 3600 and update expiry")
          (step s8 "list: merge dynamic slot rows and static SlotManager slots"))
        (egress
          :writes ["dynamic_slots" "SlotManager runtime registry" "job_store" "slot_sessions via spawn_tracked_slot"]
          :returns "job accepted / terminate receipt / extend receipt / slot inventory")
        :dispatches-to-worker "section pty :: subsection slot-orchestrator :: path {claude,gemini}-slot-dispatch (经 sole-spawn-bottleneck)"
        :memory-cross-ref ["slot-support (dynamic_slots, slot_sessions)"]
        :flow-ref "F-dynamic-slot-lifecycle + F-workflow-slot-full-lifecycle :: s2/s7; F-task-delegate-autoprovision :: s3 / F-workstation-dispatch-policy :: s2 fresh-code-alignment substrate (dynamic slot variant)"
        :called-by ["intent-layer (autopilot 派发)" "board-frontend" "unified entry pipeline plan-runner (fresh-code-alignment via dynamic slot)"]
        :workstation-dispatch-role "preferred substrate for fresh-code-alignment-session via dynamic slot; create 时显式 cwd/projectId, project-root cwd 强制由 spawn_tracked_slot 校验"
        :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: policy fresh-code-alignment-session + project-root-cwd-contract"
        :necessity-pending-review false))

    ;; ── Job (1 tool) ──
    (module job
      :mcp-file "crates/missiond-mcp/src/tools/compute/job.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/job.rs"
      :capability-family "async-job-control"

      (capability-family async-job-control
        (ingress
          :tools ["mission_job_poll"]
          :callers ["intent-layer" "external MCP client"])
        (logic-core
          (step s1 "poll reads one job by id")
          (step s2 "list returns running/completed counts and all jobs")
          (step s3 "cancel marks running job failed with user cancellation"))
        (egress
          :flows ["F-dynamic-slot-lifecycle observe create job" "trivial-single-step job_store control"]
          :memory-cross-ref ["in-memory job_store"]))

      (tool mission_job_poll
        :desc "轮询异步 Job 状态 poll/list/cancel"
        :actions ["poll" "list" "cancel"]
        :required ["job_id"]
        :optional ["action"]
        (ingress
          :schema "job_id required for poll/cancel; action defaults poll; list ignores job_id in handler"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "route action poll/list/cancel")
          (step s2 "poll: read job_store by job_id or return not found")
          (step s3 "list: count running/completed and return all jobs")
          (step s4 "cancel: if job running, mark failed with 'Cancelled by user'"))
        (egress
          :reads ["job_store"]
          :writes ["job_store status on cancel"]
          :returns "job status/list/cancel receipt")
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["system-support"]
        :flow-ref "trivial-single-step job_store control; observes F-dynamic-slot-lifecycle create jobs"
        :called-by ["intent-layer"]
        :necessity-pending-review false))

    ;; ── Flow Run (1 tool — 唯一已有 flow orchestration 的 tool) ──
    (module flow_run
      :mcp-file "crates/missiond-mcp/src/tools/compute/flow_run.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
      :capability-family "flow-engine-v2-entry"

      (capability-family flow-engine-v2-entry
        (ingress
          :tools ["mission_flow_run"]
          :callers ["intent-layer" "external MCP client" "board-frontend"])
        (logic-core
          (step s1 "list shows available YAML flows")
          (step s2 "status reads board task flow_context")
          (step s3 "run creates board task, initializes FlowContext from params, persists running state, runs flow inline"))
        (egress
          :flows ["F5-flow-engine-v2-node-execution"]
          :memory-cross-ref ["board_tasks.flow_context" "board_tasks.flow_phase" "board_tasks.status"]))

      (tool mission_flow_run
        :desc "Flow Engine v2 declarative YAML → node-sequence 执行器"
        :actions ["run" "list" "status"]
        :required ["flow_id"]
        :optional ["params" "action" "task_id"]
        (ingress
          :schema "action defaults run; run requires flow_id; status requires task_id; list requires no effective flow_id in handler"
          :callers ["intent-layer" "external MCP client" "board-frontend"])
        (logic-core
          (step s1 "list: loader::list_flows returns available definitions")
          (step s2 "status: load board task and parse flow_context")
          (step s3 "run: load flow definition by flow_id")
          (step s4 "run: create tracking board task with flow_template")
          (step s5 "run: initialize FlowContext from params and persist running status/phase/context")
          (step s6 "run: execute runner::run_flow inline")
          (step s7 "on success set phase=completed/status=done; on error set phase=failed/status=failed"))
        (egress
          :reads ["$MISSIOND_HOME/flows/*.yaml" "board_tasks for status"]
          :writes ["board_tasks" "flow_context" "flow_phase" "status"]
          :returns "flow list/status/run result")
        :dispatches-to-worker "section engine-cluster :: subsection flow-engine-v2 (3 path: load/dispatch/persist)"
        :memory-cross-ref ["board (board_tasks.flow_context / flow_phase / status)"]
        :flow-ref "F5-flow-engine-v2-node-execution"
        :called-by ["intent-layer" "external MCP client" "board-frontend"]
        :necessity-pending-review false
        :note "此 tool 是 tools → flow → worker 完整 5 跳链路的唯一模板, 其他 77 tools 将来借鉴其模式"))

    ;; ── Forge (2 tools) ──
    (module forge
      :mcp-file "crates/missiond-mcp/src/tools/compute/forge.rs"
      :handler-file "crates/missiond-daemon/src/handlers/compute/forge.rs"
      :added "commit 34167db"
      :capability-family "forge-bridge"

      (capability-family forge-bridge
        (ingress
          :tools ["mission_forge_build" "mission_forge_lint"]
          :callers ["intent-layer" "lisp_survey_worker"])
        (logic-core
          (step s1 "resolve registered project id to project root")
          (step s2 "shell out to forge build or forge lint")
          (step s3 "capture stdout/stderr/exit_code and return structured result"))
        (egress
          :flows ["F-forge-build" "F-forge-lint"]
          :intent-layer-cross-ref ["forge 本体 lisp→IR→rust" "governance lint"]))

      (tool mission_forge_build
        :desc "shell out 'forge build <root>' — lisp → IR → rust 冲压"
        :required ["project"]
        :optional ["dry_run" "output_dir"]
        (ingress
          :schema "project required; dry_run and output_dir optional"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "resolve project id through ProjectRegistry")
          (step s2 "build command: forge build <project_root> plus --dry-run/output-dir")
          (step s3 "execute process and capture stdout/stderr/exit code")
          (step s4 "return status ok/error and command metadata"))
        (egress
          :external-side-effects ["forge build may regenerate generated.rs files unless dry_run"]
          :returns "{status, exit_code, stdout, stderr, project_id, project_root, command}")
        :dispatches-to-worker "section worker-side-computation :: path forge-build-bridge"
        :cross-ref-intent-layer "forge 本体 (lisp→IR→rust 冲压器) 归 intent-layer pillar"
        :memory-cross-ref ["project-management"]
        :flow-ref "F-forge-build"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_forge_lint
        :desc "shell out 'forge lint <root>' — governance lint on intent.lisp"
        :required ["project"]
        (ingress
          :schema "project required"
          :callers ["intent-layer" "lisp_survey_worker (post-survey lint)"])
        (logic-core
          (step s1 "resolve project id through ProjectRegistry")
          (step s2 "build command: forge lint <project_root>")
          (step s3 "execute process and capture stdout/stderr/exit code")
          (step s4 "return status plus violations_raw from stdout"))
        (egress
          :reads ["project intent lisp files via external forge"]
          :returns "{status, exit_code, stdout, stderr, violations_raw, project_id, project_root, command}")
        :dispatches-to-worker "section worker-side-computation :: path forge-build-bridge"
        :cross-ref-intent-layer "governance lint 归 intent-layer pillar :: governance component"
        :memory-cross-ref ["project-management"]
        :flow-ref "F-forge-lint"
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
      :capability-family "kb-knowledge-lifecycle"

      (capability-family kb-knowledge-lifecycle
        (ingress
          :tools ["mission_kb_query" "mission_kb_remember" "mission_kb_mutate" "mission_kb_ops" "mission_kb_batch_set_project" "mission_embedding_ops" "mission_code_search"]
          :callers ["intent-layer" "board-frontend" "external MCP client" "worker context assembly"])
        (logic-core
          (step s1 "query/code_search provide retrieval surfaces for context assembly and direct inspection")
          (step s2 "remember/mutate/batch_set_project mutate KB entries, metadata, graph edges, AST links, and embedding refresh triggers")
          (step s3 "kb_ops governs KB quality through gc, compact, analyze, discover, queue_status, and execute_plan")
          (step s4 "embedding_ops exposes message embedding stats/backfill into the shared embedding worker pipeline")
          (step s5 "egress flows feed F10 context assembly, F7 embedding pipeline, and KB governance queues"))
        (egress
          :flows ["F10-context-assembly :: s3 retrieval-fusion" "F-kb-mutation-to-index" "F-kb-governance-ops" "F7-embedding-pipeline"]
          :memory-cross-ref ["kb-manager kb_entries/kb_embeddings/kb graph/ast links" "embedding-support" "project-management" "board context for analyze"]
          :events ["MemoryEvent::KBBatchMutated" "TaskEvent::Created when execute_plan dispatches merge/distill"]))

      (tool mission_kb_query
        :desc "知识库查询 search/get/list (FTS5 + Embedding 混合 RRF)"
        :actions ["search" "get" "list"]
        :optional ["action" "query" "category" "limit" "offset" "search_mode" "key"]
        (ingress
          :default-action "search"
          :schema "search uses query/category/limit/offset/search_mode; get requires key; list accepts category/limit/offset/compact"
          :callers ["intent-layer (context assembly)" "board-frontend/api/kb" "external MCP client"])
        (logic-core
          (step s1 "route action: get → mission_kb_get; list → mission_kb_list; otherwise search")
          (step s2 "search: if query empty and category empty, return KB list")
          (step s3 "search: retrieve FTS ranked IDs, fallback to LIKE for Chinese")
          (step s4 "search: embed query when service is available, score cache by cosine, apply similarity floor")
          (step s5 "search: RRF merge FTS/vector candidates, apply temporal decay and drop-off filter")
          (step s6 "search: exact mode returns relevance order; explore mode applies MMR diversity")
          (step s7 "trim or omit large detail, inject FTS snippets, update access stats")
          (step s8 "get/list return exact entry or paginated list/compact list"))
        (egress
          :reads ["kb_entries" "kb_embeddings/search cache"]
          :writes ["kb access stats when search returns results"]
          :returns "ranked KB results / exact entry / paginated list")
        :dispatches-to-worker "section worker-side-computation :: path retrieval-fusion (4 路并发检索)"
        :memory-cross-ref ["kb-manager (kb_entries, kb_embeddings)"]
        :flow-ref "F10-context-assembly :: s3 retrieval-fusion (search); trivial-single-step get/list"
        :called-by ["intent-layer (context assembly)" "board-frontend/api/kb" "external MCP client"]
        :necessity-pending-review false
        :note "最密集的搜索消费者 — 每次 LLM 调用都可能触发")

      (tool mission_kb_remember
        :desc "记录知识到长期记忆 (已存在则更新)"
        :required ["category" "key" "summary"]
        :optional ["detail" "source" "confidence"]
        :categories-enum "preference / memory / memory:architecture / memory:bugfix / memory:debug / memory:ops / memory:feature / memory:decision / memory:platform / project / architecture / architecture:summary / decision / policy:decision / feature / infra / procedure"
        (ingress
          :schema "category/key/summary required; detail/source/confidence optional"
          :callers ["intent-layer (主动记忆)" "external MCP client (agent 自主)"])
        (logic-core
          (step s1 "run content quality guard against verbose logs/stack traces")
          (step s2 "kb_remember upserts kb_entries")
          (step s3 "enqueue EmbeddingTask::ProcessKBEntry for async embedding refresh")
          (step s4 "if detail.consolidated_from exists, add supersedes graph edges")
          (step s5 "if detail.symbol exists, search AST and add AST-KB link")
          (step s6 "publish MemoryEvent::KBBatchMutated")
          (step s7 "for newly created entry, detect semantic conflicts, optional confidence downweight, add contradicts edges"))
        (egress
          :writes ["kb_entries" "kb graph edges" "kb_ast_links" "kb_embeddings asynchronously"]
          :emits ["MemoryEvent::KBBatchMutated"]
          :returns "remember result plus optional conflicts/conflictWarning")
        :dispatches-to-worker "N/A — memory 直写 (触发 embedding_worker via EmbeddingTask)"
        :memory-cross-ref ["kb-manager"]
        :event-emits ["KbEntryCreated / KbEntryUpdated"]
        :flow-ref "F-kb-mutation-to-index + F7-embedding-pipeline"
        :called-by ["intent-layer (主动记忆)" "external MCP client (agent 自主)"]
        :necessity-pending-review false)

      (tool mission_kb_mutate
        :desc "KB 写操作 forget/update/import"
        :actions ["forget" "update" "import"]
        :required ["action"]
        (ingress
          :schema "action required; forget uses key or keys; update uses key plus patch fields; import uses format/path"
          :callers ["intent-layer" "board-frontend/api/kb DELETE/PATCH"])
        (logic-core
          (step s1 "route action: update → mission_kb_update; import → mission_kb_import; forget+keys → batch forget; otherwise forget one key")
          (step s2 "forget: delete entry, remove embedding cache, delete graph edges and AST links")
          (step s3 "batch forget: parse keys array/JSON string/comma string and delete batch")
          (step s4 "update: quality guard changed summary/detail, update fields, enqueue embedding only when content_changed")
          (step s5 "import: load supported source such as servers_yaml and remember entries as infra KB")
          (step s6 "publish MemoryEvent::KBBatchMutated on delete/update"))
        (egress
          :writes ["kb_entries" "kb graph edges deleted" "kb_ast_links deleted" "embedding cache invalidated" "kb_embeddings asynchronously"]
          :emits ["MemoryEvent::KBBatchMutated"]
          :returns "deleted/update/import result")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "F-kb-mutation-to-index"
        :called-by ["intent-layer" "board-frontend/api/kb DELETE/PATCH"]
        :necessity-pending-review false)

      (tool mission_kb_ops
        :desc "KB 运维 gc/analyze/discover/queue_status/execute_plan/compact"
        :actions ["gc" "analyze" "discover" "queue_status" "execute_plan" "compact"]
        :required ["action"]
        (ingress
          :schema "action required; each action consumes its own option subset"
          :callers ["intent-layer (定期运维)" "manual MCP debug"])
        (logic-core
          (step s1 "route action: compact direct; analyze/discover/queue_status/execute_plan/gc legacy dispatch")
          (step s2 "gc: stats/stale/duplicates/clean_stale/clean_duplicates inspect or delete KB")
          (step s3 "compact: rule-based dryRun/delete for low confidence, stale state/ops/debug/bugfix, expired scratchpad")
          (step s4 "analyze: read paginated KB, redact sensitive fields, optionally include board context, call LLM gateway")
          (step s5 "analyze consolidation_plan: parse JSON and save operations into kb_ops queue when save_plan=true")
          (step s6 "queue_status: list operations by plan/status and optional plan summary")
          (step s7 "execute_plan: expire stale ops, mark running, apply delete/update, or dispatch merge/distill as legacy memory task")
          (step s8 "discover: resolve infra key/SSH credentials, probe remote host, remember infra KB entry"))
        (egress
          :reads ["kb_entries" "board_tasks when include_board_context" "kb_ops queue" "infra registry/credential KB"]
          :writes ["kb_entries deleted/updated" "kb_ops queue/status" "infra KB entry" "legacy task when merge/distill dispatched"]
          :emits ["TaskEvent::Created when execute_plan dispatches memory task"]
          :returns "gc/compact/analyze/discover/queue/execute result")
        :dispatches-to-worker "varies (analyze 走 sonnet gateway; discover 走 SSH; compact 走 DB)"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "F-kb-governance-ops"
        :called-by ["intent-layer (定期运维)" "manual MCP debug"]
        :necessity-pending-review false
        :split-decision "keep consolidated: 6 actions share KB governance/audit/ops-queue boundary; split only if product UI needs separate public surfaces")

      (tool mission_kb_batch_set_project
        :desc "批量设置 KB 条目项目归属"
        :required ["assignments"]
        :added "commit 3c10d21"
        (ingress
          :schema "assignments required; each assignment has key and optional project_id"
          :callers ["intent-layer" "board-frontend (/api/kb PATCH)"])
        (logic-core
          (step s1 "deserialize assignments list")
          (step s2 "for each key, update kb_entries.project_id to project_id or NULL")
          (step s3 "count updated and collect not_found")
          (step s4 "return batch summary"))
        (egress
          :writes ["kb_entries.project_id"]
          :returns "{updated, not_found, total}")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager" "project-management"]
        :flow-ref "F-kb-mutation-to-index :: s2 metadata update"
        :called-by ["intent-layer" "board-frontend (/api/kb PATCH)"]
        :necessity-pending-review false)

      (tool mission_embedding_ops
        :desc "Embedding 操作 stats/backfill"
        :actions ["stats" "backfill"]
        :required ["action"]
        (ingress
          :schema "action required; stats reads provider/current stats; backfill triggers message embedding phase from stored cursor"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "stats: read message_embedding_stats and current provider id")
          (step s2 "backfill: read resume cursor for message_embeddings phase")
          (step s3 "backfill: enqueue EmbeddingTask::RunBackfillPhase(MessageEmbeddings, cursor)")
          (step s4 "return trigger status and current stats"))
        (egress
          :reads ["message embedding stats" "backfill phase cursor"]
          :writes ["embedding task queue signal only; durable embedding writes happen in worker"]
          :returns "stats or backfill triggered")
        :dispatches-to-worker "section xjp-router-gateway :: path xjp-router-embedding (v0.3 新) + workers/sonnet/embedding_worker"
        :memory-cross-ref ["embedding-support" "kb-manager"]
        :flow-ref "F7-embedding-pipeline :: stats/backfill"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_code_search
        :desc "代码结构 L3 搜索 (AST 索引)"
        :required ["query"]
        :optional ["repo" "file_path" "node_type" "limit"]
        (ingress
          :schema "query required; repo/file_path/node_type/limit are filters"
          :callers ["intent-layer" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "search ast_nodes through AST FTS with expanded limit")
          (step s2 "post-filter by repo, file_path prefix, and node_type")
          (step s3 "render structured hits with signature/calls/docstring/stub")
          (step s4 "for top impl hits, expand related AST nodes by symbol")
          (step s5 "return results and optional related list"))
        (egress
          :reads ["ast_nodes" "ast_files" "ast_search_hits"]
          :returns "{query, count, results, related?}")
        :dispatches-to-worker "section worker-side-computation :: path retrieval-fusion (code_prefetch 主导)"
        :memory-cross-ref ["kb-manager (ast_nodes, ast_files, ast_search_hits)"]
        :flow-ref "F10-context-assembly :: s3 retrieval-fusion (AST/code)"
        :called-by ["intent-layer" "board-frontend" "external MCP client"]
        :necessity-pending-review false))

    ;; ── Memory Ops (1 tool) ──
    (module memory_ops
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/memory.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
      :capability-family "memory-extraction-control"

      (capability-family memory-extraction-control
        (ingress
          :tools ["mission_memory"]
          :callers ["intent-layer" "memory extraction slots"])
        (logic-core
          (step s1 "pending serves realtime extraction batches and marks the current extraction cycle as served")
          (step s2 "pause toggles memory domain pause through ControlTree")
          (step s3 "token_stats delegates to conversation token usage stats read model"))
        (egress
          :flows ["F-extraction-pipeline" "F-runtime-control-governance" "trivial token stats read"]
          :memory-cross-ref ["conversation_messages" "slot_tasks" "kb_stats" "token_usage_ledger"]))

      (tool mission_memory
        :desc "记忆与 Token 管理 pending/pause/token_stats"
        :actions ["pending" "pause" "token_stats"]
        :required ["action"]
        :optional ["paused" "sessionId" "slotId" "since" "groupBy"]
        (ingress
          :schema "action required; pause accepts paused; token_stats accepts sessionId/slotId/since/groupBy"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "route action pending/pause/token_stats")
          (step s2 "pending: debounce served batch while realtime extraction is Sending/WaitingForSlotIdle")
          (step s3 "pending: read pending realtime messages with limit 60, format extraction prompt, mark pending_served")
          (step s4 "pause: toggle or set memory domain pause in ControlTree; resume removes legacy memory_paused flag")
          (step s5 "token_stats: delegate to conversation handler mission_token_stats"))
        (egress
          :reads ["pending realtime conversation messages" "extraction_state" "token_usage_ledger via conversation handler"]
          :writes ["extraction_state.pending_served" "ControlTree memory domain pause" "legacy memory_paused flag removed on resume"]
          :returns "pending extraction text / pause receipt / token stats")
        :dispatches-to-worker "section engine-cluster :: intent-engine :: memory-scheduler-queue (pending); section orchestration-governance :: pause (pause)"
        :memory-cross-ref ["conversation-logs" "llm-support (token_usage_ledger)"]
        :flow-ref "F-extraction-pipeline :: pending read; F-runtime-control-governance :: memory pause; token_stats read model"
        :called-by ["intent-layer"]
        :necessity-pending-review false
        :split-decision "keep consolidated: 3 actions share memory-domain operator surface; flow-ref already separates extraction/control/token read paths"))

    ;; ── Board (8 tools) ──
    (module board
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/board.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/board.rs"
      :capability-family "board-task-lifecycle"

      (capability-family board-task-lifecycle
        (ingress
          :tools ["mission_board_query" "mission_board_create" "mission_board_update" "mission_board_delete" "mission_board_claim" "mission_board_retry" "mission_board_note_add" "mission_board_decompose" "mission_submit_phase_result"]
          :callers ["intent-layer" "board-frontend" "worker/autopilot" "external MCP client"]
          :note "mission_submit_phase_result 的 MCP 壳在 board.rs, handler 在 sysinfra/misc.rs, 逻辑属于 board flow phase")
        (logic-core
          (step s1 "query/list/get/search/summary 读 board state; clear_done 是 cleanup 写操作")
          (step s2 "create 写 board_task open, 可初始化 flow_template/flow_phase/flow_context")
          (step s3 "claim 通过 SQL CAS open→running, 绑定 executor/session")
          (step s4 "update/retry/delete/note_add 修改 board state 并发 BoardEvent 或 progress note")
          (step s5 "decompose 派 legacy task 到 slot 生成 child DAG")
          (step s6 "submit_phase_result 校验 engineering phase artifact, 推进 flow_phase 并可能创建 decision question"))
        (egress
          :flows ["F1-board-task-main-lifecycle" "F2-board-task-decompose" "F-board-submit-phase" "trivial-single-step board state ops"]
          :memory-cross-ref ["board (board_tasks, board_task_notes, flow_context)" "system-support (agent_questions via phase gate)" "legacy tasks queue (decompose submit task)"]
          :events ["BoardEvent::TaskCreated" "BoardEvent::Updated" "BoardEvent::StatusChanged" "BoardEvent::Deleted" "BoardEvent::Claimed" "BoardEvent::NoteAdded" "SlotEvent::TaskDispatched" "QuestionEvent::Created"]))

      (tool mission_board_query
        :desc "任务板统一查询 list/get/search/summary/clear_done"
        :actions ["list" "get" "search" "summary" "clear_done"]
        :optional ["action" "status" "includeHidden" "id" "ids" "includeChildren" "query" "project" "category" "parentId" "limit" "since"]
        (ingress
          :default-action "list"
          :schema "get requires id or ids; list/search/summary use filters; clear_done has no required fields"
          :callers ["intent-layer" "board-frontend/api/tasks" "external MCP client"])
        (logic-core
          (step s1 "determine action from mission_board_query.action or legacy tool name")
          (step s2 "list: list_board_tasks(status, includeHidden), then optional project filter")
          (step s3 "get: get_board_tasks_with_context(ids, includeChildren) or get_board_task_with_notes(id)")
          (step s4 "search: search_board_tasks(BoardSearchInput)")
          (step s5 "summary: board_summary(since)")
          (step s6 "clear_done: clear_done_board_tasks()"))
        (egress
          :reads ["board_tasks" "board_task_notes"]
          :writes ["board_tasks cleanup when action=clear_done"]
          :returns "json_pretty task/list/search/summary or text cleanup count")
        :dispatches-to-worker "N/A — memory 读"
        :memory-cross-ref ["board"]
        :flow-ref "trivial-single-step (board read; clear_done cleanup write)"
        :called-by ["intent-layer" "board-frontend/api/tasks" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_board_create
        :desc "创建任务 (支持 parentId / DAG dependsOn / Flow flowTemplate)"
        :required ["title"]
        :optional ["description" "priority" "category" "project" "server" "dueDate" "parentId" "assignee" "autoExecute" "promptTemplate" "hidden" "flowTemplate" "dependsOn"]
        (ingress
          :schema "title required; optional DAG parentId/dependsOn; optional flowTemplate initializes flow fields"
          :callers ["intent-layer" "board-frontend" "worker conversation hooks" "slot-generated decompose children"])
        (logic-core
          (step s1 "deserialize CreateBoardTaskInput")
          (step s2 "store.create_board_task(input) writes board_tasks status=open")
          (step s3 "if flowTemplate present, initialize flow_phase=investigate and flow_context default")
          (step s4 "publish BoardEvent::TaskCreated")
          (step s5 "return created task"))
        (egress
          :writes ["board_tasks" "board_tasks.flow_phase/flow_context when flowTemplate present"]
          :emits ["BoardEvent::TaskCreated"]
          :downstream "F1-board-task-main-lifecycle :: s2 scan-decide when autoExecute=true"
          :returns "created board task json")
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick (dispatch 时触发)"
        :memory-cross-ref ["board"]
        :event-emits ["BoardTaskCreated"]
        :flow-ref "F1-board-task-main-lifecycle :: s1"
        :called-by ["intent-layer" "board-frontend" "worker (conversation_logger 创建 memory-hook)"]
        :necessity-pending-review false)

      (tool mission_board_update
        :desc "更新任务 (单个 id 或批量 ids)"
        :optional ["id" "ids" "title" "description" "status" "priority" "category" "project" "server" "dueDate" "parentId" "assignee" "autoExecute" "promptTemplate" "hidden" "flowPhase" "flowTemplate" "dependsOn"]
        :flow-phases ["investigate" "consult_gemini_1" "plan" "consult_gemini_2" "execute" "finalize" "done"]
        (ingress
          :schema "single update requires id; batch requires ids; toggle legacy path maps to status transition"
          :callers ["intent-layer" "autopilot flow-progression" "board-frontend"])
        (logic-core
          (step s1 "detect batch mode via ids or legacy mission_board_batch_update")
          (step s2 "for status changes, read old_status before update")
          (step s3 "update_board_task per id or toggle_board_task legacy path")
          (step s4 "publish BoardEvent::StatusChanged if status changed, else BoardEvent::Updated")
          (step s5 "if marking done, spawn harvest_decisions_for_task")
          (step s6 "record session-task binding for single update"))
        (egress
          :writes ["board_tasks" "session_task_bindings implicit runtime map"]
          :emits ["BoardEvent::Updated" "BoardEvent::StatusChanged"]
          :side-effects ["decision_harvest when status=done"]
          :returns "updated task or batch result")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["board"]
        :event-emits ["BoardEvent::Updated" "BoardEvent::StatusChanged"]
        :flow-ref "F1-board-task-main-lifecycle :: s5/status update; otherwise trivial field update"
        :called-by ["intent-layer" "autopilot (flow-progression)" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_delete
        :desc "删除任务 (级联子任务)"
        :required ["id"]
        (ingress
          :schema "id required"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "load task title for event payload")
          (step s2 "delete_board_task(id), cascading children per store contract")
          (step s3 "if deleted > 0, publish BoardEvent::Deleted")
          (step s4 "return {deleted, id}"))
        (egress
          :writes ["board_tasks delete"]
          :emits ["BoardEvent::Deleted"]
          :returns "{deleted, id}")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["board"]
        :flow-ref "trivial-single-step (board delete + BoardEvent::Deleted)"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_claim
        :desc "原子认领任务 (仅 open 且未认领时成功)"
        :required ["taskId"]
        :optional ["executorId" "executorType"]
        (ingress
          :schema "taskId required; executorType default manual_session; executorId explicit or current_session_id fallback"
          :callers ["intent-layer" "autopilot" "external MCP client"])
        (logic-core
          (step s1 "resolve executor_id and executor_type")
          (step s2 "store.claim_board_task(taskId, executor_id, executor_type) SQL CAS")
          (step s3 "on success, record session-task binding")
          (step s4 "publish BoardEvent::Claimed")
          (step s5 "on failure, inspect existing task to return claimed/status/not-found reason"))
        (egress
          :writes ["board_tasks.status=running" "claim_executor_id/type" "lease fields"]
          :emits ["BoardEvent::Claimed"]
          :returns "claimed task or error reason")
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick (CAS claim)"
        :memory-cross-ref ["board"]
        :event-emits ["BoardEvent::Claimed"]
        :flow-ref "F1-board-task-main-lifecycle :: s3"
        :called-by ["intent-layer" "autopilot" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_board_retry
        :desc "重试失败/阻塞任务 (reset 状态, 可同步 reset 下游)"
        :required ["taskId"]
        :optional ["resetDownstream"]
        (ingress
          :schema "taskId required; resetDownstream default true"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "load task or fail")
          (step s2 "store.retry_board_task(task_id, resetDownstream) resets task and optionally downstream")
          (step s3 "add progress note with reset count")
          (step s4 "return reset summary"))
        (egress
          :writes ["board_tasks reset status/fields" "board_task_notes progress"]
          :returns "text reset summary"
          :downstream "F1-board-task-main-lifecycle can re-enter scan/claim when reopened")
        :dispatches-to-worker "section engine-cluster :: intent-engine :: autopilot-tick"
        :memory-cross-ref ["board"]
        :flow-ref "F1-board-task-main-lifecycle :: reset/downstream-cascade"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_note_add
        :desc "为任务添加进度笔记"
        :required ["taskId" "content"]
        :optional ["noteType" "author"]
        (ingress
          :schema "taskId and content required; noteType/author optional"
          :callers ["intent-layer" "worker" "board-frontend"])
        (logic-core
          (step s1 "add_board_task_note(input)")
          (step s2 "refresh session-task binding if task exists")
          (step s3 "publish BoardEvent::NoteAdded with 80-char preview")
          (step s4 "return note json"))
        (egress
          :writes ["board_task_notes"]
          :emits ["BoardEvent::NoteAdded"]
          :returns "note json")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["board (notes)"]
        :flow-ref "trivial-single-step (board note + BoardEvent::NoteAdded)"
        :called-by ["intent-layer" "worker" "board-frontend"]
        :necessity-pending-review false)

      (tool mission_board_decompose
        :desc "一键拆分任务 (派 slot 调查后自动建 DAG 子任务)"
        :required ["taskId"]
        :optional ["slotId" "hints"]
        (ingress
          :schema "taskId required; slotId defaults slot-coder-1; hints optional"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "load parent board task")
          (step s2 "guard parent status == open and has no existing subtasks")
          (step s3 "build decompose prompt with hints + state.skills.build_context(task.title)")
          (step s4 "submit legacy coder task and bind target slot")
          (step s5 "if slot Idle, send_fire_and_forget and emit SlotEvent::TaskDispatched")
          (step s6 "add parent progress note")
          (step s7 "slot later calls mission_board_create and mission_board_note_add"))
        (egress
          :writes ["legacy tasks row" "board_task_notes progress" "child board_tasks later by slot"]
          :emits ["SlotEvent::TaskDispatched if immediate dispatch" "BoardEvent::TaskCreated later per child"]
          :returns "text dispatch receipt")
        :dispatches-to-worker "handlers/knowledge/board.rs → validate parent/open/no subtasks → submit_task(coder) → optional pty.send_fire_and_forget"
        :event-emits ["SlotEvent::TaskDispatched (if immediate dispatch)"]
        :memory-cross-ref ["board" "skill context" "slot-support"]
        :flow-ref "F2-board-task-decompose"
        :called-by ["intent-layer"]
        :necessity-pending-review false))

    ;; ── Cascade (6 tools: cc_query/cc_swarm 其实按 handler 归 cascade) ──
    (module cascade
      :mcp-file "varies — cc_query/cc_swarm mcp 壳在 tools/compute/cc_tasks.rs; 其他在 knowledge/cascade.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
      :note "cascade handler 同时服务 compute 域的 cc_tasks 与 knowledge 域的 cascade_*, 按 mcp-dispatch 分类全列 knowledge"
      :capability-family "cascade-universe-repair"

      (capability-family cascade-universe-repair
        (ingress
          :tools ["mission_universe_graph" "mission_cascade_plan" "mission_cascade_trigger" "mission_cascade_lint"]
          :callers ["intent-layer" "external MCP client"])
        (logic-core
          (step s1 "all tools resolve and whitelist universe manifest path")
          (step s2 "graph/lint/plan/trigger all build forge_core universe graph")
          (step s3 "plan computes dry-run blast radius")
          (step s4 "trigger checks execution kill-switch, emits start event, runs blocking repair plan, emits completion event")
          (step s5 "lint validates universe integrity without executing repair"))
        (egress
          :flows ["F-cascade-execution"]
          :events ["TaskEvent::CascadeTriggered" "TaskEvent::CascadeCompleted"]
          :external-runtime ["forge_core::universe_graph" "forge_core::cascade"]))

      (tool mission_universe_graph
        :desc "universe manifest → service/dependency graph"
        :optional ["manifestPath" "format"]
        (ingress
          :schema "manifestPath optional; format defaults json and may be text"
          :callers ["intent-layer" "external MCP client"])
        (logic-core
          (step s1 "resolve manifestPath or UNIVERSE_MANIFEST default")
          (step s2 "canonicalize path and require it under UNIVERSE_ROOT or /Users/jinchen/Projects")
          (step s3 "call forge_core::universe_graph::resolve_universe_graph")
          (step s4 "render graph as text or structured JSON"))
        (egress
          :reads ["universe manifest"]
          :returns "service/dependency graph")
        :dispatches-to-worker "handlers/knowledge/cascade.rs → forge_core::universe_graph::resolve_universe_graph"
        :memory-cross-ref []
        :flow-ref "F-cascade-execution :: s1/s2"
        :called-by ["intent-layer" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_cascade_plan
        :desc "cascade 规划"
        :required ["service"]
        :optional ["manifestPath" "changed"]
        (ingress
          :schema "service required; changed defaults empty; manifestPath optional"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "resolve and whitelist manifest path")
          (step s2 "build universe graph and derive manifest dir")
          (step s3 "build ServiceDelta{service, changed}")
          (step s4 "create forge_core cascade plan with dry_run=true")
          (step s5 "return phases and upstream_map"))
        (egress
          :reads ["universe manifest" "service graph"]
          :returns "dry-run cascade plan")
        :dispatches-to-worker "handlers/knowledge/cascade.rs → forge_core::universe_graph + forge_core::cascade::create_plan(dry_run=true)"
        :memory-cross-ref []
        :flow-ref "F-cascade-execution :: s1-s3"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_cascade_trigger
        :desc "cascade 触发"
        :required ["service"]
        :optional ["manifestPath" "changed" "maxCycles"]
        (ingress
          :schema "service required; maxCycles defaults 3; CASCADE_TRIGGER_ENABLED can disable execution"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "check CASCADE_TRIGGER_ENABLED kill-switch")
          (step s2 "resolve and whitelist manifest path")
          (step s3 "build universe graph and ServiceDelta")
          (step s4 "publish TaskEvent::CascadeTriggered")
          (step s5 "spawn_blocking create_plan + execute_plan with dry_run=false and max_repair_cycles")
          (step s6 "publish TaskEvent::CascadeCompleted with repaired/failed/hard_halted/duration")
          (step s7 "return execution report phases"))
        (egress
          :emits ["TaskEvent::CascadeTriggered" "TaskEvent::CascadeCompleted"]
          :external-side-effects ["forge_core cascade repair commands"]
          :returns "cascade execution report")
        :dispatches-to-worker "handlers/knowledge/cascade.rs → spawn_blocking forge_core::cascade::execute_plan"
        :event-emits ["TaskEvent::CascadeTriggered" "TaskEvent::CascadeCompleted"]
        :memory-cross-ref []
        :flow-ref "F-cascade-execution :: s1-s7"
        :called-by ["intent-layer"]
        :necessity-pending-review false)

      (tool mission_cascade_lint
        :desc "cascade lint"
        :optional ["manifestPath"]
        (ingress
          :schema "manifestPath optional"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "resolve and whitelist manifest path")
          (step s2 "build universe graph and derive manifest dir")
          (step s3 "call forge_core::universe_graph::validate_universe_integrity")
          (step s4 "return clean/warnings/failed with violation counts"))
        (egress
          :reads ["universe manifest" "service graph"]
          :returns "integrity status and violations")
        :dispatches-to-worker "handlers/knowledge/cascade.rs → forge_core::universe_graph::validate_universe_integrity"
        :memory-cross-ref []
        :flow-ref "F-cascade-execution :: s1/s2/s8"
        :called-by ["intent-layer"]
        :necessity-pending-review false))

    ;; ── Skill (4 tools) ──
    (module skill
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/skill.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/skill.rs"
      :capability-family "skill-knowledge-and-execution"

      (capability-family skill-knowledge-and-execution
        (ingress
          :tools ["mission_skill_query" "mission_skill_context" "mission_skill_mutate" "mission_skill_exec"]
          :callers ["intent-layer" "external MCP client" "skill workflow internal steps"])
        (logic-core
          (step s1 "query exposes skill registry, search, action metadata, topics, and execution stats")
          (step s2 "context builds or resolves task-specific skill bundles with KB/infra/board dependencies")
          (step s3 "mutate updates skill topics/blocks/files/versions and triggers skill embedding refresh")
          (step s4 "exec runs skill-local workflow blocks through sequential MCP tool dispatch and skill_executions audit"))
        (egress
          :flows ["F-skill-knowledge-lifecycle" "F-skill-workflow-execution" "F10-context-assembly" "F7-embedding-pipeline"]
          :memory-cross-ref ["skill_topics" "skill_blocks" "skill_versions" "skill_executions" "kb-manager" "board when include_board"]
          :events ["embedding worker task signal"]))

      (tool mission_skill_query
        :desc "Skill 查询 list/search/topics/actions/stats"
        :actions ["list" "search" "topics" "actions" "stats"]
        :required ["action"]
        :optional ["query" "skill"]
        (ingress
          :schema "action required; search uses query; actions/stats may filter by skill"
          :callers ["intent-layer" "external MCP client (Claude Code skill discovery)"])
        (logic-core
          (step s1 "route action to list/search/topics/actions/stats inner handler")
          (step s2 "list returns in-memory skill metadata from state.skills")
          (step s3 "search combines name/aka bonus, skill FTS, skill embedding cosine, RRF scoring")
          (step s4 "search records top topic hits")
          (step s5 "topics reads skill_topic_list")
          (step s6 "actions parses actions_json and workflow step counts from skill files")
          (step s7 "stats reads skill_execution_stats"))
        (egress
          :reads ["state.skills index" "skill_topics" "skill_embeddings/cache" "skill_executions" "skill files for workflow step counts"]
          :writes ["skill topic hit counters"]
          :returns "skill list/search/topics/actions/stats json")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager (skill 相关表)"]
        :flow-ref "F-skill-knowledge-lifecycle :: s2/s3/s9"
        :called-by ["intent-layer" "external MCP client (Claude Code skill discovery)"]
        :necessity-pending-review false)

      (tool mission_skill_context
        :desc "Skill 上下文构建 build/resolve (含 requires 依赖)"
        :actions ["build" "resolve"]
        :required ["action" "query"]
        :optional ["skill" "include_board"]
        (ingress
          :schema "action/query required; resolve may use direct skill and include_board"
          :callers ["intent-layer (LLM 前置 context)" "external MCP client"])
        (logic-core
          (step s1 "route action: build → mission_context_build; resolve → mission_context_resolve")
          (step s2 "build: state.skills.build_context(query)")
          (step s3 "build: KB search adds confidence/access ranked budget block")
          (step s4 "resolve: choose primary skill by direct skill or skill search")
          (step s5 "resolve: expand requires_json dependencies to two layers")
          (step s6 "resolve: aggregate infra IDs and KB categories from dependencies")
          (step s7 "resolve: optionally search board tasks by query"))
        (egress
          :reads ["skill_topics" "requires_json" "infra registry" "kb_entries" "board_tasks when include_board"]
          :returns "text context for build; structured skills/infra/kb/board bundle for resolve")
        :dispatches-to-worker "section context-assembly :: path context-bundle-assembly"
        :memory-cross-ref ["kb-manager" "board (若 include_board)"]
        :flow-ref "F-skill-knowledge-lifecycle :: s4/s5 + F10-context-assembly"
        :called-by ["intent-layer (LLM 前置 context)" "external MCP client"]
        :necessity-pending-review false)

      (tool mission_skill_mutate
        :desc "Skill 写 upsert/record/render/rollback"
        :actions ["upsert" "record" "render" "rollback"]
        :required ["action"]
        :optional ["topic" "section_title" "content" "sort_order" "skill" "version_id"]
        (ingress
          :schema "action required; upsert requires topic/section_title/content; record requires topic/content; rollback requires skill and optional version_id"
          :callers ["intent-layer" "skill_exec (内部 record)"])
        (logic-core
          (step s1 "route action to upsert/record/render/rollback")
          (step s2 "upsert: auto-create topic if missing, update existing section or insert new skill block")
          (step s3 "record: auto-create topic if missing, insert fragment block")
          (step s4 "upsert/record: materialize topic to SKILL.md and enqueue ProcessSkillTopic embedding")
          (step s5 "render: materialize one topic or all skills")
          (step s6 "rollback: restore selected skill version to file, or list available versions")
          (step s7 "rollback restore: re-ingest skill directory"))
        (egress
          :writes ["skill_topics" "skill_blocks" "SKILL.md files" "skill_versions restored to file" "skill embedding task signal"]
          :returns "text/json mutation result")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["kb-manager"]
        :flow-ref "F-skill-knowledge-lifecycle :: s6/s7/s8 + F7-embedding-pipeline when content changes"
        :called-by ["intent-layer" "skill_exec (内部 record)"]
        :necessity-pending-review false)

      (tool mission_skill_exec
        :desc "执行 Skill workflow (顺序 MCP 工具步骤)"
        :required ["skill" "action"]
        :optional ["dry_run" "params"]
        (ingress
          :schema "skill/action required; dry_run previews steps; params inject runtime variables"
          :callers ["intent-layer"])
        (logic-core
          (step s1 "handler parses SkillExecArgs and calls AppState::execute_workflow(depth=0)")
          (step s2 "execute_workflow guards recursion depth and concurrent same action")
          (step s3 "load skill_topic, read skill file, parse workflow block matching action")
          (step s4 "if requires_approval and not dry_run, return PendingApproval; if dry_run, return step preview")
          (step s5 "insert skill_execution row and execute context_hooks with 10s timeout")
          (step s6 "merge params into context, then run workflow steps sequentially")
          (step s7 "each step resolves variables, calls MCP tool with 30s timeout, stores save_as output")
          (step s8 "on error apply skip/retry/fallback/stop, with MAX_STEP_VISITS and MAX_DEPTH guards")
          (step s9 "persist success/failed status, steps_completed, context_json, error, duration_ms"))
        (egress
          :writes ["skill_executions" "downstream writes from called MCP tools"]
          :returns "WorkflowResult success/failed/pending approval/preview")
        :dispatches-to-worker "section engine-cluster :: intent-engine :: workflow-executor-runtime"
        :memory-cross-ref ["project-management (skill_topics / skill_executions)" "kb-manager (skill context 间接)"]
        :flow-ref "F-skill-workflow-execution"
        :called-by ["intent-layer"]
        :necessity-pending-review false
        :note "与 flow-engine-v2 并存但不是同一路径: skill workflow 读 skill 文件 workflow block, 每 step 走 MCP dispatch + skill_executions 审计"))

    ;; ── Insight (1 tool) ──
    (module insight
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/insight.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/insight.rs"
      :capability-family "strategic-insight-read"

      (capability-family strategic-insight-read
        (ingress
          :tools ["mission_insight"]
          :callers ["intent-layer" "指挥官查看"])
        (logic-core
          (step s1 "read KB entry strategic-state")
          (step s2 "select section all/profile/trajectory/patterns/proposals/friction")
          (step s3 "render strategic-state detail as Markdown report"))
        (egress
          :flows ["trivial-single-step strategic-state KB read model"]
          :memory-cross-ref ["kb_entries key=strategic-state"]))

      (tool mission_insight
        :desc "查看 MissionD 战略认知 (开发轨迹/协作模式/反面模式/摩擦点)"
        :optional ["section"]
        :sections ["all" "profile" "trajectory" "patterns" "proposals" "friction"]
        (ingress
          :schema "section optional; defaults all"
          :callers ["intent-layer (自省)" "指挥官查看"])
        (logic-core
          (step s1 "read state.store.kb_get('strategic-state')")
          (step s2 "if missing or detail empty, return explanatory text")
          (step s3 "render selected sections: profile, trajectory, patterns, proposals, friction and anti-patterns")
          (step s4 "return Markdown report"))
        (egress
          :reads ["kb_entries strategic-state detail"]
          :returns "strategic insight Markdown")
        :dispatches-to-worker "N/A — 纯读"
        :memory-cross-ref ["kb-manager (insight 类 kb_entries)"]
        :flow-ref "trivial-single-step (strategic-state KB read model)"
        :called-by ["intent-layer (自省)" "指挥官查看"]
        :necessity-pending-review false))

    ;; ── Project (1 tool, 多 action) ──
    (module project
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/project.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/project.rs"
      :added "commit 76900d1 + 84ac1a6 + 8438a7d"
      :capability-family "project-registry-context"

      (capability-family project-registry-context
        (ingress
          :tools ["mission_project"]
          :callers ["intent-layer" "board-frontend/api/projects"])
        (logic-core
          (step s1 "registry actions list/get/set_active/sync/init/import_universe mutate or read projects")
          (step s2 "context/memories/vault_sync assemble project-level file, KB, conversation, GitHub, slot, and memory views")
          (step s3 "survey shells out forge survey and may update intent_path"))
        (egress
          :flows ["F9-project-init for init" "direct project-management registry ops" "forge survey bridge"]
          :memory-cross-ref ["projects" "conversation-logs" "kb-manager" "slot-support"]))

      (tool mission_project
        :desc "项目管理 list/get/set_active/sync/init/context/memories"
        :actions ["list" "get" "set_active" "sync" "init" "context" "memories" "vault_sync" "import_universe" "survey"]
        :required ["action"]
        :optional ["id" "path" "slots" "active" "file" "manifest" "level" "check" "dry_run"]
        :special-action-init "canonicalize path → derive id → git remote → scan intent.lisp → upsert → backfill → reload SharedProjectRegistry"
        (ingress
          :schema "action defaults list in handler; init requires path; get/context/memories/vault_sync/survey require id"
          :callers ["intent-layer" "board-frontend/api/projects"])
        (logic-core
          (step s1 "list: read projects and enrich with local lisp file scan")
          (step s2 "get/set_active: read or update one project row")
          (step s3 "sync: scan ~/.claude/projects, derive project ids, upsert missing projects with git remote")
          (step s4 "init: canonicalize path, derive id, read git remote, scan intent candidates, upsert project, backfill conversations, reload ProjectRegistry")
          (step s5 "context: assemble intent metadata, GitHub web URL, conversation stats/recent, project memories, KB stats, configured slot runtime state")
          (step s6 "memories: list memory files or read selected file")
          (step s7 "survey: shell out forge survey with level/check/dry_run and update intent_path on successful non-dry run")
          (step s8 "vault_sync: copy project lisp files into ~/.missiond/vault/<id>, write _meta.json, mark project reference")
          (step s9 "import_universe: parse universe manifest services/monorepos, upsert projects, reload ProjectRegistry"))
        (egress
          :reads ["projects" "git remote" "intent lisp files" "conversation stats/recent" "project memory files" "kb stats" "SlotManager slots"]
          :writes ["projects" "conversation.project_id backfill" "ProjectRegistry runtime cache" "~/.missiond/vault/<id>" "intent_path after survey"]
          :returns "project list/detail/context/memory/survey/import/vault result")
        :dispatches-to-worker "section orchestration-governance :: path daemon-bootstrap (ProjectRegistry reload)"
        :memory-cross-ref ["project-management" "conversation-logs (stats)" "kb-manager (stats)" "slot-support"]
        :flow-ref "F9-project-init (init); direct registry/context/memories/vault/import/survey ops"
        :called-by ["intent-layer" "board-frontend/api/projects"]
        :necessity-pending-review false))

    ;; ── Intent (1 tool) ──
    (module intent
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/intent.rs"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/intent.rs"
      :added "commit ec269d7"
      :capability-family "intent-file-read"

      (capability-family intent-file-read
        (ingress
          :tools ["mission_intent"]
          :callers ["intent-layer" "external MCP client" "主 Claude"])
        (logic-core
          (step s1 "resolve project from arg or CWD and ProjectRegistry")
          (step s2 "resolve intent file from DB intent_path or common candidates")
          (step s3 "read/list/section/summary/paths expose intent lisp content at different granularity"))
        (egress
          :flows ["trivial-single-step project intent file read/path scan"]
          :intent-layer-cross-ref ["per-project intent.lisp"]))

      (tool mission_intent
        :desc "读 per-project intent.lisp (read/section/summary/list)"
        :actions ["read" "section" "summary" "list" "paths"]
        :required ["action"]
        :candidates-paths [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
        (ingress
          :schema "action defaults summary; read/section/summary/paths accept project; section requires section"
          :callers ["intent-layer (agent 调查自身)" "external MCP client (Claude Code 导航)" "主 Claude (jarvis-manual 工作流)"])
        (logic-core
          (step s1 "list: read all projects and resolve intent path existence/line count")
          (step s2 "read/section/summary/paths: resolve project id exactly or fuzzy from CWD/query")
          (step s3 "resolve intent file from stored intent_path or candidate paths")
          (step s4 "read: return full file unless split/large; then return paths")
          (step s5 "section: extract named top-level S-expression by paren counting")
          (step s6 "summary: extract header, design constraints, pillar index, available sections")
          (step s7 "paths: scan .missiond/.jarvis/root intent*.lisp files for parallel loading"))
        (egress
          :reads ["projects" "project intent lisp files"]
          :returns "intent file text / section text / summary / file path list")
        :dispatches-to-worker "N/A — file 读"
        :cross-ref-intent-layer "读的是 intent-layer pillar 拥有的 lisp 文件, handler 从 file 读"
        :memory-cross-ref ["project-management"]
        :flow-ref "trivial-single-step (project intent file read/path scan)"
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
      :capability-family "conversation-observation-and-reconcile"

      (capability-family conversation-observation-and-reconcile
        (ingress
          :tools ["mission_conversation_query" "mission_conversation_analyze" "mission_conversation_reconcile"]
          :callers ["intent-layer" "board-frontend" "external MCP client" "ops/debug"])
        (logic-core
          (step s1 "query exposes conversation/message/context/event/user-index read models plus label mutations")
          (step s2 "analyze exposes retrospective/trajectory/activity analysis entry points")
          (step s3 "reconcile bridges JSONL source files into durable conversation tables"))
        (egress
          :flows ["F6-conversation-jsonl-ingest" "F8-retrospective-to-memory" "trivial conversation read/write labels"]
          :memory-cross-ref ["conversation-logs" "system-support (reconcile watermarks)" "kb-manager via downstream retrospective memory"]))

      (tool mission_conversation_query
        :desc "对话统一查询 list/get/search/message_search/context/events"
        :actions ["list" "get" "search" "message_search" "context" "events" "user_index" "set_label" "delete_label"]
        :optional ["action" "status" "conversationType" "taskId" "sessionId" "tail" "sinceId" "includeRaw" "query" "queryMode" "timeRange" "project" "excludeSessionId" "offset" "role" "toolName" "messageId" "before" "after" "eventType" "limit" "since" "until"]
        (ingress
          :default-action "list"
          :schema "action optional; each read action uses its own filters; set_label/delete_label mutate labels"
          :callers ["intent-layer (历史回看)" "board-frontend/api/conversations" "external MCP client (auto-memory)"])
        (logic-core
          (step s1 "route action list/get/search/message_search/context/events/user_index/set_label/delete_label")
          (step s2 "list/get read conversations by status/type/task/session/project/tail filters")
          (step s3 "search and message_search query conversation text with role/tool/time filters and optional hybrid retrieval")
          (step s4 "context returns neighboring messages around messageId/before/after")
          (step s5 "events reads conversation event stream by session/eventType/time")
          (step s6 "user_index returns indexed user messages for review")
          (step s7 "set_label/delete_label mutate conversation labels"))
        (egress
          :reads ["conversations" "conversation_messages" "conversation_events" "tool_calls"]
          :writes ["conversation labels for set_label/delete_label"]
          :returns "conversation/list/search/context/events/label result")
        :dispatches-to-worker "section worker-side-computation :: path retrieval-fusion (search hybrid mode)"
        :memory-cross-ref ["conversation-logs"]
        :flow-ref "trivial-single-step conversation read/write label; search may use retrieval-fusion"
        :called-by ["intent-layer (历史回看)" "board-frontend/api/conversations" "external MCP client (auto-memory)"]
        :necessity-pending-review false
        :note "用户在 CLAUDE.md 指定: 读历史会话 → mission_conversation_query(action=get); 复盘 → mission_retrospective")

      (tool mission_conversation_analyze
        :desc "对话分析 retrospective/trajectory/activity"
        :actions ["retrospective" "trajectory" "activity"]
        :required ["action"]
        :optional ["sessionId" "depth" "toolUseId" "since" "until" "limit"]
        (ingress
          :schema "action required; retrospective/trajectory/activity consume session/tool/time filters"
          :callers ["intent-layer" "指挥官"])
        (logic-core
          (step s1 "route action retrospective/trajectory/activity")
          (step s2 "retrospective reads session conversation window and returns or triggers retrospective analysis path")
          (step s3 "trajectory follows agent/tool trajectory around session/toolUseId")
          (step s4 "activity aggregates recent conversation activity by time window")
          (step s5 "return analysis report or structured observation"))
        (egress
          :reads ["conversation_messages" "tool_calls" "retrospectives/deep_analysis when available"]
          :writes ["retrospectives/deep_analysis only when retrospective path persists downstream"]
          :returns "retrospective/trajectory/activity analysis")
        :dispatches-to-worker "section engine-cluster :: subsection learning-engine (intent-layer 主 ownership) + retro_worker"
        :memory-cross-ref ["conversation-logs" "system-support (deep_analysis)"]
        :flow-ref "F8-retrospective-to-memory (retrospective action); trajectory/activity direct observation"
        :called-by ["intent-layer" "指挥官"]
        :necessity-pending-review false)

      (tool mission_conversation_reconcile
        :desc "JSONL-DB 对账 (不传 sessionId 全量扫)"
        :optional ["sessionId"]
        (ingress
          :schema "sessionId optional; absent means background full reconciliation"
          :callers ["intent-layer (运维)" "手动 debug"])
        (logic-core
          (step s1 "if sessionId present, locate and reconcile that session JSONL path")
          (step s2 "if sessionId absent, spawn full reconciliation background worker")
          (step s3 "update conversation rows/messages/tool calls and reconciliation watermarks")
          (step s4 "return reconcile receipt or background task acknowledgement"))
        (egress
          :reads ["Claude/Codex/Gemini JSONL conversation files"]
          :writes ["conversations" "conversation_messages" "tool_calls" "reconcile_watermarks"]
          :returns "single-session reconcile result or full reconcile started")
        :dispatches-to-worker "section worker-cluster :: worker-local :: cli-ingestion functional-group (reconcile_worker / gemini_reconcile_worker)"
        :memory-cross-ref ["conversation-logs" "system-support (reconcile_watermarks)"]
        :flow-ref "F6-conversation-jsonl-ingest (manual reconcile)"
        :called-by ["intent-layer (运维)" "手动 debug"]
        :necessity-pending-review false))

    ;; ── Question (1 tool) + LLM trace / Decision stats / Gemini auth / Incident ──
    ;; 这 5 个 tool 都在 tools/comm/question.rs 的 mcp 壳, handler 散在 comm + sysinfra
    (module question
      :mcp-file "crates/missiond-mcp/src/tools/comm/question.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs"
      :capability-family "question-decision-and-diagnostics"

      (capability-family question-decision-and-diagnostics
        (ingress
          :tools ["mission_question" "mission_llm_trace" "mission_decision_stats" "mission_gemini_auth" "mission_incident"]
          :callers ["intent-layer" "worker generated questions" "board-frontend" "ops/debug"])
        (logic-core
          (step s1 "question create/list/get/answer/dismiss manages human/agent blocking decisions")
          (step s2 "decision_stats exposes decision cascade aggregate state")
          (step s3 "llm_trace, gemini_auth, and incident are diagnostic/control shims delegated by question handler"))
        (egress
          :flows ["F3-agent-question-block-resume" "F9-decision-cascade stats read" "trivial LLM/auth/incident diagnostics"]
          :memory-cross-ref ["agent_questions" "board tasks" "llm-support" "system-support incidents"]))

      (tool mission_question
        :desc "Agent 待决策问题管理 create/list/get/answer/dismiss"
        :actions ["create" "list" "get" "answer" "dismiss"]
        :required ["action"]
        :optional ["id" "question" "context" "taskId" "slotId" "sessionId" "target" "options" "decisionType" "answer" "status" "limit"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs"
        (ingress
          :schema "action required; create needs question/context target fields; answer/dismiss require id"
          :callers ["intent-layer" "worker (生成提问)" "board-frontend (回答 UI)"])
        (logic-core
          (step s1 "route action create/list/get/answer/dismiss")
          (step s2 "create: infer task_id from sole running autopilot task when omitted")
          (step s3 "create: persist agent_question with target/options/decision_type/session/slot context")
          (step s4 "create: publish QuestionEvent::Created when target=master")
          (step s5 "list/get: read questions by status/id with limit")
          (step s6 "answer: persist answer, publish TaskEvent::Completed and QuestionEvent::Resolved")
          (step s7 "dismiss: mark resolved/dismissed and publish QuestionEvent::Resolved"))
        (egress
          :reads ["agent_questions" "board tasks for inference"]
          :writes ["agent_questions status/answer"]
          :emits ["QuestionEvent::Created" "QuestionEvent::Resolved" "TaskEvent::Completed"]
          :returns "question create/list/get/answer/dismiss result")
        :dispatches-to-worker "N/A — 可能触发 decision-engine (intent-layer primary)"
        :memory-cross-ref ["system-support (agent_questions)" "board (可能 blocked)"]
        :flow-ref "F3-agent-question-block-resume"
        :called-by ["intent-layer" "worker (生成提问)" "board-frontend (回答 UI)"]
        :necessity-pending-review false)

      (tool mission_llm_trace
        :desc "LLM 调用链路追踪 gemini_trace/stats/watch/auth/jarvis_logs/jarvis_trace"
        :actions ["gemini_trace" "gemini_stats" "gemini_watch" "gemini_auth" "jarvis_logs" "jarvis_trace"]
        :required ["action"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs (delegates to sysinfra/misc.rs legacy trace handlers)"
        (ingress
          :schema "action required; action maps to gemini/jarvis trace legacy handlers"
          :callers ["intent-layer (自省 LLM 行为)" "board-frontend/api/system/llm-traces"])
        (logic-core
          (step s1 "route action gemini_trace/gemini_stats/gemini_watch/gemini_auth/jarvis_logs/jarvis_trace")
          (step s2 "rewrite gemini_watch watch_action into delegated action field when needed")
          (step s3 "delegate to existing misc/gemini/jarvis trace read or auth operation")
          (step s4 "return trace/stat/watch/auth/log result"))
        (egress
          :reads ["llm-support gemini_requests" "jarvis logs/traces"]
          :writes ["Gemini auth mode only for auth action"]
          :returns "LLM trace/stat/watch/auth/log result")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["llm-support (gemini_requests)"]
        :flow-ref "trivial-single-step LLM diagnostic/control shim"
        :called-by ["intent-layer (自省 LLM 行为)" "board-frontend/api/system/llm-traces"]
        :necessity-pending-review false)

      (tool mission_decision_stats
        :desc "Decision Engine 统计"
        :optional ["hours"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs"
        (ingress
          :schema "hours optional"
          :callers ["intent-layer" "指挥官"])
        (logic-core
          (step s1 "parse hours time window")
          (step s2 "read state.store.decision_stats(hours)")
          (step s3 "return decision cascade aggregate stats"))
        (egress
          :reads ["decision stats read model"]
          :returns "decision stats json")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["system-support"]
        :flow-ref "F9-decision-cascade (stats read)"
        :called-by ["intent-layer" "指挥官"]
        :necessity-pending-review false
        :note "归属: decision-engine 逻辑本身属 intent-layer pillar, 此 tool 是必要观测查询面")

      (tool mission_gemini_auth
        :desc "Gemini CLI 认证模式切换"
        :optional ["mode"]
        :modes ["apikey" "google" "status"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs (delegates to sysinfra/misc.rs)"
        (ingress
          :schema "mode optional; apikey/google/status"
          :callers ["指挥官" "setup 脚本"])
        (logic-core
          (step s1 "parse requested auth mode or status")
          (step s2 "delegate to sysinfra/misc Gemini auth handler")
          (step s3 "read or update Gemini CLI auth mode")
          (step s4 "return status/receipt"))
        (egress
          :reads ["Gemini auth config"]
          :writes ["Gemini auth config when mode changes"]
          :returns "auth mode status/receipt")
        :dispatches-to-worker "N/A — 配置修改"
        :memory-cross-ref []
        :flow-ref "trivial-single-step auth config control"
        :called-by ["指挥官" "setup 脚本"]
        :necessity-pending-review false
        :note "必要 setup/config 面; mcp 壳放 comm/question.rs, handler 在 sysinfra/misc — 位置迁移可等代码整理")

      (tool mission_incident
        :desc "AIOps Incident 管理 test/list/get/remediate/status/close"
        :actions ["test" "list" "get" "remediate" "status" "close"]
        :required ["action"]
        :optional ["severity" "title" "source" "server_id" "limit" "id" "description" "reason" "actor"]
        :handler-file "crates/missiond-daemon/src/handlers/comm/question.rs (delegates to sysinfra/misc.rs)"
        (ingress
          :schema "action required; test creates synthetic incident; list/get/status read incidents; remediate creates/links board task; close requires reason+actor and safety gate"
          :callers ["intent-layer" "aiops 健康扫描 worker (产)" "board-frontend"])
        (logic-core
          (step s1 "route action test/list/get/remediate/status/close")
          (step s2 "test: build incident from severity/title/source/server_id and persist through misc handler")
          (step s3 "list: read recent incidents by limit")
          (step s4 "get/status: read incident + linked board task + recent notes + next_action")
          (step s5 "remediate: replay or synthesize incident through aiops::triage_incident, create/link board task")
          (step s6 "close: require reason+actor, guard user-owned/non-ops tasks, mark board task done and emit IncidentEvent::Resolved"))
        (egress
          :reads ["incidents" "board_tasks" "board_task_notes"]
          :writes ["incidents when action=test/remediate" "board_tasks/notes when remediate/close"]
          :returns "incident receipt/list/detail/remediation status")
        :dispatches-to-worker "worker::infra::aiops triage path; PtySlot incidents may auto-create Opus remediation slot via existing behavior"
        :memory-cross-ref ["system-support (incidents)"]
        :flow-ref "F-incident-reaction (code-aligned remediation playbook)"
        :called-by ["intent-layer" "aiops 健康扫描 worker (产)" "board-frontend"]
        :necessity-pending-review false
        :note "必要 AIOps/board incident 面; mcp 壳放 comm/question.rs, handler 在 sysinfra — 位置迁移可等代码整理"))

    ;; ── Router Chat (2 tools) ──
    (module router_chat
      :mcp-file "crates/missiond-mcp/src/tools/comm/router_chat.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/router_chat.rs"
      :capability-family "router-chat-session"

      (capability-family router-chat-session
        (ingress
          :tools ["mission_router_chat" "mission_router_chat_manage"]
          :callers ["intent-layer" "gemini-router slot" "board-frontend" "external MCP client"])
        (logic-core
          (step s1 "chat normalizes messages and optional task_id scoped conversation state")
          (step s2 "chat assembles rolling summary, active history, KB/board context, files, and context budget")
          (step s3 "chat calls Gemini/Router and persists only new turn messages when task_id is present")
          (step s4 "manage reads/mutates conversation history/archive/stats")
          (step s5 "compress summarizes older messages and advances rolling summary cursor"))
        (egress
          :flows ["F-router-chat-session"]
          :memory-cross-ref ["router_chat_conversations" "router_chat_messages" "router_chat_archive" "kb_entries" "board_tasks"]
          :external-service ["Gemini direct API for multimodal" "Router/GeminiClient for normal chat"]))

      (tool mission_router_chat
        :desc "通过 AI 路由器与 Gemini 等模型多轮对话"
        :optional ["messages" "message" "task_id" "context" "model" "max_tokens" "search" "files" "idle_timeout" "channel" "api_key_alias"]
        (ingress
          :schema "messages or message required; task_id/context/model/max_tokens/search/files/idle_timeout/channel/api_key_alias optional"
          :callers ["intent-layer" "gemini-router slot (registered-tasks)" "external MCP client"])
        (logic-core
          (step s1 "normalize single message shorthand into OpenAI-style messages[]")
          (step s2 "if task_id present, get/create router_chat conversation and prepend rolling summary + active history")
          (step s3 "if context=kb|board|both, load KB/board state and append to first user message")
          (step s4 "if files present, canonicalize paths, deny sensitive files, inline text or prepare binary via Gemini File API")
          (step s5 "apply context budget; attachment mode refuses silent truncation")
          (step s6 "call direct Gemini API for multimodal or Router/GeminiClient for normal chat")
          (step s7 "if task_id present, append new user messages and assistant response to history")
          (step s8 "return response, usage/model/tool_calls/warnings/conversation_id when available"))
        (egress
          :reads ["router_chat summary/history" "kb_entries" "board_tasks" "attached files"]
          :writes ["router_chat_messages when task_id present"]
          :external-calls ["Gemini File API" "Router/GeminiClient chat completion"]
          :returns "LLM response payload")
        :dispatches-to-worker "section llm-gateways :: path gemini-unified-gateway (目前); section xjp-router-gateway :: path xjp-router-chat-future (未来)"
        :external-service "XJP Router (HTTP 代理 Gemini/Sonnet/Minimax)"
        :memory-cross-ref ["conversation-logs (router 历史)" "llm-support"]
        :flow-ref "F-router-chat-session :: s1-s6"
        :called-by ["intent-layer" "gemini-router slot (registered-tasks)" "external MCP client"]
        :necessity-pending-review false
        :note "`interactive caller` 豁免 llm_gate (REQUEST_CALLER='router_chat', check_interactive_exempt)")

      (tool mission_router_chat_manage
        :desc "Gemini 对话管理 history/list/delete/clear/delete_message/restore/stats/compress"
        :actions ["history" "list" "delete" "clear" "delete_message" "restore" "stats" "compress"]
        :required ["action"]
        :optional ["task_id" "conversation_id" "message_id" "limit" "count" "batch_size" "keep_recent"]
        (ingress
          :schema "action required; history uses task_id; delete/clear restore use conversation_id or task_id; compress uses conversation_id or task_id"
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "route action to history/list/delete/clear/delete_message/restore/stats/compress legacy handlers")
          (step s2 "history/list/stats read router chat conversations and messages")
          (step s3 "clear/delete/delete_message archive messages before removing")
          (step s4 "restore rehydrates archived messages for a conversation")
          (step s5 "compress loads old unsummarized messages, builds summary prompt, calls Gemini, validates summary quality")
          (step s6 "compress snapshots previous summary and optimistic-lock updates summary cursor"))
        (egress
          :reads ["router_chat_conversations" "router_chat_messages" "router_chat_archive"]
          :writes ["router_chat_messages/archive" "router_chat rolling summary"]
          :external-calls ["Gemini summarizer when action=compress"]
          :returns "history/list/delete/clear/restore/stats/compress result")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["conversation-logs"]
        :flow-ref "F-router-chat-session :: s7/s8"
        :called-by ["intent-layer" "board-frontend"]
        :necessity-pending-review false))

    ;; ── Timeline (1 tool) ──
    (module timeline
      :mcp-file "crates/missiond-mcp/src/tools/comm/timeline.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/timeline.rs"
      :capability-family "timeline-observation"

      (capability-family timeline-observation
        (ingress
          :tools ["mission_timeline"]
          :callers ["intent-layer" "board-frontend"])
        (logic-core
          (step s1 "query/search/stats read event timeline with filters")
          (step s2 "trace reconstructs trace-scoped event chain and enriches Gemini completion details"))
        (egress
          :flows ["trivial-single-step event-bus read model"]
          :cross-ref-pillar-four ["event-bus pillar :: event_log"]))

      (tool mission_timeline
        :desc "系统时间轴 query/trace/stats/search"
        :actions ["query" "trace" "stats" "search"]
        :required ["action"]
        :optional ["eventType" "traceId" "since" "until" "limit" "offset" "keyword"]
        (ingress
          :schema "action required; query/search use event/time/pagination filters; trace requires traceId for trace view"
          :callers ["intent-layer" "board-frontend/api/timeline/events"])
        (logic-core
          (step s1 "route action query/trace/stats/search")
          (step s2 "query filters event rows by event_type/trace_id/since/until/limit/offset")
          (step s3 "trace reads events by trace_id and enriches gemini_request_completed with Gemini request detail")
          (step s4 "stats aggregates timeline event statistics")
          (step s5 "search performs keyword search with time filters"))
        (egress
          :reads ["event_log/timeline rows" "gemini_requests for trace enrichment"]
          :returns "timeline query/trace/stats/search result")
        :dispatches-to-worker "N/A — 读 event-bus pillar (跨 pillar)"
        :cross-ref-pillar-four "event-bus pillar :: event_log 表"
        :memory-cross-ref []
        :flow-ref "trivial-single-step (event-bus pillar read)"
        :called-by ["intent-layer" "board-frontend/api/timeline/events"]
        :necessity-pending-review false))

    ;; ── Audit (1 tool) ──
    (module audit
      :mcp-file "crates/missiond-mcp/src/tools/comm/audit.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/audit.rs"
      :capability-family "audit-observation"

      (capability-family audit-observation
        (ingress
          :tools ["mission_audit"]
          :callers ["intent-layer" "指挥官 debug"])
        (logic-core
          (step s1 "trace/detail/stats/export provide tool-call audit projections")
          (step s2 "export composes task, notes, conversations, and tool calls into Markdown"))
        (egress
          :flows ["trivial-single-step audit read/export model"]
          :memory-cross-ref ["conversation-logs tool_calls" "board task/note data for export"]))

      (tool mission_audit
        :desc "对话工具调用审计 trace/detail/stats/export"
        :actions ["trace" "detail" "stats" "export"]
        :required ["action"]
        :optional ["sessionId" "toolId" "taskId" "toolFilter" "includeReasoning" "includeMessages"]
        (ingress
          :schema "action required; trace/stats require sessionId; detail requires toolId; export requires taskId"
          :callers ["intent-layer (自省)" "指挥官 debug"])
        (logic-core
          (step s1 "route action trace/detail/stats/export")
          (step s2 "trace loads conversation and tool calls, optionally interleaves reasoning/messages")
          (step s3 "detail loads one tool call by toolId and parses raw_input/raw_output")
          (step s4 "stats aggregates tool calls by tool/status with first/last timestamps")
          (step s5 "export loads board task/notes and linked conversations, then renders Markdown"))
        (egress
          :reads ["conversations" "conversation_messages" "tool_calls" "board_tasks" "board_task_notes"]
          :returns "Markdown trace/export or structured detail/stats")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["conversation-logs (tool_calls)"]
        :flow-ref "trivial-single-step audit read/export model"
        :called-by ["intent-layer (自省)" "指挥官 debug"]
        :necessity-pending-review false))

    ;; ── Retrospective (1 tool) ──
    (module retrospective
      :mcp-file "crates/missiond-mcp/src/tools/comm/conversation.rs (共用 conversation.rs 模块)"
      :handler-file "crates/missiond-daemon/src/handlers/comm/retrospective.rs"
      :capability-family "retrospective-management"

      (capability-family retrospective-management
        (ingress
          :tools ["mission_retrospective_manage"]
          :callers ["intent-layer" "指挥官"])
        (logic-core
          (step s1 "list returns recent retrospective results")
          (step s2 "backfill counts eligible sessions and spawns retro_worker backfill from since"))
        (egress
          :flows ["F8-retrospective-to-memory"]
          :memory-cross-ref ["retrospectives" "deep_analysis" "conversation-logs"]))

      (tool mission_retrospective_manage
        :desc "复盘管理 list/backfill"
        :actions ["list" "backfill"]
        :required ["action"]
        :optional ["limit" "since"]
        (ingress
          :schema "action required; list uses limit; backfill requires since"
          :callers ["intent-layer" "指挥官 '会话复盘'"])
        (logic-core
          (step s1 "route action list/backfill")
          (step s2 "list reads recent retrospective records with limit")
          (step s3 "backfill validates since and counts sessions needing retrospective")
          (step s4 "backfill spawns background retro_worker::backfill")
          (step s5 "return retrospective list or backfill started receipt"))
        (egress
          :reads ["retrospectives" "conversations needing backfill"]
          :writes ["retrospectives/deep_analysis asynchronously through retro worker"]
          :returns "retrospective list or backfill receipt")
        :dispatches-to-worker "section worker-cluster :: worker-sonnet :: path retro-worker-cycle"
        :memory-cross-ref ["conversation-logs (retrospectives)" "system-support (deep_analysis)"]
        :flow-ref "F8-retrospective-to-memory"
        :called-by ["intent-layer" "指挥官 '会话复盘'"]
        :necessity-pending-review false
        :note "CLAUDE.md 指定: 会话复盘 → mission_retrospective"))

    ;; ── Beacon (1 tool) ──
    (module beacon
      :mcp-file "crates/missiond-mcp/src/tools/knowledge/kb.rs (mcp 壳在 kb module)"
      :handler-file "crates/missiond-daemon/src/handlers/knowledge/kb.rs (legacy mission_beacon_* handlers; consolidated mapper is code-alignment pending)"
      :capability-family "beacon-code-map"
      :note "mcp 归 knowledge/kb; Lisp 架构要求 consolidated mission_beacon(action=list|map|upsert) 映射到 legacy mission_beacon_list/map/tag; 代码对齐阶段再实施"

      (capability-family beacon-code-map
        (ingress
          :tools ["mission_beacon"]
          :callers ["intent-layer" "ast_sync_worker (间接)" "external MCP client"])
        (logic-core
          (step s1 "intended action list reads all beacons")
          (step s2 "intended action map reads beacon topology by name")
          (step s3 "intended action upsert maps to legacy tag/annotate path, edits source comment and upserts DB node")
          (step s4 "code-alignment target: daemon dispatch maps consolidated mission_beacon to existing mission_beacon_list/map/tag handlers"))
        (egress
          :flows ["architecture-designed direct beacon read/upsert" "ast_sync indirect refresh"]
          :memory-cross-ref ["beacon_nodes" "ast_nodes"]
          :file-side-effects ["upsert/tag may write source file comments"]))

      (tool mission_beacon
        :desc "代码信标操作 list/map/upsert"
        :actions ["list" "map" "upsert"]
        :required ["action"]
        :optional ["name" "file_path" "symbol" "feature" "annotation"]
        :runtime-status "architecture target; code-alignment pending for consolidated mission_beacon dispatcher; legacy mission_beacon_* names exist"
        (ingress
          :schema "action required; map uses name; upsert uses file_path/symbol/feature/annotation"
          :callers ["intent-layer" "ast_sync_worker (间接)" "external MCP client"])
        (logic-core
          (step s1 "list: intended to call beacon_list and return all beacon records")
          (step s2 "map: intended to call beacon_map(name) and return nodes/files")
          (step s3 "upsert/tag: read target source file, find symbol declaration, insert @beacon comment if absent")
          (step s4 "upsert/tag: ensure beacon row and upsert beacon node with repo/file/symbol/annotation")
          (step s5 "annotate branch updates beacon node annotation without source edit")
          (step s6 "legacy mission_beacon_* names remain backward-compatible for older worker prompts"))
        (egress
          :reads ["beacon_nodes" "source file for upsert/tag"]
          :writes ["source file beacon comment" "beacon_nodes"]
          :returns "beacon list/map/upsert receipt after code alignment")
        :dispatches-to-worker "section worker-cluster :: worker-local :: path ast-sync-worker-cycle (间接)"
        :memory-cross-ref ["kb-manager (beacon_nodes)"]
        :flow-ref "architecture-designed direct beacon list/map/upsert; code-alignment pending; ast_sync indirect"
        :called-by ["intent-layer" "ast_sync_worker (间接)"]
        :necessity-pending-review false))

    ;; ── Codex Ops (1 tool) ──
    (module codex_ops
      :mcp-file "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
      :handler-file "crates/missiond-daemon/src/handlers/comm/codex_ops.rs"
      :added "commit ec269d7"
      :capability-family "codex-ops-read"

      (capability-family codex-ops-read
        (ingress
          :tools ["mission_codex_ops"]
          :callers ["intent-layer" "指挥官 debug"])
        (logic-core
          (step s1 "recent scans codex_cli conversations and returns newest tool calls")
          (step s2 "thread reads one Codex thread metadata and tool calls")
          (step s3 "tool_stats aggregates tool call success/error counts across Codex threads"))
        (egress
          :flows ["trivial-single-step codex_ingestion read model"]
          :memory-cross-ref ["conversation-logs source=codex_cli" "tool_calls"]))

      (tool mission_codex_ops
        :desc "查询 Codex CLI 操作历史 (from ~/.codex/state_5.sqlite via codex_ingestion_worker)"
        :actions ["recent" "thread" "tool_stats"]
        :required ["action"]
        :optional ["threadId" "toolFilter" "since" "project" "limit"]
        (ingress
          :schema "action required; recent/tool_stats accept since/project/limit; thread requires threadId and optional toolFilter/limit"
          :callers ["intent-layer (查 Codex 历史)" "指挥官 debug"])
        (logic-core
          (step s1 "route action recent/thread/tool_stats")
          (step s2 "recent: list codex_cli conversations, filter project, pull per-thread tool calls, filter since at tool_call timestamp, sort newest")
          (step s3 "thread: load conversation metadata, pull all tool calls for threadId, optional toolFilter, sort newest, truncate")
          (step s4 "tool_stats: scan codex_cli conversations and aggregate per-tool total/success/error counts")
          (step s5 "return structured operation history or aggregate stats"))
        (egress
          :reads ["conversations source=codex_cli" "tool_calls"]
          :returns "recent calls / thread detail / tool stats")
        :dispatches-to-worker "N/A — 读 codex_ingestion_worker 已摄入的 conversations"
        :memory-cross-ref ["conversation-logs"]
        :flow-ref "trivial-single-step (recent/thread/tool_stats read model over codex_ingestion 产出)"
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
      :capability-family "daemon-system-control"

      (capability-family daemon-system-control
        (ingress
          :tools ["mission_sys_logs" "mission_sys_config" "mission_daemon_update"]
          :callers ["指挥官" "intent-layer" "autopilot dynamic config"])
        (logic-core
          (step s1 "logs exposes daemon log tail with level/grep filters")
          (step s2 "config exposes whitelisted YAML get/list/patch with per-file lock and hot reload")
          (step s3 "daemon_update performs self-build/binary-replace/restart lifecycle"))
        (egress
          :flows ["F-daemon-update-restart" "trivial system log/config read-write"]
          :file-cross-ref ["logs/missiond.log*" "slots.yaml" "llm.yaml" "permissions.yaml" "current missiond binary"]))

      (tool mission_sys_logs
        :desc "读 MissionD daemon 运行日志尾部"
        :optional ["lines" "level" "grep"]
        (ingress
          :schema "lines optional max 500; level all/warn/error; grep case-insensitive"
          :callers ["指挥官 debug" "intent-layer"])
        (logic-core
          (step s1 "resolve log directory from MISSIOND_LOG_FILE or default mission home logs")
          (step s2 "find latest missiond.log* file")
          (step s3 "read log file and take expanded tail window")
          (step s4 "filter by level and grep keyword")
          (step s5 "return last requested lines or no-match message"))
        (egress
          :reads ["MissionD log file"]
          :returns "text log tail")
        :dispatches-to-worker "N/A — 读文件"
        :memory-cross-ref []
        :flow-ref "trivial-single-step daemon log read"
        :called-by ["指挥官 debug" "intent-layer"]
        :necessity-pending-review false)

      (tool mission_sys_config
        :desc "读/写 daemon 配置"
        :actions ["get" "patch" "list"]
        :optional ["action" "file" "path" "value"]
        :allowed-files ["slots.yaml" "llm.yaml" "permissions.yaml"]
        (ingress
          :default-action "get"
          :schema "get/list/patch; file must be whitelisted basename; patch requires JSON Pointer path and value"
          :callers ["指挥官 setup" "autopilot (动态配置)"])
        (logic-core
          (step s1 "route action get/patch/list")
          (step s2 "list returns allowed config filenames")
          (step s3 "get resolves whitelisted file under mission home, reads YAML, converts to JSON")
          (step s4 "patch acquires per-file lock and reads YAML")
          (step s5 "patch converts YAML to JSON, applies JSON Pointer value, serializes back to YAML")
          (step s6 "patch writes file; slots.yaml additionally calls reload_slots_config and returns reload delta"))
        (egress
          :reads ["slots.yaml" "llm.yaml" "permissions.yaml"]
          :writes ["whitelisted config file on patch"]
          :side-effects ["slots config hot reload on slots.yaml patch"]
          :returns "config JSON/list/patch receipt")
        :dispatches-to-worker "N/A"
        :memory-cross-ref ["system-support (daemon_state)"]
        :flow-ref "trivial-single-step daemon config read/write"
        :called-by ["指挥官 setup" "autopilot (动态配置)"]
        :necessity-pending-review false)

      (tool mission_daemon_update
        :desc "daemon 自更新"
        :optional ["skip_build"]
        (ingress
          :schema "skip_build optional boolean; default false"
          :callers ["指挥官"])
        (logic-core
          (step s1 "resolve current executable path as binary_dest and project root from CARGO_MANIFEST_DIR")
          (step s2 "if skip_build=false, run cargo build --release --package missiond-daemon")
          (step s3 "verify target/release/missiond exists")
          (step s4 "copy to temp file, chmod executable, atomically rename over current binary")
          (step s5 "on macOS, ad-hoc codesign the replaced binary")
          (step s6 "if launchd service exists, spawn delayed launchctl kickstart -k after response")
          (step s7 "otherwise write and spawn fallback restart script"))
        (egress
          :writes ["current missiond binary" "temp restart script when no launchd"]
          :external-side-effects ["cargo build" "codesign" "launchctl kickstart or process restart"]
          :returns "update/restart receipt before MCP disconnect")
        :dispatches-to-worker "N/A — 外部流程"
        :memory-cross-ref []
        :flow-ref "F-daemon-update-restart"
        :called-by ["指挥官"]
        :necessity-pending-review false)

      ;; mission_control 已在 compute/worker.rs 定义 - 不重复)
      )

    ;; ── Infra (2 tools) ──
    (module infra
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
      :capability-family "infra-registry-and-diagnostics"

      (capability-family infra-registry-and-diagnostics
        (ingress
          :tools ["mission_infra_query" "mission_infra_ops"]
          :callers ["指挥官" "intent-layer" "aiops worker"])
        (logic-core
          (step s1 "query reads runtime infra registry by list/get filters")
          (step s2 "ops health delegates daemon health snapshot")
          (step s3 "ops reachability runs parallel network/channel probes")
          (step s4 "ops diagnose runs SSH-based remote OS diagnostics with KB credential fallback"))
        (egress
          :flows ["F-infra-diagnostics" "trivial infra registry read"]
          :memory-cross-ref ["system-support incidents" "credential KB for diagnose fallback"]))

      (tool mission_infra_query
        :desc "基础设施查询 list/get"
        :actions ["list" "get"]
        :required ["action"]
        :optional ["id" "role" "provider"]
        (ingress
          :schema "action required; list accepts role/provider; get requires id"
          :callers ["指挥官" "intent-layer (ops 任务)"])
        (logic-core
          (step s1 "route action list/get")
          (step s2 "list reads state.infra registry and filters by role or provider")
          (step s3 "get reads one server by id")
          (step s4 "return server list/detail or not-found error"))
        (egress
          :reads ["runtime infra registry loaded from servers.yaml"]
          :returns "infra server list/detail")
        :dispatches-to-worker "N/A — 读 infra/servers.yaml 或 DB"
        :memory-cross-ref []
        :flow-ref "trivial-single-step infra registry read"
        :called-by ["指挥官" "intent-layer (ops 任务)"]
        :necessity-pending-review false)

      (tool mission_infra_ops
        :desc "基础设施运维 health/reachability/diagnose"
        :actions ["health" "reachability" "diagnose"]
        :required ["action"]
        :optional ["target" "channels" "checks"]
        (ingress
          :schema "action required; reachability/diagnose require target; channels/checks filter probe sets"
          :callers ["指挥官" "aiops worker" "intent-layer"])
        (logic-core
          (step s1 "route action health/reachability/diagnose")
          (step s2 "health delegates mission_health in sysinfra/misc to read PTY/memory/event/gemini/stats snapshot")
          (step s3 "reachability resolves server host/lan/tailscale/ssh/health_endpoint from infra registry")
          (step s4 "reachability runs selected lan_ping/public_ping/tailscale/ssh/deploy_agent probes concurrently")
          (step s5 "diagnose resolves SSH targets from registry, user@host, or raw host")
          (step s6 "diagnose searches credential KB for password fallback")
          (step s7 "diagnose runs selected shell checks remotely and parses SECTION output")
          (step s8 "compute severity and return structured health/reachability/diagnose result"))
        (egress
          :reads ["infra registry" "PTY status" "ControlTree memory state" "extraction states" "credential KB"]
          :external-calls ["ping" "tailscale status" "TCP connect" "deploy_agent HTTP" "ssh/sshpass remote shell"]
          :returns "health snapshot / reachability matrix / OS diagnosis")
        :dispatches-to-worker "section worker-cluster :: worker-local (aiops 跨 pillar 到 system pillar infra/aiops.rs)"
        :memory-cross-ref ["system-support (incidents)"]
        :flow-ref "F-infra-diagnostics"
        :called-by ["指挥官" "aiops worker"]
        :necessity-pending-review false))

    ;; ── Permission (2 tools) ──
    (module permission
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
      :added "Phase 1-5 upgrade 2026-04-12"
      :capability-family "permission-policy-and-learned-rules"

      (capability-family permission-policy-and-learned-rules
        (ingress
          :tools ["mission_permission_query" "mission_permission_mutate"]
          :callers ["intent-layer" "指挥官 audit" "PTY permission injector/debug"])
        (logic-core
          (step s1 "query reads static permission config, learned rules, or merged_for_slot debug projection")
          (step s2 "mutate updates role/slot policy, auto-allow patterns, reloads config, or revokes learned permissions")
          (step s3 "egress joins worker learned-permission flow used by PTY confirm automation"))
        (egress
          :flows ["F-learned-permission"]
          :file-cross-ref ["learned_permissions.yaml" "permissions.yaml/static policy"]
          :worker-cross-ref ["worker pillar :: pty :: learned-permissions"]))

      (tool mission_permission_query
        :desc "权限查询 get/learned_list (含 merged_for_slot debug 视图)"
        :actions ["get" "learned_list" "merged_for_slot"]
        :required ["action"]
        :optional ["scopeType" "scopeId" "slotId"]
        (ingress
          :schema "action required; learned_list accepts scopeType/scopeId; merged_for_slot requires slotId"
          :callers ["intent-layer" "指挥官 audit" "PTY permission debug"])
        (logic-core
          (step s1 "route action get/learned_list/merged_for_slot")
          (step s2 "get returns current static permission config")
          (step s3 "learned_list reads all learned permissions or one scope")
          (step s4 "merged_for_slot resolves slot role/cwd, project id, static role/slot rules, and learned spawn-visible rules")
          (step s5 "return permission config/rules/merged view"))
        (egress
          :reads ["permission config in memory" "learned_permissions.yaml" "SlotManager slots" "ProjectRegistry"]
          :returns "permission config / learned rules / merged slot view")
        :dispatches-to-worker "section pty :: subsection learned-permissions :: mcp-merged-view"
        :memory-cross-ref []
        :file-reads ["learned_permissions.yaml"]
        :flow-ref "F-learned-permission :: read/debug views + trivial static config read"
        :called-by ["intent-layer" "指挥官 audit"]
        :necessity-pending-review false)

      (tool mission_permission_mutate
        :desc "权限写 set_role/set_slot/auto_allow/reload/revoke"
        :actions ["set_role" "set_slot" "auto_allow" "reload" "revoke"]
        :required ["action"]
        :optional ["role" "slotId" "rule" "pattern" "scopeType" "scopeId" "toolPattern"]
        (ingress
          :schema "action required; set_role/set_slot require rule; auto_allow requires role or slotId plus pattern; revoke requires scopeType/scopeId/toolPattern"
          :callers ["指挥官 (手动配置)" "intent-layer"])
        (logic-core
          (step s1 "route action set_role/set_slot/auto_allow/reload/revoke")
          (step s2 "set_role updates role PermissionRule")
          (step s3 "set_slot updates slot PermissionRule")
          (step s4 "auto_allow appends role or slot auto_allow pattern")
          (step s5 "reload reloads permission policy from disk")
          (step s6 "revoke removes learned permission by scope and toolPattern")
          (step s7 "return mutation receipt"))
        (egress
          :reads ["permission config" "learned_permissions.yaml"]
          :writes ["permission config in memory/disk via permission manager" "learned_permissions.yaml on revoke"]
          :returns "permission mutation receipt")
        :dispatches-to-worker "section pty :: subsection learned-permissions :: path learned-permission-read (reload)"
        :memory-cross-ref []
        :file-writes ["learned_permissions.yaml"]
        :flow-ref "F-learned-permission :: manual set/reload/revoke"
        :called-by ["指挥官 (手动配置)" "intent-layer"]
        :necessity-pending-review false))

    ;; ── Power (1 tool) ──
    (module power
      :mcp-file "crates/missiond-mcp/src/tools/sysinfra/power.rs"
      :handler-file "crates/missiond-daemon/src/handlers/sysinfra/health.rs → misc.rs"
      :capability-family "power-control-mvp"

      (capability-family power-control-mvp
        (ingress
          :tools ["mission_power_control"]
          :callers ["指挥官" "intent-layer"])
        (logic-core
          (step s1 "status probes SSH TCP reachability for target/server host")
          (step s2 "wake/suspend currently records requested intent and returns server metadata")
          (step s3 "future implementation wires per-target WoL/cloud suspend APIs"))
        (egress
          :flows ["trivial MVP power request/status; status overlaps F-infra-diagnostics"]
          :cross-ref-system-layer ["infra registry" "future WoL/cloud API"]))

      (tool mission_power_control
        :desc "物理服务器电源管控 wake(WoL/gcloud)/suspend/status"
        :required ["target" "action"]
        :actions ["wake" "suspend" "status"]
        (ingress
          :schema "target and action required; action wake/suspend/status"
          :callers ["指挥官" "intent-layer (大任务前唤醒 GPU 机)"])
        (logic-core
          (step s1 "validate target/action")
          (step s2 "look up target in infra registry and attach server metadata if found")
          (step s3 "status: choose registered host or raw target and probe TCP port 22 with 3s timeout")
          (step s4 "wake: log wake requested and return MVP receipt")
          (step s5 "suspend: log suspend requested and return MVP receipt")
          (step s6 "unknown action returns error"))
        (egress
          :reads ["infra registry"]
          :external-calls ["TCP connect for status"]
          :future-side-effects ["Wake-on-LAN/cloud API wake" "remote/cloud suspend"]
          :returns "power status/request receipt")
        :dispatches-to-worker "N/A — 外部 WoL / gcloud API"
        :memory-cross-ref []
        :flow-ref "trivial-single-step MVP wake/suspend request; status overlaps F-infra-diagnostics TCP probe"
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
        (ingress
          :schema "taskId/artifactType/content required; requiresMasterDecision optional"
          :callers ["intent-layer (agent 阶段完成)" "external MCP client"]
          :board-family "board-task-lifecycle :: flow phase submission")
        (logic-core
          (step s1 "load board task and require flow_phase")
          (step s2 "parse EngineeringPhase and validate artifactType matches current phase")
          (step s3 "persist artifact into FlowContext field")
          (step s4 "advance flow_phase to next EngineeringPhase and update board task")
          (step s5 "add flow-engine progress note")
          (step s6 "if Plan→Execute, create risk review agent_question and emit QuestionEvent::Created")
          (step s7 "if requiresMasterDecision present, create implementation decision question and emit QuestionEvent::Created"))
        (egress
          :writes ["board_tasks.flow_context" "board_tasks.flow_phase" "board_task_notes" "agent_questions when gated"]
          :emits ["QuestionEvent::Created"]
          :returns "phase advanced text result"
          :downstream "F-board-submit-phase")
        :dispatches-to-worker "handlers/sysinfra/misc.rs → validate EngineeringPhase artifact → persist FlowContext → advance flow_phase"
        :event-emits ["QuestionEvent::Created (Plan→Execute hard gate or requiresMasterDecision soft gate)"]
        :memory-cross-ref ["board (board_tasks.flow_phase / flow_context)"]
        :flow-ref "F-board-submit-phase"
        :called-by ["intent-layer (agent 阶段完成)" "external MCP client"]
        :necessity-pending-review false))

    (sysinfra-contract-summary
      :memory-cross-ref ["system-support" "board"]
      :file-writes ["control_tree.json" "learned_permissions.yaml"]
      :cross-pillar ["system-layer (config/log)" "worker pillar (infra/aiops)"]))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.6 MCP Surface Lifecycle — promoted implementations + future backlog
  ;; ══════════════════════════════════════════════════════════
  (section mcp-surface-lifecycle
    :status "mission_execution + mission_capability_usage + mission_directive/plan/workflow + mission_global_instruction promoted to implemented; future backlog currently empty; unified-entry-pipeline 复用 mission_directive/plan/workflow/execution 既有 surface, 不新增 tool; workstation dispatch 全程复用 mission_pty_*/mission_compute_slot/mission_task_delegate, 不新增 tool"
    :actual-count 83
    :future-count 0
    :promotion-rule "实现任一 future surface 前, 先更新 mcp-defs / mcp-dispatch / tools count / flow index, 再写 handler code"
    :unified-entry-tool-policy "F-intent-alignment-plan-execution-loop 不引入新 tool; mission_directive(action=compile) 是 message intake 入口; 详 flow pillar :: future-flows :: unified-entry-future-candidates (mission_message / mission_invoke 仅候选, 不计入 83)"
    :workstation-dispatch-tool-policy "F-workstation-dispatch-policy 不引入新 tool; resident-lisp 复用 mission_pty_send/mission_task_delegate; fresh-code-alignment 走 mission_pty_spawn/mission_compute_slot; execution coordination 仍是 mission_execution; agent-team 是任务 .md 文字提示, 不需要 daemon side tool"

    (implemented-surface mission_execution
      :status "code-aligned; 12-action manager + ExecutionEvent emission implemented"
      :actions ["open" "list" "claim" "heartbeat" "release" "deviate" "decide" "issue" "complete" "status" "audit" "repair"]
      :owner-boundary "tools owns schema; worker owns manager mechanics; memory owns execution protocol/file shape"
      :unified-pipeline-role "MissionD 统一入口 execution substrate — F-intent-alignment-plan-execution-loop :: s6 execution-runner 的底层协调面 (未来 plan-runner 内部消费; 当前由人/上层 actor 调用)"
      :workstation-dispatch-record "execution coordination 可记录本次 PLAN 节点选用的工位策略: dispatch_strategy ∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback}; 写入由 plan-runner(open) 时附带, 供 evidence-collector 与 capability-usage-monitor 事后回放"
      :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: execution-strategy-record + flow pillar :: F-workstation-dispatch-policy :: s4 record-strategy"
      :dispatch-strategy-field-status "architecture-designed; mission_execution schema 暂未含 dispatch_strategy 字段, 待 code-alignment 时补 (向后兼容默认 unknown)"
      :code ["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
      (ingress
        :schema "action required; execution_id required except open/list; scope/phase/claimer fields per action; future open 接受 dispatch_strategy"
        :callers ["multi-agent code/lisp alignment sessions" "intent-layer manager UI" "external MCP client" "unified entry pipeline plan-runner (future internal caller)"])
      (logic-core
        (step s1 "validate action + required fields; normalize execution_id/parent_design/scope")
        (step s2 "dispatch to worker :: agent-execution-manager-interface")
        (step s3 "manager reads/writes memory :: agent-execution-coordination slots")
        (step s4 "status/audit/repair return structured reports; mutating actions return receipt + allocated id/claim"))
      (egress
        :writes ["*-execution.lisp via manager" "tool_calls audit"]
        :reads ["execution companion logs" "parent design Lisp"]
        :flow-ref "F-execution-log-governance / F-intent-alignment-plan-execution-loop :: s6 execution-runner substrate / F-workstation-dispatch-policy :: s4 record-strategy"
        :event-bus "ExecutionEvent::* emitted for mutating/audit/repair actions"
        :returns "mission_execution action JSON"))

    (implemented-surface mission_directive
      :status "code-aligned partial; management surface implemented; compile is dry-run until directive-compiler actor exists"
      :actions ["compile" "list" "get" "approve" "archive" "version_chain"]
      :owner-boundary "tools owns schema; intent-layer owns directive-compiler; memory directive-layer owns store"
      :unified-pipeline-role "MissionD 统一入口 message intake / alignment 管理面 — F-intent-alignment-plan-execution-loop :: s1 message-intake + s3 alignment-review-gate (file-first SSOT 是 .missiond/alignment/<topic>/intent-alignment.lisp; directive 表是 DB 镜像)"
      :code ["crates/missiond-mcp/src/tools/knowledge/directive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive.rs"]
      (ingress
        :schema "action required; compile takes utterance/source/conversation_id?; approve/archive take directive_id; source ∈ {message, architecture_lisp_delta, user_request} for unified intake"
        :callers ["Claude Code user utterance capture" "intent-layer actor" "external MCP client" "unified entry pipeline message intake (s1)"])
      (logic-core
        (step s1 "validate action and source utterance or directive_id")
        (step s2 "compile returns dry-run compiled directive preview; persist=true writes draft row until directive-compiler actor exists")
        (step s3 "read/control actions call DirectiveLayerStore directive_* APIs")
        (step s4 "list/get/approve/archive/version_chain are full manager actions over directive rows")
        (step s5 "approval gates can emit QuestionEvent in future when human confirmation is required"))
      (egress
        :writes ["directive table when compile persist=true / approve / archive"]
        :reads ["directive table" "version chain"]
        :flow-ref "F-intent-alignment-plan-execution-loop :: s1 message-intake + s3 alignment-review-gate (unified pipeline 主入口) / F-directive-plan-workflow-compile :: directive branch (management surface code-aligned; compiler actor pending)"
        :returns "directive id/status/compiled sexp/version chain"))

    (implemented-surface mission_plan
      :status "code-aligned partial; management surface implemented; compile is dry-run and execute is bridge descriptor until plan-compiler/execution actor exists"
      :actions ["compile" "list" "get" "by_task" "approve" "mark" "supersede" "execute" "record_evidence"]
      :owner-boundary "tools owns schema; intent-layer owns plan-compiler; memory directive-layer owns plan store; board links task execution"
      :unified-pipeline-role "MissionD 统一入口 plan 管理面 — F-intent-alignment-plan-execution-loop :: s4 plan-authoring + s5 plan-review-gate + s6 execution-runner bridge + s7 evidence sidecar (file-first SSOT 是 .missiond/plans/<topic>/PLAN.lisp; plan 表是 DB 镜像)"
      :execute-contract "execute 返回 next_call descriptor 是临时管理面契约; 未来由 MissionD plan-runner 内部 dispatch (architecture-designed); 不允许 client 把 next_call 私有解析当作长期方案"
      :dispatch-strategy-consumer "未来 plan-runner 在 execute 时读取 PLAN.lisp 节点的 :dispatch-strategy + :target_project, 按 dispatch-decision-matrix 选 resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback; 当前 next_call descriptor 不携带此信息, 由 caller 自行解读"
      :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: dispatch-decision-matrix + flow pillar :: F-workstation-dispatch-policy"
      :code ["crates/missiond-mcp/src/tools/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
      (ingress
        :schema "action required; compile takes directive_id/board_task_id?; execute/record_evidence take plan_id; mark/supersede take plan_id"
        :callers ["intent-layer directive pipeline" "board task planning UI" "external MCP client" "unified entry pipeline plan-authoring/review/runner stages"])
      (logic-core
        (step s1 "validate directive/plan/task identifiers")
        (step s2 "compile returns dry-run plan preview; persist=true writes draft row and requires board_task_id")
        (step s3 "future plan-compiler writes real plan sexp DAG/FSM with sexp_hash and board_task binding")
        (step s4 "approve/mark/supersede transition plan FSM through DirectiveLayerStore — plan-review-gate 的管理面")
        (step s5 "execute returns next_call bridge descriptor for mission_execution / mission_task_delegate / mission_flow_run; rejects unknown targets and non-approved plans; future plan-runner consumes this internally instead of returning to caller")
        (step s6 "record_evidence writes sidecar .missiond/v2/plans/<plan_id>.evidence.json for future workflow distillation — evidence-collector 当前 partial 实现"))
      (egress
        :writes ["plan table" "optional board_tasks.source_directive_id / plan binding" ".missiond/v2/plans/<plan_id>.evidence.json"]
        :reads ["directive" "plan" "board_tasks by_task"]
        :flow-ref "F-intent-alignment-plan-execution-loop :: s4 plan-authoring + s5 plan-review-gate + s6 execution-runner + s7 evidence-collection / F-directive-plan-workflow-compile :: plan branch (management surface code-aligned; compiler/runner actors pending)"
        :returns "plan id/status/DAG summary/supersede chain/next_call bridge descriptor"))

    (implemented-surface mission_workflow
      :status "code-aligned partial; list/get/match/record_execution implemented; apply read-only; distill/compile_methodology dry-run; run_methodology not implemented"
      :actions ["list" "get" "match" "apply" "distill" "record_execution" "compile_methodology" "run_methodology"]
      :owner-boundary "tools owns schema; intent-layer owns workflow-distiller; memory directive-layer owns workflow store"
      :unified-pipeline-role "MissionD 统一入口 workflow distillation 管理面 — F-intent-alignment-plan-execution-loop :: s8 workflow-distillation (file-first SSOT 是 .missiond/workflows/<topic>.lisp; workflow 表是 DB 镜像)"
      :code ["crates/missiond-mcp/src/tools/knowledge/workflow.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"]
      (ingress
        :schema "action required; match/apply take intent/context; distill takes plan_id or successful execution ref; compile/run_methodology take workflow_path/name + params"
        :callers ["intent-layer reuse engine" "skill/flow authoring loop" "external MCP client" "unified entry pipeline workflow-distillation stage"])
      (logic-core
        (step s1 "list/get/match read workflow templates and match_rules")
        (step s2 "apply returns a candidate reusable workflow without executing it directly")
        (step s3 "distill returns dry-run preview or writes draft template with persist=true until workflow-distiller actor exists")
        (step s4 "record_execution updates workflow usage stats and success/failure feedback")
        (step s5 "compile_methodology reads .missiond/workflows/<name>.lisp and returns preview; YAML emitter actor pending")
        (step s6 "run_methodology returns next-step pointer to compile_methodology + mission_flow_run; it does not execute directly"))
      (egress
        :writes ["workflow table" "workflow execution stats"]
        :reads ["workflow table" "plan/directive evidence"]
        :flow-ref "F-intent-alignment-plan-execution-loop :: s8 workflow-distillation / F-directive-plan-workflow-compile :: workflow distill/match branch code-aligned partial / F-methodology-to-executable-compile :: compile/run methodology branch compiler pending"
        :returns "workflow template/match/apply/distill result"))

    (implemented-surface mission_global_instruction
      :status "code-aligned; read/edit full; reload returns manual-reload-required because Claude Code owns session bootstrap"
      :actions ["read" "edit" "reload"]
      :owner-boundary "tools owns schema; intent-layer owns global-claudemd-manager; filesystem owns ~/.claude/CLAUDE.md"
      :code ["crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"]
      (ingress
        :schema "action required; edit takes new_content + dry_run? + allow_empty?; caller cannot provide path"
        :callers ["指挥官" "intent-layer governance" "external MCP client"])
      (logic-core
        (step s1 "read resolves dirs::home_dir()/.claude/CLAUDE.md and returns content/path/size/sha256/mtime/utf8_lossy; missing file returns status=not_found")
        (step s2 "edit validates new_content; rejects empty content unless allow_empty=true; dry_run returns preview without backup/write")
        (step s3 "real edit copies existing file to ~/.claude/CLAUDE.md.bak.<UTC stamp>, then writes tmp file and atomic rename")
        (step s4 "identical content short-circuits to status=noop with no backup")
        (step s5 "reload returns status=manual-reload-required and daemon_reload_supported=false; daemon does not fake Claude Code session reload"))
      (egress
        :file-writes ["~/.claude/CLAUDE.md when edit applies"]
        :reads ["~/.claude/CLAUDE.md"]
        :flow-ref "trivial-single-step read/edit/manual-reload"
        :returns "content / dry-run preview / write receipt / manual reload receipt"))

    (implemented-surface mission_capability_usage
      :status "code-aligned partial; c55fd61 implements 5 actions; event emission and semantic merge detection deferred"
      :actions ["snapshot" "report" "candidates" "mark" "ack"]
      :owner-boundary "tools owns MCP schema; memory owns read-model; flow owns monitoring choreography; intent-layer owns lifecycle decision"
      :code ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"]
      (ingress
        :schema "action required; window? default 30d; scope=tool|flow|both; project?; candidate_id? for mark/ack; dry_run optional for mark"
        :callers ["architecture cleanup review" "intent-layer governance" "external MCP client"])
      (logic-core
        (step s1 "validate action/window/scope and refuse destructive action; mark/ack only records review status")
        (step s2 "snapshot/report/candidates call F-capability-usage-monitoring and memory capability-usage-read-model")
        (step s3 "candidates returns evidence-ranked active/quiet/stale/never-used/shadowed/protected buckets; merge-candidate bucket present but semantic parser deferred")
        (step s4 "mark records human decision intent to sidecar but does not edit tool registry or flow catalog")
        (step s5 "ack closes a report item only after follow_up_ref points to PLAN.lisp / mission_execution / board task"))
      (egress
        :writes ["<project_root>/.missiond/v2/capability-usage-review.json for mark/ack" "no daemon_state JSON cache because daemon_state is i64-only"]
        :reads ["conversation_tool_calls" "board_tasks.flow_template" "MCP registry" "YAML flow registry" "review sidecar"]
        :flow-ref "F-capability-usage-monitoring (code-aligned partial)"
        :event-bus "ObservabilityEvent::CapabilityUsageSnapshot / CapabilityStaleCandidate emitted after snapshot/candidates computation"
        :returns "usage snapshot / candidate report / review receipt")))

  ;; ══════════════════════════════════════════════════════════
  ;; 3.7 Tool Governance — schema / audit / reload
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
      :memory-module "conversation-logs + system-support derived usage monitor (v0.5.4)"
      :writer "gen_gateway.rs 路由入口 (每 tools/call 前后)"
      :consumed-by "memory :: system-support :: capability-usage-read-model + flow :: F-capability-usage-monitoring")

    (tool-registry-runtime
      :target "crates/missiond-mcp/src/gen_gateway.rs"
      :mechanism "tool_name → handler_fn 数据驱动 dispatch, Forge 冲压生成"))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (T001-T010)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (T001 :status RESOLVED :resolved-at "2026-04-21"
          :finding "83 tools 确认 (按 current MCP registry + handler 文件枚举). mcp-defs 头部 '67 tools' 旧注释未更新. mission_execution / mission_capability_usage / mission_directive / mission_plan / mission_workflow / mission_global_instruction 已从 future surface 提升为 actual")
    (T002 :status "resolved-by-flow-v0.2"
          :finding "flow v0.2 已建/修正 tool-backed-flows-index: 第一批把 task_delegate / compute_slot / skill_exec / cascade_* 从 pending 提升为 named flow; 其余仍按 trivial/shared-flow 分批填")
    (T003 :status "architecture-resolved"
          :note "83 tool capability-family 分类已完成; necessity-pending-review 已清零. 后续若要删/合并工具, 属产品清理或 breaking schema cleanup")
    (T004 :status RESOLVED :resolved-at "2026-04-21"
          :finding "6 历史原因 (pause / slot_history / inbox / incident / gemini_auth / submit_phase_result) + 1 故意设计 (mission_beacon in KB domain). 根因: old-slot→new-slot 迁移时工具定义留原 group, handler 集中到 misc.rs. 详 phase-B-scan-findings § B.4")
    (T005 :status "architecture-decided"
          :note "mission_minimax_process 保留为 legacy alias until next breaking MCP schema cleanup; 当前不从 83 删除")
    (T006 :status "architecture-decided"
          :note "mission_memory 3 action 不拆; 作为 memory-domain operator surface 保留, flow-ref 分别指向 extraction/control/token read")
    (T007 :status "architecture-decided"
          :finding "2026-04-25 code scan: mission_skill_exec 是独立 skill workflow executor (skill file workflow block + 30s MCP step dispatch + skill_executions), 不是 flow-engine-v2 的别名"
          :decision "保留独立 executor; future adapter 可把 skill workflow block 转成 FlowDefinition 或反向索引, 但不把 mission_skill_exec 并入 mission_flow_run")
    (T008 :status "architecture-decided"
          :note "mission_kb_ops 6 action 不拆; 作为 KB governance/ops queue 聚合面保留, 未来仅在产品 UI 需要独立公开面时拆")
    (T009 :status "architecture-resolved-code-cleanup-pending"
          :note "handler 位置历史债已标注到对应 tool; 不阻塞架构. 代码对齐阶段可整理 sys_config/daemon_update/power_control/infra/permission 的 module placement")
    (T010 :status "resolved-by-v0.5"
          :note "当前 83 tools 已完成 capability-family 梳理与 flow-ref 分类; v0.7 同步 execution/capability usage/directive-plan-workflow/global-instruction promoted surfaces 并收敛 T003/T005/T006/T008/T009. future-mcp-surfaces backlog 当前为 0"))
)
