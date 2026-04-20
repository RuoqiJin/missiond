;; ══════════════════════════════════════════════════════
;; MissionD — System Layer (Detailed Draft)
;; 目标: 把旧地图里分散的 types / transport / rpc / utils / state machines 折叠成一个运行时底座 pillar
;; ══════════════════════════════════════════════════════

(pillar system-layer
  :status "草稿 v0.1 — 由旧的 transport/types/rpc/utils/state-machines 五块重整"
  :actual-state-sources ["intent-types.lisp"
                         "intent-rpc-gateway.lisp"
                         "intent-pillar-transport-bootstrap.lisp"
                         "intent-pillar-state-machines.lisp"
                         "intent-pure-utility.lisp"]

  (purpose "无业务语义的运行时底座 — 类型、启动、传输、RPC、纯工具")

  (pillar-ingress
    (entry-1 "进程启动")
    (entry-2 "stdio / IPC / WebSocket 传输")
    (entry-3 "worker / handler / tool 对共享类型和纯函数的调用"))

  (pillar-core
    (core-1 "gen_types = 跨 crate 单一类型真理源")
    (core-2 "bootstrap = 守护进程启动时的严格依赖序列")
    (core-3 "RPC gateway = tool_name 路由与 error code 归一")
    (core-4 "state machines = 生命周期合法迁移的语义底板")
    (core-5 "pure utils = 无 I/O 的可复用确定性算法"))

  (pillar-egress
    (egress-1 "把 AppState / 传输 / 类型能力交给 worker 与 tools")
    (egress-2 "把 JSON-RPC / IPC / WS 包装成统一运行时接口")
    (egress-3 "把合法状态机与 helper 函数注入上层模块"))

  ;; ─────────────────────────────────────────────────────
  ;; 类型系统
  ;; ─────────────────────────────────────────────────────
  (path shared-types
    (ingress
      :source "任意 crate 需要共享枚举/结构体/serde 边界"
      :entry-components ["gen_types.rs"])
    (logic-core
      (step s1 "在 intent-types.lisp 定义 enum / struct")
      (step s2 "Forge 生成 gen_types.rs")
      (step s3 "派生 Serialize/Deserialize / as_str/from_str")
      (step s4 "供 DB / IPC / JSON-RPC / event payload 共用")
      (step s5 "上层模块按这些类型约束状态和值域"))
    (egress
      :returns "shared runtime types"))

  (path lifecycle-state-machines
    (ingress
      :source "board / PTY / task / question / extraction 等生命周期变化"
      :entry-components ["state-machines declarations"])
    (logic-core
      (step s1 "为每类实体声明状态集合")
      (step s2 "声明允许的 transition 与 trigger")
      (step s3 "作为上层 handler / engine 的合法迁移约束")
      (step s4 "帮助避免非法跳转与灰色状态")
      (step s5 "形成类型 + 状态机双重约束"))
    (egress
      :returns "lifecycle semantics"))

  ;; ─────────────────────────────────────────────────────
  ;; 启动 / 进程 / 传输
  ;; ─────────────────────────────────────────────────────
  (path daemon-bootstrap
    (ingress
      :source "main.rs 进程启动"
      :entry-components ["daemon-init" "state-management"])
    (logic-core
      (step s1 "初始化基础设施: DB pool / embed model / event-bus")
      (step s2 "加载 ProjectRegistry")
      (step s3 "启动 PTYManager / SlotManager / MissionControl")
      (step s4 "初始化 LLM gateways")
      (step s5 "初始化 context pipeline / worker registry / control tree")
      (step s6 "spawn workers + autopilot + ipc handler + ws server"))
    (egress
      :returns "可服务的 daemon runtime"
      :invariant "下游必须依赖上游就绪, 不允许逆序"))

  (path app-state-distribution
    (ingress
      :source "handler / worker / subscriber 需要共享运行时依赖"
      :entry-components ["AppState"])
    (logic-core
      (step s1 "把 db / event_bus / slot_manager / llm_gateway / context_pipeline / project_registry 等封装进 AppState")
      (step s2 "以 Arc<RwLock<...>> 或等价共享方式下发")
      (step s3 "启动后尽量只读访问, 把权威状态留在 DB + event_bus")
      (step s4 "减少大范围 mutable global state")
      (step s5 "供 tool / worker / infra 统一读取"))
    (egress
      :returns "shared application runtime"))

  (path ipc-transport
    (ingress
      :source "mcp ↔ daemon 通信请求"
      :entry-components ["ipc/mod.rs" "ipc-handler" "mcp-client"])
    (logic-core
      (step s1 "建立 Unix socket / TCP 连接")
      (step s2 "封装请求与响应消息")
      (step s3 "daemon 端解析并路由到 handler")
      (step s4 "等待处理完成或超时")
      (step s5 "把结果回送到请求方"))
    (egress
      :returns "IPC response"))

  (path websocket-stream
    (ingress
      :source "前端订阅 board / trace / screenshot / timeline-like streams"
      :entry-components ["ws/server.rs" "screenshot_broker.rs" "jarvis_trace.rs"])
    (logic-core
      (step s1 "客户端建立 WS 连接")
      (step s2 "server 注册订阅")
      (step s3 "上游 runtime 把 screenshot / trace / frontend event 推给 broker")
      (step s4 "server 序列化并广播")
      (step s5 "前端消费更新"))
    (egress
      :returns "frontend stream payload"))

  ;; ─────────────────────────────────────────────────────
  ;; RPC Gateway
  ;; ─────────────────────────────────────────────────────
  (path json-rpc-gateway
    (ingress
      :source "stdio JSON-RPC / daemon IPC JSON-RPC"
      :entry-components ["gen_server.rs" "missiond-mcp gateway"])
    (logic-core
      (step s1 "解析 initialize / notifications/initialized / tools/list / tools/call / ping")
      (step s2 "按数据驱动路由表映射 tool_name")
      (step s3 "生成统一错误码: UNKNOWN_TOOL / INVALID_PARAM / DB_ERROR 等")
      (step s4 "把请求转给 tools pillar")
      (step s5 "序列化返回包"))
    (egress
      :returns "JSON-RPC response"))

  ;; ─────────────────────────────────────────────────────
  ;; 纯工具函数
  ;; ─────────────────────────────────────────────────────
  (path semantic-parsing-helpers
    (ingress
      :source "extractor / parser / PTY processing"
      :entry-components ["gen_parsing.rs"])
    (logic-core
      (step s1 "处理 spinner / args / 状态栏括号内容 / idle prompt 等纯逻辑")
      (step s2 "保证无 I/O、无副作用")
      (step s3 "供 semantic parser / extractor 重复调用")
      (step s4 "用测试样例锁定行为")
      (step s5 "避免把这些细节散落在业务代码里"))
    (egress
      :returns "pure parsing result"))

  (path string-and-budget-utils
    (ingress
      :source "任何需要安全截断或 token 预算估算的上层调用"
      :entry-components ["string helpers" "context budget"])
    (logic-core
      (step s1 "safe_byte_truncate / safe_char_truncate 保证 UTF-8 安全")
      (step s2 "estimate_tokens 粗估 token 开销")
      (step s3 "allocate_budget 做多源边际分配")
      (step s4 "把纯算法结果返回给调用方")
      (step s5 "避免业务层重复实现危险切片或预算逻辑"))
    (egress
      :returns "safe truncation / token budget plan")))
