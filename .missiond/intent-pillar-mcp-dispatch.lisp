;; MissionD — Pillar: mcp-dispatch
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar mcp-dispatch
    (purpose "JSON-RPC stdio server → handler dispatch across 4 domains")

    (component mcp-server
      :target "crates/missiond-mcp/src/bin/mission-mcp.rs"
      (gateway-impl :target "crates/missiond-mcp/src/gateway_impl.rs")
      (gen-gateway  :target "crates/missiond-mcp/src/gen_gateway.rs"))

    (component compute-tools
      :target "crates/missiond-mcp/src/tools/compute/"
      (tool mission_pty_spawn      :handler "handlers::compute::pty")
      (tool mission_pty_send       :handler "handlers::compute::pty")
      (tool mission_pty_key        :handler "handlers::compute::pty")
      (tool mission_pty_text       :handler "handlers::compute::pty")
      (tool mission_pty_read       :handler "handlers::compute::pty")
      (tool mission_pty_screenshot :handler "handlers::compute::pty")
      (tool mission_pty_status     :handler "handlers::compute::pty")
      (tool mission_pty_signal     :handler "handlers::compute::pty")
      (tool mission_pty_confirm    :handler "handlers::compute::pty")
      (tool mission_task_submit    :handler "handlers::compute::task")
      (tool mission_task_query     :handler "handlers::compute::task")
      (tool mission_task_cancel    :handler "handlers::compute::task")
      (tool mission_task_delegate  :handler "handlers::compute::task_delegate")
      (tool mission_compute_slot   :handler "handlers::compute::compute_slot")
      (tool mission_slots          :handler "handlers::compute::slot")
      (tool mission_worker         :handler "handlers::compute::worker"
        ;; commit e18d0bf: handle_control 新增 target_type="project" → ControlManager.set_project(id, paused)
        ;; MCP schema enum: ["global","provider","domain","worker","slot_role","project"]
        )
      (tool mission_job_poll       :handler "handlers::compute::job")
      (tool mission_flow_run       :handler "handlers::compute::flow_run"
        :added "49bd316"
        ;; Flow Engine v2 entrypoint — actions: run | list | status
        ;; see (component flow-engine-v2) for full detail
        )
      ;; commit 34167db: Forge Lisp→Rust stamping tools via forge CLI
      (tool mission_forge_build    :handler "handlers::compute::forge"
        :added "34167db"
        :params (project :required dry_run :optional output_dir :optional)
        :returns "{status, exit_code, stdout, stderr, project_id, project_root, command}"
        :doc "shell out `forge build <root>` — ProjectRegistry lookup for project root, FORGE_BIN env override")
      (tool mission_forge_lint     :handler "handlers::compute::forge"
        :added "34167db"
        :params (project :required)
        :returns "{status, exit_code, stdout, stderr, violations_raw, project_id, project_root, command}"
        :doc "shell out `forge lint <root>` — governance lint on intent.lisp; completes xjpcode↔missiond↔forge neural triangle")
      (tool mission_sonnet_process :handler "handlers::compute::process")
      (tool mission_minimax_process :handler "handlers::compute::minimax")
      (tool mission_agent          :handler "handlers::compute::cc_tasks"))

    (component knowledge-tools
      :target "crates/missiond-mcp/src/tools/knowledge/"
      (tool mission_kb_query       :handler "handlers::knowledge::kb")
      (tool mission_kb_mutate      :handler "handlers::knowledge::kb")
      (tool mission_kb_ops         :handler "handlers::knowledge::kb")
      (tool mission_kb_remember    :handler "handlers::knowledge::kb"
        :params-added "project: Option<String> — 所属项目 ID (commit 3c10d21)")
      ;; commit 3c10d21: 批量设置 KB 条目项目归属
      (tool mission_kb_batch_set_project :handler "handlers::knowledge::kb"
        :added "3c10d21"
        :params (assignments :"Vec<{key, project_id?}>")
        :returns "{updated, not_found, total}"
        :doc "一次性分配多条 KB 条目到指定项目; project_id 为空/null 表示清除归属(设为全局)")
      (tool mission_memory         :handler "handlers::knowledge::memory")
      (tool mission_board_query    :handler "handlers::knowledge::board")
      (tool mission_board_create   :handler "handlers::knowledge::board")
      (tool mission_board_update   :handler "handlers::knowledge::board")
      (tool mission_board_delete   :handler "handlers::knowledge::board")
      (tool mission_board_claim    :handler "handlers::knowledge::board")
      (tool mission_board_retry    :handler "handlers::knowledge::board")
      (tool mission_board_note_add :handler "handlers::knowledge::board")
      (tool mission_board_decompose :handler "handlers::knowledge::board")
      (tool mission_cc_query       :handler "handlers::knowledge::cascade")
      (tool mission_cc_swarm       :handler "handlers::knowledge::cascade")
      (tool mission_skill_query    :handler "handlers::knowledge::skill")
      (tool mission_skill_exec     :handler "handlers::knowledge::skill")
      (tool mission_skill_mutate   :handler "handlers::knowledge::skill")
      (tool mission_skill_context  :handler "handlers::knowledge::skill")
      (tool mission_insight        :handler "handlers::knowledge::insight")
      (tool mission_embedding_ops  :handler "handlers::knowledge::kb")
      (tool mission_code_search    :handler "handlers::knowledge::kb")
      (tool mission_universe_graph :handler "handlers::knowledge::cascade")
      (tool mission_cascade_plan   :handler "handlers::knowledge::cascade")
      (tool mission_cascade_trigger :handler "handlers::knowledge::cascade")
      (tool mission_cascade_lint   :handler "handlers::knowledge::cascade")
      ;; commit ec269d7: read project intent.lisp from MCP
      (tool mission_intent         :handler "handlers::knowledge::intent"
        :added "ec269d7"
        :actions (read section summary list)
        :doc "read / query .missiond/intent.lisp for a registered project; fuzzy project lookup (substring match, single match auto-resolves, multiple matches return 'ambiguous' error listing candidates)"
        (action read    :params (project :required section :optional)  :returns "raw lisp content")
        (action section :params (project :required section-name :required) :returns "matching s-expression block")
        (action summary :params (project :required)                    :returns "high-level summary from intent")
        (action list    :params ()                                      :returns "Vec<{project_id, intent_path, exists}>")
        :candidates (".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"))

      ;; P4+P5 (commit 76900d1): mission_project — 项目管理 MCP 工具
      ;; commit 84ac1a6: 新增 init action; list 响应增加 lispFiles/lispCount
      (tool mission_project        :handler "handlers::knowledge::project"
        :actions (list get set_active sync init context memories)
        :doc "init: 一步注册新项目(path→git-remote→intent-scan→upsert+backfill+reload); list: 列出所有项目(附带lispFiles/lispCount); get: 按id获取; set_active: 设置活跃状态; sync: 扫描 ~/.claude/projects/ 自动发现注册; context: 项目全景聚合; memories: 读取 Claude Code 项目记忆"
        (action init
          :params (path :required id :optional slots :optional)
          :steps ("canonicalize path" "derive id from dir name (or param)" "git remote get-url origin → github_url" "scan .missiond/.jarvis/ intent.lisp" "upsert project" "backfill_project_id(path%) + backfill_project_id(claude-encoded-pattern%)" "reload SharedProjectRegistry")
          :returns (id path githubUrl intentPath backfilledConversations status="registered")
          :added "84ac1a6")
        ;; commit 8438a7d: Project Context Aggregator
        (action context
          :params (id :required)
          :added "8438a7d"
          :returns "{project, intent(metadata+pillars+survey_date), github(url+web_url), conversations(stats+recent), memories(dir+count+index+entries), kb(category stats), slots(configured+active)}"
          :sources ("build_intent_summary: read intent.lisp → extract survey-date/last-updated/pillar names, no full load"
                    "build_github_info: git@ → https:// normalization"
                    "conversation_stats_by_project: PG aggregate query"
                    "recent_conversations_by_project: PG latest N"
                    "project_memory::list_memories: filesystem scan + YAML frontmatter parsing"
                    "project_memory::read_memory_index: MEMORY.md content"
                    "kb_stats_by_project: PG category group-by"
                    "build_slots_info: configured slots × pty runtime state"))
        (action memories
          :params (id :required file :optional)
          :added "8438a7d"
          :returns "file given → raw content; file omitted → {dir, count, index, entries:[{file,name,mem_type,description}]}"
          :module "handlers/knowledge/project_memory.rs (130L)"
          :doc "scan ~/.claude/projects/{encoded-path}/memory/*.md, parse YAML frontmatter (name/type/description), path traversal protection (reject / \\ ..)")
        ))

    (component comm-tools
      :target "crates/missiond-mcp/src/tools/comm/"
      (tool mission_conversation_query     :handler "handlers::comm::conversation")
      (tool mission_conversation_analyze   :handler "handlers::comm::conversation")
      (tool mission_conversation_reconcile :handler "handlers::comm::conversation")
      (tool mission_question               :handler "handlers::comm::question")
      (tool mission_router_chat            :handler "handlers::comm::router_chat")
      (tool mission_router_chat_manage     :handler "handlers::comm::router_chat")
      (tool mission_timeline               :handler "handlers::comm::timeline")
      (tool mission_audit                  :handler "handlers::comm::audit")
      (tool mission_retrospective_manage   :handler "handlers::comm::retrospective")
      (tool mission_llm_trace              :handler "handlers::comm::audit")
      (tool mission_decision_stats         :handler "handlers::comm::conversation")
      (tool mission_slot_history           :handler "handlers::comm::timeline")
      (tool mission_beacon                 :handler "handlers::comm::audit")
      ;; commit ec269d7: new Codex CLI integration tool
      (tool mission_codex_ops             :handler "handlers::comm::codex_ops"
        :added "ec269d7"
        :actions (query history stats)
        :source "~/.codex/state_5.sqlite (via CodexIngestionWorker)"
        :doc "query Codex CLI operation history ingested into MissionD conversation timeline"))

    (component sysinfra-tools
      :target "crates/missiond-mcp/src/tools/sysinfra/"
      (tool mission_control          :handler "handlers::sysinfra::system")
      (tool mission_daemon_update    :handler "handlers::sysinfra::system")
      (tool mission_sys_config       :handler "handlers::sysinfra::system")
      (tool mission_sys_logs         :handler "handlers::sysinfra::system")
      (tool mission_infra_ops        :handler "handlers::sysinfra::infra")
      (tool mission_infra_query      :handler "handlers::sysinfra::infra")
      (tool mission_permission_query :handler "handlers::sysinfra::permission")
      (tool mission_permission_mutate :handler "handlers::sysinfra::permission")
      (tool mission_power_control    :handler "handlers::sysinfra::power")
      (tool mission_pause            :handler "handlers::sysinfra::misc")
      (tool mission_inbox            :handler "handlers::sysinfra::misc")
      (tool mission_incident         :handler "handlers::sysinfra::misc")
      (tool mission_submit_phase_result :handler "handlers::sysinfra::misc")
      (tool mission_gemini_auth      :handler "handlers::sysinfra::misc")))
