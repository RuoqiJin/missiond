;; ══════════════════════════════════════════
;; Pattern: mcp-tool
;; Project: MissionD
;; 24 modules · 67 tools · 4 domains
;; Stamped from Jarvis intent-mcp.lisp
;; ══════════════════════════════════════════

;; ── Arg type vocabulary ──
;; string | integer | number | boolean | array | object
;; :enum restricts to fixed values
;; :required marks field as mandatory (goes into JSON Schema "required")
;; :default provides fallback for lenient extraction

;; ════════════════════════════════════════════════════════════
;;  Domain: knowledge — Board · KB · Skill · Memory · Insight
;; ════════════════════════════════════════════════════════════

(mcp-module board
  :target "crates/missiond-mcp/src/tools/knowledge/board.rs"

  (tool mission_board_query
    :description "任务板统一查询。action: list(树形列表), get(详情), search(搜索), summary(统计), clear_done(批量清除已完成)"
    (input
      (action    string :enum (list get search summary clear_done) :description "查询类型，默认 list")
      (status    string :description "[list/search] 状态: open/done/running/failed/blocked")
      (includeHidden boolean :description "[list/search] 包含隐藏任务，默认 false")
      (id        string :description "[get] 单个任务 ID")
      (ids       array  :description "[get] 批量任务 ID")
      (includeChildren boolean :description "[get] 内嵌子任务详情，默认 false")
      (query     string :description "[search] 全文搜索 title+description")
      (project   string :description "[search] 项目过滤")
      (category  string :description "[search] 分类: deploy/dev/infra/test/other")
      (parentId  string :description "[search] 父任务 ID")
      (limit     integer :default 50 :description "[search] 最大返回条数")
      (since     string :description "[summary] 起始时间 ISO 8601"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_board_create
    :description "创建新任务。支持子任务(parentId)、DAG依赖(dependsOn)、Flow模式(flowTemplate)"
    (input
      (title     string :required :description "任务标题")
      (description string :description "任务描述")
      (priority  string :enum (high medium low) :default "medium")
      (category  string :enum (deploy dev infra test other) :default "other")
      (project   string :description "关联项目")
      (server    string :description "关联服务器")
      (dueDate   string :description "截止日期 YYYY-MM-DD")
      (parentId  string :description "父任务 ID")
      (assignee  string :description "分配的 PTY 工位 ID")
      (autoExecute boolean :default false :description "due_date 到达时自动执行")
      (promptTemplate string :description "自动执行 prompt 模板")
      (hidden    boolean :default false :description "隐藏任务")
      (flowTemplate string :description "Flow 模板名(如 engineering)")
      (dependsOn array  :description "DAG 依赖前置任务 ID 列表"))
    :returns "Value")

  (tool mission_board_update
    :description "更新任务。传 id 更新单个，传 ids 批量更新"
    (input
      (id        string :description "单个任务 ID(与 ids 二选一)")
      (ids       array  :description "批量任务 ID(与 id 二选一)")
      (title     string)
      (description string)
      (status    string :enum (open running done failed blocked))
      (priority  string :enum (high medium low))
      (category  string :enum (deploy dev infra test other))
      (project   string)
      (server    string)
      (dueDate   string)
      (parentId  string)
      (assignee  string)
      (autoExecute boolean)
      (promptTemplate string)
      (hidden    boolean)
      (flowPhase string :enum (investigate consult_gemini_1 plan consult_gemini_2 execute finalize done))
      (flowTemplate string)
      (dependsOn array))
    :returns "Value")

  (tool mission_board_delete
    :description "删除任务(级联删除子任务)"
    (input
      (id string :required :description "任务 ID"))
    :returns "Value")

  (tool mission_board_claim
    :description "认领任务。原子操作：仅当 open 且未被认领时成功，成功后自动变为 running"
    (input
      (taskId     string :required :description "要认领的任务 ID")
      (executorId string :description "执行者标识，不填用 MCP session ID")
      (executorType string :enum (pty_slot manual_session) :default "manual_session"))
    :returns "Value")

  (tool mission_board_note_add
    :description "为任务添加进度笔记"
    (input
      (taskId   string :required)
      (content  string :required :description "笔记内容(支持 markdown)")
      (noteType string :enum (progress summary note) :default "note")
      (author   string :description "作者标识"))
    :returns "Value")

  (tool mission_board_decompose
    :description "一键拆分任务。派工位调查代码后自动创建带 DAG 依赖链的子任务序列"
    (input
      (taskId string :required :description "要拆分的父任务 ID")
      (slotId string :default "slot-coder-1" :description "执行拆分的工位 ID")
      (hints  string :description "拆分方向提示"))
    :returns "Value")

  (tool mission_board_retry
    :description "重试失败/阻塞的任务。重置状态为 open 并解除 claim"
    (input
      (taskId string :required)
      (resetDownstream boolean :default true :description "同时重置所有下游依赖任务"))
    :returns "Value")

  (tool mission_submit_phase_result
    :description "Flow 任务阶段产出物提交。系统自动推进到下一阶段"
    (input
      (taskId       string :required :description "Board 任务 ID")
      (artifactType string :required :enum (investigation_report execution_plan execution_result commit_hash))
      (content      string :required :description "产出物内容")
      (requiresMasterDecision string :description "困惑描述，触发 Decision Engine 审核"))
    :returns "Value"))

(mcp-module kb
  :target "crates/missiond-mcp/src/tools/knowledge/kb.rs"

  (tool mission_kb_query
    :description "知识库查询(统一入口)。search: FTS5+Embedding 混合 RRF; get: 按 key 精确查询; list: 列出条目"
    (input
      (action    string :enum (search get list) :default "search")
      (query     string :description "[search] 搜索关键词")
      (category  string :description "[search/list] 分类过滤")
      (limit     integer :default 10 :description "[search] 返回条数上限(最大 50)")
      (offset    integer :default 0 :description "[search] 分页偏移")
      (search_mode string :enum (exact explore) :default "explore" :description "[search] exact=纯相关性; explore=含 MMR 多样性")
      (key       string :description "[get] 精确 key"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_kb_remember
    :description "记录知识到长期记忆。key 已存在则更新。主动调用无需许可"
    (input
      (category   string :required :enum (preference memory memory:architecture memory:bugfix memory:debug memory:ops memory:feature memory:decision memory:platform project architecture architecture:summary decision policy:decision feature infra procedure))
      (key        string :required :description "唯一标识")
      (summary    string :required :description "结论性摘要 ≤400字")
      (detail     object :description "结构化详情 JSON")
      (source     string :enum (conversation discovery import) :default "conversation")
      (confidence number :default 1.0 :description "置信度 0.0-1.0"))
    :returns "Value"
    :validation ((summary :min-len 5 :max-len 400)))

  (tool mission_kb_mutate
    :description "知识库写操作。forget: 删除; update: 部分更新; import: 导入"
    (input
      (action    string :required :enum (forget update import))
      (key       string :description "[forget/update] 目标 key")
      (keys      array  :description "[forget] 批量删除 key 列表(优先于 key)")
      (category  string :description "[update] 新分类")
      (summary   string :description "[update] 新摘要 ≤400字")
      (detail    object :description "[update] 新详情 JSON")
      (confidence number :description "[update] 新置信度")
      (linked_task_id string :description "[update] 关联 Board 任务 ID(空串清除)")
      (format    string :enum (servers_yaml json) :description "[import] 导入格式")
      (path      string :description "[import] 文件路径"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_kb_ops
    :description "知识库运维。gc: 治理; analyze: AI分析; discover: SSH探测; queue_status/execute_plan: 操作队列; compact: 清理低质量"
    (input
      (action    string :required :enum (gc analyze discover queue_status execute_plan compact))
      (gc_action string :description "[gc] stats/stale/duplicates/clean_stale/clean_duplicates")
      (days      integer :description "[gc] stale 天数阈值(默认 30)")
      (mode      string :enum (overview consolidation_plan custom) :description "[analyze] 分析模式")
      (target_category string :description "[analyze] 按分类过滤(前缀匹配)")
      (limit     integer :description "[analyze/execute_plan] 每次最大条目数")
      (offset    integer :description "[analyze] 分页偏移")
      (include_board_context boolean :description "[analyze] 注入 Board 任务上下文")
      (custom_prompt string :description "[analyze] 自定义分析 prompt(mode=custom)")
      (model     string :description "[analyze] 使用的模型")
      (max_tokens integer :description "[analyze] 最大响应 token 数(默认 16384)")
      (save_plan boolean :default true :description "[analyze] consolidation_plan 自动保存到队列")
      (task_id   string :description "[analyze] 关联 Board 任务 ID")
      (host      string :description "[discover] SSH 目标: user@ip 或 infra key")
      (port      integer :default 22 :description "[discover] SSH 端口")
      (password  string :description "[discover] SSH 密码(不填用密钥)")
      (plan_id   string :description "[queue_status/execute_plan] 批次 ID")
      (status    string :description "[queue_status] pending/running/done/skipped/failed")
      (dryRun    boolean :default true :description "[compact] 预览模式"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_beacon
    :description "代码信标操作。list: 列出所有; map: 获取拓扑 L3; upsert: 打/更新标签"
    (input
      (action    string :required :enum (list map upsert))
      (name      string :description "[map/upsert] 信标名称")
      (file_path string :description "[upsert] 文件路径")
      (symbol    string :description "[upsert] 目标函数/结构体名称")
      (feature   string :description "[upsert] 信标名称(新建时必填)")
      (annotation string :description "[upsert] 人工注释"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_code_search
    :description "搜索代码结构 L3(AST 索引)。tree-sitter 提取的签名和调用关系"
    (input
      (query     string :required :description "搜索查询(函数名/结构体名/自然语言)")
      (repo      string :description "限定仓库名")
      (file_path string :description "限定文件路径前缀")
      (node_type string :enum (function struct enum impl trait mod const type))
      (limit     integer :default 20))
    :returns "Value"))

(mcp-module skill
  :target "crates/missiond-mcp/src/tools/knowledge/skill.rs"

  (tool mission_skill_query
    :description "Skill 知识库查询。list/search/topics/actions/stats"
    (input
      (action string :required :enum (list search topics actions stats))
      (query  string :description "[search] 搜索关键词")
      (skill  string :description "按 Skill 名筛选"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_skill_context
    :description "Skill 上下文构建。build: 简单匹配; resolve: 递归聚合含 requires 依赖"
    (input
      (action string :required :enum (build resolve))
      (query  string :required :description "任务关键词/描述")
      (skill  string :description "直接指定 skill name 跳过搜索")
      (include_board boolean :description "[resolve] 包含 Board 相关任务"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_skill_mutate
    :description "Skill 写操作。upsert: 创建/更新章节; record: 记录碎片; render: 重建 .md; rollback: 回滚"
    (input
      (action  string :required :enum (upsert record render rollback))
      (topic   string :description "Skill 主题名")
      (section_title string :description "[upsert] 章节标题")
      (content string :description "[upsert/record] 内容")
      (sort_order integer :description "[upsert] 章节排序")
      (skill   string :description "[rollback] Skill 名称")
      (version_id integer :description "[rollback] 版本 ID"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_skill_exec
    :description "执行 Skill 中定义的 workflow。顺序执行 MCP 工具步骤"
    (input
      (skill   string :required :description "Skill 名称")
      (action  string :required :description "Action ID(workflow block id)")
      (dry_run boolean :default false :description "预览模式")
      (params  object :description "运行时参数覆盖(注入为 ${key})"))
    :returns "Value"))

(mcp-module memory_ops
  :target "crates/missiond-mcp/src/tools/knowledge/memory.rs"

  (tool mission_memory
    :description "记忆与 Token 管理。pending: 待分析对话; pause: 暂停/恢复; token_stats: Token 消耗统计"
    (input
      (action    string :required :enum (pending pause token_stats))
      (paused    boolean :description "[pause] true=暂停, false=恢复, 省略=toggle")
      (sessionId string :description "[token_stats] 按会话 ID 过滤")
      (slotId    string :description "[token_stats] 按工位 ID 过滤")
      (since     string :description "[token_stats] 时间过滤 ISO 8601")
      (groupBy   string :enum (session slot model day) :description "[token_stats] 聚合维度"))
    :returns "Value"
    :dispatch-on action))

(mcp-module insight
  :target "crates/missiond-mcp/src/tools/knowledge/insight.rs"

  (tool mission_insight
    :description "查看 MissionD 战略认知 — 开发轨迹、协作模式、反面模式、摩擦点。纯只读"
    (input
      (section string :enum (all profile trajectory patterns proposals friction) :default "all"))
    :returns "Value"))

;; ════════════════════════════════════════════════════════════════════
;;  Domain: compute — Task · Process · PTY · CC · Minimax · Worker · Slot · Job
;; ════════════════════════════════════════════════════════════════════

(mcp-module task
  :target "crates/missiond-mcp/src/tools/compute/task.rs"

  (tool mission_task_submit
    :description "提交任务给专家 Agent。async: 异步提交; sync: 同步等待结果"
    (input
      (action   string :enum (async sync) :default "async")
      (role     string :required :description "专家角色(如 secret/deploy/memory)")
      (prompt   string :description "[async] 任务提示词")
      (question string :description "[sync] 问题")
      (slotId   string :description "指定目标工位 ID(跳过 role 匹配)")
      (timeoutMs number :default 120000 :description "[sync] 超时毫秒数"))
    :returns "Value")

  (tool mission_task_query
    :description "任务状态查询。status/list/ack/track"
    (input
      (action  string :required :enum (status list ack track))
      (taskId  string :description "[status/track] 任务 ID")
      (status  string :description "[list] queued/running/done/failed")
      (limit   integer :default 20 :description "[list] 最大返回数")
      (since   integer :description "[ack] finished_at > since(epoch ms)"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_task_cancel
    :description "取消任务"
    (input
      (taskId string :required))
    :returns "Value"))

(mcp-module task_delegate
  :target "crates/missiond-mcp/src/tools/compute/task_delegate.rs"

  (tool mission_task_delegate
    :description "声明式任务委派：描述目标，Daemon 自主选 slot/创建/执行/回报"
    (input
      (objective string :required :description "任务目标(自然语言)")
      (intent    string :enum (code ops research general) :default "general" :description "意图类型决定 slot 模板")
      (cwd       string :default "~/Projects" :description "工作目录")
      (timeout_secs integer :default 1800 :description "超时秒数(上限 7200)")
      (priority  string :enum (high medium low) :default "medium")
      (depends_on array :description "前置任务 ID 列表(DAG)")
      (context_hints array :description "上下文关键词，预加载 KB/Skill")
      (model string :description "可选 CLI 模型覆盖；default/claude-code-default 表示不传 --model")
      (model_profile string :description "可选模型 profile；coding-default-opus-4-7 表示 Claude Code Default(Opus 4.7/1M)"))
    :returns "Value"))

(mcp-module process
  :target "crates/missiond-mcp/src/tools/compute/process.rs"

  (tool mission_agent
    :description "Agent 进程管理。spawn/kill/restart/list"
    (input
      (action  string :required :enum (spawn kill restart list))
      (slotId  string :description "[spawn/kill/restart] 工位 ID")
      (visible boolean :description "[spawn/restart] 打开终端窗口")
      (autoRestart boolean :description "[spawn] 崩溃后自动重启"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_slots
    :description "列出所有工位配置"
    (input)
    :returns "Value")

  (tool mission_inbox
    :description "获取收件箱消息"
    (input
      (unreadOnly boolean :default true :description "仅未读")
      (limit      number  :default 10))
    :returns "Value"))

(mcp-module pty
  :target "crates/missiond-mcp/src/tools/compute/pty.rs"

  (tool mission_pty_spawn
    :description "启动 PTY 交互式会话。默认异步返回不等待 Idle"
    (input
      (slotId     string :required)
      (waitForIdle boolean :default false :description "等待 Claude 就绪")
      (timeoutSecs number :default 60 :description "Idle 超时秒数(waitForIdle=true 时生效)")
      (autoRestart boolean :description "崩溃后自动重启")
      (mcpConfigPath string :description "MCP 配置文件路径(JSON)"))
    :returns "Value")

  (tool mission_pty_send
    :description "向 PTY 发送消息。默认 fire-and-forget"
    (input
      (slotId  string :required)
      (message string :required)
      (waitForResponse boolean :default false :description "阻塞等待回复")
      (timeoutMs number :default 300000 :description "[waitForResponse=true] 默认/上下限由 V3 workstation-config timeout-policy pty-send-blocking 投影"))
    :returns "Value")

  (tool mission_pty_read
    :description "读取 PTY 内容。screen/history/logs"
    (input
      (action string :required :enum (screen history logs))
      (slotId string :required)
      (lines  number :description "[screen] 最后 N 行"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_pty_signal
    :description "向 PTY 发送信号。kill/interrupt"
    (input
      (action string :required :enum (kill interrupt))
      (slotId string :required))
    :returns "Value"
    :dispatch-on action)

  (tool mission_pty_confirm
    :description "发送确认响应(工具使用确认对话框)"
    (input
      (slotId   string :required)
      (response string :required :description "true/false 或选项编号或直接输入"))
    :returns "Value")

  (tool mission_pty_status
    :description "获取 PTY 会话状态"
    (input
      (slotId string :description "工位 ID(不填返回所有)"))
    :returns "Value")

  (tool mission_pty_screenshot
    :description "截取 PTY 终端截图(PNG)，返回文件路径"
    (input
      (slotId string :required))
    :returns "Value"))

(mcp-module cc_tasks
  :target "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"

  (tool mission_cc_query
    :description "Claude Code 任务监控。sessions/tasks/overview/in_progress"
    (input
      (action     string :required :enum (sessions tasks overview in_progress))
      (sessionId  string :description "[tasks] 会话 ID")
      (projectPath string :description "[sessions/tasks] 项目路径")
      (activeOnly boolean :default true :description "[sessions] 仅活跃会话"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_cc_swarm
    :description "通过 PTY 触发 Claude Code Swarm 模式并行执行"
    (input
      (slotId    string :required :description "PTY 工位 ID")
      (tasks     array  :required :description "要执行的任务列表")
      (teammateCount number :default 3 :description "并行 Agent 数量")
      (timeoutMs number :default 600000 :description "PTY 等待超时；默认/上下限由 V3 workstation-config timeout-policy claudecode-swarm 投影"))
    :returns "Value"))

(mcp-module minimax
  :target "crates/missiond-mcp/src/tools/compute/minimax.rs"

  (tool mission_minimax_process
    :description "[Deprecated → mission_sonnet_process] 调用 Sonnet 处理文本"
    (input
      (text      string :required :description "待处理文本")
      (task      string :required :enum (summarize translate custom))
      (prompt    string :description "[custom] 自定义 prompt")
      (targetLang string :default "en" :description "[translate] 目标语言")
      (maxChars  integer :default 200 :description "[summarize] 目标字数"))
    :returns "Value")

  (tool mission_sonnet_process
    :description "调用 Claude Sonnet 处理文本。直接 HTTP 调用无 PTY 开销"
    (input
      (text      string :required :description "待处理文本")
      (task      string :required :enum (summarize translate custom))
      (prompt    string :description "[custom] 自定义 prompt")
      (targetLang string :default "en" :description "[translate] 目标语言")
      (maxChars  integer :default 200 :description "[summarize] 目标字数"))
    :returns "Value"))

(mcp-module worker
  :target "crates/missiond-mcp/src/tools/compute/worker.rs"

  (tool mission_worker
    :description "后台 Worker + LLM 闸口管理。list: 列出状态+闸口; control: 暂停/恢复"
    (input
      (action  string :required :enum (list control))
      (target  string :description "[control] Worker 名或 LLM 闸口(gemini/sonnet/codex)")
      (control_action string :enum (pause resume status) :description "[control] 操作"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_control
    :description "统一调控闸口。级联机制: 关 provider=sonnet 自动暂停依赖 Sonnet 的 Worker"
    (input
      (target_type string :required :enum (global provider domain worker slot_role))
      (target_name string :description "名称(global 时可省略)")
      (action      string :required :enum (pause resume status)))
    :returns "Value"))

(mcp-module slot
  :target "crates/missiond-mcp/src/tools/compute/slot.rs"

  (tool mission_slot_history
    :description "查询工位任务历史(realtime_extract/deep_analysis/kb_gc 等)"
    (input
      (slotId   string :description "工位 ID(不传查所有)")
      (taskType string :description "realtime_extract/deep_analysis/kb_gc")
      (status   string :description "pending/running/completed/failed")
      (limit    integer :default 20)
      (stats    boolean :description "true 时只返回汇总统计"))
    :returns "Value")

  (tool mission_pause
    :description "全局暂停/恢复所有工位的工作分派"
    (input
      (action string :enum (pause resume status) :default "status"))
    :returns "Value"))

(mcp-module compute_slot
  :target "crates/missiond-mcp/src/tools/compute/compute_slot.rs"

  (tool mission_compute_slot
    :description "动态计算工位管理。create/terminate/extend/list。TTL 生命周期，最多 5 个活跃"
    (input
      (action  string :required :enum (create terminate extend list))
      (template string :description "[create] 模板 ID: coder/ops")
      (objective string :description "[create] 意图描述；仅作为工位元数据，不会被自动发送")
      (initial_prompt string :description "[create] 可选显式 warm-up prompt；只有传入此字段才会在 slot Idle 后发送")
      (cwd     string :description "[create] 工作目录")
      (max_ttl integer :default 14400 :description "[create] 最大 TTL 秒(默认 4h, 上限 8h)")
      (model string :description "[create] 可选 CLI 模型覆盖；default/claude-code-default 表示不传 --model")
      (model_profile string :description "[create] 可选模型 profile；coding-default-opus-4-7 表示 Claude Code Default(Opus 4.7/1M)")
      (slot_id string :description "[terminate/extend] 动态 slot ID")
      (additional_seconds integer :description "[extend] 延长秒数(每次上限 3600)")
      (status  string :description "[list] active/terminated"))
    :returns "Value"
    :dispatch-on action))

(mcp-module job
  :target "crates/missiond-mcp/src/tools/compute/job.rs"

  (tool mission_job_poll
    :description "轮询异步 Job 状态。返回 status(running/completed/failed) + result/error"
    (input
      (job_id string :required :description "Job ID")
      (action string :enum (poll list cancel) :default "poll"))
    :returns "Value"))

;; ════════════════════════════════════════════════════════════════
;;  Domain: comm — RouterChat · Question · Conversation · Timeline · Audit
;; ════════════════════════════════════════════════════════════════

(mcp-module router_chat
  :target "crates/missiond-mcp/src/tools/comm/router_chat.rs"

  (tool mission_router_chat
    :description "通过 AI 路由器与 Gemini 等模型多轮对话。传 task_id 自动持久化历史"
    (input
      (messages  array  :description "本轮新消息(传 task_id 时历史自动加载)")
      (message   string :description "单条 user 消息(便捷模式，与 messages 二选一)")
      (task_id   string :description "关联 Board 任务 ID(自动加载/保存历史)")
      (context   string :enum (board kb both none) :default "none" :description "自动注入上下文")
      (model     string :description "模型(不传用 Router 默认)")
      (max_tokens integer :default 16384)
      (search    boolean :default false :description "启用 Google 搜索增强")
      (files     array  :description "本地文件路径列表(≤1MB 文本, ≤10MB 二进制)")
      (idle_timeout integer :default 120 :description "CLI 模式空闲超时秒数")
      (channel   string :enum (apikey google) :description "Gemini 认证渠道")
      (api_key_alias string :description "指定 API key 别名"))
    :returns "Value")

  (tool mission_router_chat_manage
    :description "Gemini 对话管理。history/list/delete/clear/delete_message/restore/stats/compress"
    (input
      (action    string :required :enum (history list delete clear delete_message restore stats compress))
      (task_id   string :description "[history/delete/clear/compress] Board 任务 ID")
      (conversation_id string :description "[delete/clear/restore/compress] 对话 ID")
      (message_id integer :description "[delete_message] 消息 ID")
      (count     integer :default 2 :description "[clear] 清理最后 N 条(-1=全部)")
      (limit     integer :default 50 :description "[list] 最大返回数")
      (batch_size integer :default 20 :description "[compress] 压缩批次大小")
      (keep_recent integer :default 10 :description "[compress] 保留最近 N 条不压缩"))
    :returns "Value"
    :dispatch-on action))

(mcp-module question
  :target "crates/missiond-mcp/src/tools/comm/question.rs"

  (tool mission_question
    :description "Agent 待决策问题管理。create/list/get/answer/dismiss"
    (input
      (action   string :required :enum (create list get answer dismiss))
      (id       string :description "[get/answer/dismiss] 问题 ID")
      (question string :description "[create] 问题描述")
      (context  string :description "[create] 补充上下文")
      (taskId   string :description "[create] 关联 Board 任务 ID")
      (slotId   string :description "[create] 提问工位 ID")
      (sessionId string :description "[create] 提问会话 ID")
      (target   string :enum (user master) :default "user" :description "决策目标")
      (options  string :description "[create] 结构化选项 JSON 数组")
      (decisionType string :enum (architecture implementation debug investigation risk preference) :description "[create] 决策类型影响路由")
      (answer   string :description "[answer] 回答/指示")
      (status   string :description "[list] pending/answered/dismissed")
      (limit    integer :description "[list] 返回条数"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_llm_trace
    :description "LLM 调用链路追踪。gemini_trace/gemini_stats/gemini_watch/gemini_auth/jarvis_logs/jarvis_trace"
    (input
      (action    string :required :enum (gemini_trace gemini_stats gemini_watch gemini_auth jarvis_logs jarvis_trace))
      (caller    string :enum (router_chat flow_engine kb_analyze embedding) :description "[gemini_trace] 调用方")
      (session_id string :description "[gemini_trace] Claude Code session ID")
      (status    string :description "[gemini_trace/jarvis_logs] 状态过滤")
      (limit     integer :default 20 :description "返回条数")
      (watch_action string :enum (start stop status) :description "[gemini_watch] 操作")
      (trace_id  string :description "[jarvis_trace] Trace ID")
      (mode      string :enum (apikey google status) :default "status" :description "[gemini_auth] 认证模式"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_decision_stats
    :description "Decision Engine 统计：各 Tier 命中率、待处理数、降级数"
    (input
      (hours integer :default 24 :description "统计时间窗口(小时)"))
    :returns "Value")

  (tool mission_gemini_auth
    :description "Gemini CLI 认证模式切换"
    (input
      (mode string :enum (apikey google status) :default "status"))
    :returns "Value")

  (tool mission_incident
    :description "AIOps Incident 管理。test/list/get/remediate/status/close — F-incident-reaction"
    (input
      (action   string :required :enum (test list get remediate status close))
      (id       string :description "[get/remediate/status/close] Incident ID")
      (severity string :default "warning" :description "[test/remediate] critical/high/warning")
      (title    string :description "[test/remediate] Incident 标题")
      (description string :description "[remediate] 详细描述（仅新建 incident 时使用）")
      (source   string :default "manual" :description "[test/remediate] health_check/deploy_center/sentry/manual/pty_slot")
      (server_id string :description "[test/remediate] 关联 infra server ID")
      (limit    number :default 20 :description "[list] 返回条数(最大 100)")
      (reason   string :description "[close] 关闭理由（必填，自由文本）")
      (actor    string :description "[close] 执行者标识（必填，如 commander/slot-ops/oncall）"))
    :returns "Value"
    :dispatch-on action))

(mcp-module conversation
  :target "crates/missiond-mcp/src/tools/comm/conversation.rs"

  (tool mission_conversation_query
    :description "对话统一查询。list/get/search/message_search/context/events/audit_classification/backfill_classification/audit_message_roles/backfill_message_roles/turn_backfill"
    (input
      (action   string :enum (list get search message_search context events audit_classification backfill_classification audit_message_roles backfill_message_roles turn_backfill) :default "list")
      (status   string :description "[list] active/completed")
      (conversationType string :description "[list] user/worker/meta/system/all")
      (taskId   string :description "[list] 按 Board 任务 ID 过滤")
      (sessionId string :description "[get/search/events/audit_message_roles/backfill_message_roles/turn_backfill] 会话 ID")
      (tail     integer :default 50 :description "[get] 最近 N 条消息")
      (sinceId  integer :description "[get] 增量拉取(ID 大于此值)")
      (includeRaw boolean :default false :description "[get] 完整消息含 rawContent/model")
      (query    string :description "[search/message_search] 搜索关键词")
      (queryMode string :enum (hybrid fts semantic) :default "hybrid" :description "[search] 搜索模式")
      (timeRange string :enum (last_24h last_7d last_30d) :description "[search/message_search] 时间范围")
      (project  string :description "[search] 项目过滤")
      (excludeSessionId string :description "[search] 排除特定会话")
      (offset   integer :description "[search] 分页偏移")
      (role     string :enum (user assistant tool_result system thinking) :description "[message_search] 消息角色")
      (toolName string :description "[message_search] 工具名过滤")
      (messageId integer :description "[context] 锚点消息 ID")
      (before   integer :default 3 :description "[context] 锚点前取几条(最大 50)")
      (after    integer :default 5 :description "[context] 锚点后取几条(最大 50)")
      (eventType string :description "[events] 事件类型过滤")
      (limit    integer :description "[list/search/message_search/events] 最大返回数")
      (since    string :description "[list] 起始时间(ISO/相对格式)")
      (until    string :description "[list] 结束时间")
      (source   string :description "[audit_classification/backfill_classification] provider source filter, e.g. claude_code")
      (minConfidence number :description "[audit_classification/backfill_classification] minimum confidence threshold")
      (apply    boolean :description "[backfill_classification/backfill_message_roles] true to write reviewed repairs")
      (rebuildTurns boolean :description "[backfill_classification/backfill_message_roles] rebuild conversation_turns after repair"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_conversation_analyze
    :description "对话分析。retrospective: 会话复盘; trajectory: 子 Agent 思维链; activity: 活动报告"
    (input
      (action    string :required :enum (retrospective trajectory activity))
      (sessionId string :description "[retrospective] 会话 ID")
      (depth     string :enum (quick detailed full) :default "quick" :description "[retrospective] 分析深度")
      (toolUseId string :description "[trajectory] 父 tool_use ID")
      (since     string :description "[activity] 起始时间")
      (until     string :description "[activity] 结束时间")
      (limit     integer :default 200 :description "[trajectory] 最大返回数"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_conversation_reconcile
    :description "JSONL-DB 对账。不传 sessionId 触发全量扫描"
    (input
      (sessionId string :description "指定会话 ID 立即对账"))
    :returns "Value")

  (tool mission_retrospective_manage
    :description "复盘管理。list: 已完成复盘; backfill: 批量回填"
    (input
      (action string :required :enum (list backfill))
      (limit  integer :default 10 :description "[list] 最大返回数")
      (since  string :description "[backfill] 起始时间 ISO datetime"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_embedding_ops
    :description "Embedding 操作。stats: 覆盖率统计; backfill: 触发回填"
    (input
      (action string :required :enum (stats backfill)))
    :returns "Value"
    :dispatch-on action))

(mcp-module timeline
  :target "crates/missiond-mcp/src/tools/comm/timeline.rs"

  (tool mission_timeline
    :description "系统时间轴。query/trace/stats/search"
    (input
      (action   string :required :enum (query trace stats search))
      (eventType string :description "[query] 事件类型过滤")
      (traceId  string :description "[query/trace] 因果链 trace_id")
      (since    string :description "起始时间(相对格式 1h/24h/7d 或 ISO)")
      (until    string :description "结束时间")
      (limit    integer :default 50 :description "[query/search] 返回条数")
      (offset   integer :default 0 :description "[query] 分页偏移")
      (keyword  string :description "[search] 搜索关键字"))
    :returns "Value"
    :dispatch-on action))

(mcp-module audit
  :target "crates/missiond-mcp/src/tools/comm/audit.rs"

  (tool mission_audit
    :description "对话工具调用审计。trace/detail/stats/export"
    (input
      (action    string :required :enum (trace detail stats export))
      (sessionId string :description "[trace/stats] 会话 ID")
      (toolId    string :description "[detail] tool_use_id")
      (taskId    string :description "[export] Board 任务 ID")
      (toolFilter array :description "[trace] 只看指定工具")
      (includeReasoning boolean :default false :description "[trace] 包含推理文本")
      (includeMessages boolean :default true :description "[export] 包含会话消息"))
    :returns "Value"
    :dispatch-on action))

;; ════════════════════════════════════════════════════════════
;;  Domain: sysinfra — Infra · Permission · Power · System
;; ════════════════════════════════════════════════════════════

(mcp-module infra
  :target "crates/missiond-mcp/src/tools/sysinfra/infra.rs"

  (tool mission_infra_query
    :description "基础设施查询。list: 列出服务器; get: 单个详情"
    (input
      (action   string :required :enum (list get))
      (id       string :description "[get] 服务器 ID")
      (role     string :description "[list] 角色: build/deploy/gpu/vpn/production")
      (provider string :description "[list] 云厂商: gcp/aliyun/self-hosted"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_infra_ops
    :description "基础设施运维。health/reachability/diagnose"
    (input
      (action   string :required :enum (health reachability diagnose))
      (target   string :description "[reachability/diagnose] Infra ID 或 user@host")
      (channels array  :description "[reachability] 探测通道: lan_ping/public_ping/tailscale/ssh/deploy_agent")
      (checks   array  :description "[diagnose] 检查项: system/crashes/top_cpu/temperatures/journal_errors/docker/network/gpu"))
    :returns "Value"
    :dispatch-on action))

(mcp-module permission
  :target "crates/missiond-mcp/src/tools/sysinfra/permission.rs"

  (tool mission_permission_query
    :description "权限查询。get: 获取配置; learned_list: 已学习的权限规则"
    (input
      (action   string :required :enum (get learned_list))
      (scopeType string :enum (role slot) :description "[learned_list] 作用域类型")
      (scopeId  string :description "[learned_list] 作用域 ID"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_permission_mutate
    :description "权限写操作。set_role/set_slot/auto_allow/reload/revoke"
    (input
      (action   string :required :enum (set_role set_slot auto_allow reload revoke))
      (role     string :description "[set_role/auto_allow] 角色名称")
      (slotId   string :description "[set_slot/auto_allow] 工位 ID")
      (rule     object :description "[set_role/set_slot] 权限规则{auto_allow,require_confirm,deny}")
      (pattern  string :description "[auto_allow] 工具模式(如 secret_*)")
      (scopeType string :enum (role slot) :description "[revoke] 作用域类型")
      (scopeId  string :description "[revoke] 作用域 ID")
      (toolPattern string :description "[revoke] 工具名称或模式"))
    :returns "Value"
    :dispatch-on action))

(mcp-module power
  :target "crates/missiond-mcp/src/tools/sysinfra/power.rs"

  (tool mission_power_control
    :description "物理服务器电源管控。wake(WoL/gcloud)/suspend/status"
    (input
      (target string :required :description "服务器 ID(如 win-3090ti/ecs/gcp-prod)")
      (action string :required :enum (wake suspend status)))
    :returns "Value"))

(mcp-module system
  :target "crates/missiond-mcp/src/tools/sysinfra/system.rs"

  (tool mission_sys_logs
    :description "读取 MissionD daemon 运行日志尾部"
    (input
      (lines integer :description "返回尾部行数(默认 50, 最大 500)")
      (level string :enum (all warn error) :default "all" :description "日志级别过滤")
      (grep  string :description "关键词过滤(大小写不敏感)"))
    :returns "Value")

  (tool mission_sys_config
    :description "结构化配置管理。读取/修改 slots.yaml/llm.yaml/permissions.yaml"
    (input
      (action string :enum (get patch list) :default "get")
      (file   string :enum (slots.yaml llm.yaml permissions.yaml) :description "[get/patch] 配置文件")
      (path   string :description "[patch] JSON Pointer(如 /slots/0/description)")
      (value  object :description "[patch] 新值(任意 JSON)"))
    :returns "Value"
    :dispatch-on action)

  (tool mission_daemon_update
    :description "一键更新 MissionD daemon。cargo build → 替换二进制 → 重启"
    (input
      (skip_build boolean :default false :description "跳过 cargo build 直接重启"))
    :returns "Value"))
