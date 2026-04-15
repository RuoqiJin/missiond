;; MissionD — Pillar: llm-context
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar llm-context
    (purpose "multi-provider LLM dispatch + budget-constrained prompt assembly")

    (component llm-gateway
      :target "crates/missiond-daemon/src/llm/llm_gateway.rs"
      (providers
        (gemini-gateway  :target "crates/missiond-daemon/src/llm/gemini_client.rs")
        (sonnet-gateway  :target "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
          ;; commit 43c80f4: model name cpapi-claude-sonnet → claude-sonnet (SONNET_MODEL const + llm_gateway.rs)
          )
        (minimax-gateway :target "crates/missiond-daemon/src/llm/minimax_gateway.rs")))

    (component llm-gate
      :target "crates/missiond-daemon/src/llm/llm_gate.rs"
      (description "rate limiter + 429 backoff + quota tracking")
      ;; commit 43c80f4: 新增 check_interactive_exempt(provider) — 用 REQUEST_CALLER task-local 判断
      ;;   - REQUEST_CALLER == "router_chat" → is_interactive_caller() = true → 绕过 gate
      ;;   - 语义：用户发起的 MCP 调用(router_chat 等)豁免 gate 限制；后台 worker 仍受 gate 约束
      ;;   - gemini_client.rs 的 check() 已改为调用 check_interactive_exempt()
      )

    (component gemini-driver  :target "crates/missiond-daemon/src/llm/gemini_driver.rs")
    (component gemini-file-api :target "crates/missiond-daemon/src/llm/gemini_file_api.rs")
    (component gemini-cli     :target "crates/missiond-daemon/src/llm/gemini_cli.rs")
    (component codex-cli      :target "crates/missiond-daemon/src/llm/codex_cli.rs")
    (component prompts        :target "crates/missiond-daemon/src/llm/prompts.rs")

    (component context-pipeline
      :target "crates/missiond-daemon/src/context/context_pipeline.rs"
      (source-priority
        slot-env -> skill-context -> kb-entries -> conversation-history
        -> topology-map -> claude-md)
      (depends (db slot_manager)))

    (component context-budget  :target "crates/missiond-daemon/src/context/context_budget.rs")
    (component slot-env        :target "crates/missiond-daemon/src/context/slot_env.rs")
    (component topology-map    :target "crates/missiond-daemon/src/context/topology_map.rs")
    (component claude-md-sync  :target "crates/missiond-daemon/src/context/claude_md_sync.rs"))

