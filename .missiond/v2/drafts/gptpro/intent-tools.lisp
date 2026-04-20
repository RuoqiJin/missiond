;; ══════════════════════════════════════════════════════
;; MissionD — Tools Pillar (Detailed Draft)
;; 目标: 把 67 个 MCP 工具按“入点 / 核心 / 出点”重新整理
;; ══════════════════════════════════════════════════════

(pillar tools
  :status "草稿 v0.1 — 以 mcp-dispatch + mcp-defs 为代码真相"
  :actual-state-sources ["intent-pillar-mcp-dispatch.lisp" "intent-mcp-defs.lisp"]
  :secondary-sources    ["intent-rpc-gateway.lisp" "v2/intent.lisp :: pillar tools"]

  (purpose "通过 MCP JSON-RPC 向 Claude Code / 其他 agent / 前端暴露能力")

  (pillar-ingress
    (entry-1 "stdio JSON-RPC (MCP 协议)")
    (entry-2 "IPC 间接调用 (daemon 内部转发)")
    (entry-3 "tool schema 查询 / 工具执行 / 初始化握手"))

  (pillar-core
    (core-1 "tool schema: 67 个工具的输入输出声明")
    (core-2 "dispatch: tool_name → handler module → action 分派")
    (core-3 "四大运行时域: compute / knowledge / comm / sysinfra")
    (core-4 "tool-call-log: 所有工具调用的审计轨迹"))

  (pillar-egress
    (egress-1 "返回 JSON-RPC result/error")
    (egress-2 "驱动 worker / memory / event-bus / system-layer")
    (egress-3 "写入 tool call log / 审计表"))

  (tool-catalog
    :count 67
    (runtime-domains
      (compute   "PTY / task / worker / slot / job / flow / forge / process")
      (knowledge "KB / board / skill / memory / insight / project / intent / cascade")
      (comm      "conversation / question / router_chat / timeline / audit / retrospective / codex_ops")
      (sysinfra  "system / infra / permission / power / pause / inbox / incident / gemini_auth")))

  ;; ─────────────────────────────────────────────────────
  ;; 通用调用主路径
  ;; ─────────────────────────────────────────────────────
  (path mcp-tool-call
    (ingress
      :source "MCP client 发来的 initialize / tools/list / tools/call / ping"
      :entry-components ["missiond-mcp stdio server" "gen_server / gateway_impl"])
    (logic-core
      (step s1 "解析 JSON-RPC envelope")
      (step s2 "tools/call 时按 tool_name 找 schema 与 handler")
      (step s3 "做参数校验 / action 分派 / 默认值补齐")
      (step s4 "通过 mcp_client / ipc / 直接 handler 调用进入 daemon")
      (step s5 "收集返回文本 / 结构化 JSON / error code, 回包给客户端"))
    (egress
      :returns "JSON-RPC result / error"
      :writes  "tool call audit"))

  ;; ─────────────────────────────────────────────────────
  ;; compute 域
  ;; ─────────────────────────────────────────────────────
  (domain compute
    :targets ["crates/missiond-mcp/src/tools/compute/"
              "daemon/src/handlers/compute/"]

    (path pty-control
      (ingress
        :tools ["mission_pty_spawn" "mission_pty_send" "mission_pty_read" "mission_pty_screenshot" "mission_pty_status" "mission_pty_signal" "mission_pty_confirm"])
      (logic-core
        (step s1 "handler::compute::pty 解析 action")
        (step s2 "调用 PTYManager / session")
        (step s3 "必要时桥接 semantic parser / screenshot / signal / confirm")
        (step s4 "收集终端状态或输出片段")
        (step s5 "回包给工具调用方"))
      (egress
        :returns "PTY control result"
        :side-effects "可能触发 SlotEvent / SessionEvent"))

    (path task-api
      (ingress
        :tools ["mission_task_submit" "mission_task_query" "mission_task_cancel" "mission_task_delegate"])
      (logic-core
        (step s1 "handlers::compute::task 或 task_delegate 解析用户意图")
        (step s2 "进入 tasks / board_tasks / delegate 流程的当前实现")
        (step s3 "必要时交给 slot / worker / supervisor")
        (step s4 "跟踪状态、ack、cancel 或 delegated result")
        (step s5 "返回 task status / tracking payload"))
      (egress
        :returns "task lifecycle result"
        :writes  "tasks 或 board_tasks, 取决于具体实现路径"))

    (path worker-and-slot-control
      (ingress
        :tools ["mission_worker" "mission_compute_slot" "mission_slots" "mission_job_poll"])
      (logic-core
        (step s1 "mission_worker 操作 control-tree: global/provider/domain/worker/slot_role/project")
        (step s2 "mission_compute_slot 操作 dynamic slot lifecycle")
        (step s3 "mission_slots / mission_job_poll 读取 slot 与 job runtime")
        (step s4 "返回 pause/resume 状态、slot 信息或 job 进度"))
      (egress
        :returns "runtime control summary"
        :writes  "control_tree / dynamic_slots / slot runtime state"))

    (path flow-and-forge
      (ingress
        :tools ["mission_flow_run" "mission_forge_build" "mission_forge_lint"])
      (logic-core
        (step s1 "flow_run 进入 flow-engine-v2 的 run/list/status")
        (step s2 "forge_* 通过 ProjectRegistry 定位项目根路径")
        (step s3 "调用 flow engine 或外部 forge CLI")
        (step s4 "把执行 stdout/stderr/status 整理成统一返回")
        (step s5 "必要时触发后续 survey / worker / board 路径"))
      (egress
        :returns "flow status / forge result")))

  ;; ─────────────────────────────────────────────────────
  ;; knowledge 域
  ;; ─────────────────────────────────────────────────────
  (domain knowledge
    :targets ["crates/missiond-mcp/src/tools/knowledge/"
              "daemon/src/handlers/knowledge/"]

    (path kb-query-and-mutate
      (ingress
        :tools ["mission_kb_query" "mission_kb_remember" "mission_kb_mutate" "mission_kb_ops" "mission_kb_batch_set_project" "mission_embedding_ops" "mission_code_search"])
      (logic-core
        (step s1 "handlers::knowledge::kb 解析 action")
        (step s2 "进入查询 / remember / mutate / maintenance / import / scoped search")
        (step s3 "必要时触发 embedding、project scoping、batch update")
        (step s4 "聚合结果或运维报告")
        (step s5 "返回给客户端"))
      (egress
        :returns "KB result / ops report"
        :writes  "knowledge / embeddings / project scope / queue status"))

    (path board-lifecycle-tools
      (ingress
        :tools ["mission_board_query" "mission_board_create" "mission_board_update" "mission_board_delete" "mission_board_claim" "mission_board_retry" "mission_board_note_add" "mission_board_decompose"])
      (logic-core
        (step s1 "handlers::knowledge::board 根据 action 进入 board store")
        (step s2 "创建 / 修改 / claim / retry / note / decompose")
        (step s3 "必要时与 autopilot / flow-engine / slot dispatch 协同")
        (step s4 "写入 board_tasks / notes / question side effects")
        (step s5 "返回 board 结果或 DAG 结果"))
      (egress
        :returns "board operation result"
        :writes  "board 模块表"
        :emits   "BoardEvent"))

    (path skill-and-memory-recall
      (ingress
        :tools ["mission_skill_query" "mission_skill_exec" "mission_skill_mutate" "mission_skill_context" "mission_memory" "mission_insight"])
      (logic-core
        (step s1 "skill handler 做 list/search/topics/actions/stats/build/resolve")
        (step s2 "memory / insight handler 做综合检索与上下文召回")
        (step s3 "必要时调用 hybrid retrieval / skill dependency resolution")
        (step s4 "skill_exec 可顺序执行 workflow steps")
        (step s5 "返回 skill context 或综合洞察"))
      (egress
        :returns "skill context / memory insight"
        :writes  "skill versions / executions / 部分 memory side effects"))

    (path project-and-intent-access
      (ingress
        :tools ["mission_project" "mission_intent" "mission_cc_query" "mission_cc_swarm" "mission_universe_graph" "mission_cascade_plan" "mission_cascade_trigger" "mission_cascade_lint"])
      (logic-core
        (step s1 "mission_project 处理 list/get/set_active/sync/init/context/memories")
        (step s2 "mission_intent 读取 per-project intent.lisp, 支持 read/section/summary/list")
        (step s3 "cascade / universe_graph 组装跨项目或多 agent 任务")
        (step s4 "project-init 走 path canonicalize → git remote → intent scan → upsert → backfill → reload")
        (step s5 "返回项目上下文 / intent 片段 / cascade plan"))
      (egress
        :returns "project context / intent content / cascade result"
        :writes  "projects / backfill / project memory references")))

  ;; ─────────────────────────────────────────────────────
  ;; comm 域
  ;; ─────────────────────────────────────────────────────
  (domain comm
    :targets ["crates/missiond-mcp/src/tools/comm/"
              "daemon/src/handlers/comm/"]

    (path conversation-and-question
      (ingress
        :tools ["mission_conversation_query" "mission_conversation_analyze" "mission_conversation_reconcile" "mission_decision_stats" "mission_question"])
      (logic-core
        (step s1 "conversation handler 做 list/get/search/analyze/reconcile")
        (step s2 "question handler 处理提问、回答、状态流转")
        (step s3 "必要时联动 board blocked/open 逻辑")
        (step s4 "聚合统计与上下文片段")
        (step s5 "返回 conversation / question 结果"))
      (egress
        :returns "conversation view / question status"
        :writes  "conversation_logs / agent_questions / board side effects"))

    (path router-chat-timeline-audit
      (ingress
        :tools ["mission_router_chat" "mission_router_chat_manage" "mission_timeline" "mission_audit" "mission_llm_trace" "mission_retrospective_manage" "mission_slot_history" "mission_beacon" "mission_codex_ops"])
      (logic-core
        (step s1 "router_chat handler 读写 router 会话与归档")
        (step s2 "timeline / audit / llm_trace / retrospective 从 observability 与 conversation 数据取视图")
        (step s3 "slot_history 返回 slot task 历史")
        (step s4 "beacon / codex_ops 分别读代码信标与 Codex ingest 结果")
        (step s5 "统一整理为 comm 类输出"))
      (egress
        :returns "comm observability / archive / history views")))

  ;; ─────────────────────────────────────────────────────
  ;; sysinfra 域
  ;; ─────────────────────────────────────────────────────
  (domain sysinfra
    :targets ["crates/missiond-mcp/src/tools/sysinfra/"
              "daemon/src/handlers/sysinfra/"]

    (path system-control
      (ingress
        :tools ["mission_control" "mission_daemon_update" "mission_sys_config" "mission_sys_logs" "mission_pause"])
      (logic-core
        (step s1 "进入 system / misc handler")
        (step s2 "读取或修改 daemon 配置与 pause 状态")
        (step s3 "收集日志 / 配置 / 系统状态")
        (step s4 "把结果返回给客户端"))
      (egress
        :returns "daemon/system summary"
        :writes  "config / control tree / logs"))

    (path infra-permission-power
      (ingress
        :tools ["mission_infra_ops" "mission_infra_query" "mission_permission_query" "mission_permission_mutate" "mission_power_control" "mission_incident" "mission_inbox" "mission_submit_phase_result" "mission_gemini_auth"])
      (logic-core
        (step s1 "infra handler 处理 discovery / query / ops")
        (step s2 "permission handler 查询或修改 learned permissions")
        (step s3 "power / incident / inbox / auth 等走 misc 或专门 handler")
        (step s4 "必要时联动 AIOps / observability / global state")
        (step s5 "返回 infra / permission / incident 结果"))
      (egress
        :returns "sysinfra result"
        :writes  "incidents / inbox / permissions / infra state")))

  (tooling-governance
    (schema-source-of-truth "intent-mcp-defs.lisp")
    (runtime-dispatch-source-of-truth "intent-pillar-mcp-dispatch.lisp")
    (principle "schema 与 handler 要分开看: schema 管暴露面, dispatch 管真实执行入口"))
)
