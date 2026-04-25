;; ══════════════════════════════════════════════════════
;; MissionD v2 — Pillar Source Index / 真实状态索引
;; 用途: 判断“哪个旧地图文件更能代表代码真实状态”
;; 结论: memory / event-bus 以新架构专档为准; 其余 pillar 以旧地图压缩包为主,
;;       再用 v2/intent.lisp 做命名与设计哲学校正。
;; ══════════════════════════════════════════════════════

(source-of-truth missiond-v2
  (authority-order
    (rule-1
      :scope  "memory / event-bus"
      :use    ["misonD 正在重新设计的新的架构图.zip"]
      :files  ["intent-memory.lisp"
               "intent-memory-history.lisp"
               "intent-memory-execution.lisp"
               "intent-event-bus.lisp"
               "intent-event-bus-execution.lisp"]
      :reason "这两个 pillar 已进入 v2 专档时代, 旧地图只保留历史切面价值")

    (rule-2
      :scope  "worker / tools / system-layer / flow"
      :use    ["missiond 旧的地图.zip"]
      :files  ["intent-pillar-event-workers.lisp"
               "intent-pillar-llm-context.lisp"
               "intent-pillar-engines.lisp"
               "intent-pillar-mcp-dispatch.lisp"
               "intent-pillar-transport-bootstrap.lisp"
               "intent-pillar-semantic-parser.lisp"
               "intent-pillar-state-machines.lisp"
               "intent-flows.lisp"
               "intent-types.lisp"
               "intent-rpc-gateway.lisp"
               "intent-pure-utility.lisp"
               "intent-mcp-defs.lisp"]
      :reason "这些文件仍最接近代码现状的物理切面")

    (rule-3
      :scope  "intent-layer"
      :use    ["misonD 正在重新设计的新的架构图.zip::intent.lisp"
               "missiond 旧的地图.zip::全部 intent*.lisp 本身"]
      :reason "intent-layer 本来就是对这些 lisp 源文件本身的治理, 不存在单一旧 pillar 文件")

    (rule-4
      :scope  "命名 / pillar 总纲 / 设计哲学"
      :use    ["misonD 正在重新设计的新的架构图.zip::intent.lisp"]
      :reason "v2 七大 pillar 的命名、边界和哲学都在这里定稿, 但它更偏导航, 不是最细的代码实况图")

    (rule-5
      :scope  "执行期真实发现 / 施工期偏差"
      :use    ["*-execution.lisp" "*-history.lisp"]
      :reason "真实 drift、阶段决策、未决问题, 这些信息不在 frozen 设计正文里, 只在 execution/history 里最真实"))

  (old-to-new-crosswalk
    (memory
      :new-pillar "pillar memory"
      :old-files   ["intent-pillar-db-core.lisp"
                    "intent-pillar-db-agents.lisp"
                    "intent-pillar-db-observability.lisp"
                    "intent-pillar-db-pipeline.lisp"
                    "intent-db-*.lisp"]
      :note "这些旧文件现在主要作为 memory v2 各 module 的来源切面")

    (worker
      :new-pillar "pillar worker"
      :old-files   ["intent-pillar-event-workers.lisp"
                    "intent-pillar-llm-context.lisp"
                    "intent-pillar-engines.lisp"
                    "intent-pillar-semantic-parser.lisp"
                    "intent-domain-engines.lisp"]
      :note "worker 的真实代码状态主要埋在旧的 event-workers / llm-context / engines 三份里")

    (tools
      :new-pillar "pillar tools"
      :old-files   ["intent-pillar-mcp-dispatch.lisp" "intent-mcp-defs.lisp"]
      :note "dispatch 给运行时入口; defs 给 schema 真相")

    (intent-layer
      :new-pillar "pillar intent-layer"
      :old-files   ["intent.lisp" "intent-pillar-*.lisp" "intent-db-*.lisp" "intent-mcp-defs.lisp"]
      :note "intent-layer 的对象就是这些 lisp 源文件本身")

    (system-layer
      :new-pillar "pillar system-layer"
      :old-files   ["intent-types.lisp"
                    "intent-rpc-gateway.lisp"
                    "intent-pillar-transport-bootstrap.lisp"
                    "intent-pillar-state-machines.lisp"
                    "intent-pure-utility.lisp"]
      :note "运行时底座在旧地图里是分散的, 需要二次重整")

    (flow
      :new-pillar "pillar flow"
      :old-files   ["intent-flows.lisp" "intent-pillar-engines.lisp"]
      :note "flow 旧图有 end-to-end 名单, engines 旧图有 autopilot / flow-v2 的执行细节"))

  (pillar-sources
    (pillar worker
      :primary-code-sources
        ["intent-pillar-event-workers.lisp"
         "intent-pillar-llm-context.lisp"
         "intent-pillar-engines.lisp"]
      :secondary-sources
        ["intent-pillar-semantic-parser.lisp"
         "intent-domain-engines.lisp"
         "intent-pillar-transport-bootstrap.lisp"]
      :design-corrections-from-v2
        ["intent.lisp :: pillar worker"
         "intent-memory-execution.lisp :: D006 (worker footprint drift 提醒)"]
      :judgement "旧地图最像代码实况; v2 intent.lisp 用来纠正命名和理想边界")

    (pillar tools
      :primary-code-sources
        ["intent-pillar-mcp-dispatch.lisp" "intent-mcp-defs.lisp"]
      :secondary-sources
        ["intent-rpc-gateway.lisp" "intent.lisp :: pillar tools"]
      :judgement "mcp-dispatch=handler 入口真相, mcp-defs=schema 真相")

    (pillar intent-layer
      :primary-code-sources
        ["intent.lisp :: pillar intent-layer"
         "全量 intent*.lisp 文件集合"]
      :secondary-sources
        ["intent-pillar-event-workers.lisp :: lisp-survey-worker"
         "intent-pillar-mcp-dispatch.lisp :: mission_intent / mission_forge_*"
         "intent-flows.lisp :: project-init / project-context 相关 flow"]
      :judgement "这里没有单一旧图; 需要从 v2 总纲 + lisp 文件生态反推")

    (pillar system-layer
      :primary-code-sources
        ["intent-types.lisp"
         "intent-rpc-gateway.lisp"
         "intent-pillar-transport-bootstrap.lisp"]
      :secondary-sources
        ["intent-pillar-state-machines.lisp" "intent-pure-utility.lisp"]
      :judgement "system-layer 在旧图里是分裂存在的, 必须重新折叠成一个 pillar")

    (pillar flow
      :primary-code-sources ["intent-flows.lisp"]
      :secondary-sources    ["intent-pillar-engines.lisp" "intent.lisp :: pillar flow"]
      :judgement "旧 flows 给场景目录, 新 flow pillar 给编排哲学和 board-centric 结构"))

  (operational-findings
    (finding-1
      :from "intent-event-bus-execution.lisp"
      :effect "execution-log 已证明可作为多 agent 协作共享内存层")
    (finding-2
      :from "intent-memory-execution.lisp"
      :effect "execution-log 模式已复制到第二个 pillar, 不再只是 event-bus 特例")
    (finding-3
      :from "intent-memory-execution.lisp"
      :effect "出现重复 D010, 说明共享执行层必须引入正式 ID allocator 与 claim lease"))

  ;; ──────────────────────────────────────────────────
  ;; Part 2 · v2 Stable Section-ID Registry (v0.2 新增)
  ;; ──────────────────────────────────────────────────
  ;; 目的:
  ;;   - 在主 Lisp 真正压缩/拆分前, 把每个 pillar 的关键 section
  ;;     绑定到一个 stable section-id, 后续无论标题/行号怎么改,
  ;;     cross-ref 不丢
  ;;   - status 落在 architecture-dsl.lisp status-taxonomy 7 值
  ;;   - implements 是仓库根起的相对路径列表
  ;; 命名: "<pillar>.<kind>.<local>" kebab-case 全小写
  ;; 当前完成度:
  ;;   - 7 pillar 全覆盖 (主大节点)
  ;;   - 子 module / section 覆盖度按"压缩前必读"挑选, 后续可扩
  ;; ──────────────────────────────────────────────────
  (source-index v2
    :scope "missiond-v2"
    :version "v0.3 — section-id baseline + precompression coverage expansion 2026-04-26"
    :status-taxonomy-ref "architecture-dsl.lisp :: status-taxonomy"
    :section-id-policy-ref "architecture-dsl.lisp :: section-id-policy"
    :section-entry-extended-ref "architecture-dsl.lisp :: section-entry-extended (wave 12 task 06)"
    :compression-safe-field-policy "true=可走 compression-policy.allowed; false=保护正文 (frozen / control-plane / contract-zone); 缺省视为 unknown, 默认按 false 处理"

    ;; ── pillar memory ──
    (pillar-section-index
      :pillar memory
      :source-file ".missiond/v2/intent-memory.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar memory"
      :execution-log ".missiond/v2/intent-memory-execution.lisp"
      :status code-aligned-partial

      (section-entry
        :section-id "memory.pillar-root"
        :title "pillar memory (root)"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory"
        :status code-aligned-partial
        :owns-tables 56
        :note "9 module + cross-cutting + pillar-interfaces; v0.5.6")

      (section-entry
        :section-id "memory.module.project-management"
        :title "module project-management"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module project-management"
        :status code-aligned-partial
        :implements
          ["crates/missiond-core/src/db/pg/project.rs"
           "crates/missiond-daemon/src/handlers/knowledge/intent.rs"]
        :owns-tables 5)

      (section-entry
        :section-id "memory.module.board"
        :title "module board"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module board"
        :status code-aligned
        :implements
          ["crates/missiond-core/src/db/board.rs"]
        :owns-tables 4
        :owns-tools ["mission_board_*" "mission_question"])

      (section-entry
        :section-id "memory.module.kb-manager"
        :title "module kb-manager"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module kb-manager"
        :status code-aligned-partial
        :implements
          ["crates/missiond-core/src/db/knowledge.rs"]
        :owns-tables 9)

      (section-entry
        :section-id "memory.module.conversation-logs"
        :title "module conversation-logs"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module conversation-logs"
        :status code-aligned-partial
        :implements
          ["crates/missiond-core/src/db/conversation.rs"]
        :owns-tables 15)

      (section-entry
        :section-id "memory.module.directive-layer"
        :title "module directive-layer"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module directive-layer"
        :status code-aligned-partial
        :owns-tables 3
        :note "v0.4.19 directive 表 rename; store+manager code-aligned partial; actor pending")

      (section-entry
        :section-id "memory.module.llm-support"
        :title "module llm-support"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module llm-support"
        :status code-aligned-partial
        :owns-tables 3)

      (section-entry
        :section-id "memory.module.slot-support"
        :title "module slot-support"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module slot-support"
        :status code-aligned-partial
        :owns-tables 3)

      (section-entry
        :section-id "memory.module.system-support"
        :title "module system-support"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module system-support"
        :status code-aligned-partial
        :owns-tables 14)

      (section-entry
        :section-id "memory.module.embedding-support"
        :title "module embedding-support (column-ownership)"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module embedding-support"
        :status architecture-designed
        :owns-tables 0
        :note "管 5 承载表 + 1 audit 表的 embedding 列契约, 不独占行"))

    ;; ── pillar worker ──
    (pillar-section-index
      :pillar worker
      :source-file ".missiond/v2/intent-worker.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar worker"
      :execution-log ".missiond/v2/worker-pillar-execution.lisp"
      :status code-aligned-partial

      (section-entry
        :section-id "worker.pillar-root"
        :title "pillar worker (root)"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker"
        :status code-aligned-partial
        :note "v0.5 phase-C — 9 section recursive contract")

      (section-entry
        :section-id "worker.section.pty"
        :title "section pty (PTY transport)"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section pty"
        :status code-aligned
        :implements
          ["crates/missiond-pty/src/manager.rs"
           "crates/missiond-pty/src/session.rs"
           "crates/missiond-daemon/src/workers/local/pty_event_worker.rs"
           "crates/missiond-daemon/src/slot_manager/"
           "crates/missiond-daemon/src/slot_orchestrator/"])

      (section-entry
        :section-id "worker.section.llm-gateways"
        :title "section llm-gateways"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section llm-gateways"
        :status code-aligned-partial
        :implements
          ["crates/missiond-daemon/src/llm/llm_gateway.rs"
           "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
           "crates/missiond-daemon/src/llm/xjp_router_client.rs"
           "crates/missiond-daemon/src/llm/gemini_driver.rs"
           "crates/missiond-daemon/src/llm/codex_cli.rs"])

      (section-entry
        :section-id "worker.section.xjp-router-gateway"
        :title "section xjp-router-gateway"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section xjp-router-gateway"
        :status code-aligned-partial
        :implements ["crates/missiond-daemon/src/llm/xjp_router_client.rs"]
        :note "qwen3 唯一 embedding provider, 禁降级")

      (section-entry
        :section-id "worker.section.context-assembly"
        :title "section context-assembly"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section context-assembly"
        :status code-aligned
        :implements ["crates/missiond-daemon/src/context/"])

      (section-entry
        :section-id "worker.section.workers"
        :title "section workers (19 后台 worker)"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section workers"
        :status code-aligned
        :implements
          ["crates/missiond-daemon/src/workers/sonnet/"
           "crates/missiond-daemon/src/workers/codex/"
           "crates/missiond-daemon/src/workers/gemini/"
           "crates/missiond-daemon/src/workers/local/"])

      (section-entry
        :section-id "worker.section.engine-cluster"
        :title "section engine-cluster (autopilot + flow-engine)"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section engine-cluster"
        :status code-aligned
        :implements
          ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
           "crates/missiond-daemon/src/engine/flow/"])

      (section-entry
        :section-id "worker.section.orchestration-governance"
        :title "section orchestration-governance"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section orchestration-governance"
        :status code-aligned-partial
        :implements
          ["crates/missiond-daemon/src/workers/registry.rs"
           "crates/missiond-daemon/src/control_tree.rs"])

      (section-entry
        :section-id "worker.section.claudecode-workstation-orchestration"
        :title "section claudecode-workstation-orchestration"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section claudecode-workstation-orchestration"
        :status operational-practice
        :note "policy / dispatch_strategy companion log already aligned; full ExecutionEvent metadata pending")

      (section-entry
        :section-id "worker.section.worker-side-computation"
        :title "section worker-side-computation"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section worker-side-computation"
        :status code-aligned-partial))

    ;; ── pillar tools ──
    (pillar-section-index
      :pillar tools
      :source-file ".missiond/v2/intent-tools.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar tools"
      :status code-aligned-partial

      (section-entry
        :section-id "tools.pillar-root"
        :title "pillar tools (root)"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools"
        :status code-aligned-partial
        :owns-tools ["83 actual MCP tools — 详见 intent-mcp-defs.lisp"])

      (section-entry
        :section-id "tools.section.rpc-gateway"
        :title "section rpc-gateway (mcp transport + dispatch)"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section rpc-gateway"
        :status code-aligned
        :implements
          ["crates/missiond-mcp/"
           "crates/missiond-daemon/src/infra/mcp_client.rs"
           ".missiond/intent-mcp-defs.lisp"])

      (section-entry
        :section-id "tools.section.mcp-surface-lifecycle"
        :title "section mcp-surface-lifecycle"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle"
        :status code-aligned-partial
        :note "directive/plan/workflow + execution + capability_usage + global_instruction surfaces 已 code-aligned partial")

      (section-entry
        :section-id "tools.section.tool-governance"
        :title "section tool-governance"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section tool-governance"
        :status architecture-designed
        :note "no new tool — mission_message/mission_invoke remain future-candidate"))

    ;; ── pillar event-bus (PROTECTED, only metadata entries allowed) ──
    (pillar-section-index
      :pillar event-bus
      :source-file ".missiond/v2/intent-event-bus.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar event-bus"
      :execution-log ".missiond/v2/intent-event-bus-execution.lisp"
      :status protected
      :protection-reason "file-governance lock=architecture-unlocked-but-record-required; 本 task 不允许改正文, 只允许在本 index 加 :section-id 元数据条目"

      (section-entry
        :section-id "event-bus.pillar-root"
        :title "pillar event-bus (root, frozen design)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus"
        :status protected
        :note "v1.3.4 — 13 domain enum, code-aligned for ExecutionEvent + CapabilityUsage ObservabilityEvent")

      (section-entry
        :section-id "event-bus.section.ingress"
        :title "section ingress (log.append)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section ingress"
        :status protected
        :implements
          ["crates/missiond-core/src/event/log/mod.rs"
           "crates/missiond-core/src/event/pipeline/step3_commit/handle.rs"])

      (section-entry
        :section-id "event-bus.section.core"
        :title "section core (7-step pipeline)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section core"
        :status protected
        :implements
          ["crates/missiond-core/src/event/pipeline/step1_guard/"
           "crates/missiond-core/src/event/pipeline/step2_decide/"
           "crates/missiond-core/src/event/pipeline/step3_commit/"
           "crates/missiond-core/src/event/pipeline/step4_ack/"
           "crates/missiond-core/src/event/pipeline/step5_tail/"
           "crates/missiond-core/src/event/pipeline/step6_gate/"
           "crates/missiond-core/src/event/pipeline/step7_fanout/"])

      (section-entry
        :section-id "event-bus.section.egress"
        :title "section egress (subscription/cursor)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section egress"
        :status protected
        :implements ["crates/missiond-core/src/event/subscription/"])

      (section-entry
        :section-id "event-bus.section.cross-cutting"
        :title "section cross-cutting (causation / metrics / chaos)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section cross-cutting"
        :status protected)

      (section-entry
        :section-id "event-bus.section.deferred"
        :title "section deferred (FreezeAndCatchUp / Prometheus)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section deferred"
        :status architecture-designed)

      (section-entry
        :section-id "event-bus.section.persistence-layer"
        :title "section persistence-layer (4 表)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section persistence-layer"
        :status protected
        :implements
          ["crates/missiond-core/migrations/20260419000000_event_log.sql"]
        :owns-tables 4))

    ;; ── pillar intent-layer ──
    (pillar-section-index
      :pillar intent-layer
      :source-file ".missiond/v2/intent-intent-layer.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar intent-layer"
      :status code-aligned-partial

      (section-entry
        :section-id "intent-layer.pillar-root"
        :title "pillar intent-layer (root)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer"
        :status code-aligned-partial
        :note "v0.4 phase-B — unified-entry-pipeline actor v0 已 code-aligned partial")

      (section-entry
        :section-id "intent-layer.section.self-description-files"
        :title "section self-description-files"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section self-description-files"
        :status code-aligned)

      (section-entry
        :section-id "intent-layer.section.forge-compilation"
        :title "section forge-compilation"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section forge-compilation"
        :status architecture-designed
        :note "外部仓库 ~/Projects/jarvis-forge")

      (section-entry
        :section-id "intent-layer.section.global-claudemd"
        :title "section global-claudemd"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section global-claudemd"
        :status code-aligned
        :implements
          ["crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
           "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"])

      (section-entry
        :section-id "intent-layer.section.action-instruction-specs"
        :title "section action-instruction-specs"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section action-instruction-specs"
        :status code-aligned-partial
        :note "schema 实际归 memory directive-layer; 本 section 概念性 cross-ref")

      (section-entry
        :section-id "intent-layer.section.workflows"
        :title "section workflows"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section workflows"
        :status code-aligned-partial)

      (section-entry
        :section-id "intent-layer.section.unified-entry-pipeline"
        :title "section unified-entry-pipeline"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline"
        :status code-aligned-partial
        :note "directive-compiler v0 / plan-compiler v0 / plan-runner v0 / workflow-distiller v0 / methodology compiler v0")

      (section-entry
        :section-id "intent-layer.section.capability-evolution-governance"
        :title "section capability-evolution-governance"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section capability-evolution-governance"
        :status code-aligned-partial
        :note "semantic evidence v1: 5 sources + lisp hint merge-candidate")

      (section-entry
        :section-id "intent-layer.section.lisp-survey-dual-owned"
        :title "section lisp-survey-dual-owned"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section lisp-survey-dual-owned"
        :status code-aligned
        :implements ["crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"])

      (section-entry
        :section-id "intent-layer.section.arch-maintenance-dual-owned"
        :title "section arch-maintenance-dual-owned"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section arch-maintenance-dual-owned"
        :status code-aligned-partial)

      (section-entry
        :section-id "intent-layer.section.learning-engine"
        :title "section learning-engine"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section learning-engine"
        :status code-aligned-partial)

      (section-entry
        :section-id "intent-layer.section.flow-engine-v1-project-lifecycle"
        :title "section flow-engine-v1-project-lifecycle"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section flow-engine-v1-project-lifecycle"
        :status code-aligned-partial)

      (section-entry
        :section-id "intent-layer.section.action-instruction-actor"
        :title "section action-instruction-actor"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor"
        :status code-aligned-partial)

      (section-entry
        :section-id "intent-layer.section.state-machines-owned"
        :title "section state-machines-owned"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section state-machines-owned"
        :status code-aligned-partial))

    ;; ── pillar system-layer ──
    (pillar-section-index
      :pillar system-layer
      :source-file ".missiond/v2/intent-system-layer.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar system-layer"
      :status code-aligned-partial

      (section-entry
        :section-id "system-layer.pillar-root"
        :title "pillar system-layer (root)"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer"
        :status code-aligned-partial)

      (section-entry
        :section-id "system-layer.section.core-types"
        :title "section core-types"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer :: section core-types"
        :status code-aligned)

      (section-entry
        :section-id "system-layer.section.process-transport"
        :title "section process-transport"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer :: section process-transport"
        :status code-aligned)

      (section-entry
        :section-id "system-layer.section.infra-modules"
        :title "section infra-modules"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer :: section infra-modules"
        :status code-aligned-partial)

      (section-entry
        :section-id "system-layer.section.rpc-gateway"
        :title "section rpc-gateway (system-side)"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer :: section rpc-gateway"
        :status code-aligned)

      (section-entry
        :section-id "system-layer.section.pure-utils"
        :title "section pure-utils"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer :: section pure-utils"
        :status code-aligned)

      (section-entry
        :section-id "system-layer.section.state-machines-overview"
        :title "section state-machines-overview"
        :source-file ".missiond/v2/intent-system-layer.lisp"
        :local-path "pillar system-layer :: section state-machines-overview"
        :status code-aligned-partial))

    ;; ── pillar flow ──
    (pillar-section-index
      :pillar flow
      :source-file ".missiond/v2/intent-flow.lisp"
      :navigation-anchor ".missiond/v2/intent.lisp :: pillar flow"
      :status code-aligned-partial

      (section-entry
        :section-id "flow.pillar-root"
        :title "pillar flow (root)"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow"
        :status code-aligned-partial
        :note "v0.7 phase-C — narrative pillar, no code ownership")

      (section-entry
        :section-id "flow.unified-entry-pipeline"
        :title "F-intent-alignment-plan-execution-loop"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop"
        :status code-aligned-partial
        :note "directive → plan → workflow + plan-runner + dispatch_strategy")

      (section-entry
        :section-id "flow.capability-usage-monitoring"
        :title "F-capability-usage-monitoring"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-capability-usage-monitoring"
        :status code-aligned-partial
        :note "5 sources + lisp hint merge candidate")

      (section-entry
        :section-id "flow.workstation-dispatch-policy"
        :title "F-workstation-dispatch-policy"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-workstation-dispatch-policy"
        :status operational-practice
        :note "companion log dispatch_strategy 已落")

      (section-entry
        :section-id "flow.execution-log-governance"
        :title "F-execution-log-governance"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-execution-log-governance"
        :status code-aligned-partial
        :note "mission_execution 12-action manager + execution companion log; scoped commit handoff 接入")

      (section-entry
        :section-id "flow.scoped-commit-handoff"
        :title "F-scoped-commit-handoff"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-scoped-commit-handoff"
        :status architecture-designed
        :note "execution Lisp control plane + scoped git commit durability plane"))

    ;; ──────────────────────────────────────────────────
    ;; v0.3 (wave 12 / task 06) — precompression coverage expansion
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 在主 Lisp 真正压缩前, 把 7 个高变动语义区扩成 stable section-id anchor
    ;;   - 新增 :compression-safe? 字段 (optional, see architecture-dsl :: section-entry-extended)
    ;;   - 本批 entry 仅做"语义锚点"声明, 不动任何主 Lisp 正文
    ;;   - status 全部落在 architecture-dsl :: status-taxonomy 7 值之内
    ;; 7 区域:
    ;;   1) execution coordination / scoped commit handoff
    ;;   2) file-first artifacts
    ;;   3) review gate
    ;;   4) PLAN DAG scheduler
    ;;   5) methodology compiler / semantic lifting
    ;;   6) capability usage semantic evidence
    ;;   7) workstation orchestration
    ;; ──────────────────────────────────────────────────
    (precompression-coverage-expansion v0.3
      :date "2026-04-26"
      :decided-by "wave 12 / task 06 lisp source-index expansion session"
      :scope "为 7 个高变动语义区落 stable section-id, 让未来压缩/拆 shard 有锚点"
      :non-goal "本任务不真正压缩主 Lisp、不拆 shard"
      :compression-safe-field-rule "true=可走 compression-policy.allowed; false=保护正文 (frozen / control-plane / contract-zone)"

      ;; ── 区域 1 · execution coordination / scoped commit handoff ──
      (section-entry
        :section-id "memory.helper.agent-execution-coordination"
        :title "helper agent-execution-coordination (control-plane protocol)"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module board :: helper agent-execution-coordination"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
           "crates/missiond-core/src/db/board.rs"]
        :note "v0.5.8 双平面协议: control-plane (claim/lease/heartbeat/deviation/decision/issue/completion/verification); D010 教训锁 — 不允许 scheduler 自建 ID 池")

      (section-entry
        :section-id "memory.helper.scoped-commit-contract"
        :title "scoped-commit-contract (durability-plane)"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module board :: helper agent-execution-coordination :: scoped-commit-contract"
        :status architecture-designed
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["flow.scoped-commit-handoff" "intent-layer.execution-handoff-dual-plane"]
        :note "task-file operational-practice; daemon enforce (auto stage/commit/preflight) pending — 契约段, 不压缩")

      (section-entry
        :section-id "intent-layer.execution-handoff-dual-plane"
        :title "execution-handoff dual-plane rule (R013/R014 anchor)"
        :source-file ".missiond/v2/architecture-dsl.lisp"
        :local-path "defdsl architecture-v1 :: semantic-rules :: R013 + R014"
        :status code-aligned
        :compression-safe? false
        :note "R013 execution-dual-plane / R014 scoped-commit-subset 是契约规则, 不压缩")

      ;; ── 区域 2 · file-first artifacts ──
      (section-entry
        :section-id "memory.directive-layer.file-first-artifacts"
        :title "file-first-artifacts (artifact registry)"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module directive-layer :: file-first-artifacts"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"]
        :note "5 artifact: intent-alignment-lisp / plan-lisp / workflow-lisp / plan-evidence-sidecar / plan-node-state-projection — schema 字段是契约不压缩")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.file-first-ssot"
        :title "unified-entry-pipeline :file-first-ssot anchor"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: :file-first-ssot"
        :status code-aligned-partial
        :compression-safe? true
        :cross-ref ["memory.directive-layer.file-first-artifacts" "flow.unified-entry-pipeline"]
        :note "writer 当前在 directive/plan/workflow handler 内联; foundation helper handlers/knowledge/file_artifacts.rs (wave11 task 完成) — status 段可压缩, 内容/路径不动")

      (section-entry
        :section-id "flow.file-vs-db-contract"
        :title "F-intent-alignment-plan-execution-loop :: :file-vs-db-contract"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop :: :file-vs-db-contract"
        :status code-aligned-partial
        :compression-safe? false
        :note "file 是 SSOT, DB 是镜像 — 这是契约, 不允许压缩正文")

      ;; ── 区域 3 · review gate ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.alignment-review-gate"
        :title "role alignment-review-gate"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role alignment-review-gate"
        :status code-aligned-partial
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"]
        :note "compile/persist 后可选发 QuestionEvent::Created; emission 字段 emit_review_question / review_question_id 已 code-aligned")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.plan-review-gate"
        :title "role plan-review-gate"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role plan-review-gate"
        :status code-aligned-partial
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :note "compile/persist 后可选发 QuestionEvent::Created; approve/archive/mark/supersede 可选发 Resolved/Decision")

      (section-entry
        :section-id "flow.alignment-review-gate-stage"
        :title "F-intent-alignment-plan-execution-loop :: s3 alignment-review-gate"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop :: s3 alignment-review-gate"
        :status code-aligned-partial
        :compression-safe? true
        :cross-ref ["intent-layer.unified-entry-pipeline.alignment-review-gate"])

      (section-entry
        :section-id "flow.plan-review-gate-stage"
        :title "F-intent-alignment-plan-execution-loop :: s5 plan-review-gate"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop :: s5 plan-review-gate"
        :status code-aligned-partial
        :compression-safe? true
        :cross-ref ["intent-layer.unified-entry-pipeline.plan-review-gate"])

      ;; ── 区域 4 · PLAN DAG scheduler ──
      (section-entry
        :section-id "intent-layer.actor.plan-dag-scheduler"
        :title "actor plan-dag-scheduler (full DAG architecture)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler"
        :status architecture-designed
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["memory.helper.agent-execution-coordination"
                    "memory.directive-layer.file-first-artifacts"
                    "flow.execution-runner-dag-scheduler"]
        :note "v0 plan-runner 单节点 dispatch 已 code-aligned; v1 完整 11-stage / per-node FSM / claim-lease 复用 agent-execution-coordination, 协议正文不压缩")

      (section-entry
        :section-id "flow.execution-runner-dag-scheduler"
        :title "F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler"
        :status architecture-designed
        :compression-safe? false
        :note "11-stage logic-core + node schema + node FSM + claim-lease + anti-patterns + open-questions; 协议契约段不压缩")

      (section-entry
        :section-id "memory.directive-layer.plan-node-state-projection"
        :title "artifact plan-node-state-projection (DAG evidence sidecar)"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module directive-layer :: file-first-artifacts :: artifact plan-node-state-projection"
        :status architecture-designed
        :compression-safe? false
        :note "evidence sidecar nodes[<node_id>] block: per-node attempts/claim_id/start-end/inner_result/acceptance_pass/rollback_path; v0 plan_runner_dispatch entry shape 必须向后兼容")

      ;; ── 区域 5 · methodology compiler / semantic lifting ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.methodology-compiler"
        :title "methodology compiler v0 (unified-entry-pipeline path)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: path methodology-to-executable-compile"
        :status code-aligned-partial
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :note "v0 paren-validate + (step …) 抽取 + executable YAML; 高阶 semantic lifting / forge compiler 仍 pending — pending 段可压缩, 阶段步骤不压缩")

      (section-entry
        :section-id "flow.methodology-to-executable-compile"
        :title "F-methodology-to-executable-compile"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-methodology-to-executable-compile"
        :status code-aligned-partial
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-daemon/src/engine/flow/loader.rs"
           "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
           "crates/missiond-mcp/src/tools/compute/flow_run.rs"]
        :note "methodology Lisp SSOT → executable YAML → flow-engine-v2 run; semantic lifting (phases/anti-patterns/authority) + longest-prefix cwd resolver + record_execution-distill 联动 仍 pending")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.semantic-lifting-pending"
        :title "semantic lifting pending anchor"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: pending semantic-lifting"
        :status pending
        :compression-safe? true
        :note "高阶 semantic phase / anti-pattern lifting / forge compiler 入口尚未排期")

      ;; ── 区域 6 · capability usage semantic evidence ──
      (section-entry
        :section-id "intent-layer.capability-evolution-governance.semantic-evidence-v1"
        :title "capability-evolution-governance semantic evidence v1"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section capability-evolution-governance"
        :status code-aligned-partial
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/comm/capability_usage.rs"
           "crates/missiond-mcp/src/tools/comm/capability_usage.rs"]
        :cross-ref ["flow.capability-usage-monitoring"
                    "tools.surface.mission-capability-usage"
                    "memory.system-support.capability-usage-read-model"]
        :note "5 sources + lisp hint merge-candidate 已 code-aligned; semantic merge 自动决策 / DispatcherEvent / WorkerEvent / ExecutionEvent 聚合 / workflow stats 仍 pending — pending 块可压缩")

      (section-entry
        :section-id "memory.system-support.capability-usage-read-model"
        :title "derived-read-model capability-usage-read-model"
        :source-file ".missiond/v2/intent-memory.lisp"
        :local-path "pillar memory :: module system-support :: derived-read-model capability-usage-read-model"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/comm/capability_usage.rs"]
        :note "5 sources read-only; schema 是契约段不压缩, status 句子可压缩")

      (section-entry
        :section-id "tools.surface.mission-capability-usage"
        :title "implemented-surface mission_capability_usage"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_capability_usage"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
           "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"]
        :note "schema (action/window/scope/replacement_target/dry_run) 是契约段不压缩")

      ;; ── 区域 7 · workstation orchestration ──
      (section-entry
        :section-id "worker.section.claudecode-workstation-orchestration.dispatch-decision-matrix"
        :title "dispatch-decision-matrix (策略决策表)"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section claudecode-workstation-orchestration :: dispatch-decision-matrix"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
        :note "策略 ∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback}; 表格本身是契约不压缩")

      (section-entry
        :section-id "worker.section.claudecode-workstation-orchestration.execution-strategy-record"
        :title "execution-strategy-record (companion log meta)"
        :source-file ".missiond/v2/intent-worker.lisp"
        :local-path "pillar worker :: section claudecode-workstation-orchestration :: execution-strategy-record"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-core/src/event/events/execution.rs"]
        :note "dispatch_strategy / target_project / requested_cwd 已写入 companion log meta; ExecutionEvent::Opened 扩展同字段 (wave12 task 03 进行中)")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.workstation-dispatch-policy"
        :title "workstation-dispatch-policy (intent-layer cross-ref)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: workstation-dispatch-policy"
        :status operational-practice
        :compression-safe? true
        :cross-ref ["worker.section.claudecode-workstation-orchestration"
                    "worker.section.claudecode-workstation-orchestration.dispatch-decision-matrix"
                    "worker.section.claudecode-workstation-orchestration.execution-strategy-record"
                    "flow.workstation-dispatch-policy"]
        :note "narrative 段, 重复 rationale 文本可压缩 (compression-policy.allowed.compress-redundant-pointers)"))

    ;; ── 已声明但本次未细化的 section, 后续再补 ──
    (deferred-coverage
      :reason "v0.2 baseline 覆盖 7 pillar 顶层; v0.3 (wave 12 task 06) 扩了 7 高变动语义区; 仍有以下未细化项, 等后续 wave 再补"
      :scope-deferred
        ["pillar memory 内 cross-cutting / pillar-interfaces 的 5 surface 矩阵"
         "pillar worker section workers 内 19 worker 的 per-worker entry"
         "pillar tools 83 tool 的 per-tool section-id (现仅按 section 分组, capability_usage 已有专项)"
         "pillar intent-layer 各 actor 内部 step (directive-compiler / plan-compiler / workflow-distiller 内部)"
         "pillar event-bus 4 表内部字段索引 (frozen 文件, 不强行细化)"
         "pillar flow 其他 ~17 个非主线 flow (F9-project-init / F-incident-reaction / F-execution-log-governance 等)"
         "scoped-commit-handoff 的 daemon enforce step-level entry (待 daemon 实现后回填)"
         "PLAN DAG scheduler 内部 11 stage 的 per-stage entry (待 v1 plan-runner 实现后回填)"]))

  ;; ──────────────────────────────────────────────────
  ;; Part 3 · 当前判断与下一步路径
  ;; ──────────────────────────────────────────────────
  (judgement-now
    :date "2026-04-26"
    :decided-by "wave 11 lisp-source-index-precompression + wave 12 task 06 source-index-expansion sessions"
    :wave12-task-06-non-goal
      ["本任务不真正压缩主 Lisp"
       "本任务不拆任何 shard (拆 shard 仍要等 split-policy.wait-for-conditions 全满足之后)"
       "本任务只扩 stable section-id / source-index 覆盖面, 让未来压缩有锚点"]
    :why-no-main-compression-yet
      ["主大 lisp 正文是其他并行会话 (file-first writer / review gate / PLAN DAG) 的 anchor"
       "若现在压缩, 那些会话的 cross-ref 会失锚, 出现回退成本"
       "压缩需要的 section-id / status taxonomy / split rule 必须先冻结 — 这正是 wave 11 + wave 12 task 06 的工作"]
    :pre-compression-checklist
      ["section-id 在 source index 已落 (wave 11 完成 7 pillar baseline; wave 12 task 06 扩 7 高变动语义区, 共 +22 entry, 含 :compression-safe? 标注)"
       "status-taxonomy 已在 architecture-dsl.lisp 冻结 7 值"
       "split-policy 已写明 wait-for-conditions"
       "compression-policy 已写明 forbidden 红线 (ingress/logic-core/egress 不动)"
       "frozen 文件 (event-bus / event-bus-execution) 在本 index 标 protected, 不参与压缩"
       "checker phase-3-precompression 已写入 architecture-dsl.lisp (待 checker 升级实现)"
       "wave 12 新增 checker phase-3.1: section-id 唯一性 / file+local-path+status 必填 (待 checker 升级实现)"]
    :unblock-conditions-for-real-compression
      ["条件 1 — file-first writer (alignment.lisp / PLAN.lisp / workflow.lisp) 落地"
       "条件 2 — review gate 能基于 artifact 自动出 QuestionEvent"
       "条件 3 — PLAN DAG scheduler 跑过最小闭环 + ExecutionEvent dispatch metadata code-aligned"]
    :next-step
      ["条件全满足后, 由 lisp-review skill 牵头, 按 compression-policy.allowed 三类做批量压缩"
       "压缩 PR 必须带 git diff --check + checker --all-v2 + 对应 *-execution.lisp D-deviation"
       "物理 split (拆 shard) 是压缩之后的事, 不和压缩混在一起"
       "压缩前以 :compression-safe? 字段做白名单过滤 — false 的 section 即使在 compression-policy.allowed 之内也保留正文"]))
