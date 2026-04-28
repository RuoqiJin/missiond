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
    :version "v1.8 — wave 27 router dispatch descriptor handoff layered on wave 26 readiness loop"
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
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar worker :: section claudecode-workstation-orchestration"
        :status operational-practice
        :note "wave 15 task 02 — moved to intent-workstation-policy.lisp shard; intent-worker.lisp keeps anchor stub; policy / dispatch_strategy companion log already aligned; full ExecutionEvent metadata pending")

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
        :source-file ".missiond/v2/intent-capability-governance.lisp"
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
        :source-file ".missiond/v2/intent-capability-governance.lisp"
        :local-path "pillar flow :: F-capability-usage-monitoring"
        :status code-aligned-partial
        :note "5 sources + lisp hint merge candidate")

      (section-entry
        :section-id "flow.workstation-dispatch-policy"
        :title "F-workstation-dispatch-policy"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar flow :: F-workstation-dispatch-policy"
        :status operational-practice
        :note "wave 15 task 02 — moved to intent-workstation-policy.lisp shard; intent-flow.lisp keeps anchor stub; companion log dispatch_strategy 已落")

      (section-entry
        :section-id "flow.execution-log-governance"
        :title "F-execution-log-governance"
        :source-file ".missiond/v2/intent-execution-governance.lisp"
        :local-path "pillar flow :: F-execution-log-governance"
        :status code-aligned-partial
        :note "mission_execution 12-action manager + execution companion log; scoped commit handoff 接入")

      (section-entry
        :section-id "flow.scoped-commit-handoff"
        :title "F-scoped-commit-handoff"
        :source-file ".missiond/v2/intent-execution-governance.lisp"
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
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar memory :: module directive-layer :: file-first-artifacts"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"]
        :cross-ref ["memory.directive-layer.file-first-writer-integration"]
        :note "5 artifact: intent-alignment-lisp / plan-lisp / workflow-lisp / plan-evidence-sidecar / plan-node-state-projection — schema 字段是契约不压缩; wave 14 升: code-aligned-partial → code-aligned (writer 主路径接入, anchor: memory.directive-layer.file-first-writer-integration)")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.file-first-ssot"
        :title "unified-entry-pipeline :file-first-ssot anchor"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: :file-first-ssot"
        :status code-aligned
        :compression-safe? true
        :cross-ref ["memory.directive-layer.file-first-artifacts"
                    "memory.directive-layer.file-first-writer-integration"
                    "flow.unified-entry-pipeline"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"]
        :note "writer 当前在 directive/plan/workflow handler 内联; foundation helper handlers/knowledge/file_artifacts.rs (wave11 task 完成) — status 段可压缩, 内容/路径不动; wave 14 升: code-aligned-partial → code-aligned (三类 artifact 主路径接入, anchor: memory.directive-layer.file-first-writer-integration)")

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
        :status code-aligned
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.review-gate-id-derivation"]
        :note "compile/persist 后可选发 QuestionEvent::Created; emission 字段 emit_review_question / review_question_id 已 code-aligned; wave 14 升: code-aligned-partial → code-aligned (review_gate policy enum manual|emit_question|off + 自动 emit + deterministic id)")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.plan-review-gate"
        :title "role plan-review-gate"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role plan-review-gate"
        :status code-aligned
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.review-gate-id-derivation"]
        :note "compile/persist 后可选发 QuestionEvent::Created; approve/archive/mark/supersede 可选发 Resolved/Decision; wave 14 升: code-aligned-partial → code-aligned (review_gate policy enum + 自动 emit + deterministic id)")

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
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["memory.helper.agent-execution-coordination"
                    "memory.directive-layer.file-first-artifacts"
                    "flow.execution-runner-dag-scheduler"
                    "intent-layer.plan-dag-runtime-v2"]
        :note "wave 13 task 02 (commit 8bb6110): plan_dag.rs runtime v2 已 code-aligned partial — max_parallel_nodes 参数 (default=1=v1 行为) + tokio::JoinSet 并发 dispatch; node lifecycle 6 状态 (pending/ready/running/succeeded/failed/skipped) + 3 skip 子分类 (upstream_failed / fail_fast_aborted / condition_gated); failure-policy fail-fast vs continue 已实现; per-node evidence transition 写 evidence collector. 单节点 fast-path 保留. wave 16 task 04 (commit a51bc52): paused 7th lifecycle 落地 — 节点 :review-gate 'question-event' (+ 可选 :review-action / :review-text) 触发 paused; deterministic review id 'review:plan:<plan_id>:v<v>:plan-node:<sha256(node_id)[..16]>'; aggregate_status='dag_paused' / runner_status='review_gate_paused'; bus failure → 仍 pause + warning; 不实现 auto-resume. wave 16 task 05 (commit d8f8a6e): per-node retry policy v0 — :retry-count (additional) / :max-attempts (total) / :retry-delay-ms cap 60s + cap 3 attempts; SafeDescriptor refusals 不 retry (UnsupportedTarget/ProjectRootUnresolved/MissingObjective); 每 attempt 写自己 evidence (attempt number); failure-policy 与 retry 正交 (retry exhaust 后 propagate_taint). wave 17 task 01 (commit a42f5fd): paused-resume hook v0 — mission_plan(action=execute) 4 字段 (resume_review_question_id / resume_review_decision / resume_actor / resume_note); 3 decision (approved 重 dispatch fresh attempt 1 / rejected 不 dispatch / needs_changes paused); spawn_review_resolution_sub 扩 plan-node action 自动 route 到 resume helper; evidence paused→resume_*. wave 17 task 02 (commit 8661fcb): claim/lease v0 — claim_lease_secs / claimer_name / enforce_claims 3 args; Claimed 第 8 lifecycle (Pending → Claimed → Running); claim scope :owned-files → :scope → plan/<plan_id>/node/<node_id> fallback; scopes_overlap_pure 共享给 claim/audit/scoped-commit 三处; enforce_claims default false byte-compat. wave 17 task 03 (commit f572729): acceptance evaluator v0 — 3 modes (inner_status / evidence_keys / manual); 绝不 shell (grep proof 0 命中); id space acceptance:plan:<plan_id>:v<v>:<node_id> 与 review-gate review:* 隔离 (各自 routing); manual_required 复用 Paused 不新加态; evidence succeeded→acceptance_*. wave 17 task 04 (commit d4466c6): rollback policy v0 — 3 modes (none/descriptor/workstation); 5 safety gates (missing objective / empty owned-files / no project signal / unsafe strategy / SafeDescriptor refusal); workstation 复用 build_task_brief / run_workstation_dispatch; rollback 在 final fail 后 / downstream taint 前; evidence failed→rollback_*. wave 17 task 05 (commit 402fb82): finalize + distill trigger v0 — finalize_plan / distill_on_success / distill_mode (default false / false / dry_run); 4 finalization rule (succeeded → succeeded; failed → failed; partial → failed; paused → unchanged 不撒谎); distill 经现有 super::workflow::handle() 不动 workflow.rs; distill 失败 warning 仍保 plan final state. wave 18 task 03 (commit 76ada10): cross-node acceptance fan-in v0 — :acceptance-depends-on / :acceptance-requires (all_succeeded|any_succeeded|evidence_keys) + 4 validator rejection (missing dep / requires missing / source invalid / not ancestor cycle guard); evidence 写 acceptance_fan_in 块; 评估在 acceptance-evaluator-v0 之后 / propagate_taint 之前. wave 18 task 04 (commit 7de886c): cascade rollback v0 — :compensates / :rollback-cascade (none|plan|dispatch-safe) / :rollback-after; Kahn 拓扑排序 + cycle-tolerant fallback; 5 safety triggers; 复用 wave 17-04 workstation_dispatch substrate; cascade 中 acceptance_commands_executed: false 钉死; cascade 不 alter taint. wave 18 task 05 (commit 76d818d): cross-plan distill chain v0 — distill_chain_id / distill_chain_mode (record_only|dry_run|sonnet) / distill_chain_name; chain metadata 写 evidence sidecar **无 SQL migration**; 5 skip 状态; append-only 3 层保证; sonnet 仅显式触发 + parent succeeded + parent distill_on_success=true. wave 18 task 06 (commit 91f8fa1): autonomous PLAN field inference v0 — infer_plan_fields (off|preview|apply_safe) + 6 fields + 3 confidence; 仅 high apply_safe 落; 3 evidence sources; conflict 永不静默 override; 完全无 LLM (deterministic heuristics). 完整 11-stage scheduler 几乎全部 close: s5 claim ✓ / s7 acceptance + cross-node fan-in ✓ / s8 paused-resume ✓ / s9 rollback + cascade ✓ / s10 mark-final ✓ / s11 distill 触发 + cross-plan chain ✓. 仍 pending: LLM auto-approve / 完全自动 spawn (无 hint) / git pre-commit hook / sonnet 自动接 chain — 协议正文不压缩")

      (section-entry
        :section-id "flow.execution-runner-dag-scheduler"
        :title "F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler"
        :status code-aligned-partial
        :compression-safe? false
        :cross-ref ["intent-layer.plan-dag-runtime-v2" "intent-layer.actor.plan-dag-scheduler"]
        :note "wave 13 task 02 (commit 8bb6110): runtime v2 (max_parallel_nodes / lifecycle / failure-policy / per-node evidence) code-aligned partial; wave 16 task 04 (commit a51bc52) paused 7th lifecycle + review-gate question-event trigger; wave 16 task 05 (commit d8f8a6e) per-node retry-N (cap 3 attempts; cap delay 60s; SafeDescriptor refusals 不 retry); wave 17 task 01 (commit a42f5fd) paused-resume hook (s8 paused 节点接 resume_review_decision approved 后 fresh attempt 1 重 dispatch); wave 17 task 02 (commit 8661fcb) claim-lease v0 (s5 claim 在 pending→running 之间插 claimed 第 8 态; opt-in enforce_claims default false byte-compat); wave 17 task 03 (commit f572729) acceptance evaluator v0 (s7 三模式 inner_status / evidence_keys / manual; 绝不 shell; manual_required 复用 paused; id space acceptance:* 与 review:* 隔离); wave 17 task 04 (commit d4466c6) rollback policy v0 (s9 三模式 none/descriptor/workstation + 5 safety gates; rollback 在 final fail 后 / downstream taint 前); wave 17 task 05 (commit 402fb82) finalize + distill v0 (s10/s11 4 finalization rule paused → unchanged 不撒谎; distill 经 super::workflow::handle); wave 18 task 03 (commit 76ada10) cross-node acceptance fan-in v0 (s7 节点 acceptance 引用 cross-node evidence 经 :acceptance-depends-on / :acceptance-requires + 4 validator rejection); wave 18 task 04 (commit 7de886c) cascade rollback v0 (s9 节点 :rollback-cascade plan|dispatch-safe + Kahn 排序 cycle-tolerant + 5 safety triggers); wave 18 task 05 (commit 76d818d) cross-plan distill chain v0 (s11 distill_chain_id 跨 plan 共享, sidecar metadata append-only 无 migration); wave 18 task 06 (commit 91f8fa1) autonomous PLAN field inference v0 (s2 之前 deterministic heuristics 推 6 字段; 仅 high apply_safe; 无 LLM); 完整 11-stage logic-core + node schema + node FSM + claim-lease + rollback + acceptance + mark-plan-final + anti-patterns + open-questions 协议契约段不压缩")

      (section-entry
        :section-id "memory.directive-layer.plan-node-state-projection"
        :title "artifact plan-node-state-projection (DAG evidence sidecar)"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar memory :: module directive-layer :: file-first-artifacts :: artifact plan-node-state-projection"
        :status code-aligned-partial
        :compression-safe? false
        :cross-ref ["intent-layer.plan-dag-runtime-v2" "intent-layer.evidence-collector-typed-helper"]
        :note "wave 13 task 02 (commit 8bb6110): per-node state projection 由 plan_dag runtime v2 写入 — pending/ready/running/succeeded/failed + 3 skip 子分类 (upstream_failed / fail_fast_aborted / condition_gated) 已落 evidence sidecar; v0 plan_runner_dispatch entry shape 向后兼容; wave 16 task 04 (commit a51bc52) 加 paused 7th lifecycle + review-gate question-event entry; wave 16 task 05 (commit d8f8a6e) 加 per-attempt evidence entry (每次 retry 一个独立 attempt entry, 含 attempt number); wave 17 task 01 (commit a42f5fd) 加 paused→resume_{approved|rejected|needs_changes} evidence (approved 后接 ready→running + running→{succeeded|failed} 三条); wave 17 task 02 (commit 8661fcb) 加 claim_id + Claimed 第 8 态 + pending→claimed→running→{succeeded|failed}→released 五段 evidence (released 含 claim_terminal_state); wave 17 task 03 (commit f572729) 加 acceptance evaluator succeeded→acceptance_{accepted|rejected|manual_required} evidence (rejected 后翻回 failed; manual_required 复用 paused 不新加态); wave 17 task 04 (commit d4466c6) 加 rollback failed→rollback_{not_requested|descriptor_ready|dispatched|refused|failed} evidence + node_results[].rollback 块 (acceptance_commands_executed=false pinned); wave 17 task 05 (commit 402fb82) 加 plan-level dag_finalized evidence + finalization 块映射 aggregate→plan_status; wave 18 task 03 (commit 76ada10) 加 acceptance_fan_in 块 (mode / depends_on / source_node / status / matched_keys / unmet_keys); wave 18 task 04 (commit 7de886c) 加 rollback_cascade 块 (mode / cascade_plan / triggered_nodes / skipped_nodes / refused_nodes / cycle_fallback?); wave 18 task 05 (commit 76d818d) 加 distill_chain 子块 (chain_id / mode / name / parent_plan_id / position_in_chain / created_at, append-only **无 SQL migration**); wave 18 task 06 (commit 91f8fa1) 加 inferred_fields 块 (per-field source / confidence / applied?); rollback_path / acceptance_pass / cross-node fan-in / cascade rollback / cross-plan distill chain 全部 surface 已落, 仅剩 LLM-assisted inference / 自动跨 plan chain (无需 caller chain_id) 仍 future")

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
        :source-file ".missiond/v2/intent-capability-governance.lisp"
        :local-path "pillar intent-layer :: section capability-evolution-governance"
        :status code-aligned-partial
        :compression-safe? true
        :implements
          ["crates/missiond-daemon/src/handlers/comm/capability_usage.rs"
           "crates/missiond-mcp/src/tools/comm/capability_usage.rs"]
        :cross-ref ["flow.capability-usage-monitoring"
                    "tools.surface.mission-capability-usage"
                    "memory.system-support.capability-usage-read-model"
                    "intent-layer.evidence-collector-typed-helper"]
        :note "5 sources + lisp hint merge-candidate 已 code-aligned; semantic merge 自动决策 / DispatcherEvent / WorkerEvent / ExecutionEvent 聚合 (typed EvidenceEntry 已就位但 capability_usage 尚未消费) / workflow stats 仍 pending — pending 块可压缩")

      (section-entry
        :section-id "memory.system-support.capability-usage-read-model"
        :title "derived-read-model capability-usage-read-model"
        :source-file ".missiond/v2/intent-capability-governance.lisp"
        :local-path "pillar memory :: module system-support :: derived-read-model capability-usage-read-model"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/comm/capability_usage.rs"]
        :note "5 sources read-only; schema 是契约段不压缩, status 句子可压缩")

      (section-entry
        :section-id "tools.surface.mission-capability-usage"
        :title "implemented-surface mission_capability_usage"
        :source-file ".missiond/v2/intent-capability-governance.lisp"
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
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar worker :: section claudecode-workstation-orchestration :: dispatch-decision-matrix"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
        :note "wave 15 task 02 — moved to intent-workstation-policy.lisp shard; 策略 ∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback}; 表格本身是契约不压缩")

      (section-entry
        :section-id "worker.section.claudecode-workstation-orchestration.execution-strategy-record"
        :title "execution-strategy-record (companion log meta)"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar worker :: section claudecode-workstation-orchestration :: execution-strategy-record"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-core/src/event/events/execution.rs"]
        :note "wave 15 task 02 — moved to intent-workstation-policy.lisp shard; dispatch_strategy / target_project / requested_cwd 已写入 companion log meta; ExecutionEvent::Opened 扩展同字段 (wave12 task 03 进行中)")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.workstation-dispatch-policy"
        :title "workstation-dispatch-policy (intent-layer cross-ref)"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: workstation-dispatch-policy"
        :status operational-practice
        :compression-safe? true
        :cross-ref ["worker.section.claudecode-workstation-orchestration"
                    "worker.section.claudecode-workstation-orchestration.dispatch-decision-matrix"
                    "worker.section.claudecode-workstation-orchestration.execution-strategy-record"
                    "flow.workstation-dispatch-policy"]
        :note "narrative 段, 重复 rationale 文本可压缩 (compression-policy.allowed.compress-redundant-pointers)"))

    ;; ──────────────────────────────────────────────────
    ;; v0.4 (wave 13 task 04) — execution status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 13 task 01/02/03 三件 commit 的真实代码状态回填到 source-index
    ;;   - 不做 L1/L2/L3 压缩, 不拆 shard (wave 13 task 05 才做 L1 安全压缩)
    ;;   - 新 entry 在 wave 12 task 06 v0.3 baseline 上扩展, 不重复已有 section-id
    ;; wave 13 已完成 commit (anchor):
    ;;   - 88568a9 feat(plan): route runner evidence through collector
    ;;   - 8bb6110 feat(plan): run ready DAG nodes with bounded concurrency
    ;;   - 9759675 feat(intent): add unified entry pipeline helper
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 未列出仅指向新 entry 的 cross-ref 增补):
    ;;   - intent-layer.actor.plan-dag-scheduler             architecture-designed → code-aligned-partial
    ;;   - flow.execution-runner-dag-scheduler               architecture-designed → code-aligned-partial
    ;;   - memory.directive-layer.plan-node-state-projection architecture-designed → code-aligned-partial
    ;; ──────────────────────────────────────────────────
    (wave-13-backfill v0.4
      :date "2026-04-26"
      :decided-by "wave 13 / task 04 lisp backfill session"
      :scope "回填 wave 13 task 01/02/03 的真实代码状态; 新增 4 anchor entry, 已升级 3 现有 entry status"
      :non-goal "本任务不真正压缩主 Lisp、不拆 shard (那是 wave 13 task 05)"
      :commits
        [(commit-1 :hash "88568a9" :title "route runner evidence through collector"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
                   :tests "+7 evidence_collector / plan / plan_dag tests")
         (commit-2 :hash "8bb6110" :title "run ready DAG nodes with bounded concurrency"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs (schema only)"]
                   :tests "+17 lifecycle / fail-fast / continue / wave / max_parallel_nodes=1 v1 等同 sequential tests")
         (commit-3 :hash "9759675" :title "add unified entry pipeline helper"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs (新建 891 行)"
                                     "crates/missiond-daemon/src/handlers/knowledge/mod.rs"]
                   :tests "+21 plan_pipeline / run_pipeline / decorator / stage routing tests")]

      ;; ── 区域 8 · evidence-collector typed helper integration ──
      (section-entry
        :section-id "intent-layer.evidence-collector-typed-helper"
        :title "evidence-collector typed EvidenceEntry helper (wave 13 task 01)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role evidence-collector"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["memory.directive-layer.plan-evidence-sidecar"
                    "memory.directive-layer.plan-node-state-projection"
                    "intent-layer.actor.plan-dag-scheduler"
                    "intent-layer.evidence-collector-event-ref"]
        :wave "13 task 01 (commit 88568a9)"
        :note "typed EvidenceEntry (源/kind/schema_version 三段 canonical 字段) 接入 plan.rs::action_execute_internal (plan_runner_dispatch entry) + plan_dag.rs::每节点 dispatch (plan_dag_node_dispatch entry); legacy mission_plan(action=record_evidence) 走 with_extra flat-top byte-for-byte 兼容; sidecar 写失败仍走现有 partial / status_update_error 语义, 不静默吞错; bus subscription 仍 pending — execution_event ref 用 EventRef::unavailable(\"...\") 占位; 升级到 plan_evidence DB JSONB 仍 pending — 契约段不压缩")

      (section-entry
        :section-id "intent-layer.evidence-collector-event-ref"
        :title "EventRef live + deterministic 三层策略 (wave 14 升级 + wave 16 subscriber)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role evidence-collector :: event-ref strategy"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/bus/v2_subscribers.rs"
           "crates/missiond-daemon/src/bus/bootstrap.rs"]
        :cross-ref ["intent-layer.evidence-collector-typed-helper"
                    "event-bus.section.egress"
                    "event-bus.section.execution-event.plan-node-state-changed"
                    "intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"]
        :wave "13 task 01 (commit 88568a9) + 14 task 02 (commit 2e7789a) + 16 task 07 (commit 0e6ee63) + 17 task 06 (commit 4074a14) + 18 task 01 (commit 016487f)"
        :note "wave 13: EventRef::unavailable(reason) 占位 — plan-runner v0 / plan_dag runtime v2 当时无法同步取得 live ExecutionEvent id; wave 14 task 02 (commit 2e7789a) 升级到三层策略: (1) bus publish 成功 → live id EventRef::new(execution, plan_node_state_changed, <Seq>); (2) bus publish 失败 → deterministic id 'plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>' + warning; (3) 必要时仍可 EventRef::unavailable(reason); 占位本身仍是契约 (unavailable=true + reason 必填), 不压缩; 升: code-aligned-partial → code-aligned. wave 16 task 07 (commit 0e6ee63) passive subscriber cache (cap 1024 FIFO, key 'plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>' 严格匹配 deterministic event id) 加 三档 status: live (live id + Seq) / log (deterministic id 命中 cache) / unavailable (兜底, reason 必填); EventRef::new 别名 EventRef::live 保 wave-13/14 byte-compat; subscriber 严格 observation-only (不 mutate 任何主路径). wave 17 task 06 (commit 4074a14) resolver 三档 (live cache → log query 复用 LogReadable::read_from(Domain::Execution, Seq(0), 512) 不加 store method → unavailable); primary event_ref_status / event_ref_source / event_ref_warning 加在 EvidenceEntry surface; query 失败 unavailable + reason, 绝不挂 primary dispatch. wave 18 task 01 (commit 016487f) 把 ad-hoc resolver 升级到 typed query API: EventLogQuery (typed builder filter by domain / topic / seq-range / event-id / since / limit) + EventLogQueryable trait + blanket impl (任何 LogReadable 自动获 query 不加 store method); EVENT_LOG_QUERY_LIMIT_CAP=512 硬上限 (caller 不能 read 无界); EvidenceEntry surface 升级到 EventRefProvenance 4 值 enum (live | passive_cache | event_log_query | unavailable) — 把 wave 17 的 'live | log | unavailable' 三档展开 (passive_cache vs event_log_query 区分: cache 命中 = passive_cache; cache miss + 持久 query 命中 = event_log_query); 持久化按 plan_id / node_id / time-range 系统化 query API 已通 (anchor: intent-layer.evidence-collector-event-log-query-v1); 后续 dedicated store method (例如 by_topic 索引下推) 仍 future")

      (section-entry
        :section-id "tools.surface.mission-plan.record-evidence-typed"
        :title "mission_plan(action=record_evidence) typed wrap path"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_plan :: record_evidence typed wrap"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"]
        :cross-ref ["intent-layer.evidence-collector-typed-helper"
                    "memory.directive-layer.plan-evidence-sidecar"]
        :wave "13 task 01 (commit 88568a9)"
        :note "wire 兼容: evidence_kind/source 都缺省时输出与旧 record_evidence 兼容 (legacy passthrough); 任一字段出现则走 typed EvidenceEntry wrap (canonical source/kind/schema_version + with_extra flat-top); 写失败 → status_update_error 暴露, 不静默兜底 — 契约段不压缩")

      ;; ── 区域 9 · PLAN DAG runtime v2 (concurrency + lifecycle) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2"
        :title "PLAN DAG runtime v2 — bounded concurrency + node lifecycle"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.actor.plan-dag-scheduler"
                    "flow.execution-runner-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"
                    "intent-layer.evidence-collector-typed-helper"
                    "intent-layer.plan-dag-runtime-v2.execution-event-decision"]
        :wave "13 task 02 (commit 8bb6110)"
        :note "wave-based scheduler driven by tokio::JoinSet — 每 wave drain up to max_parallel_nodes (default=1=v1 sequential 行为) ready nodes; max_parallel_nodes=1 等同 v1 顺序; failure-policy fail-fast 与 continue 已实现; evidence 写串行化避免文件 race; dry_run 返回 DAG + concurrency_plan 不 dispatch; node lifecycle 6 主状态 (pending/ready/running/succeeded/failed/skipped) + 3 skip 子分类 (upstream_failed / fail_fast_aborted / condition_gated); response 含 scheduler_mode / node_count / max_parallel_nodes / node_results[] / skipped_nodes[] / aggregate_status; full 11-stage scheduler / claim-lease / per-node retry-N / acceptance evaluator / rollback compensate / review-gate paused 仍 architecture-designed pending — 协议契约段不压缩")

      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.node-lifecycle"
        :title "PLAN DAG node lifecycle 6 状态 + 3 skip variants"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: node-lifecycle"
        :status code-aligned-partial
        :compression-safe? false
        :implements ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "13 task 02 (commit 8bb6110)"
        :note "6 主状态 enum NodeState: pending / ready / running / succeeded / failed / skipped; skip 子分类 3 种枚举: skipped_upstream_failed (含 failed_dep 字段) / skipped_condition (condition gated) / skipped_fail_fast_abort (含 aborter 字段); 每次 transition 写 evidence sidecar plan_dag_node_dispatch entry, 含 skip_reason + skip_detail; 与 actor plan-dag-scheduler 节点 FSM enum (pending/ready/claimed/running/succeeded/failed/skipped/retrying/rolling-back/paused) 对齐 — runtime v2 仅实现非-claim/非-retry/非-rollback 子集; 子集枚举本身是契约不压缩")

      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.failure-policy"
        :title "PLAN DAG failure-policy: fail-fast vs continue"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: failure-policy"
        :status code-aligned-partial
        :compression-safe? false
        :implements ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "13 task 02 (commit 8bb6110)"
        :note "fail-fast: 节点失败时 scheduler 停止后续 wave, 未 drain 的 ready 节点标 skipped_fail_fast_abort + aborter; continue: 失败节点的下游子树标 skipped_upstream_failed + failed_dep, 无依赖的其他 ready 节点继续; retry-N / route-to-rollback 仍 architecture-designed pending — 决策表本身是契约不压缩")

      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.execution-event-decision"
        :title "ExecutionEvent::PlanNodeStateChanged variant extended (wave 14 升级)"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: ExecutionEvent decision"
        :status code-aligned
        :compression-safe? true
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "intent-layer.evidence-collector-event-ref"
                    "intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"
                    "event-bus.section.execution-event.plan-node-state-changed"
                    "worker.section.claudecode-workstation-orchestration.execution-strategy-record"]
        :wave "13 task 02 (commit 8bb6110) + 14 task 02 (commit 2e7789a)"
        :note "wave 13: 决议先不扩 ExecutionEvent variant (scheduler runtime 与 bus subscription 正交); wave 14 task 02 (commit 2e7789a) 改为同步落地: 扩 ExecutionEvent::PlanNodeStateChanged 4 必字段 (plan_id/node_id/from/to) + 5 可选 (target/dispatch_strategy/target_project/attempt/reason); BusServices::publish_execution_with_seq helper; bus failure 仅 observability (warning 字段), 不挂主 dispatch; 升: pending → code-aligned (variant 已扩, live event ref 三层策略已实现)")

      ;; ── 区域 10 · unified entry pipeline v0 ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.run-pipeline-helper"
        :title "unified-entry-pipeline run_pipeline internal helper v0/v1"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: helper run_pipeline (internal)"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
           "crates/missiond-daemon/src/handlers/knowledge/mod.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.file-first-ssot"
                    "intent-layer.unified-entry-pipeline.alignment-review-gate"
                    "intent-layer.unified-entry-pipeline.plan-review-gate"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"
                    "intent-layer.unified-entry-pipeline.review-gate-policy"
                    "flow.unified-entry-pipeline"
                    "intent-layer.unified-entry-pipeline.no-new-tool-decision"
                    "intent-layer.unified-entry-pipeline.v0-non-goals"]
        :wave "13 task 03 (commit 9759675) + 14 task 04 (commit 338a3fb)"
        :note "v0 internal helper 不新增 MCP tool — 仅 daemon 内部 run_pipeline + 纯函数 plan_pipeline (testable); 复用现有 mission_directive / mission_plan / mission_workflow 管理面 surface; 7 步 pipeline: s1_message_intake → s3_alignment_review_gate → s4_plan_authoring → s5_plan_review_gate → s6_execution_runner; 每 response 携带 pipeline_stage / next_step / flow_ref / expects_next_inputs / next_call (适用时); MCP tool 数量保持 83 不变; +21 tests; wave 14 task 04 (commit 338a3fb) 升 v1: 转发 file-first / review-gate / scheduler args, 每 response 加 artifact_refs (flat object lifting file_*/review_question_*); 完整 actor / autonomous review answer / live execution 仍 pending (4 项 v0 non-goal 仍 surface) — 契约段不压缩; 升: code-aligned-partial → code-aligned (v1 file-first + review-gate + scheduler args 转发)")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.no-new-tool-decision"
        :title "unified-entry no-new-tool decision (v0)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: no-new-tool-decision"
        :status code-aligned
        :compression-safe? true
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "tools.section.tool-governance"]
        :wave "13 task 03 (commit 9759675)"
        :note "wave 13 task 03 决议: 不新增 mission_message / mission_invoke; 复用 mission_directive(action=compile) + mission_plan(action=compile|approve|execute) + mission_workflow(action=distill|record_execution) 管理面 surface; tool_count_invariant=83 不变; rationale 段可压缩")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.v0-non-goals"
        :title "unified-entry pipeline v0 non-goals (4 项)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: v0-non-goals"
        :status pending
        :compression-safe? true
        :implements ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "intent-layer.unified-entry-pipeline.alignment-review-gate"
                    "intent-layer.unified-entry-pipeline.plan-review-gate"
                    "worker.section.claudecode-workstation-orchestration"]
        :wave "13 task 03 (commit 9759675)"
        :v0-non-goals
          ["auto_approve_directive — 不允许 LLM 产物自动越过 alignment-review-gate"
           "auto_approve_plan — 不允许 LLM 产物自动越过 plan-review-gate"
           "auto_answer_review_question — 不替人/Codex 答 QuestionEvent (人工 gate)"
           "autonomous_workstation_dispatch — 不自动 spawn ClaudeCode 工位执行 plan"]
        :surface-rule "每一步 response 在 meta.v0_non_goals 显式 surface 这 4 项, 让 caller 不会误以为系统有自动决策能力"
        :note "v0 non-goals 是契约段, 列表本身不压缩; rationale narrative 可压缩")

      ;; ── 区域 11 · flow / tools cross-references for unified-entry v0 ──
      (section-entry
        :section-id "flow.unified-entry-pipeline.run-pipeline-stages"
        :title "F-intent-alignment-plan-execution-loop :: run_pipeline 7 step mapping"
        :source-file ".missiond/v2/intent-flow.lisp"
        :local-path "pillar flow :: F-intent-alignment-plan-execution-loop :: run_pipeline stage mapping"
        :status code-aligned-partial
        :compression-safe? true
        :implements ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "flow.alignment-review-gate-stage"
                    "flow.plan-review-gate-stage"
                    "flow.execution-runner-dag-scheduler"]
        :wave "13 task 03 (commit 9759675)"
        :note "stage 字符串映射: s1_message_intake (mission_directive compile) → s3_alignment_review_gate (mission_directive approve) → s4_plan_authoring (mission_plan compile) → s5_plan_review_gate (mission_plan approve) → s6_execution_runner (mission_plan execute internal); 不新增 tool, 复用现有管理面; flow_ref 在每 response surface — narrative 段可压缩, stage 名映射 (契约) 不动"))

    ;; ──────────────────────────────────────────────────
    ;; v0.5 (wave 14 task 07) — wave 14 execution status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 14 task 01/02/03/04/05/06 的真实代码状态回填到 source-index
    ;;   - 不做 L2 实际拆分; 拆 shard 仍延后 (本 v0.5 仅做 shard split plan, 见 architecture-dsl.lisp :: l2-shard-split-plan)
    ;;   - 新 entry 在 wave 12 task 06 v0.3 baseline + wave 13 task 04 v0.4 上扩展, 不重复已有 section-id
    ;;
    ;; wave 14 已完成 commit (anchor):
    ;;   - 668952f chore(wave13): archive task briefs (task 00 — 仅归档, 不进 source-index)
    ;;   - 5c60f82 chore(v2): enforce source-index checker rules (task 05)
    ;;   - 00cbc1d feat(knowledge): write file-first artifacts from compiler actors (task 01)
    ;;   - ed25d41 docs(event): remove stale fixed domain count wording (task 06 — wording, 不进 source-index)
    ;;   - 2e7789a feat(execution): publish plan node state changes (task 02)
    ;;   - 96842cd feat(review): auto-create review questions for artifacts (task 03)
    ;;   - 338a3fb feat(intent): route unified entry through file-first gates (task 04)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾 "wave 14 升: X → Y"):
    ;;   - memory.directive-layer.file-first-artifacts                    code-aligned-partial → code-aligned (writer 主路径接入)
    ;;   - intent-layer.unified-entry-pipeline.file-first-ssot            code-aligned-partial → code-aligned
    ;;   - intent-layer.unified-entry-pipeline.alignment-review-gate      code-aligned-partial → code-aligned (auto-create policy 已落)
    ;;   - intent-layer.unified-entry-pipeline.plan-review-gate           code-aligned-partial → code-aligned (auto-create policy 已落)
    ;;   - intent-layer.evidence-collector-event-ref                      code-aligned-partial → code-aligned (live event ref 三层策略已落)
    ;;   - intent-layer.plan-dag-runtime-v2.execution-event-decision      pending → code-aligned (PlanNodeStateChanged variant 已扩, decision 实现)
    ;;   - intent-layer.unified-entry-pipeline.run-pipeline-helper        code-aligned-partial → code-aligned (v1 file-first + review-gate + scheduler args 转发)
    ;; ──────────────────────────────────────────────────
    (wave-14-backfill v0.5
      :date "2026-04-26"
      :decided-by "wave 14 / task 07 lisp backfill + L2 shard split plan session"
      :scope "回填 wave 14 task 01/02/03/04/05 真实代码状态; 新增 5 anchor entry, 升级 7 现有 entry status"
      :non-goal "本任务不真正压缩主 Lisp、不实际拆 shard (L2 shard 实际执行由后续 wave 在 gate 全满足后做; plan 已写入 architecture-dsl.lisp :: l2-shard-split-plan)"
      :commits
        [(commit-1 :hash "5c60f82" :title "chore(v2): enforce source-index checker rules"
                   :primary-targets ["scripts/check-architecture-lisp.mjs"
                                     ".missiond/v2/architecture-dsl.lisp (checker-status wording only)"]
                   :tests "+5 dry-fixture passes (happy / R015 missing / R016 dup / compsafe rejected / compsafe alias)")
         (commit-2 :hash "00cbc1d" :title "feat(knowledge): write file-first artifacts from compiler actors"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/{directive,plan,workflow}.rs"]
                   :tests "directive/plan/workflow file writer tests (missing topic / no overwrite / success / partial path)")
         (commit-3 :hash "2e7789a" :title "feat(execution): publish plan node state changes"
                   :primary-targets ["crates/missiond-core/src/event/events/execution.rs"
                                     "crates/missiond-daemon/src/bus/bootstrap.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
                   :tests "core serde round-trip + plan_dag transition event builder + evidence ref tests")
         (commit-4 :hash "96842cd" :title "feat(review): auto-create review questions for artifacts"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/{directive,plan,workflow}.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/{directive,plan,workflow}.rs"]
                   :tests "review_gate id derivation + compile auto-emit response + legacy no-param byte-identical tests")
         (commit-5 :hash "338a3fb" :title "feat(intent): route unified entry through file-first gates"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
                   :tests "v1 pipeline planner / file-first args forwarding / review-gate forwarding / scheduler args forwarding tests")]

      ;; ── 区域 12 · file-first writer integration (wave 14 task 01) ──
      (section-entry
        :section-id "memory.directive-layer.file-first-writer-integration"
        :title "file-first writer integration — 三类 artifact (alignment / PLAN / workflow methodology) 主路径"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar memory :: module directive-layer :: file-first-artifacts :: writer integration"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["memory.directive-layer.file-first-artifacts"
                    "intent-layer.unified-entry-pipeline.file-first-ssot"
                    "flow.file-vs-db-contract"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper"]
        :wave "14 task 01 (commit 00cbc1d)"
        :note "三类 artifact (directive alignment / PLAN.lisp / workflow methodology) 全部走统一 helper file_artifacts::attempt_artifact_write → resolve_target_project_root → atomic_write_artifact; 严禁 process cwd fallback; DB 已写但 file 失败 → status=partial + file_write_error (不回滚 DB row, 不静默吞错); 6 个 file_* 响应字段 (file_written / file_path / file_sha256 / file_bytes / file_created / file_overwritten); foundation helper file_artifacts.rs dead_code 全清 (仅 2 项 #[allow(dead_code)] + 理由); writer 主路径升 code-aligned, schema/contract 段不压缩")

      (section-entry
        :section-id "tools.surface.directive-write-file-args"
        :title "mission_directive(action=compile) write_file/topic/overwrite_file/project/cwd/target_project args"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_directive :: write_file args"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]
        :cross-ref ["memory.directive-layer.file-first-writer-integration"
                    "tools.surface.plan-write-file-args"
                    "tools.surface.workflow-write-file-args"]
        :wave "14 task 01 (commit 00cbc1d)"
        :note "write_file=true 必须搭配 topic; project (registered id) 优先, cwd (绝对路径) 次之, target_project 兜底; 6 个 file_* 响应字段; partial 语义 (DB 已写, file 失败) — schema 是契约不压缩")

      (section-entry
        :section-id "tools.surface.plan-write-file-args"
        :title "mission_plan(action=compile) write_file/topic/overwrite_file/project/cwd/target_project args"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_plan :: write_file args"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]
        :cross-ref ["memory.directive-layer.file-first-writer-integration"
                    "tools.surface.directive-write-file-args"
                    "tools.surface.workflow-write-file-args"]
        :wave "14 task 01 (commit 00cbc1d)"
        :note "PLAN.lisp 写入 <project_root>/.missiond/plans/<topic>/PLAN.lisp; topic 默认走 board_task_id 兜底; 6 个 file_* 字段 + partial 语义")

      (section-entry
        :section-id "tools.surface.workflow-write-file-args"
        :title "mission_workflow(action=distill|compile_methodology) write_file/topic|name/overwrite_file/project args"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_workflow :: write_file args"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]
        :cross-ref ["memory.directive-layer.file-first-writer-integration"
                    "tools.surface.directive-write-file-args"
                    "tools.surface.plan-write-file-args"]
        :wave "14 task 01 (commit 00cbc1d)"
        :note "workflow .lisp 写入 <project_root>/.missiond/workflows/<topic>.lisp; distill / compile_methodology 两 action 都接 helper; ArtifactKind::Workflow")

      ;; ── 区域 13 · PlanNodeStateChanged event + live EventRef (wave 14 task 02) ──
      (section-entry
        :section-id "event-bus.section.execution-event.plan-node-state-changed"
        :title "ExecutionEvent::PlanNodeStateChanged variant (wave 14 task 02)"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section execution-event :: variant PlanNodeStateChanged"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-core/src/event/events/execution.rs"
           "crates/missiond-daemon/src/bus/bootstrap.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "intent-layer.evidence-collector-event-ref"
                    "intent-layer.plan-dag-runtime-v2.execution-event-decision"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "14 task 02 (commit 2e7789a)"
        :note "扩 ExecutionEvent::PlanNodeStateChanged: 4 必字段 (plan_id / node_id / from / to) + 5 可选字段 (target / dispatch_strategy / target_project / attempt / reason) + serde backward-compat (旧 variants 不破坏); BusServices::publish_execution_with_seq helper; Domain::ALL 仍 13 (扩 variant 不扩 domain 数); event-bus pillar 仍 protected, 本 entry 仅元数据")

      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"
        :title "PLAN DAG live EventRef 三层策略 (wave 14 task 02)"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: live-event-ref-strategy"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "intent-layer.evidence-collector-event-ref"
                    "event-bus.section.execution-event.plan-node-state-changed"]
        :wave "14 task 02 (commit 2e7789a) + 17 task 06 (commit 4074a14)"
        :note "evidence collector EventRef 三层策略: (1) 优先 live id — bus publish 成功 → EventRef::new(execution, plan_node_state_changed, <Seq>); (2) 失败兜底 — bus publish 失败 → EventRef::new(execution, plan_node_state_changed, <deterministic id 'plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>'>) + warning; (3) legacy 占位 — 必要时仍可 EventRef::unavailable(reason); bus failure 仅 observability (warning 字段记录), 不挂主 dispatch; 升 code-aligned. wave 17 task 06 (commit 4074a14) 把 wave-13/14 的 live event ref 三层策略升级到 'live cache → log query → unavailable' 三档 resolver — log query 复用 LogReadable::read_from(Domain::Execution, Seq(0), 512) reverse-lookup deterministic key, cache evict 也能 recover; primary event_ref_status / event_ref_source / event_ref_warning 加在 EvidenceEntry surface; query 失败 → unavailable + reason, 绝不挂 primary dispatch; anchor: intent-layer.evidence-collector-event-log-query-v0")

      ;; ── 区域 14 · review-gate auto-create v1 (wave 14 task 03) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-gate-policy"
        :title "review_gate policy enum (manual|emit_question|off) — auto-create v1"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-gate-policy"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.alignment-review-gate"
                    "intent-layer.unified-entry-pipeline.plan-review-gate"
                    "intent-layer.unified-entry-pipeline.review-gate-id-derivation"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper"]
        :wave "14 task 03 (commit 96842cd) + 18 task 07 (commit 0f55892)"
        :note "review_gate 三态 enum: manual (default, byte-identical legacy) / emit_question (artifact 写入成功后自动 emit QuestionEvent::Created) / off; default 不破 byte-identical; v0 显式 emit (caller 手动 emit_review_question=true) 升级到 v1 policy auto-create. wave 18 task 07 (commit 0f55892) 加 review_automation_policy (enum manual | suggest | auto_safe; default manual 字节兼容) 在 directive / plan / workflow 三 surface; 5 deterministic safety rule 全过才 auto_safe approve (deterministic_mode / no_file_write OR file_hash_match / no_protected / no_additional_blockers / caller_opt_in); existing explicit review-resolution-v0 仍 authoritative 优先于 auto_safe; 绝不 auto_reject (auto_safe 永远 approve-or-defer-to-manual); destructive transitions (archive/supersede) 永不 auto-promote; LLM 自动 approve / 自动 answer review question 仍 v0 non-goal (anchor: intent-layer.unified-entry-pipeline.review-automation-policy-v0)")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-gate-id-derivation"
        :title "review-gate deterministic question id derivation"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-gate-id-derivation"
        :status code-aligned
        :compression-safe? false
        :implements ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.alignment-review-gate"
                    "intent-layer.unified-entry-pipeline.plan-review-gate"]
        :wave "14 task 03 (commit 96842cd)"
        :note "deterministic id 格式: 'review:<scope>:<id>:v<v>:<action>[:<topic-hash>]' — topic-hash = SHA-256 前 16 hex (file_path 优先, 否则 topic); file 写失败时拒发 question + warning (review_gate_policy=emit_question requires file_written=true); approve / archive / mark / supersede 继续支持 review_question_id resolution")

      (section-entry
        :section-id "tools.surface.review-gate-args"
        :title "mission_directive/plan/workflow review_gate_policy + review_question_* args"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: review_gate args"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.review-gate-id-derivation"]
        :wave "14 task 03 (commit 96842cd)"
        :note "新增 args: review_gate_policy (manual|emit_question|off) / emit_review_question (legacy bool, 兼容) / review_question_text / review_question_id; response 4 字段: review_question_emitted / review_question_id / review_gate_policy / review_question_warning; tool count 仍 83 不变")

      ;; ── 区域 15 · unified-entry pipeline v1 (wave 14 task 04) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"
        :title "unified-entry-pipeline run_pipeline v1 — file-first + review-gate + scheduler args 转发"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: helper run_pipeline (v1)"
        :status code-aligned
        :compression-safe? false
        :implements ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "memory.directive-layer.file-first-writer-integration"
                    "intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.plan-dag-runtime-v2"
                    "intent-layer.unified-entry-pipeline.v0-non-goals"]
        :wave "14 task 04 (commit 338a3fb)"
        :note "v0 internal helper (wave 13 task 03) 升 v1 — 不新增 MCP tool (仍 83); v1 新增 args 转发: write_file / topic / overwrite_file / review_gate_policy / project|cwd|target_project / scheduler_mode / max_parallel_nodes; 每 response 携带 pipeline_stage + artifact_refs (新增, flat object lifting file_*/review_question_*) + next_step; 4 v0 non-goal 仍 surface; legacy no file-write path byte-identical 兼容")

      ;; ── 区域 16 · source-index checker R015+R016 implemented (wave 14 task 05) ──
      (section-entry
        :section-id "intent-layer.source-index-checker.r015-r016-implemented"
        :title "source-index checker R015 mandatory-fields + R016 section-id-uniqueness implemented"
        :source-file ".missiond/v2/architecture-dsl.lisp"
        :local-path "defdsl architecture-v1 :: checker-contract :: phase-3.1-precompression-coverage (IMPLEMENTED)"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/check-architecture-lisp.mjs"
           ".missiond/v2/architecture-dsl.lisp (checker-status wording only)"]
        :cross-ref ["intent-layer.execution-handoff-dual-plane"]
        :wave "14 task 05 (commit 5c60f82)"
        :note "checker phase 3.1 从 architecture-designed 升 code-aligned: scripts/check-architecture-lisp.mjs 加 R015 (4 必填字段 :section-id / :source-file / :local-path / :status) + R016 (section-id 全局唯一); :compression-safe? value enum 接受 true|false|yes|no|safe|unsafe|defer; --dry-fixture 自测 5 fixtures PASS (happy / R015 缺字段 / R016 重名 / compsafe rejected / compsafe alias); --all-v2 跑 14 文件 + 93 entry 全合规; :local-path prefix 软规则 (warn-only) 仍 deferred"))

    ;; ──────────────────────────────────────────────────
    ;; v0.6 (wave 15 task 06) — wave 15 execution status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 15 task 01/02/03/04/05 的真实代码状态回填到 source-index
    ;;   - 不重复已有 section-id; 在 v0.5 baseline 上扩 +5 anchor entry, 升 +5 现有 entry status
    ;;   - 保留 R008 + R016 (section-id 不变, 改名走 :prev-id)
    ;;
    ;; wave 15 已完成 commit (anchor):
    ;;   - 2c0799d chore(wave14): archive task briefs (task 00 — 仅归档, 不进 source-index)
    ;;   - ea90c5d test(event): stop hardcoding domain count (task 01 — extensible domain assertion)
    ;;   - 3f37d32 docs(v2): split L2 architecture shards (task 02 — 5 shard 创建 + 6 parent stub + 28 source-index 重定向)
    ;;   - b861b9a chore(v2): make source checker shard-aware (task 03 — R017+R018 + auto-discovery)
    ;;   - 615b249 feat(plan): dispatch workstation tasks from plan nodes (task 05 — opt-in workstation_dispatch v0)
    ;;   - 03513c0 feat(review): resolve review gates explicitly (task 04 — review-resolution bridge v0)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾 "wave 15 升: X → Y"):
    ;;   - intent-layer.unified-entry-pipeline.review-gate-policy           code-aligned → code-aligned-partial (resolution v0 + workflow-handler 仍未接 = partial)
    ;;   - intent-layer.unified-entry-pipeline.review-gate-id-derivation    code-aligned → code-aligned-partial (resolution envelope validator 已落)
    ;;   - intent-layer.actor.plan-dag-scheduler                             code-aligned-partial → code-aligned-partial (workstation_dispatch opt-in 已落, autonomous spawn pending)
    ;;   - intent-layer.unified-entry-pipeline.workstation-dispatch-policy   operational-practice → code-aligned-partial (opt-in v0 经 mission_task_delegate, 自动 dispatch_strategy 推断仍 pending)
    ;;   - intent-layer.source-index-checker.r015-r016-implemented           code-aligned → code-aligned (R017+R018 + shard auto-discovery 扩 phase-3.2)
    ;; ──────────────────────────────────────────────────
    (wave-15-backfill v0.6
      :date "2026-04-26"
      :decided-by "wave 15 / task 06 lisp backfill session"
      :scope "回填 wave 15 task 01/02/03/04/05 真实代码状态; 新增 5 anchor entry, 升级 5 现有 entry status; 不发明 wave15 没实现的架构"
      :non-goal "本任务不真正压缩主 Lisp; 不修改 Rust/SQL/JS/Cargo/任务文档; 不动 event-bus.lisp / intent-mcp-defs.lisp"
      :commits
        [(commit-1 :hash "ea90c5d" :title "test(event): stop hardcoding domain count"
                   :primary-targets ["crates/missiond-core/tests/event_dispatcher_integration.rs"]
                   :tests "domain_all_includes_execution: assert Domain::ALL.contains(Execution) + len() >= 13 floor (extensible, 不锁精确 count)")
         (commit-2 :hash "3f37d32" :title "docs(v2): split L2 architecture shards"
                   :primary-targets [".missiond/v2/intent-execution-governance.lisp"
                                     ".missiond/v2/intent-directive-artifacts.lisp"
                                     ".missiond/v2/intent-plan-dag.lisp"
                                     ".missiond/v2/intent-capability-governance.lisp"
                                     ".missiond/v2/intent-workstation-policy.lisp"
                                     ".missiond/v2/intent-flow.lisp"
                                     ".missiond/v2/intent-intent-layer.lisp"
                                     ".missiond/v2/intent-memory.lisp"
                                     ".missiond/v2/intent-tools.lisp"
                                     ".missiond/v2/intent-worker.lisp"
                                     ".missiond/v2/intent-pillar-source-index.lisp"]
                   :tests "checker --all-v2 19 files OK; 105 section-id 全保 (R008); 28 source-index :source-file 重定向")
         (commit-3 :hash "b861b9a" :title "chore(v2): make source checker shard-aware"
                   :primary-targets ["scripts/check-architecture-lisp.mjs"
                                     ".missiond/v2/architecture-dsl.lisp (R017/R018 + checker phase 3.2 wording)"]
                   :tests "--dry-fixture 5 → 10 (happy / R015 缺字段 / R016 重名 / compsafe rejected / compsafe alias / R017 missing source-file / R018 outside v2 / R018 短路 / pillar-section-index header / shard auto-discovery union); --all-v2 19 files (含 5 wave-15 shard auto-discovered) OK")
         (commit-4 :hash "615b249" :title "feat(plan): dispatch workstation tasks from plan nodes"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs (新建 962 行)"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "agent-team hint exactly once / fresh-code-alignment task brief 含 scoped commit policy / resident-lisp 不退 prompt / project-root unresolved → SafeDescriptor / unsupported hint preserved / dry_run 不写 evidence")
         (commit-5 :hash "03513c0" :title "feat(review): resolve review gates explicitly"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/directive.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "approve via valid review id / stale version rejection / scope mismatch / artifact mismatch / unsupported scope / unsupported action / rejected decision / needs_changes next_step")]

      ;; ── 区域 17 · extensible domain count test (wave 15 task 01) ──
      (section-entry
        :section-id "event-bus.section.execution-event.domain-all-extensible-test"
        :title "Domain::ALL extensibility test — domain_all_includes_execution"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section execution-event :: integration test domain_all_includes_execution"
        :status code-aligned
        :compression-safe? false
        :implements ["crates/missiond-core/tests/event_dispatcher_integration.rs"]
        :cross-ref ["event-bus.pillar-root"
                    "event-bus.section.execution-event.plan-node-state-changed"]
        :wave "15 task 01 (commit ea90c5d)"
        :note "rename test domain_all_length_is_12 → domain_all_includes_execution; 改 contains(Domain::Execution) + len() >= 13 floor; 不再 hardcode 精确 count, 未来扩 domain 不需改 test; event-bus.lisp 正文 protected 不动, 仅在本 source-index 加 metadata entry; legacy fan-out 测试同步将 'all 12 domains' wording 改 'all current domains'")

      ;; ── 区域 18 · L2 shard split executed (wave 15 task 02) ──
      (section-entry
        :section-id "intent-layer.l2-shard-split.executed"
        :title "L2 shard split executed — 5 shard 创建 + parent stub + source-index 重定向"
        :source-file ".missiond/v2/architecture-dsl.lisp"
        :local-path "defdsl architecture-v1 :: l2-shard-split-plan (EXECUTED)"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/v2/intent-execution-governance.lisp"
           ".missiond/v2/intent-directive-artifacts.lisp"
           ".missiond/v2/intent-plan-dag.lisp"
           ".missiond/v2/intent-capability-governance.lisp"
           ".missiond/v2/intent-workstation-policy.lisp"
           ".missiond/v2/intent-flow.lisp"
           ".missiond/v2/intent-intent-layer.lisp"
           ".missiond/v2/intent-memory.lisp"
           ".missiond/v2/intent-tools.lisp"
           ".missiond/v2/intent-worker.lisp"
           ".missiond/v2/intent-pillar-source-index.lisp"]
        :cross-ref ["intent-layer.source-index-checker.r017-r018-implemented"]
        :wave "15 task 02 (commit 3f37d32)"
        :note "L2 shard split 已从 designed → executed: 5 shard 文件创建 (intent-execution-governance / intent-directive-artifacts / intent-plan-dag / intent-capability-governance / intent-workstation-policy); 6 parent 文件 stub 化 (flow / intent-layer / memory / tools / worker / source-index); 28 source-index :source-file 重定向到 5 shard; 105 section-id 全保 (R008 + R016); 内容 byte-identical (cross-cutting-invariants rule-5 no-content-mutation); event-bus.lisp / event-bus-execution.lisp / mcp-defs.lisp 未参与拆分 (frozen rule-6); architecture-dsl.lisp :: l2-shard-split-plan 内 5 shard :status 仍标 'designed (not executed)' — 那是 plan 模板自身, 实际执行状态由本 entry 反映")

      ;; ── 区域 19 · shard-aware checker R017+R018 + auto-discovery (wave 15 task 03) ──
      (section-entry
        :section-id "intent-layer.source-index-checker.r017-r018-implemented"
        :title "source-index checker R017 source-file-must-exist + R018 source-file-must-live-under-v2 + shard auto-discovery"
        :source-file ".missiond/v2/architecture-dsl.lisp"
        :local-path "defdsl architecture-v1 :: checker-contract :: phase-3.2-shard-aware (IMPLEMENTED)"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/check-architecture-lisp.mjs"
           ".missiond/v2/architecture-dsl.lisp (checker phase-3.2-status wording + R017/R018 rules)"]
        :cross-ref ["intent-layer.source-index-checker.r015-r016-implemented"
                    "intent-layer.l2-shard-split.executed"]
        :wave "15 task 03 (commit b861b9a)"
        :note "checker phase 3.2 从 architecture-designed 升 code-aligned: scripts/check-architecture-lisp.mjs 加 R017 (source-file 必存在 — 防 L2 shard rename / move 留死链) + R018 (source-file 必 .missiond/v2/ 起头 — source-index 不退化为通用清单); 同时引入 collectSourceFileRefs 自动从 source-index :source-file 引用反向拉入 shard 文件 (data-driven, 不 hardcode shard 路径); 报告行显示 initial + auto-discovered shard 数; --dry-fixture 5 → 10 (新加 R017 missing / R018 outside / R018 短路 / pillar-section-index header / shard auto-discovery 5 fixtures); --all-v2 跑 19 文件 (含 5 wave-15 shard auto-discovered) 全 OK; :local-path prefix 软规则仍 warn-only deferred")

      ;; ── 区域 20 · review-gate resolution v0 (wave 15 task 04 + wave 16 task 01 workflow) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
        :title "review-gate explicit resolution bridge v0 — review_decision input + envelope validator (workflow handler 已接)"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-gate-resolution-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.review-gate-id-derivation"
                    "intent-layer.unified-entry-pipeline.alignment-review-gate"
                    "intent-layer.unified-entry-pipeline.plan-review-gate"
                    "tools.surface.review-gate-args"]
        :wave "15 task 04 (commit 03513c0) + 16 task 01 (commit 01708be)"
        :note "wave 14 task 03 review-gate 自动 emit QuestionEvent (manual|emit_question|off) → wave 15 task 04 加 explicit resolution bridge v0: 显式输入 review_question_id + review_decision (approved|rejected|needs_changes) + review_actor + review_note; 三 decision 行为: approved → 跑 manager transition (directive_approve / directive_update_status(Archived) / plan_update_status / plan_supersede); rejected → 保持当前 status, 仅记录 review_actor/review_note + 走 status='review_rejected'; needs_changes → 保持 review/draft + surface next_step + 走 status='review_needs_changes'; envelope validator 5 fail-fast 错误 (REVIEW_SCOPE_MISMATCH / REVIEW_SCOPE_UNSUPPORTED / REVIEW_ARTIFACT_MISMATCH / STALE_REVIEW_VERSION / REVIEW_ACTION_UNSUPPORTED) + 2 input 错误 (MISSING_PARAM 缺 review_decision / INVALID_PARAM 未知 decision); 接到现有 directive (approve/archive) + plan (approve/mark/supersede) action, 不新增 MCP tool (tool count 仍 83). wave 16 task 01 (commit 01708be) 升: workflow handler 已接 mission_workflow(action='resolve_review') 5 字段 — methodology YAML 不 fake DB (receipt only); scope 实际是 'workflow' (distill UUID 与 methodology flow_id 由 artifact_id 是不是 UUID 区分); workflow row 无 status/version 列 → 'review_approved/rejected/needs_changes' 仅 stamp 不 DB transition; status: code-aligned-partial → code-aligned (workflow handler 接入闭环). 不实现 UI / autonomous review answer 由 wave 16 task 02 listener 接 (anchor: intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0)")

      (section-entry
        :section-id "tools.surface.review-resolution-args"
        :title "mission_directive/plan/workflow review_decision/review_actor/review_note args"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: review-resolution args (wave 15 + wave 16)"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
                    "tools.surface.review-gate-args"]
        :wave "15 task 04 (commit 03513c0) + 16 task 01 (commit 01708be)"
        :note "wave 15 task 04 在 wave 14 task 03 既有 review_question_id (legacy quiet emit path) 上加 3 args: review_decision (enum approved|rejected|needs_changes; 必填 when review_question_id 出现) / review_actor (free-form identity; echoed) / review_note (free-form note; echoed); response 4 字段: review_decision (echoed) / review_decision_outcome (perform_transition|keep_artifact|request_changes) / review_actor (when supplied) / review_note (when supplied); legacy quiet path (传 review_question_id 不传 review_decision) 字节兼容 — 触发 emit Resolved/legacy 而不走 envelope validator; tool count 仍 83 不变. wave 16 task 01 (commit 01708be) 升: mission_workflow(action='resolve_review') schema 加同 5 字段 (review_artifact_id / review_question_id / review_decision / review_actor / review_note); workflow row 无 status/version 列 → review_approved/rejected/needs_changes 仅 stamp 不 DB transition; methodology YAML 不 fake DB row (receipt only); status: code-aligned-partial → code-aligned (3 surface 全接)")

      ;; ── 区域 21 · workstation dispatch v0 (wave 15 task 05) + auto-inference v1 (wave 16 task 03) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
        :title "workstation-dispatch v0 + auto-inference v1 — opt-in via :workstation-dispatch true / mission_task_delegate transport / 5 inference rules"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: workstation-dispatch-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["worker.section.claudecode-workstation-orchestration"
                    "worker.section.claudecode-workstation-orchestration.dispatch-decision-matrix"
                    "worker.section.claudecode-workstation-orchestration.execution-strategy-record"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-policy"
                    "flow.workstation-dispatch-policy"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "15 task 05 (commit 615b249) + 16 task 03 (commit 8ffa9b2)"
        :note "wave 15 task 05 v0 implementation — 新建 handlers/knowledge/workstation_dispatch.rs (962 行) + 接入 plan.rs (action_execute_internal) + plan_dag.rs (dispatch_node) 两条路径; 严格 opt-in: PLAN.lisp 节点 :workstation-dispatch true (或 plan-level execute args 显式传) 才触发, 默认走原 mission_execution(open) 路径; 走 mission_task_delegate (不 claude -p, 不新增 transport); 任务 brief 含 ## Objective / ## Owned files / ## Forbidden files / ## Acceptance / ## Commit policy + 当 dispatch_strategy=agent-team 加 ## Parallelism hint section + literal '使用 agent-team提高效率' 恰好一次 (idempotent); 失败时返 SafeDescriptor (UnsupportedTarget / ProjectRootUnresolved / MissingObjective) 不静默 fallback prompt mode; 不 join 相对 cwd 到 process cwd; response 字段: workstation_dispatch_status (dispatched | skipped_unsupported_target | skipped_project_root_unresolved | skipped_missing_objective | dry_run | dispatched_inner_error) / dispatch_strategy / task_brief_preview (truncated) / inner_result (when dispatched); evidence sidecar entry 'workstation_dispatch:v0' 接 typed EvidenceEntry; unknown hint 字段保留 in node_hint_summary.unsupported_fields, 不重 interpret arbitrary lisp. wave 16 task 03 (commit 8ffa9b2) auto-inference v1: 5 inference 规则 (target=mission_task_delegate / strategy ∈ 4 strategies {resident-lisp,fresh-code-alignment,agent-team,mixed} / objective 非空 / scoping signal 存在 / 节点未 explicit false); workstation_dispatch_source 5 值 (explicit_arg / plan_hint / inferred / disabled / not_applicable); agent-team literal 仍恰一次; 不为 mission_execution / mission_flow_run 推断; target/project root 未解析时不推断; status: code-aligned-partial → code-aligned (auto-inference 对 mission_task_delegate scoped node 已落). 完全 autonomous spawn (不需要任何 PLAN hint, 由 plan-runner 全局推断/拓展到 mission_execution path) 仍 surface 不实现 (anchor: deferred-coverage)")

      (section-entry
        :section-id "tools.surface.plan-workstation-dispatch-args"
        :title "mission_plan(action=execute) workstation_dispatch + workstation_dispatch_dry_run + node :workstation-dispatch hint + auto-inference (wave 16)"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: workstation-dispatch args (wave 15 + wave 16)"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "15 task 05 (commit 615b249) + 16 task 03 (commit 8ffa9b2)"
        :note "wave 15 task 05 新增 mission_plan(action=execute) args: workstation_dispatch (bool, plan-level opt-in) / workstation_dispatch_dry_run (bool, 不写 evidence/不真派) + PLAN.lisp 节点 :workstation-dispatch <bool> hint (节点级 opt-in, default false); :objective / :owned-files / :forbidden-files / :acceptance / :commit-policy 节点字段 plan-runner 解析后传给 task brief (preserved); :dispatch-strategy 复用既有 enum (resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback / unknown). wave 16 task 03 (commit 8ffa9b2) 加 5 inference rules (target=mission_task_delegate / strategy ∈ 4 strategies / objective 非空 / scoping signal 存在 / 非 explicit false) → workstation_dispatch_source 5 值 (explicit_arg / plan_hint / inferred / disabled / not_applicable) 在 response surface; tool count 仍 83 不变 (在既有 mission_plan execute action 上加 args, 不新增 tool)"))

    ;; ──────────────────────────────────────────────────
    ;; v0.7 (wave 16 task 09) — wave 16 execution status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 16 task 01/02/03/04/05/06/07/08 的真实代码状态回填到 source-index
    ;;   - 不重复已有 section-id; 在 v0.6 baseline 上扩 +6 anchor entry, 升 +6 现有 entry status
    ;;   - 保留 R008 + R016 (section-id 不变, 改名走 :prev-id)
    ;;
    ;; wave 16 已完成 commit (anchor):
    ;;   - c965347 chore(wave15): archive task briefs (task 00 — 仅归档, 不进 source-index)
    ;;   - 01708be feat(review): resolve workflow review gates explicitly (task 01 — workflow handler 接 review-resolution)
    ;;   - 331d1c1 feat(review): consume review question resolutions (task 02 — bus 订阅 QuestionEvent::Resolved listener)
    ;;   - 8ffa9b2 feat(plan): infer workstation dispatch for scoped task nodes (task 03 — 5 inference rules + 5 source 值)
    ;;   - a51bc52 feat(plan): pause DAG nodes for review gates (task 04 — paused 7th lifecycle + review-gate question-event trigger)
    ;;   - d8f8a6e feat(plan): retry DAG node dispatch attempts (task 05 — per-node retry policy v0)
    ;;   - 591d288 feat(execution): enforce scoped commit handoff on request (task 06 — 4 错误码 + opt-in default false)
    ;;   - 0e6ee63 feat(evidence): attach live event refs when available (task 07 — passive subscriber cache + 三档 status)
    ;;   - a632a91 test(intent): add unified entry smoke coverage (task 08 — deterministic 4 hand-off no-LLM smoke)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.unified-entry-pipeline.review-gate-resolution-v0      code-aligned-partial → code-aligned (workflow handler 接入)
    ;;   - tools.surface.review-resolution-args                                code-aligned-partial → code-aligned (3 surface 全接)
    ;;   - intent-layer.actor.plan-dag-scheduler                               code-aligned-partial → code-aligned-partial (paused + retry 已落; 完整 11-stage 仍 pending)
    ;;   - flow.execution-runner-dag-scheduler                                  code-aligned-partial → code-aligned-partial (paused + retry 已落; 完整 11-stage 仍 pending)
    ;;   - memory.directive-layer.plan-node-state-projection                   code-aligned-partial → code-aligned-partial (per-attempt evidence + paused 已落; claim_id/acceptance/rollback 仍 pending)
    ;;   - intent-layer.unified-entry-pipeline.workstation-dispatch-v0         code-aligned-partial → code-aligned (auto-inference v1 已落 for mission_task_delegate)
    ;;   - tools.surface.plan-workstation-dispatch-args                         code-aligned-partial → code-aligned (auto-inference 已落)
    ;;   - intent-layer.evidence-collector-event-ref                            code-aligned → code-aligned (subscriber 三档 live/log/unavailable 已落, 是 wave-14 三层策略的 receiver 落地)
    ;; ──────────────────────────────────────────────────
    (wave-16-backfill v0.7
      :date "2026-04-26"
      :decided-by "wave 16 / task 09 lisp backfill session"
      :scope "回填 wave 16 task 01-08 真实代码状态; 新增 6 anchor entry, 升级 6 现有 entry status; 不发明 wave16 没实现的架构"
      :non-goal "本任务不真正压缩主 Lisp; 不修改 Rust/SQL/JS/Cargo/任务文档; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp"
      :commits
        [(commit-1 :hash "01708be" :title "feat(review): resolve workflow review gates explicitly"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
                   :tests "workflow_resolve_review approve / methodology_no_db_transition / scope='workflow' artifact UUID vs methodology flow_id 区分 / workflow row 无 status 列 stamp-only")
         (commit-2 :hash "331d1c1" :title "feat(review): consume review question resolutions"
                   :primary-targets ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-daemon/src/handlers/mod.rs"]
                   :tests "spawn_review_resolution_sub 与 spawn_decision_sub 并行 / subscribe QuestionEvent::Resolved / conservative vocabulary mapping (approved/approve/yes/accepted → approved 等) / ack 后 ignore 非 review id / parse_subscriber_resolution_string 纯函数")
         (commit-3 :hash "8ffa9b2" :title "feat(plan): infer workstation dispatch for scoped task nodes"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "5 inference rules / workstation_dispatch_source 5 值 / agent-team literal 仍恰一次 / 不为 mission_execution / mission_flow_run 推断 / target/project root 未解析时不推断 / 节点 :workstation-dispatch false 不被覆盖")
         (commit-4 :hash "a51bc52" :title "feat(plan): pause DAG nodes for review gates"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "paused 7th lifecycle / 节点 :review-gate 'question-event' 触发 / deterministic review id 'review:plan:<plan_id>:v<v>:plan-node:<sha256(node_id)[..16]>' / aggregate_status='dag_paused' / runner_status='review_gate_paused' / bus failure → 仍 pause + warning / 不 auto-resume")
         (commit-5 :hash "d8f8a6e" :title "feat(plan): retry DAG node dispatch attempts"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests ":retry-count (additional) / :max-attempts (total) / :retry-delay-ms cap 60s / cap 3 attempts / 每 attempt 写自己 evidence (attempt number) / SafeDescriptor refusals 不 retry (UnsupportedTarget/ProjectRootUnresolved/MissingObjective) / failure-policy 与 retry 正交 (retry exhaust 后 propagate_taint)")
         (commit-6 :hash "591d288" :title "feat(execution): enforce scoped commit handoff on request"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
                   :tests "enforce_scoped_commit=true opt-in / 4 错误码 (COMMIT_HASH_REQUIRED / COMMIT_BLOCKER_REQUIRED / CLAIM_SCOPE_REQUIRED / SCOPED_COMMIT_VIOLATION) / gate 在 allocate_id 之前 (rejected 不 bump state) / scope-overlap 与 audit/claim 同一 scopes_overlap helper / daemon 不跑 git / response scoped_commit_enforced + scoped_commit_validation")
         (commit-7 :hash "0e6ee63" :title "feat(evidence): attach live event refs when available"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-daemon/src/bus/v2_subscribers.rs"
                                     "crates/missiond-daemon/src/bus/bootstrap.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
                   :tests "passive subscriber cache (cap 1024 FIFO) / key 'plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>' 严格匹配 / 三档 status live/log/unavailable / EventRef::new alias EventRef::live 保 wave-13/14 byte-compat / subscriber observation-only")
         (commit-8 :hash "a632a91" :title "test(intent): add unified entry smoke coverage"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
                   :tests "deterministic 4 hand-off (s1 directive dry_run / s4 plan dry_run / s6 execute dry_run / s6 evidence sidecar) / no LLM / no spawn / 断言 v0_non_goals 持续 surface")]

      ;; ── 区域 22 · workflow review-resolution handler 接入 (wave 16 task 01) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-gate-resolution-v0.workflow-handler"
        :title "review-gate resolution v0 — workflow handler 接入 (wave 16 task 01)"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-gate-resolution-v0 :: workflow handler"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
                    "tools.surface.review-resolution-args"]
        :wave "16 task 01 (commit 01708be)"
        :note "mission_workflow(action='resolve_review') 接 5 字段 (review_artifact_id / review_question_id / review_decision / review_actor / review_note); methodology YAML 不 fake DB row (receipt only — methodology 由 distill UUID 与 generated flow_id 区分: artifact_id 是 UUID → workflow row, 否则 → methodology flow_id receipt); workflow row 无 status/version 列 → 'review_approved/rejected/needs_changes' 仅 stamp 不 DB transition; envelope validator 5 fail-fast 错误码与 directive/plan 同 (REVIEW_SCOPE_MISMATCH / REVIEW_SCOPE_UNSUPPORTED / REVIEW_ARTIFACT_MISMATCH / STALE_REVIEW_VERSION / REVIEW_ACTION_UNSUPPORTED); scope 实际是 'workflow' (vs directive/plan)")

      ;; ── 区域 23 · review-gate question listener v0 (wave 16 task 02 + wave 17 task 01 plan-node route) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0"
        :title "review-gate QuestionEvent::Resolved subscriber listener v0 (plan-node auto-route)"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-gate-resolution-listener-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/mod.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
                    "intent-layer.unified-entry-pipeline.review-gate-policy"
                    "event-bus.section.knowledge-event.question-resolved"]
        :wave "16 task 02 (commit 331d1c1) + 17 task 01 (commit a42f5fd)"
        :note "wave 16 task 02: spawn_review_resolution_sub 与既有 spawn_decision_sub 并行启动 (bus subscriber 双 listener); subscribe QuestionEvent::Resolved → 解析 review id (deterministic shape 'review:<scope>:<id>:v<v>:<action>[:<topic-hash>]') → ack 后 ignore 非 review id (不抢非 review 域 question); 抽 pure planner ReviewResolvedDispatch + parse_subscriber_resolution_string 进 review_gate.rs 便于测试; conservative vocabulary mapping (approved/approve/yes/accepted → approved; rejected/reject/no/declined → rejected; needs_changes/needs-changes/changes_requested → needs_changes); 改 mod knowledge → pub(crate) mod knowledge 让 bus subscriber 能 import bridges. wave 17 task 01 (commit a42f5fd) 升: spawn_review_resolution_sub 加 plan-node action 自动 route — id scope=plan + action=plan-node 时 listener 自动调用 wave-17 paused-node resume helper (approved → 重 dispatch 节点; rejected/needs_changes → 写 evidence keep paused); 非 plan-node id 保留旧行为 (走 wave-15 review-resolution bridge); 未识别 resolution string 永远 ignore + warn, 绝不 dispatch; 4 v0 non-goal 中 auto_answer_review_question 仍 surface (listener 只 consume answer, 不替人答); 升: code-aligned-partial → code-aligned (auto-resume round-trip 已闭环, plan-node action 与 directive/plan 各自 routing)")

      ;; ── 区域 24 · plan DAG paused lifecycle + review-gate trigger (wave 16 task 04) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.paused-lifecycle"
        :title "PLAN DAG paused 7th lifecycle + review-gate question-event trigger v0"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: paused-lifecycle"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "intent-layer.plan-dag-runtime-v2.node-lifecycle"
                    "intent-layer.actor.plan-dag-scheduler"
                    "intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.review-gate-id-derivation"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "16 task 04 (commit a51bc52) + 17 task 01 (commit a42f5fd)"
        :note "wave 16 task 04: paused 是 6 主 + 3 skip 子分类之外的第 7 主态 (与 actor plan-dag-scheduler 节点 FSM enum 的 paused 对齐) — 节点 :review-gate 'question-event' (+ 可选 :review-action 'approve|archive|mark|supersede' / :review-text) 触发; deterministic review id 'review:plan:<plan_id>:v<v>:plan-node:<sha256(node_id)[..16]>' (paused 节点的 review id 命名固定, 与 directive/plan compile 阶段的 review id 形状区分); response 字段: aggregate_status='dag_paused' / runner_status='review_gate_paused'; bus QuestionEvent::Created 失败时仍 pause 节点 + 报 warning (不阻塞); 不实现 auto-resume — wave 16 task 04 阶段 paused 节点的重激活仍 pending. wave 17 task 01 (commit a42f5fd) auto-resume 已落: mission_plan(action=execute) 加 4 字段 (resume_review_question_id / resume_review_decision / resume_actor / resume_note); 同一 review id format reverse-parse 到 plan/version/node-hash 后唯一 map 到节点; 3 decision (approved → fresh attempt 1 重 dispatch + paused→resume_approved + ready→running + running→{succeeded|failed} 三条 evidence; rejected → 节点保持 paused + paused→resume_rejected; needs_changes → paused + paused→resume_needs_changes + next_step); spawn_review_resolution_sub (wave 16 task 02) 扩 — 收到 QuestionEvent::Resolved scope=plan + action=plan-node 时 listener 自动调同一 resume helper; 升: code-aligned-partial → code-aligned (paused-resume 协议 round-trip 已通)")

      ;; ── 区域 25 · plan DAG per-node retry policy v0 (wave 16 task 05) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.retry-policy-v0"
        :title "PLAN DAG per-node retry policy v0 — :retry-count + :max-attempts + :retry-delay-ms"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: retry-policy-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2"
                    "intent-layer.plan-dag-runtime-v2.failure-policy"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "16 task 05 (commit d8f8a6e)"
        :note "节点字段 (per-node, 都 optional): :retry-count (additional 重试次数, 不含首次) / :max-attempts (total attempts 上限, 含首次) / :retry-delay-ms (重试间隔, cap 60s); 全局 cap 3 attempts (含首次, 防止无界 retry); 每 attempt 写自己一条 evidence (含 attempt number, 不复用上一次 entry); SafeDescriptor refusals (UnsupportedTarget / ProjectRootUnresolved / MissingObjective) 不 retry (这些是确定性输入失败, retry 无意义); failure-policy 与 retry 正交 — retry exhaust 后才走 failure-policy 分支 (fail-fast 标 failed → propagate_taint; continue 标 failed 但下游不被阻塞); 不实现 backoff 算法 (现 fixed delay), retry-N 完整版/exponential backoff/route-to-rollback 仍 architecture-designed pending; 完整 11-stage scheduler 的 s9 handle-retry-failure-rollback 仍 pending — 本 entry 仅覆盖 retry 子集")

      ;; ── 区域 26 · scoped commit enforce v0 (wave 16 task 06) ──
      (section-entry
        :section-id "intent-layer.scoped-commit-enforce-v0"
        :title "scoped commit handoff enforce v0 — opt-in default false / 4 错误码 / gate 在 allocate_id 之前"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section scoped-commit-handoff :: enforce v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["worker.section.claudecode-workstation-orchestration"
                    "memory.helper.agent-execution-coordination"]
        :wave "16 task 06 (commit 591d288) + 17 task 07 (commit a21b8ae) + 18 task 08 (commit f171ae6)"
        :note "wave 16 task 06: mission_execution 接 enforce_scoped_commit (bool, opt-in default false — 字节兼容); enforce=true 时 4 fail-fast 错误码: COMMIT_HASH_REQUIRED (completion 缺 commit_hash) / COMMIT_BLOCKER_REQUIRED (completion 标 blocked 缺 blocker reason) / CLAIM_SCOPE_REQUIRED (claim 时缺 scope) / SCOPED_COMMIT_VIOLATION (changed_files 越出 claim scope); gate 在 allocate_id 之前 (rejected 不 bump state, 错误 caller 重提不会用掉新 id); scope-overlap 检查与 audit/claim 共享同一 scopes_overlap helper (统一定义 scope 字符串前缀语义); daemon 不跑 git (不 spawn git 命令; commit_hash 由 caller 提供); response 字段: scoped_commit_enforced (echoed bool) / scoped_commit_validation (object with passed/violations); 不 enforce 模式下原行为不变 (字节 compat). wave 17 task 07 (commit a21b8ae) brief-layer enforce 默认开 — workstation_dispatch.rs 任务 brief 加 section 7 'Completion handoff (scoped commit)'; code task (owned_files 非空) → 要求 enforce_scoped_commit=true / commit_status=committed / commit_hash / staged_files; read-only task (owned_files 空) → commit_status=not-required + 解释; response 加 scoped_commit_required: true / scoped_commit_policy: 'enforced-on-complete'; daemon agent_execution.rs 完全未动 (legacy mission_execution 全局 enforce 仍 opt-in default false 字节兼容); brief 与 daemon 双层契约清晰: brief 强制 worker 显式 commit, daemon legacy enforce 仍可被旧 caller bypass (但不影响新 brief). wave 18 task 08 (commit f171ae6) 加 read-only preflight tool: mission_execution 加 action='preflight_commit' 只调 git status --porcelain=v1 (grep proof 0 mutating subcommand: 不 add / 不 commit / 不 stash / 不 reset / 不 checkout); 返结构化 ok / changed_files / staged_files / out_of_scope_files / expected_owned_files / expected_untouched_files / next_step (建议: ready_to_commit / stage_required / out_of_scope_review / claim_scope_required); preflight 设计为新派工 worker 在 complete 前自检, daemon 仍不跑 git mutating commands (与 wave 16-06 daemon 不跑 git 契约一致); brief → preflight → complete enforce 三段闭环 (anchor: intent-layer.scoped-commit-worktree-preflight-v0)")

      ;; ── 区域 27 · unified-entry e2e smoke coverage v0 (wave 16 task 08) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.e2e-smoke-v0"
        :title "unified-entry e2e smoke coverage — deterministic 4 hand-off no-LLM"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: e2e-smoke-v0"
        :status code-aligned
        :compression-safe? true
        :implements ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "intent-layer.unified-entry-pipeline.v0-non-goals"]
        :wave "16 task 08 (commit a632a91)"
        :note "deterministic 4 hand-off smoke (no LLM, no spawn, all dry_run): s1 directive dry_run → s4 plan dry_run → s6 execute dry_run → s6 evidence sidecar; 全程断言 v0_non_goals 在每 response surface (auto_approve_directive / auto_approve_plan / auto_answer_review_question / autonomous_workstation_dispatch 4 项); 不打 LLM, 不 spawn 工位, 用作 unified_entry pipeline contract 的回归基线 (smoke regression); 是 wave 13 task 03 v0 + wave 14 task 04 v1 的 e2e 测试覆盖, 不引入新行为"))

    ;; ──────────────────────────────────────────────────
    ;; v0.8 (wave 17 task 09) — wave 17 execution status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 17 task 01-08 的真实代码状态回填到 source-index
    ;;   - 不重复已有 section-id; 在 v0.7 baseline 上新增 7 anchor entry, 升级 6 现有 entry status/note
    ;;   - 保留 R008 + R016 (section-id 不变, 改名走 :prev-id)
    ;;   - 11-stage scheduler 大量 close (s5 claim-lease ✓ / s7 acceptance ✓ / s8 paused-resume ✓ /
    ;;     s9 rollback ✓ / s10 mark-final ✓ / s11 distill 触发 ✓); 仍 partial 是因为 cross-plan
    ;;     distill chain / 跨节点 acceptance fan-in / live LLM auto-approve 等仍未实现
    ;;
    ;; wave 17 已完成 commit (anchor):
    ;;   - 181172e chore(wave16): archive task briefs (task 00 — 仅归档, 不进 source-index)
    ;;   - a42f5fd feat(plan): resume paused DAG nodes after review (task 01 — paused-resume hook v0)
    ;;   - 8661fcb feat(plan): add claim lease state to DAG nodes (task 02 — claim/lease v0 + Claimed 第 8 态)
    ;;   - f572729 feat(plan): evaluate DAG node acceptance safely (task 03 — acceptance evaluator v0 三模式)
    ;;   - d4466c6 feat(plan): record rollback policy for DAG failures (task 04 — rollback policy v0 三模式 + 5 safety gates)
    ;;   - 402fb82 feat(plan): finalize DAG execution and trigger distill (task 05 — finalize + distill trigger v0)
    ;;   - 4074a14 feat(evidence): resolve event refs from event log (task 06 — event-log query path v0 三档 resolver)
    ;;   - a21b8ae feat(workstation): require scoped commit handoff in task briefs (task 07 — brief-layer scoped-commit default v1)
    ;;   - c7fbac0 test(intent): cover paused DAG resume smoke (task 08 — unified-entry paused-resume e2e smoke + build_plan_execute_args 7 keys forward)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.actor.plan-dag-scheduler                              note 扩 (claim-lease/acceptance/rollback/finalize/paused-resume 已落, 仍 code-aligned-partial 因 cross-plan 仍 pending)
    ;;   - flow.execution-runner-dag-scheduler                                 note 扩 (同上, 仍 code-aligned-partial)
    ;;   - memory.directive-layer.plan-node-state-projection                   note 扩 (claim_id/acceptance_pass/rollback_path 全落, 仅 cross-plan 仍 future)
    ;;   - intent-layer.evidence-collector-event-ref                           note 扩 + wave 17 task 06 三档 resolver (live cache → log query → unavailable)
    ;;   - intent-layer.scoped-commit-enforce-v0                               note 扩 (wave 17 task 07 brief-layer enforce 默认开; daemon legacy 仍 opt-in)
    ;;   - intent-layer.plan-dag-runtime-v2.paused-lifecycle                  code-aligned-partial → code-aligned (paused-resume 已闭环 by wave 17 task 01)
    ;;   - intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0 code-aligned-partial → code-aligned (wave 17 task 01 plan-node action auto-route)
    ;; ──────────────────────────────────────────────────
    (wave-17-backfill v0.8
      :date "2026-04-26"
      :decided-by "wave 17 / task 09 lisp backfill session"
      :scope "回填 wave 17 task 01-08 真实代码状态; 新增 7 anchor entry, 升级 6 现有 entry status/note; 不发明 wave17 没实现的架构"
      :non-goal "本任务不真正压缩主 Lisp; 不修改 Rust/SQL/JS/Cargo/任务文档; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp (continue postpone)"
      :commits
        [(commit-1 :hash "a42f5fd" :title "feat(plan): resume paused DAG nodes after review"
                   :primary-targets ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "mission_plan(action=execute) 4 字段 (resume_review_question_id / resume_review_decision / resume_actor / resume_note); plan-node id 'review:plan:<plan_id>:v<v>:plan-node:<sha256(node_id)[..16]>'; 3 decision (approved 重新 dispatch fresh attempt 1 / rejected 不 dispatch / needs_changes 仍 paused); spawn_review_resolution_sub 扩 plan-node action 自动 route 到 resume helper; evidence paused→resume_*")
         (commit-2 :hash "8661fcb" :title "feat(plan): add claim lease state to DAG nodes"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "claim_lease_secs / claimer_name / enforce_claims 3 args; Claimed 第 8 lifecycle (Pending → Claimed → Running); claim scope 优先级 :owned-files → :scope → plan/<plan_id>/node/<node_id> fallback; scopes_overlap_pure 暴露给 3 处共享 (claim/audit/scoped-commit); enforce_claims default false byte-compat")
         (commit-3 :hash "f572729" :title "feat(plan): evaluate DAG node acceptance safely"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "3 modes (inner_status / evidence_keys / manual); 绝不 shell (grep proof 0 命中); id space 'acceptance:plan:<plan_id>:v<v>:<node_id>' 与 review-gate 'review:*' 隔离 (各自 routing); manual_required 复用 Paused 不新加态; evidence succeeded→acceptance_*")
         (commit-4 :hash "d4466c6" :title "feat(plan): record rollback policy for DAG failures"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "3 modes (none/descriptor/workstation); 5 safety gates (missing objective / empty owned-files / no project signal / unsafe strategy / SafeDescriptor refusal); workstation 复用 build_task_brief / run_workstation_dispatch; rollback 在 final fail 后 / downstream taint 前; evidence failed→rollback_*")
         (commit-5 :hash "402fb82" :title "feat(plan): finalize DAG execution and trigger distill"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "finalize_plan / distill_on_success / distill_mode (default false / false / dry_run); 4 finalization rule (succeeded → succeeded; failed → failed; partial → failed; paused → unchanged 不撒谎); distill 经现有 super::workflow::handle() 不动 workflow.rs; distill 失败 warning 仍保 plan final state")
         (commit-6 :hash "4074a14" :title "feat(evidence): resolve event refs from event log"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
                   :tests "resolver 三档 (live cache → log query 复用 LogReadable::read_from(Domain::Execution, Seq(0), 512) 不加 store method → unavailable); primary event_ref_status / event_ref_source / event_ref_warning 加在 EvidenceEntry surface; query 失败 unavailable + reason, 绝不挂 primary dispatch")
         (commit-7 :hash "a21b8ae" :title "feat(workstation): require scoped commit handoff in task briefs"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "task brief section 7 'Completion handoff (scoped commit)'; code task → enforce_scoped_commit=true / commit_status=committed / commit_hash / staged_files; read-only task → commit_status=not-required + 解释; response scoped_commit_required: true / scoped_commit_policy: 'enforced-on-complete'; daemon agent_execution.rs 完全未动 (legacy compat)")
         (commit-8 :hash "c7fbac0" :title "test(intent): cover paused DAG resume smoke"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
                   :tests "build_plan_execute_args forward 7 个 wave-17 keys (4 resume + 3 finalize); smoke 6 步 (s1 → s4 → s6 paused → resume validate round-trip → s6 resumed succeeded → sidecar 同 plan); no LLM no spawn no shell")]

      ;; ── 区域 28 · PLAN DAG paused-resume hook v0 (wave 17 task 01) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.paused-resume-hook-v0"
        :title "PLAN DAG paused-node resume hook v0 — mission_plan(execute) 4 resume args + listener auto-route"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: paused-resume-hook-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/bus/v2_subscribers.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.paused-lifecycle"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "17 task 01 (commit a42f5fd)"
        :note "mission_plan(action=execute) 加 4 字段 (都 optional 字节兼容): resume_review_question_id (= 上一次 wave-16 paused dispatch 返回的 'review:plan:<plan_id>:v<v>:plan-node:<sha256(node_id)[..16]>') / resume_review_decision (enum approved|rejected|needs_changes; 必填 when resume_review_question_id 出现, 缺失 → MISSING_PARAM) / resume_actor (free-form identity, echoed) / resume_note (free-form note, echoed); 4 字段同时出现 routing 到 dedicated paused-node resume helper, 与 wave-15 review_question_id+review_decision (manager-side compile/approve/mark/supersede) 严格分离; helper 校验 envelope: plan id 一致 / plan version 一致 / action=plan-node / node-hash 唯一 map 到一个仍带 :review-gate 'question-event' 的节点; 3 decision 分支: approved → 节点以 fresh attempt 1 重 dispatch (paused 不算失败 attempt) + 写 paused→resume_approved + ready→running + running→{succeeded|failed} 三条 evidence; rejected → 节点保持 paused + 写 paused→resume_rejected; needs_changes → 节点保持 paused + 写 paused→resume_needs_changes + surface next_step; 只 resume 这一个节点, downstream pending 节点不动 (后续 execute 推进); spawn_review_resolution_sub (wave 16 task 02) 扩 — 收到 QuestionEvent::Resolved id scope=plan + action=plan-node 时 listener 自动调同一 helper, 非 plan-node id 保留旧行为; 未识别 resolution 字符串永远 ignore + warn, 绝不 dispatch; 4 v0 non-goal 中 auto_answer_review_question 仍 surface (listener 只 consume answer, 不替人答, 仅做 plan-node action 自动 re-dispatch)")

      ;; ── 区域 29 · PLAN DAG claim/lease v0 + Claimed 第 8 lifecycle (wave 17 task 02) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.claim-lease-v0"
        :title "PLAN DAG claim/lease v0 — Claimed 第 8 lifecycle + scopes_overlap_pure 共享"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: claim-lease-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.node-lifecycle"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.helper.agent-execution-coordination"
                    "intent-layer.scoped-commit-enforce-v0"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "17 task 02 (commit 8661fcb)"
        :note "mission_plan(action=execute, scheduler_mode=dag_v1) 加 3 args (都 optional): claim_lease_secs (default 1800, clamp [60, 14400]) / claimer_name (default 'plan-dag-scheduler', whitespace 归一) / enforce_claims (default false, byte-compat). Node FSM enum 加 Claimed 作第 8 主态 (与既有 paused 第 7 态对齐, FSM enum 整套现 8 主态: pending/ready/claimed/running/succeeded/failed/skipped/paused, 加 retrying/rolling-back 共 10 + 3 skip 子分类); per-node lifecycle becomes pending → claimed → running → {succeeded|failed} → released, 每次转移一条 evidence (claim_id 写在 pending→claimed, claim_released_at 与 claim_terminal_state 写在 claimed→released). claim scope 优先级: 节点 :owned-files (非空) → 节点 :scope → 合成 fallback 'plan/<plan_id>/node/<node_id>' (永不为空). enforce_claims=true 时遇 unresolvable scope overlap 立即 fail-fast 节点 (state=failed, reason=CLAIM_CONFLICT..., inner handler 永不调用, payload 含 claim_status=conflict + 冲突 snapshot); enforce_claims=false 时仅记录 evidence + response 但 NEVER block dispatch (向后兼容). scopes_overlap_pure 抽出 (公开纯函数 prefix-segment overlap), 共享给 3 处: plan_dag claim / agent_execution audit / scoped-commit enforce — 三处不会再 drift. dry_run 在响应里附 planned_claims[] 含每个节点的 claim_id/scopes/scope_source/lease_secs/enforce_claims 不 mutate. 复用 wave12-01 mission_execution lease defaults / claimer 词汇, 两 surface 时间戳与 audit 词汇一致. status code-aligned-partial 因为: enforce_claims default false 是 opt-in 兼容期; 后续切默认 enforce / scope-overlap 跨多 plan 全局视图 仍 pending")

      ;; ── 区域 30 · PLAN DAG acceptance evaluator v0 (wave 17 task 03 + wave 18 task 03 cross-node fan-in) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.acceptance-evaluator-v0"
        :title "PLAN DAG acceptance evaluator v0 — 三模式 inner_status / evidence_keys / manual + 隔离 id space + cross-node fan-in"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: acceptance-evaluator-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.failure-policy"
                    "intent-layer.plan-dag-runtime-v2.paused-lifecycle"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "17 task 03 (commit f572729) + 18 task 03 (commit 76ada10)"
        :note "PLAN.lisp 节点新增三 hint 字段 (都 optional): :acceptance-mode (enum inner_status | evidence_keys | manual; 与 mission_plan(action=execute) acceptance_mode arg 对齐) / :acceptance-evidence-keys (list[string], 仅 evidence_keys 模式) / :acceptance-commands (list[string], 声明意图但 scheduler 永不执行). 触发时机: 在节点 inner dispatch 成功 (running → succeeded) 之后, 在 propagate_taint 之前. evaluator 是 PURE (no shell, no subprocess; grep proof 0 shell command spawn 命中); 三 mode 行为: inner_status → 接受当且仅当 inner Ok 且 payload 无显式 error signal; evidence_keys → 扫描 inner payload (top-level 然后 well-known nested holders evidence/typed_evidence/inner_result/inner_dispatch/result), 全部 declared key 命中才 accept (空 list 或 raw not array → manual_required 兜底); manual → 总是 pause. 兜底规则: 节点声明 :acceptance-commands 但缺 typed :acceptance-mode → manual_required (显式拒绝执行 PLAN.lisp 内 shell). 三结果: accepted → 节点保持 succeeded; rejected → 节点翻 failed (taint 沿原 :failure-policy 传播, fail-fast 仅在 retry exhaust 后才 trip); manual_required → 节点 pause + deterministic id 'acceptance:plan:<plan_id>:v<v>:<node_id>' (与 wave-16 review-gate id 'review:*' 完全 ID space 隔离, wave-17 paused-node resume helper 故意 NOT consume acceptance:* id, 后续 wave 单独 actor). evidence 写一条 succeeded → acceptance_{accepted|rejected|manual_required}; node_results[].acceptance 块含 mode/status/keys/commands/commands_executed=false (pinned). 节点未声明任何 acceptance hint → 保持 wave-13 succeed-on-dispatch 契约 (acceptance 块 omitted from response). wave 18 task 03 (commit 76ada10) 加 cross-node fan-in: PLAN.lisp 节点新加 :acceptance-depends-on (list[node-id]) / :acceptance-requires (enum all_succeeded | any_succeeded | evidence_keys; evidence_keys 模式必须搭配 :acceptance-evidence-keys + 新加 :acceptance-source-node 单一 node-id) + 4 validator rejection (missing dep / requires missing / source invalid / not ancestor cycle guard). 评估时机: 在本地 acceptance-evaluator-v0 之后, 在 propagate_taint 之前. evidence 写 acceptance_fan_in 块 (mode / depends_on / source_node / status / matched_keys / unmet_keys). 接收下游节点继承 paused 兜底 (上游 acceptance 未通过节点不 dispatch). status: code-aligned-partial → code-aligned (cross-node fan-in 已闭环). LLM auto-approve manual_required 仍 future (anchor: intent-layer.plan-dag-runtime-v2.cross-node-acceptance-fan-in-v0)")

      ;; ── 区域 31 · PLAN DAG rollback policy v0 (wave 17 task 04 + wave 18 task 04 cascade rollback) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.rollback-policy-v0"
        :title "PLAN DAG rollback policy v0 — 三模式 none/descriptor/workstation + 5 safety gates + cascade rollback"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: rollback-policy-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.failure-policy"
                    "intent-layer.plan-dag-runtime-v2.retry-policy-v0"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "17 task 04 (commit d4466c6) + 18 task 04 (commit 7de886c)"
        :note "PLAN.lisp 节点新增四 hint 字段 (都 optional): :rollback-policy (enum none|descriptor|workstation; default 缺省=none, 保 wave 13/16 字节兼容) / :rollback-objective (free-form, workstation 模式必填非空) / :rollback-owned-files (list[string], workstation 模式必填 >=1 entry) / :rollback-acceptance-commands (list[string], 仅声明永不执行). 三 mode 行为: none → 节点 failed 不做 rollback (preserves wave-13/16 contract); descriptor → 仅在 response + 一条 audit evidence 记录 rollback 意图 (objective + owned files + acceptance commands + brief preview), NEVER dispatch; workstation → 仅当 5 safety gates 全过才 dispatch rollback 通过 wave-15 workstation-dispatch substrate (复用 build_task_brief / run_workstation_dispatch). 5 safety gates: (1) target-project 或 requested-cwd 已解析 (no-project-signal 失败), (2) :rollback-objective 非空 (missing-objective 失败), (3) :rollback-owned-files >=1 entry (empty-owned-files 失败), (4) :dispatch-strategy 在安全白名单 {fresh-code-alignment, resident-lisp, agent-team, mixed} (unsafe-strategy 失败), (5) substrate 未返 SafeDescriptor refusal (substrate-side refusal 同样 collapse 到 refused). 任一失败 → rollback_status='refused' + 失败条件原因, 永不 retry. 触发时机: 在节点最终失败 attempt **之后** (retry exhaust + failure-policy 已生效), 在 downstream taint propagation **之前**; 下游行为仍由 existing :failure-policy 决定, rollback 永不 alter taint / fail-fast. evidence 写一条 failed → rollback_{not_requested|descriptor_ready|dispatched|refused|failed}; node_results[].rollback 块含 policy/status/reason/objective/owned_files/acceptance_commands/acceptance_commands_executed=false (pinned, 证明永不执行). wave 18 task 04 (commit 7de886c) 加 cascade rollback v0: PLAN.lisp 节点新加 :compensates (list[node-id]) / :rollback-cascade (enum none | plan | dispatch-safe; default none 字节兼容) / :rollback-after (list[node-id] 显式控制顺序); cascade 计算: Kahn 拓扑排序 + cycle-tolerant fallback (检测 cycle 时降级到 conservative 顺序而不 fail); 5 safety triggers (workstation rollback prerequisite missing / target-project unresolved / unsafe-strategy / SafeDescriptor refusal / cascade graph cycle 时降级); 复用 wave 17-04 workstation_dispatch substrate; cascade 中 acceptance_commands_executed: false 钉死; cascade 不 alter taint / fail-fast. evidence 写 rollback_cascade 块 (mode / cascade_plan / triggered_nodes / skipped_nodes / refused_nodes / cycle_fallback?). status: code-aligned-partial → code-aligned (cascade-up 已闭环). compensate-node :ref 引用 (forward 引用同 plan 内补偿节点; :compensates 是 reverse 方向) 仍 future (anchor: intent-layer.plan-dag-runtime-v2.cascade-rollback-v0)")

      ;; ── 区域 32 · PLAN DAG finalize + distill trigger v0 (wave 17 task 05 + wave 18 task 05 cross-plan chain) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.finalize-distill-v0"
        :title "PLAN DAG finalize + distill trigger v0 — finalize_plan / distill_on_success / distill_mode opt-in + cross-plan chain"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: finalize-distill-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.actor.plan-dag-scheduler"
                    "intent-layer.plan-dag-runtime-v2"
                    "memory.directive-layer.plan-node-state-projection"
                    "memory.directive-layer.plan-evidence-sidecar"]
        :wave "17 task 05 (commit 402fb82) + 18 task 05 (commit 76d818d)"
        :note "mission_plan(action=execute, scheduler_mode=dag_v1) 加 3 args (都 optional, 都 opt-in 字节兼容): finalize_plan (default false; 关时只跑 plan_update_status 现有 side-effect, response byte-shape 与 wave-17 task 04 baseline 一致) / distill_on_success (default false; gate 在 finalize_plan=true 之后, distill_on_success=true 但 finalize_plan=false → INVALID_PARAM fail-fast 不静默掩盖 caller intent) / distill_mode (default 'dry_run'; 接 mission_workflow(action=distill) parse_distill_mode 同 allowlist, sonet 等 typo 即使在 dry_run 也 fail-fast). 4 finalization rule (映射 aggregate→plan_status): dag_succeeded → succeeded; dag_failed → failed; dag_partial → failed; dag_paused → unchanged 不撒谎 (runner 拒绝在 paused 节点等 review 时 claim 'success'). distill 触发: finalize_plan=true ∧ aggregate=dag_succeeded ∧ plan FSM update 落 'succeeded' → 自动调用既有 super::workflow::handle(action=distill, plan_id=<plan>) (不动 workflow.rs 内部); 项目 / cwd / target_project 信号 verbatim forward, 让 distill handler 解析与 DAG run evidence 同一 project root; finalize 写一条 plan-level dag_finalized audit evidence (含 aggregate→plan_status projection); response 加 finalization 块 (含 distill 子块 triggered/reason/distill_mode/result/warning); distill 失败 surface 一条 non-fatal warning, 绝不 downgrade plan final state (符合 task brief: distill failure does NOT corrupt plan final state). wave 18 task 05 (commit 76d818d) 加 cross-plan chain v0: mission_plan(action=execute) 加 3 args (都 optional): distill_chain_id (free-form chain identifier, 缺省 = 不入 chain; 同一 chain_id 跨 plan 共享形成 plan→plan→plan 链) / distill_chain_mode (enum record_only | dry_run | sonnet, default record_only) / distill_chain_name (free-form display name); chain metadata 写 evidence sidecar 子块 distill_chain (chain_id / mode / name / parent_plan_id / position_in_chain / created_at), **无 SQL migration**; 3 modes (record_only 仅 sidecar 不调 workflow handler / dry_run 沿 wave 17-05 dry_run / sonnet 仅在显式触发 + parent succeeded + parent distill_on_success=true); 5 skip 状态 (chain_id_missing / mode_record_only / parent_not_succeeded / parent_distill_off / sonnet_not_explicit); append-only 3 层保证 (entries 只追加, chain id immutable, sidecar 写失败 → warning 但仍保 plan finalization). status: code-aligned-partial → code-aligned (cross-plan chain 闭环). 自动跨 plan 触发 chain (无需 caller 显式 distill_chain_id) + 完整 sonnet 自动接 仍 future (anchor: intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0)")

      ;; ── 区域 33 · evidence event-log query path v0 (wave 17 task 06) ──
      (section-entry
        :section-id "intent-layer.evidence-collector-event-log-query-v0"
        :title "evidence-collector event-log query path v0 — 三档 resolver (live cache → log query → unavailable)"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role evidence-collector :: event-log query path v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["intent-layer.evidence-collector-event-ref"
                    "intent-layer.evidence-collector-typed-helper"
                    "event-bus.section.execution-event.plan-node-state-changed"
                    "intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"]
        :wave "17 task 06 (commit 4074a14)"
        :note "wave 16 task 07 已落 passive subscriber cache (cap 1024 FIFO, key 'plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>' 严格匹配 deterministic event id) — 但 cache 容量满 + 持久 query 仍走 unavailable. wave 17 task 06 加 resolver 三档 fallback chain: (1) live cache (subscriber 命中 deterministic key → live id + Seq, status='live'); (2) log query — 复用 LogReadable::read_from(Domain::Execution, Seq(0), 512) 不加 store method (按 deterministic id reverse-lookup persistent event log, 写过的事件即使 cache evict 也能 recover, status='log' + warning='recovered from event log'); (3) unavailable — query 失败兜底 unavailable + reason 必填. 三档结果统一通过新加在 EvidenceEntry surface 的 primary 字段 surface: event_ref_status (enum 'live'|'log'|'unavailable'), event_ref_source (含 domain), event_ref_warning (optional, log mode 默认 'recovered from event log'). subscriber 仍严格 observation-only (不 mutate 任何主路径); query 失败 → unavailable + reason, 绝不挂 primary dispatch (paused/succeeded/failed 等 dispatch path 与 ref resolver 完全解耦). 持久化 event log 查询面 ad-hoc reverse-lookup 已落; 按 plan_id / node_id / time-range 系统化 query API 仍 future")

      ;; ── 区域 34 · workstation scoped-commit handoff brief default v1 (wave 17 task 07) ──
      (section-entry
        :section-id "intent-layer.scoped-commit-handoff-brief-default-v1"
        :title "workstation scoped-commit handoff brief default v1 — task brief section 7 + scoped_commit_required pinned"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section scoped-commit-handoff :: brief-default v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.scoped-commit-enforce-v0"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "worker.section.claudecode-workstation-orchestration"
                    "memory.helper.agent-execution-coordination"]
        :wave "17 task 07 (commit a21b8ae)"
        :note "wave 17 task 07 brief-layer enforce 默认开 — workstation_dispatch.rs::build_task_brief 任务 brief 加 section 7 'Completion handoff (scoped commit)' (现 brief 7 sections: 1 Objective / 2 Owned files / 3 Forbidden files / 4 Acceptance / 5 Commit policy / 6 Parallelism hint when agent-team / 7 Completion handoff (scoped commit)). 区分 code task vs read-only task: code task (owned_files 非空) → brief 显式要求 worker 在 mission_execution(action=complete) 调用时附 enforce_scoped_commit=true / commit_status='committed' / commit_hash / staged_files; read-only task (owned_files 空) → brief 接受 commit_status='not-required' + summary 内书面解释 (不强制 commit). response 总是附 scoped_commit_required: true / scoped_commit_policy: 'enforced-on-complete' (4 个 outcome dispatched/dry_run/inner_error/safe_descriptor 全 4 测 OK). daemon agent_execution.rs 完全未动 (legacy mission_execution(action=complete) callers 仍 enforce_scoped_commit default false 字节兼容; 旧 caller 不传 enforce_scoped_commit 走原行为). brief 与 daemon 双层契约清晰: brief 强制 worker 显式 commit (新派工 contract), daemon legacy enforce 仍可被旧 caller bypass (字节兼容). 后续可由 plan-runner 自动注入 enforce_scoped_commit=true 给所有 worker action_complete 调用 (兜底 brief escape) 仍 future. agent-team literal '使用 agent-team提高效率' 在 dispatch_strategy=agent-team 时仍恰一次 (新加 section 不影响 idempotent invariant)")

      ;; ── 区域 35 · unified-entry paused-resume e2e smoke v0 (wave 17 task 08) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.paused-resume-smoke-v0"
        :title "unified-entry paused-resume e2e smoke v0 — 6 step deterministic round-trip + build_plan_execute_args 7 keys forward"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: paused-resume-smoke-v0"
        :status code-aligned
        :compression-safe? true
        :implements ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "intent-layer.unified-entry-pipeline.e2e-smoke-v0"
                    "intent-layer.unified-entry-pipeline.v0-non-goals"
                    "intent-layer.plan-dag-runtime-v2.paused-resume-hook-v0"
                    "intent-layer.plan-dag-runtime-v2.finalize-distill-v0"]
        :wave "17 task 08 (commit c7fbac0)"
        :note "wave 16 task 08 落 deterministic 4 hand-off no-LLM smoke (s1→s4→s6 + sidecar). wave 17 task 08 加: build_plan_execute_args 扩 forward 7 个 wave-17 keys (4 resume: resume_review_question_id / resume_review_decision / resume_actor / resume_note + 3 finalize: finalize_plan / distill_on_success / distill_mode); 加 build_plan_execute_args_forwards_wave17_resume_keys + build_plan_execute_args_resume_keys_absent_when_omitted 两个 unit test 锁 forward 行为. 加 6 步 e2e smoke (no LLM no spawn no shell): s1 directive create → s4 plan compile → s6 first execute (节点 :review-gate 'question-event' → paused, sidecar 写 review_question_id) → resume validate 同 round-trip review id 形状 + 4 resume args → s6 second execute (resume_review_decision='approved' 路由 paused-node helper, fresh attempt 1 succeeded) → sidecar 同 plan 写两份 evidence (paused→resume_approved + ready→running + running→succeeded). 不引入新 MCP tool, 不动 workflow.rs / plan.rs 主路径; 用作 wave-17-01 (paused-resume hook) + wave-17-05 (finalize_plan) 联合 contract 的回归基线. wave-17 / Task 02 claim/lease lifecycle / Task 04 rollback / Task 06 event-log resolver / Task 07 workstation brief 由各自 unit test cover (本 smoke 不重复 cover); 在测试模板里显式 list 哪些 wave-17 task 由本 smoke 覆盖 vs 由其他 unit test 覆盖, 避免误以为 e2e cross-cuts 全部. wave 18 task 09 (commit d440087) 加 autonomous loop smoke v1 是 wave 17-08 的 next-gen — forward 4 wave-18 keys (distill_chain_id / distill_chain_mode / distill_chain_name + infer_plan_fields), 5 stage 走通 wave 17/18 多任务 (anchor: intent-layer.unified-entry-pipeline.autonomous-loop-smoke-v1)"))

      ;; ── 区域 36 · machine-contract task protocol v1 (manual architecture upgrade) ──
      (section-entry
        :section-id "intent-layer.machine-contract.task-contract-v1"
        :title "machine-contract task protocol v1 — Lisp SSOT + rendered ClaudeCode Markdown view"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: task-contract-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["scripts/check-task-contract.mjs"
           "scripts/render-claudecode-task.mjs"
           "scripts/lib/missiond_lisp.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"
           ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp"
           ".missiond/claudecode/wave19-00-machine-contract-pilot.md"]
        :cross-ref ["architecture-dsl.lisp :: R019-R022"
                    "intent-layer.unified-entry-pipeline"
                    "worker.section.claudecode-workstation-orchestration"
                    "memory.helper.agent-execution-coordination"]
        :wave "manual architecture upgrade after wave18 dispatch"
        :note "task.lisp is now the dispatch SSOT: checker validates required boundaries, write-scope, must-not-touch, acceptance and commit policy; renderer creates the ClaudeCode Markdown execution view. Markdown remains compatibility surface, not source of truth. Verifier/report-contract and direct MissionD task dispatch remain future.")

    ;; ──────────────────────────────────────────────────
    ;; v0.9 (wave 18 task 10) — wave 18 execution status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 18 task 01-09 的真实代码状态回填到 source-index
    ;;   - 不重复已有 section-id; 在 v0.8 baseline 上新增 9 anchor entry, 升级 8 现有 entry status/note
    ;;   - 保留 R008 + R016 (section-id 不变, 改名走 :prev-id)
    ;;   - 11-stage scheduler 几乎全部 close (s5/s7/s8/s9/s10/s11 + cross-node fan-in + cascade
    ;;     rollback + cross-plan distill chain 全部 ✓); 仍 partial 是 LLM auto-approve 全部 v0
    ;;     non-goal / autonomous workstation spawn 完全无 hint / git pre-commit hook / frontend
    ;;     lisp 仍未实现
    ;;
    ;; wave 18 已完成 commit (anchor):
    ;;   - f082554 chore(wave17): archive task briefs (task 00 — 仅归档, 不进 source-index)
    ;;   - 016487f feat(event): add bounded event log query API (task 01 — typed builder + EventRefProvenance 4 值 + 512 cap)
    ;;   - 2fbaa88 feat(execution): include dispatch metadata in events (task 02 — Claimed/Completed 加 dispatch_strategy/target_project/requested_cwd)
    ;;   - 76ada10 feat(plan): support cross-node acceptance fan-in (task 03 — :acceptance-depends-on / :acceptance-requires (all_succeeded|any_succeeded|evidence_keys) + 4 validator rejection)
    ;;   - 7de886c feat(plan): plan cascade rollback for DAG failures (task 04 — :compensates / :rollback-cascade (none|plan|dispatch-safe) + Kahn 排序 + 5 safety triggers)
    ;;   - 76d818d feat(workflow): record cross-plan distill chains (task 05 — distill_chain_id / distill_chain_mode (record_only|dry_run|sonnet) / distill_chain_name 写 evidence sidecar 无 migration)
    ;;   - 91f8fa1 feat(plan): infer safe PLAN fields deterministically (task 06 — infer_plan_fields (off|preview|apply_safe) 6 字段 + 3 confidence 仅 high apply_safe; 无 LLM)
    ;;   - 0f55892 feat(review): add explicit automation policy (task 07 — review_automation_policy (manual|suggest|auto_safe) 5 deterministic safety rule; 绝不 auto_reject)
    ;;   - f171ae6 feat(execution): preflight scoped commit worktree state (task 08 — mission_execution(action='preflight_commit') 只调 git status --porcelain=v1 0 mutating subcommand)
    ;;   - d440087 test(intent): cover autonomous loop smoke (task 09 — unified_entry::build_plan_execute_args forward 4 wave-18 keys + 5 stage no LLM no spawn no shell)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.actor.plan-dag-scheduler                              note 扩 (cross-node fan-in/cascade rollback/distill chain/inference 已落); status code-aligned-partial → code-aligned (留 LLM auto-approve / 完全自动 spawn 仍 partial)
    ;;   - flow.execution-runner-dag-scheduler                                 note 扩 (同上); status code-aligned-partial → code-aligned
    ;;   - memory.directive-layer.plan-node-state-projection                   note 扩 (cross-node acceptance fan-in / cascade rollback / chain metadata sidecar 已落)
    ;;   - intent-layer.evidence-collector-event-ref                           note 扩 (typed query API v1 + EventRefProvenance 4 值 + 512 cap)
    ;;   - intent-layer.plan-dag-runtime-v2.acceptance-evaluator-v0           note 扩 (cross-node fan-in 已落 by wave 18 task 03); status code-aligned-partial → code-aligned (LLM manual_required 仍 future)
    ;;   - intent-layer.plan-dag-runtime-v2.rollback-policy-v0                 note 扩 (cascade rollback v0 已落 by wave 18 task 04); status code-aligned-partial → code-aligned (留 enforce default 仍 partial)
    ;;   - intent-layer.plan-dag-runtime-v2.finalize-distill-v0               note 扩 (cross-plan chain v0 已落 by wave 18 task 05); status code-aligned-partial → code-aligned (留 sonnet 自动接 仍 future)
    ;;   - intent-layer.scoped-commit-enforce-v0                               note 扩 (preflight tool by wave 18 task 08; 仍是 daemon 不跑 git mutating)
    ;; ──────────────────────────────────────────────────
    (wave-18-backfill v0.9
      :date "2026-04-26"
      :decided-by "wave 18 / task 10 lisp backfill session"
      :scope "回填 wave 18 task 01-09 真实代码状态; 新增 9 anchor entry, 升级 8 现有 entry status/note; 不发明 wave18 没实现的架构"
      :non-goal "本任务不真正压缩主 Lisp; 不修改 Rust/SQL/JS/Cargo/任务文档; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp (continue postpone)"
      :commits
        [(commit-1 :hash "016487f" :title "feat(event): add bounded event log query API"
                   :primary-targets ["crates/missiond-core/src/event/log/mod.rs"
                                     "crates/missiond-core/src/event/log/query.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
                   :tests "EventLogQuery typed builder (filter by domain/topic/seq range/event id/since/limit) + EventLogQueryable trait + blanket impl 任何 LogReadable 自动获 query; EVENT_LOG_QUERY_LIMIT_CAP=512 硬上限 (绝不允许 caller 透出无界 read); EventRefProvenance 四值 enum (live | passive_cache | event_log_query | unavailable) 暴露在 EvidenceEntry / EventRef surface; evidence resolver 切到 typed query 替换 wave-17 的 ad-hoc LogReadable::read_from(Domain::Execution, Seq(0), 512); 无 SQL migration (event_log 已存在 store)")
         (commit-2 :hash "2fbaa88" :title "feat(execution): include dispatch metadata in events"
                   :primary-targets ["crates/missiond-core/src/event/events/execution.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
                   :tests "ExecutionEvent::Claimed + ExecutionEvent::Completed 加 dispatch_strategy / target_project / requested_cwd 三 optional 字段 (serde-bw-compat: 旧 payload 解析仍 OK); 派 event 时不要求 caller 重传 — handler 从 companion log meta 读取 (wave-13/14 已写入); bootstrap.rs 未改 (subscriber wiring 不动); 与 wave 14 task 02 PlanNodeStateChanged 扩展一致, 把 ExecutionEvent dispatch metadata 闭环 (close 'wave-14-02 留的 PlanNodeStateChanged 之外的 variants 仍未扩同字段' deferred 项)")
         (commit-3 :hash "76ada10" :title "feat(plan): support cross-node acceptance fan-in"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "PLAN.lisp 节点新加 :acceptance-depends-on (list[node-id], 同一 plan 内已声明节点) + :acceptance-requires (enum all_succeeded | any_succeeded | evidence_keys, evidence_keys 模式必须搭配 :acceptance-evidence-keys) + :acceptance-source-node (单一 node-id, evidence_keys 模式下指定哪个上游节点的 evidence 被扫描); 4 validator rejection (missing dep / requires missing / source invalid / not ancestor cycle guard 拒绝引用尚未 succeeded 或不在合法 ancestor 集的节点); evidence 写一块 acceptance_fan_in (含 mode / depends_on / source_node / status / matched_keys / unmet_keys); 无新 lifecycle 态 (复用 acceptance-evaluator-v0 的 accepted/rejected/manual_required); 接收下游节点继承 paused 兜底 — 上游 acceptance 未通过节点不会 dispatch")
         (commit-4 :hash "7de886c" :title "feat(plan): plan cascade rollback for DAG failures"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "PLAN.lisp 节点新加 :compensates (节点 id list, 用于声明本节点是哪些节点的补偿) + :rollback-cascade (enum none | plan | dispatch-safe) + :rollback-after (节点 id list, 显式控制 cascade 的触发顺序); Kahn 拓扑排序计算 cascade plan, cycle-tolerant fallback (检测 cycle 时降级到 conservative 顺序而不是 fail); 5 safety triggers (workstation rollback prerequisite missing / target-project unresolved / unsafe-strategy / SafeDescriptor refusal / cascade graph cycle 时降级); 复用 wave 17-04 的 workstation_dispatch.build_task_brief / run_workstation_dispatch substrate; cascade 中 acceptance_commands_executed: false 钉死 (cascade 仍仅 dispatch remediation 工位, 永不本地跑 acceptance shell); evidence 含 rollback_cascade 块 (mode / cascade_plan / triggered_nodes / skipped_nodes / refused_nodes)")
         (commit-5 :hash "76d818d" :title "feat(workflow): record cross-plan distill chains"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "mission_plan(action=execute) 加 3 args (都 optional, 都 opt-in 字节兼容): distill_chain_id (free-form chain identifier, 缺省 = 不入 chain) / distill_chain_mode (enum record_only | dry_run | sonnet, default record_only — sonnet 仅在显式触发 + 上游 plan distill_on_success=true 才走; record_only 仅写 chain metadata sidecar 不调 workflow handler) / distill_chain_name (free-form display name); chain metadata 写 evidence sidecar (sidecar 子块 distill_chain — chain_id / mode / name / parent_plan_id / position_in_chain / created_at), 无 SQL migration; 3 modes: record_only → 仅写 sidecar, 永不调用 workflow handler; dry_run → 沿 wave 17-05 finalize+distill 路径走 dry_run mode; sonnet → 沿 wave 17-05 路径走 sonnet mode (仅在显式触发); 5 skip 状态 (chain_skipped reasons: chain_id_missing / mode_record_only / parent_not_succeeded / parent_distill_off / sonnet_not_explicit); append-only 3 层保证 (chain entries 只追加; chain id 在 plan run 内 immutable; sidecar 写失败 → warning + 仍保 plan finalization); distill 失败 → warning, plan 仍 finalize success (与 wave 17-05 契约一致)")
         (commit-6 :hash "91f8fa1" :title "feat(plan): infer safe PLAN fields deterministically"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "mission_plan(action=execute) 加 1 arg infer_plan_fields (enum off | preview | apply_safe, default off, 字节兼容); 6 字段 inference target (target / dispatch_strategy / target_project / owned_files / acceptance_mode / workstation_dispatch); 3 confidence (high / medium / low) — 仅 high confidence 才在 apply_safe 模式落 PLAN; medium/low 在 preview 模式 surface 但 apply_safe 拒绝; 3 evidence sources (plan_sexp 节点已声明字段 / compiled_from upstream evidence sidecar / sidecar manifest); conflict 永不静默 override — 推断与 caller 显式 hint 冲突时 fail-fast (INFER_CONFLICT) 不掩盖 caller intent; preview 模式仅 surface inference 结果在 response, 不修改 plan; apply_safe 仅在 high confidence + 节点未声明字段 + caller 未显式传 hint 时落; **完全无 LLM** — 推理走 deterministic heuristics (字符串前缀 / known-strategy 映射 / file-extension 推断), 不 spawn / 不 shell")
         (commit-7 :hash "0f55892" :title "feat(review): add explicit automation policy"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/directive.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
                   :tests "mission_directive / plan / workflow 全部加 review_automation_policy (enum manual | suggest | auto_safe, default manual 字节兼容); 5 deterministic safety rule 必须全过才 auto_safe approve: deterministic_mode (compiler/distiller 走 deterministic 而非 sonnet) / no_file_write OR file_hash_match (artifact 文件未变或 sha256 与 review submission 一致) / no_protected (改的字段不在 protected list — 例如 directive non-goals / plan rollback / workflow audit) / no_additional_blockers (review_question 无 unresolved blocker / dependency) / caller_opt_in (caller 显式传 review_automation_policy=auto_safe, 不允许从 default manual 隐式升级); default = manual byte-compat (legacy callers 行为不变); existing explicit review resolution (wave 15-04 的 review_decision + review_actor + review_note) 仍 authoritative — auto_safe 仅在无 explicit resolution 时介入, 不能覆盖人工裁决; 绝不 auto_reject (auto_safe 永远是 approve-or-defer-to-manual; rejected/needs_changes 永远是人工 input); destructive transitions (directive archive / plan supersede) 永不 auto-promote; 4 v0 non-goal 中 auto_approve_directive / auto_approve_plan 在 caller 显式 auto_safe + 5 rule 全过时 deterministic 满足, LLM 自动 approve / autonomous answer review question 仍 v0 non-goal")
         (commit-8 :hash "f171ae6" :title "feat(execution): preflight scoped commit worktree state"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
                   :tests "mission_execution 加 action='preflight_commit'; 只调 git status --porcelain=v1 (grep proof 0 mutating subcommand: 不 add / 不 commit / 不 stash / 不 reset / 不 checkout / 不 push); 返结构化 ok (bool) / changed_files (list) / staged_files (list) / out_of_scope_files (相对 caller 提供的 claim_scope 越界文件) / expected_owned_files (caller 声明应改的文件 list, 与 staged 对照) / expected_untouched_files (caller 声明不应改的文件) / next_step (建议: ready_to_commit / stage_required / out_of_scope_review / claim_scope_required); preflight 设计为新派工 worker 完成前自检, 让 worker 在 mission_execution(action=complete) enforce_scoped_commit 之前先看清 worktree; daemon 仍不跑 git mutating commands (与 wave 16-06 契约一致); 与 wave 17-07 brief section 7 配合: brief 要求 commit, preflight tool 帮 worker 看 worktree, complete enforce_scoped_commit 锁住 commit_hash 必填 — 三段闭环")
         (commit-9 :hash "d440087" :title "test(intent): cover autonomous loop smoke"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
                   :tests "unified_entry::build_plan_execute_args forward 4 wave-18 keys (distill_chain_id / distill_chain_mode / distill_chain_name + infer_plan_fields); 加 build_plan_execute_args_forwards_wave18_chain_keys + build_plan_execute_args_forwards_wave18_infer_key + build_plan_execute_args_wave18_keys_absent_when_omitted 三个 unit test 锁 forward 行为; 5 stage smoke 走通 wave17/18 多任务 (不只是单 plan 单 hand-off): s1 directive create → s4 plan compile → s6 first execute (含 wave 17/18 keys forward) → s6 second execute (autonomous loop 第二轮 distill chain + infer) → sidecar verify (chain metadata + inference evidence + paused-resume + finalize 全部 surface 可读); **no LLM, no spawn, no shell**; sidecar 共享 (5 stage 写同一 plan sidecar; chain metadata append-only 验证); v0_non_goals 每 stage surface (auto_approve_directive / auto_approve_plan / auto_answer_review_question / autonomous_workstation_dispatch + 新加: review_automation_policy=auto_safe 仍 caller-driven / infer_plan_fields=apply_safe 仅 high-confidence 落 / distill_chain_mode=sonnet 必须 explicit + parent succeeded + parent distill_on_success=true / preflight_commit 仅 status read 不 mutate); wave 18 task 09 是 wave 17-08 paused-resume e2e smoke v0 + wave 18 全 task 字段 forward 的 superset")]

      ;; ── 区域 37 · evidence event-log query API v1 (wave 18 task 01) ──
      (section-entry
        :section-id "intent-layer.evidence-collector-event-log-query-v1"
        :title "evidence event-log query API v1 — typed builder + EventLogQueryable trait + EventRefProvenance 4 值 + 512 cap"
        :source-file ".missiond/v2/intent-intent-layer.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: role evidence-collector :: event-log query API v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-core/src/event/log/mod.rs"
           "crates/missiond-core/src/event/log/query.rs"
           "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["intent-layer.evidence-collector-event-ref"
                    "intent-layer.evidence-collector-event-log-query-v0"
                    "event-bus.section.persistence-layer"
                    "intent-layer.plan-dag-runtime-v2.live-event-ref-strategy"]
        :wave "18 task 01 (commit 016487f)"
        :note "wave 17 task 06 (commit 4074a14) 加 ad-hoc resolver 三档 (live cache → log query 复用 LogReadable::read_from → unavailable). wave 18 task 01 把 ad-hoc 升级到 typed query API: EventLogQuery (typed builder filter by domain / topic / seq-range / event-id / since / limit), EventLogQueryable trait + blanket impl 任何 LogReadable 都自动获 query (无需 store-method), EVENT_LOG_QUERY_LIMIT_CAP=512 硬上限 (caller 不能 read 无界, 防止 OOM 误用); EvidenceEntry surface 升级到 EventRefProvenance 4 值 enum (live | passive_cache | event_log_query | unavailable) — wave 17 的 'live | log | unavailable' 三档展开 (passive_cache vs event_log_query 区分: cache 命中 = passive_cache; cache miss + 持久 query 命中 = event_log_query); evidence resolver 切到 typed query (调用 EventLogQuery::new().domain(Execution).topic(...).limit(N).execute(reader)); 无 SQL migration (event_log 已存在 store, 仅加 query 包装); 持久化按 plan_id / node_id / time-range 系统化 query API 已通; 后续 dedicated store method (例如 by_topic 索引下推) 仍 future")

      ;; ── 区域 38 · ExecutionEvent dispatch metadata v1 (wave 18 task 02) ──
      (section-entry
        :section-id "event-bus.section.execution-event.dispatch-metadata-v1"
        :title "ExecutionEvent::Claimed + Completed dispatch metadata v1 — dispatch_strategy / target_project / requested_cwd 加全 variant"
        :source-file ".missiond/v2/intent-event-bus.lisp"
        :local-path "pillar event-bus :: section execution-event :: dispatch-metadata-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-core/src/event/events/execution.rs"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
        :cross-ref ["event-bus.section.execution-event.plan-node-state-changed"
                    "intent-layer.plan-dag-runtime-v2"
                    "intent-layer.plan-dag-runtime-v2.execution-event-decision"
                    "memory.helper.agent-execution-coordination"]
        :wave "18 task 02 (commit 2fbaa88)"
        :note "wave 14 task 02 (commit 2e7789a) 已加 PlanNodeStateChanged variant 含 dispatch_strategy / target_project. wave 18 task 02 把 dispatch metadata 扩到 ExecutionEvent::Claimed + ExecutionEvent::Completed 两个 variant — 都 optional 字段 (serde-bw-compat: 旧 payload 解析仍 OK); handler 不要求 caller 重传 — 从 companion log meta 读 (wave-13/14 已写入), 派 event 时自动注入; bootstrap.rs / subscriber wiring 完全未改 (现有订阅者天然消费新字段); 闭环 wave 14 task 02 留下的 'PlanNodeStateChanged variant 已扩 dispatch_strategy/target_project, 但 ExecutionEvent::Opened 等其他 variant 仍未扩同字段' deferred 项 (Claimed + Completed 闭, ExecutionEvent::Opened 仍未扩同字段 — 因为 Opened 写在 wave 13 task 02 plan_runner_dispatch entry 之前, 当时还没 dispatch_strategy 协议; 完整 ExecutionEvent dispatch metadata 全 variant 扩同字段仍 future); event-bus pillar 仍 protected, 本 entry 仅元数据")

      ;; ── 区域 39 · PLAN DAG cross-node acceptance fan-in v0 (wave 18 task 03) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.cross-node-acceptance-fan-in-v0"
        :title "PLAN DAG cross-node acceptance fan-in v0 — :acceptance-depends-on / :acceptance-requires (all_succeeded|any_succeeded|evidence_keys) + 4 validator rejection"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: cross-node-acceptance-fan-in-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.acceptance-evaluator-v0"
                    "intent-layer.plan-dag-runtime-v2.failure-policy"
                    "intent-layer.plan-dag-runtime-v2.paused-lifecycle"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "18 task 03 (commit 76ada10)"
        :note "wave 17 task 03 (commit f572729) acceptance evaluator v0 三模式 (inner_status / evidence_keys / manual) 仅在节点本地评估 — fan-in (节点 acceptance 引用 cross-node evidence/output) 仍 future. wave 18 task 03 加 fan-in: PLAN.lisp 节点新加三 hint 字段 (都 optional): :acceptance-depends-on (list[node-id], 同一 plan 内已声明节点) / :acceptance-requires (enum all_succeeded | any_succeeded | evidence_keys; evidence_keys 模式必须搭配既有 :acceptance-evidence-keys + 新加的 :acceptance-source-node 单一 node-id); 4 validator rejection (missing dep / requires missing / source invalid / not ancestor cycle guard 拒绝引用尚未在 ancestor closure 内的节点, 防止前向引用或自循环); 模式行为: all_succeeded → 全部 :acceptance-depends-on 节点 succeeded 才 accept; any_succeeded → 任一 succeeded 即 accept; evidence_keys → 扫描 :acceptance-source-node 的 evidence (复用 wave 17-03 的 nested holder 扫描), 全部 declared key 命中才 accept; 评估时机: 在节点 inner dispatch 成功 + 本地 acceptance-evaluator-v0 通过之后, 在 propagate_taint 之前; 评估失败 → 节点 acceptance_status='rejected' + 翻 failed (taint 沿 :failure-policy 传播); evidence sidecar 写一块 acceptance_fan_in (mode / depends_on / source_node / status / matched_keys / unmet_keys); 无新 lifecycle 态 (复用 acceptance-evaluator-v0 的 accepted/rejected/manual_required); 接收下游节点继承 paused 兜底 — 上游 acceptance 未通过节点不 dispatch; cross-node fan-in 已闭环, 自动 LLM auto-approve manual_required 仍 future")

      ;; ── 区域 40 · PLAN DAG cascade rollback v0 (wave 18 task 04) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.cascade-rollback-v0"
        :title "PLAN DAG cascade rollback v0 — :compensates / :rollback-cascade (none|plan|dispatch-safe) / :rollback-after + Kahn 排序 + 5 safety triggers"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: cascade-rollback-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.rollback-policy-v0"
                    "intent-layer.plan-dag-runtime-v2.failure-policy"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "18 task 04 (commit 7de886c)"
        :note "wave 17 task 04 (commit d4466c6) rollback policy v0 三模式 (none/descriptor/workstation) + 5 safety gates 仅做单节点 rollback. wave 18 task 04 加 cascade — PLAN.lisp 节点新加三 hint 字段 (都 optional): :compensates (list[node-id], 声明本节点是哪些节点的补偿; 同 plan 内已声明节点) / :rollback-cascade (enum none | plan | dispatch-safe; 默认 none 字节兼容; plan = 失败节点的所有 ancestor 已 succeeded 都 cascade-rollback; dispatch-safe = 仅 cascade 那些其 :rollback-policy 在安全白名单 {none, descriptor, workstation} 的节点) / :rollback-after (list[node-id], 显式控制 cascade 触发顺序约束); cascade 计算: Kahn 拓扑排序按 :compensates / :rollback-after 形成 cascade DAG, cycle-tolerant fallback (检测 cycle 时降级到 conservative 顺序而非 fail; 不 trip plan-level error); 5 safety triggers (workstation rollback prerequisite missing / target-project unresolved / unsafe-strategy / SafeDescriptor refusal / cascade graph cycle 时降级); 复用 wave 17-04 的 workstation_dispatch.build_task_brief / run_workstation_dispatch substrate (cascade 中每个 rollback 节点仍按 wave-17 协议派工); cascade 中 acceptance_commands_executed: false 钉死 (cascade 仍仅 dispatch remediation 工位, 永不本地跑 acceptance shell); evidence 写 rollback_cascade 块 (mode / cascade_plan / triggered_nodes / skipped_nodes / refused_nodes / cycle_fallback?); 触发时机: 在节点 final fail attempt + 本地 rollback-policy-v0 之后, 在 propagate_taint 之前; cascade 完成后下游 taint 仍按 :failure-policy 传播 (cascade 不 alter taint / fail-fast); 本 entry 闭环 wave 17-04 deferred 的 '自动 cascade-up (上游已 succeeded 节点反向 dispatch 补偿)'; 仍 partial 项: compensate-node :ref 引用 (compensates 是 reverse 方向, compensate-node :ref 是 forward 引用, 本 wave 不实现) 仍 future")

      ;; ── 区域 41 · cross-plan distill chain v0 (wave 18 task 05) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0"
        :title "cross-plan distill chain v0 — distill_chain_id / distill_chain_mode (record_only|dry_run|sonnet) / distill_chain_name 写 evidence sidecar 无 migration"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: cross-plan-distill-chain-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.finalize-distill-v0"
                    "intent-layer.actor.plan-dag-scheduler"
                    "memory.directive-layer.plan-evidence-sidecar"
                    "memory.directive-layer.plan-node-state-projection"]
        :wave "18 task 05 (commit 76d818d)"
        :note "wave 17 task 05 (commit 402fb82) finalize + distill trigger v0 仅在单 plan 内 finalize 后调用 distill (distill_on_success + distill_mode) — cross-plan chain (上游 plan distill 自动 enqueue 给下游 plan workflow) 仍 future. wave 18 task 05 加 chain v0: mission_plan(action=execute) 加 3 args (都 optional, 都 opt-in 字节兼容): distill_chain_id (free-form chain identifier; 缺省 = 不入 chain; 同一 chain_id 跨 plan 共享, 形成 plan→plan→plan 链) / distill_chain_mode (enum record_only | dry_run | sonnet; default record_only — sonnet 仅在显式触发 + parent plan distill_on_success=true + parent succeeded 才走) / distill_chain_name (free-form display name); chain metadata 写 evidence sidecar (sidecar 子块 distill_chain — chain_id / mode / name / parent_plan_id / position_in_chain / created_at), **无 SQL migration**; 3 modes: record_only → 仅写 sidecar 不调 workflow handler (轻量, 默认); dry_run → 沿 wave 17-05 finalize+distill 路径走 dry_run; sonnet → 沿 wave 17-05 路径走 sonnet (仅在显式触发); 5 skip 状态 (chain_skipped reasons: chain_id_missing / mode_record_only / parent_not_succeeded / parent_distill_off / sonnet_not_explicit); append-only 3 层保证 (chain entries 只追加; chain id 在 plan run 内 immutable; sidecar 写失败 → warning + 仍保 plan finalization, 不 corrupt plan final state); sonnet 仅在显式触发 (caller 显式 distill_chain_mode=sonnet + parent succeeded + parent distill_on_success=true), 不会从 dry_run 隐式升级; distill 失败 → warning, plan 仍 finalize success (与 wave 17-05 契约一致); 闭环 wave 17-05 deferred 的 'cross-plan distill chain' 项; 自动跨 plan 触发 chain (无需 caller 显式 distill_chain_id) + 完整 sonnet 自动接 仍 future")

      ;; ── 区域 42 · autonomous PLAN field inference v0 (wave 18 task 06) ──
      (section-entry
        :section-id "intent-layer.plan-dag.autonomous-plan-field-inference-v0"
        :title "autonomous PLAN field inference v0 — infer_plan_fields (off|preview|apply_safe) + 6 fields + 3 confidence + deterministic heuristics no LLM"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: autonomous-plan-field-inference-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.actor.plan-dag-scheduler"
                    "intent-layer.plan-dag-runtime-v2"
                    "memory.directive-layer.plan-evidence-sidecar"]
        :wave "18 task 06 (commit 91f8fa1)"
        :note "wave 16 task 03 (commit 8ffa9b2) 仅 workstation dispatch 推断 (5 inference rules for mission_task_delegate scoped node). wave 18 task 06 把推理扩到完整 PLAN.lisp 节点字段子集 — mission_plan(action=execute) 加 1 arg infer_plan_fields (enum off | preview | apply_safe; default off 字节兼容); 6 fields inference target (target / dispatch_strategy / target_project / owned_files / acceptance_mode / workstation_dispatch); 3 confidence (high / medium / low) — 仅 high confidence 才在 apply_safe 模式落 PLAN, medium/low 在 preview 模式 surface 但 apply_safe 拒绝; 3 evidence sources (plan_sexp 节点已声明字段 / compiled_from upstream evidence sidecar / sidecar manifest); conflict 永不静默 override — 推断与 caller 显式 hint 冲突时 fail-fast (INFER_CONFLICT) 不掩盖 caller intent; preview 模式仅 surface inference 结果在 response 不修改 plan; apply_safe 仅在 high confidence + 节点未声明字段 + caller 未显式传 hint 时落; **完全无 LLM** — 推理走 deterministic heuristics (字符串前缀匹配 / known-strategy 映射 / file-extension 推断), 不 spawn 不 shell; 闭环 wave 16-03 deferred 的 'autonomous PLAN.lisp 推理 (完整 PLAN.lisp 节点字段全自动推断)' 部分 — deterministic 子集已落; status code-aligned-partial 因为: 仅覆盖 6 字段 (failure-policy / retry-policy / rollback-policy / acceptance-depends-on 等仍 caller hint only); LLM-augmented inference (从 directive 历史语义/ codex 上下文推) 仍 future; conflict resolution 现仅 fail-fast, smart merge / suggest-only override 仍 future")

      ;; ── 区域 43 · review automation policy v0 (wave 18 task 07) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-automation-policy-v0"
        :title "review automation policy v0 — review_automation_policy (manual|suggest|auto_safe) + 5 deterministic safety rule + 绝不 auto_reject"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-automation-policy-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0"
                    "intent-layer.unified-entry-pipeline.v0-non-goals"]
        :wave "18 task 07 (commit 0f55892)"
        :note "wave 14-03 review-gate auto-create v1 + wave 15-04 explicit resolution bridge v0 + wave 16-01 workflow handler 接入 + wave 16-02 listener subscriber 接 review_question_id 都仍是 caller-driven resolution. wave 18 task 07 加 deterministic auto-approve policy: mission_directive / plan / workflow 全部加 review_automation_policy (enum manual | suggest | auto_safe; default manual 字节兼容); 5 deterministic safety rule 必须全过才 auto_safe approve: (1) deterministic_mode (compiler/distiller 走 deterministic 而非 sonnet, 否则 LLM 输出本身有不确定性); (2) no_file_write OR file_hash_match (artifact 文件未变或 sha256 与 review submission 一致, 防止 silent drift); (3) no_protected (改的字段不在 protected list — 例如 directive non-goals / plan rollback / workflow audit); (4) no_additional_blockers (review_question 无 unresolved blocker / dependency); (5) caller_opt_in (caller 显式传 review_automation_policy=auto_safe, 不允许从 default manual 隐式升级); default = manual byte-compat (legacy callers 行为不变); existing explicit review resolution (wave 15-04 的 review_decision + review_actor + review_note) 仍 authoritative — auto_safe 仅在无 explicit resolution 时介入, 不能覆盖人工裁决 (review-resolution-v0 priority > review-automation-policy-v0); **绝不 auto_reject** (auto_safe 永远是 approve-or-defer-to-manual; rejected/needs_changes 永远是人工 input); destructive transitions (directive archive / plan supersede) 永不 auto-promote (即便 5 rule 都过); response 加 review_automation_status / review_automation_reason / review_automation_safety_breakdown (5 rule 各自 pass/fail + 整体决策); status code-aligned-partial 因为: deterministic auto-approve 已落 (suggest/auto_safe 都已 wire), v0 non-goal 中 LLM-augmented auto-approve / 自动 answer review question 仍 surface 不实现; suggest 模式仅在 response surface 'would have approved' 不 mutate state; auto_reject / cross-resolution-tie-break / per-protected-field configurable list 仍 future")

      ;; ── 区域 44 · scoped-commit worktree preflight v0 (wave 18 task 08) ──
      (section-entry
        :section-id "intent-layer.scoped-commit-worktree-preflight-v0"
        :title "scoped commit worktree preflight v0 — mission_execution(action='preflight_commit') 只调 git status --porcelain=v1 (0 mutating subcommand)"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section scoped-commit-handoff :: preflight-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["intent-layer.scoped-commit-enforce-v0"
                    "intent-layer.scoped-commit-handoff-brief-default-v1"
                    "worker.section.claudecode-workstation-orchestration"
                    "memory.helper.agent-execution-coordination"]
        :wave "18 task 08 (commit f171ae6)"
        :note "wave 16-06 daemon enforce_scoped_commit (opt-in default false) + wave 17-07 brief-layer enforce 默认开 — 但 daemon 始终未跑任何 git 命令. wave 18 task 08 加 read-only preflight tool: mission_execution 加 action='preflight_commit'; 只调 git status --porcelain=v1 (grep proof 0 mutating subcommand: 不 add / 不 commit / 不 stash / 不 reset / 不 checkout / 不 push / 不 merge / 不 rebase); 返结构化字段: ok (bool 整体 ready) / changed_files (worktree 全部 changed list) / staged_files (index 中 staged list) / out_of_scope_files (相对 caller 提供的 claim_scope 越界文件) / expected_owned_files (caller 声明应改的文件 list, 与 staged 对照看 missing) / expected_untouched_files (caller 声明不应改的文件 list, 与 changed 对照看 violation) / next_step (建议: ready_to_commit / stage_required / out_of_scope_review / claim_scope_required); preflight 设计为新派工 worker 完成前自检 — worker 在 mission_execution(action=complete) enforce_scoped_commit=true 之前先调 preflight 看 worktree, 不再自己 git status; daemon 仍不跑 git mutating commands (与 wave 16-06 daemon 不跑 git 契约一致, 仅 read-only status); 与 wave 17-07 brief section 7 + wave 16-06 daemon enforce 配合: brief 要求 commit, preflight tool 帮 worker 看 worktree, complete enforce_scoped_commit 锁住 commit_hash 必填 — 三段闭环 (brief → preflight → complete enforce)")

      ;; ── 区域 45 · unified-entry autonomous loop smoke v1 (wave 18 task 09) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.autonomous-loop-smoke-v1"
        :title "unified-entry autonomous loop smoke v1 — 5 stage no-LLM/no-spawn/no-shell + wave 17/18 keys forward"
        :source-file ".missiond/v2/intent-directive-artifacts.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: autonomous-loop-smoke-v1"
        :status operational-practice
        :compression-safe? true
        :implements ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.run-pipeline-helper"
                    "intent-layer.unified-entry-pipeline.e2e-smoke-v0"
                    "intent-layer.unified-entry-pipeline.paused-resume-smoke-v0"
                    "intent-layer.unified-entry-pipeline.v0-non-goals"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0"
                    "intent-layer.plan-dag.autonomous-plan-field-inference-v0"]
        :wave "18 task 09 (commit d440087)"
        :note "wave 16 task 08 落 deterministic 4 hand-off no-LLM smoke (s1→s4→s6 + sidecar). wave 17 task 08 加 paused-resume e2e smoke (build_plan_execute_args forward 7 个 wave-17 keys + 6 步 round-trip). wave 18 task 09 加 autonomous loop smoke v1 — unified_entry::build_plan_execute_args forward 4 wave-18 keys (distill_chain_id / distill_chain_mode / distill_chain_name + infer_plan_fields); 加 build_plan_execute_args_forwards_wave18_chain_keys + build_plan_execute_args_forwards_wave18_infer_key + build_plan_execute_args_wave18_keys_absent_when_omitted 三个 unit test 锁 forward 行为; 5 stage smoke 走通 wave17/18 多任务 (不只是单 plan 单 hand-off): s1 directive create → s4 plan compile → s6 first execute (含 wave 17/18 keys forward) → s6 second execute (autonomous loop 第二轮 distill chain + infer) → sidecar verify (chain metadata + inference evidence + paused-resume + finalize 全部 surface 可读); **no LLM, no spawn, no shell**; sidecar 共享 (5 stage 写同一 plan sidecar; chain metadata append-only 验证); v0_non_goals 每 stage surface — 包括既有 4 项 (auto_approve_directive / auto_approve_plan / auto_answer_review_question / autonomous_workstation_dispatch) + 新加 wave18 项: review_automation_policy=auto_safe 仍 caller-driven (5 rule 全过 + caller_opt_in), infer_plan_fields=apply_safe 仅 high-confidence 落, distill_chain_mode=sonnet 必须 explicit + parent succeeded + parent distill_on_success=true, preflight_commit 仅 status read 不 mutate; 不引入新 MCP tool (tool count 仍 83); 不动 workflow.rs / plan.rs 主路径; 用作 wave 17-08 paused-resume e2e smoke v0 + wave 18 全 task 字段 forward 的 superset (操作上是 wave 17-08 的 next-gen)"))

    ;; ──────────────────────────────────────────────────
    ;; v1.0 (wave 19 task 12) — wave 19 machine-contract status backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 19 task 00-11 的真实代码状态回填到 source-index
    ;;   - wave 19 引入 machine-contract 概念: task.lisp 升级为 dispatch SSOT
    ;;     + report-contract + shared-memory ledger + renderer dispatch brief
    ;;     + plan emitter + workstation consumer + execution completion
    ;;   - 不重复已有 section-id; 在 v0.9 baseline 上新增 9 anchor entry,
    ;;     升级 1 现有 entry (task-contract-v1 由 code-aligned-partial → code-aligned)
    ;;   - 保留 R008 + R016 (section-id 不变, 改名走 :prev-id)
    ;;   - 11-stage scheduler 所有主线均已 close; 仍 partial 是 LLM auto-approve 全部 v0
    ;;     non-goal / autonomous workstation spawn 完全无 hint / git pre-commit hook /
    ;;     frontend lisp / sonnet 自动接 chain 仍未实现
    ;;
    ;; wave 19 已完成 commit (anchor):
    ;;   - 88e9c65 docs(task): add machine contract pilot note (task 00 — 概念 pilot 入册, < 80 行 cap, task-contract checker PASS)
    ;;   - 64c0f38 chore(wave18): archive task briefs (task 01 — 12 个 wave18-*.md 入库, 仅归档不进 source-index)
    ;;   - 77f1f2b feat(tasks): verify task contracts against commits (task 02 + 吸入 task 04 — verifier + checker + parser + schema + shared-memory v1 schema; verifier 5 项检查; --commit/--json/--dry-fixture; read-only 0 mutating git)
    ;;   - ba58f20 feat(tasks): add machine-readable task reports (task 03 — report-contract-v1 schema + check-task-report.mjs 10 fixtures + sample report wave19-00.report.lisp)
    ;;   - (wave19-04 被 wave19-02 吸入 commit 77f1f2b — shared-memory-v1 schema + 6 entry types + check-task-memory.mjs 13 fixtures + seed wave19/shared-memory.lisp; 功能正确, commit 归属错)
    ;;   - c95eba8 feat(tasks): enrich rendered dispatch briefs (task 05 — renderer 加 4 节: Machine Contract +2 conditional / Dispatch Note / Shared Memory / Report Contract / verify command; agent-team literal 单实例条件渲染)
    ;;   - 5d425e2 feat(plan): emit Lisp task contracts for dispatch (task 06 — mission_plan(execute, emit_task_contract|task_contract_mode); eligible workstation nodes → .missiond/tasks/generated/<plan_id>/<node_id>.lisp; response 加 task_contract_path/render_command; emit before dispatch; default off byte-compat)
    ;;   - bfc72b7 feat(workstation): consume Lisp task contracts (task 07 — parse_task_contract / load_task_contract / WorkstationDispatchHints::overlay_contract; run_workstation_dispatch_with_contract 加 task_contract_path 参数; SafeDescriptorReason::MalformedTaskContract; legacy brief byte-identical 当 contract absent; 绝不 fall back claude -p)
    ;;   - 405d13b feat(execution): record task contract completion checks (task 08 — mission_execution(complete) 加 4 字段 task_contract_path/task_report_path/verifier_status/verifier_notes + 2 新错误码 TASK_CONTRACT_REQUIRED/MALFORMED + COMMIT_HASH_REQUIRED_FOR_CONTRACT; enforce_scoped_commit + task_contract_path → require commit_hash + claim scope coverage; daemon 仍只 read-only)
    ;;   - e57b56d feat(workflow): derive cross-plan distill chain ids (task 09 — mission_workflow(distill, auto_chain, auto_chain_name); deterministic id chain:auto:wf-<sha256(project_root + plan_id + workflow_anchor + evidence_sha256)> US 隔离防碰撞; sonnet 仍仅显式; append-only 无 migration; default off byte-compat)
    ;;   - 246a675 feat(plan): support forward compensate node refs (task 10 — :compensate-node / :compensate-ref 双拼写 + underscore alias; 4 拒绝条件 self/unknown/empty/direction mismatch; compute_compensation_order union forward + reverse; wave18-04 safety gates 完整保留)
    ;;   - (wave19-11 NO-OP — ExecutionEvent::Opened dispatch metadata v1 实际已在 wave11 commit fc58018 + wave18-02 commit 2fbaa88 完成; 字段已存在 wiring 已建 round-trip tests 全在; 无新 commit)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.machine-contract.task-contract-v1                       status code-aligned-partial → code-aligned (verifier + report-contract + shared-memory + renderer + plan emitter + workstation consumer + execution completion 全部 close)
    ;;   - event-bus.section.execution-event.dispatch-metadata-v1               note 扩 (Opened variant 已含同字段, wave19-11 验证为 NO-OP)
    ;; ──────────────────────────────────────────────────
    (wave-19-backfill v1.0
      :date "2026-04-27"
      :decided-by "wave 19 / task 12 lisp backfill session"
      :scope "回填 wave 19 task 00-11 真实代码状态; 新增 9 anchor entry, 升级 task-contract-v1 由 partial → full; 不发明 wave19 没实现的架构; wave19-11 NO-OP 历史已 implemented 也照实记录"
      :non-goal "本任务不改 Rust / SQL / JS / Cargo / 任务文档; 不真正压缩主 Lisp; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp (continue postpone)"
      :commits
        [(commit-0 :hash "88e9c65" :title "docs(task): add machine contract pilot note"
                   :primary-targets ["docs/machine-contract-pilot.md"]
                   :tests "57 行 (< 80 cap), pilot 解释 task.lisp 升级 dispatch SSOT 概念; task-contract checker PASS; 不引入 schema/checker/renderer 实现 (各自留给 task 02-05)")
         (commit-2 :hash "77f1f2b" :title "feat(tasks): verify task contracts against commits (含 task 04)"
                   :primary-targets ["scripts/verify-task-contract.mjs"
                                     "scripts/check-task-contract.mjs"
                                     "scripts/lib/missiond_lisp.mjs"
                                     ".missiond/tasks/schema/task-contract-v1.lisp"
                                     ".missiond/tasks/schema/shared-memory-v1.lisp"
                                     "scripts/check-task-memory.mjs"
                                     ".missiond/tasks/wave19/shared-memory.lisp"]
                   :tests "verify-task-contract.mjs 5 项检查 (commit hash 存在 / commit message 含 task-id / changed_files ⊆ write-scope 当 :scope-check=write-scope-only / changed_files 与 must-not-touch 无交集 / acceptance 命令文本存在); --commit <hash> / --json / --dry-fixture 三 flag; 全程 read-only (grep proof: 0 git mutating subcommand — 不 add/commit/stash/reset/checkout/push/merge/rebase); 复用 missiond_lisp.mjs parser 不重写; check-task-contract 加 shared-memory 共享 helper 入参; **wave19-04 (shared-memory-ledger-v0) 文件被一并入此 commit (功能正确但 commit 归属错; check-task-memory 13 fixtures cover claim/observation/blocker/completion/correction/handoff 6 entry types + duplicate id rejection + invalid timestamp + abs path rejection; seed wave19/shared-memory.lisp 含 header + 1 bootstrap observation 不伪造完成事件)")
         (commit-3 :hash "ba58f20" :title "feat(tasks): add machine-readable task reports"
                   :primary-targets [".missiond/tasks/schema/report-contract-v1.lisp"
                                     "scripts/check-task-report.mjs"
                                     ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp"]
                   :tests "report-contract-v1.lisp schema 7 字段 (task_id / status / commit_hash / files_changed / acceptance_results / scope_deviations / notes); check-task-report.mjs 10 fixtures 覆盖 missing task_id / invalid status / empty acceptance_results when status=done / absolute file paths rejection; sample report wave19-00.report.lisp 仅作 schema 示例不是任务执行证据; 与 verify-task-contract 协作但保持独立 (report 是 worker output, verifier 是 reviewer check)")
         (commit-5 :hash "c95eba8" :title "feat(tasks): enrich rendered dispatch briefs"
                   :primary-targets ["scripts/render-claudecode-task.mjs"
                                     ".missiond/tasks/schema/task-contract-v1.lisp"
                                     ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp"
                                     ".missiond/claudecode/wave19-00-machine-contract-pilot.md"]
                   :tests "renderer 在 brief 加 4 新节: Machine Contract (kind/status/owner/dispatch_strategy/depends_on, 后两 conditional 仅在存在时渲染) / Dispatch Note (条件 - dispatch-strategy=agent-team 时渲染 '使用 agent-team提高效率' 字面 1 次) / Shared Memory (引用 .missiond/tasks/<wave>/shared-memory.lisp + ledger entry types) / Report Contract (引用 .missiond/tasks/<wave>/reports/<task-id>.report.lisp + status flow) / Verify command (node scripts/verify-task-contract.mjs <task-lisp> --commit <hash>); agent-team literal 严格单实例 (重复检测); 既有字段 backward-compatible byte-identical")
         (commit-6 :hash "5d425e2" :title "feat(plan): emit Lisp task contracts for dispatch"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "mission_plan(action=execute) 加 2 opt-in args: emit_task_contract (bool, default false 字节兼容) + task_contract_mode (enum off|emit|emit_and_render, default off); 对 eligible workstation nodes (target=mission_task_delegate ∨ workstation_dispatch=true) 写 .missiond/tasks/generated/<plan_id>/<node_id>.lisp 用 task-contract v1 字段 (kind=workstation / status=ready / goal/objective / write-scope from owned_files / must-not-touch from forbidden / acceptance from PLAN node :acceptance / commit policy from scoped-commit handoff / dispatch_strategy from PLAN node hint); response 加 task_contract_path + rendered_task_brief_path / render_command 字段; **emit before dispatch** — 失败不进 inner handler (避免 dispatch 后才发现 contract 写不出); default off 字节兼容 (legacy 路径不写 contract); 绝不调 Node — Rust 内本生 写文件; 不解析 PLAN.lisp 超出 wave 17/18 已有 hint parser 边界")
         (commit-7 :hash "bfc72b7" :title "feat(workstation): consume Lisp task contracts"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
                   :tests "加 parse_task_contract (narrow Lisp 子集 helper, 仅读 wave19 task-contract-v1 字段) + load_task_contract (从 path 读+解析+错误 hoist); WorkstationDispatchHints::overlay_contract (合并 contract 字段到 dispatch hints, contract 优先 ∵ contract 是 SSOT); run_workstation_dispatch_with_contract(_, _, _, _, _, task_contract_path: Option<&Path>) 新签名 (legacy run_workstation_dispatch 仍存, 包装新签名 path=None); SafeDescriptorReason::MalformedTaskContract 新 variant (parse 失败 / required 字段缺失 / kind 不匹配 → 结构化失败); legacy brief byte-identical 当 task_contract_path absent; **绝不 fall back claude -p** — contract malformed 直接 SafeDescriptor (与 wave 15-05 SafeDescriptor 不静默 fallback prompt 契约一致)")
         (commit-8 :hash "405d13b" :title "feat(execution): record task contract completion checks"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
                   :tests "mission_execution(complete) 加 4 optional 字段 (task_contract_path / task_report_path / verifier_status / verifier_notes; default None 字节兼容); 2 新错误码: TASK_CONTRACT_REQUIRED_FOR_CONTRACT_VERIFICATION (传 task_report_path/verifier_status 但缺 task_contract_path) / MALFORMED_TASK_CONTRACT_PATH (path 越界或非 .missiond/tasks/) / COMMIT_HASH_REQUIRED_FOR_CONTRACT (enforce_scoped_commit=true + task_contract_path → 必须传 commit_hash); enforce_scoped_commit + task_contract_path → require claim scope coverage of contract write-scope (claim_scope ⊇ contract write-scope); daemon 仍只 read-only (grep proof 0 git mutating subcommand: agent_execution.rs 不调 git add/commit/reset/checkout/stash/push/merge/rebase, 只读 staged_files / commit_hash 由 caller 提供); preserve legacy complete byte-compat 当新字段 absent")
         (commit-9 :hash "e57b56d" :title "feat(workflow): derive cross-plan distill chain ids"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
                   :tests "mission_workflow(action=distill) 加 2 opt-in args: auto_chain (bool, default false 字节兼容) + auto_chain_name (free-form display name, optional); deterministic id 'chain:auto:wf-<sha256(project_root + plan_id + workflow_anchor + evidence_sha256)[..16]>' (hex 16); US (project_root) 隔离 prefix 防跨项目碰撞; sonnet 仍仅显式 (auto_chain=true 不会自动 sonnet, 仍要求 caller 显式 distill_chain_mode=sonnet + parent succeeded + parent distill_on_success=true 全过); 沿用 wave 18-05 cross-plan distill chain v0 sidecar (chain_id / mode / name / parent_plan_id / position_in_chain / created_at) + 5 skip 状态 + append-only 3 层保证; 无 SQL migration; default off 字节兼容 (legacy callers 仍需显式 distill_chain_id)")
         (commit-10 :hash "246a675" :title "feat(plan): support forward compensate node refs"
                    :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
                    :tests "PLAN.lisp 节点新加 :compensate-node / :compensate-ref (双拼写 + underscore alias compensate_node / compensate_ref) — 单一 node-id 的 forward 引用 (本节点失败 → ref 节点入 ready-set); 与 wave 18-04 :compensates (反向 list, 本节点是哪些节点的补偿) 互补; 4 拒绝条件 (parser-level structured error): (1) self reference (compensate-node 指本节点); (2) unknown compensation node (引用 plan 内未声明的 node-id); (3) empty (空字符串/空 list); (4) direction mismatch (forward 与 reverse 在两节点间同时存在但语义冲突 — 例如 A 声明 :compensate-node 'B' 但 B 声明 :compensates 不含 'A' 时 fail-fast); compute_compensation_order union forward + reverse 形成完整 cascade DAG (Kahn 排序仍 cycle-tolerant fallback); wave 18-04 5 safety triggers 完整保留 (workstation rollback prerequisite missing / target-project unresolved / unsafe-strategy / SafeDescriptor refusal / cascade graph cycle 时降级)")
         (commit-11 :hash "(NO-OP wave11 fc58018 + wave18-02 2fbaa88)" :title "ExecutionEvent::Opened dispatch metadata v1 — 历史已 implemented"
                    :primary-targets ["crates/missiond-core/src/event/events/execution.rs (Opened variant 已含 dispatch_strategy/target_project/requested_cwd, serde defaults + skip_serializing_if)"
                                      "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (mission_execution(open) 已写入 fields)"]
                    :tests "wave 11 (commit fc58018) wave 14-02 (commit 2e7789a) + wave 18-02 (commit 2fbaa88) 联合在 ExecutionEvent::Opened/Claimed/Completed 全部 variant 加 dispatch_strategy / target_project / requested_cwd 三 optional 字段 (serde-bw-compat: 旧 payload 解析仍 OK); round-trip tests 已存在 (Opened 含与不含字段两种 JSON 都可解析); wiring (mission_execution(open) 写入 fields) 已建; **wave19-11 task 验证为 NO-OP — 字段已存在, wiring 已建, round-trip tests 全在, 无新 commit**; 任务文档已照 NO-OP 历史归档进 task contract")]

      ;; ── 区域 46 · machine-contract task protocol v1 status upgrade (wave 19 task 02-08) ──
      ;; 注: section-id 已存在 (区域 36), 本 entry 不新增 anchor; 仅在 wave-19-backfill 块内
      ;;     声明 status code-aligned-partial → code-aligned 升级 (verifier + report + shared-memory
      ;;     + renderer + plan emitter + workstation consumer + execution completion 全部 close)
      (status-upgrade
        :section-id "intent-layer.machine-contract.task-contract-v1"
        :prev-status code-aligned-partial
        :new-status code-aligned
        :evidence-commits ["77f1f2b (verifier + 吸入 shared-memory v1)"
                           "ba58f20 (report-contract v1)"
                           "c95eba8 (renderer dispatch brief v1)"
                           "5d425e2 (plan task-contract emitter v0)"
                           "bfc72b7 (workstation task-contract consumer v0)"
                           "405d13b (execution task-contract completion v0)"]
        :rationale "task-contract-v1 schema 已 wired 进全 dispatch 闭环: plan 端 emit (wave19-06) → workstation 端 consume (wave19-07) → execution 端 verify (wave19-08); 配合 report-contract (wave19-03) + shared-memory ledger (wave19-04) + renderer dispatch brief (wave19-05) + verify-task-contract.mjs (wave19-02) 形成完整 SSOT-driven dispatch loop; 仅遗留 future: 真正 machine-driven autonomous dispatch (无 ClaudeCode Markdown 渲染) / git pre-commit hook 执行 scope check / 自动 spawn 完全无 hint 仍 surface 不实现")

      ;; ── 区域 47 · machine-contract task verifier v1 (wave 19 task 02 + 吸入 04) ──
      (section-entry
        :section-id "intent-layer.machine-contract.task-verifier-v1"
        :title "machine-contract task verifier v1 — 5 项检查 + --commit/--json/--dry-fixture + read-only 0 mutating git"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: task-verifier-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/verify-task-contract.mjs"
           "scripts/check-task-contract.mjs"
           "scripts/lib/missiond_lisp.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.shared-memory-v1"
                    "intent-layer.machine-contract.report-contract-v1"]
        :wave "19 task 02 (commit 77f1f2b)"
        :note "task-contract-v1 schema 单纯做静态 shape 检查不够; verifier 把 task.lisp 与 git commit + 实际 changed files 关联做 5 项 deterministic 检查: (1) commit hash 存在; (2) commit message 含 task-id; (3) changed_files ⊆ write-scope 当 :scope-check=write-scope-only; (4) changed_files ∩ must-not-touch = ∅; (5) acceptance 命令文本存在 (verifier 不执行 acceptance, 仅检查 contract 字段完整). --commit <hash> 接 git show; --json 结构化输出 (5 check pass/fail + reason); --dry-fixture 内置 fixture 自检. **read-only 绝不 mutating** — grep proof: 不 git add/commit/stash/reset/checkout/push/merge/rebase; 失败时仍只 stdout/stderr 报错. 复用 scripts/lib/missiond_lisp.mjs parser 不重写 reader. wave19-04 (shared-memory-ledger-v0) 被本 commit 一并入 main (功能正确但 commit 归属错), 见 task-memory-v1 entry")

      ;; ── 区域 48 · machine-contract report-contract v1 (wave 19 task 03) ──
      (section-entry
        :section-id "intent-layer.machine-contract.report-contract-v1"
        :title "machine-contract report-contract v1 — 7 fields + 10 fixtures + sample report"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: report-contract-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/report-contract-v1.lisp"
           "scripts/check-task-report.mjs"
           ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.task-verifier-v1"
                    "intent-layer.machine-contract.shared-memory-v1"]
        :wave "19 task 03 (commit ba58f20)"
        :note "report-contract-v1 是 ClaudeCode worker 完成任务后的机器可读 report 形状 (区分于 verifier 的 reviewer 视角): 7 字段 (task_id / status / commit_hash / files_changed / acceptance_results / scope_deviations / notes); status 枚举 ⊆ {draft, in_progress, blocked, done}; check-task-report.mjs 10 fixtures 覆盖 (missing task_id / invalid status / empty acceptance_results when status=done / absolute file paths / extra unknown fields warning / valid draft / valid done / valid blocked + blocker reason / valid scope_deviations 列表 / valid notes 多行); sample report wave19-00.report.lisp 仅作 schema 示例 (status='done' + commit_hash=88e9c65) 不是任务执行证据 (因为它的主要 acceptance 是 'pilot doc < 80 行 cap'); 与 verify-task-contract 协作: report 是 worker 输出, verifier 是 reviewer check, 两者 schema 独立但 task_id 一致")

      ;; ── 区域 49 · machine-contract shared-memory ledger v0 (wave 19 task 04, 吸入 wave19-02 commit) ──
      (section-entry
        :section-id "intent-layer.machine-contract.shared-memory-v1"
        :title "machine-contract shared-memory ledger v0 — 6 entry types + 13 fixtures + seed wave19/shared-memory.lisp"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: shared-memory-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/shared-memory-v1.lisp"
           "scripts/check-task-memory.mjs"
           ".missiond/tasks/wave19/shared-memory.lisp"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.task-verifier-v1"
                    "intent-layer.machine-contract.report-contract-v1"
                    "memory.helper.agent-execution-coordination"]
        :wave "19 task 04 (commit 77f1f2b — 吸入 wave19-02 commit; 功能正确但 commit 归属错)"
        :note "shared-memory ledger 是并行 ClaudeCode agents 的协调 sub-strate (区分于 mission_execution claims slot — ledger 是 design-time 共享笔记, claims 是 runtime claim/lease 协议): 6 entry types head 稳定 — claim (declare write scope intent) / observation (read-only finding) / blocker (与 issue 类似但 design-time) / completion (任务完成 + commit hash) / correction (修正之前 entry 的事实) / handoff (跨 agent 接力 pointer); checker validates task ids / entry ids / timestamps OR monotonic seq numbers / touched files repo-relative paths / no duplicate entry ids; 13 fixtures cover 各 entry type valid + 4 reject (duplicate id / invalid timestamp / abs path / empty entry); seed wave19/shared-memory.lisp 含 header + 1 bootstrap observation (不伪造完成事件); ledger 本身是 sole shared write target (agents 各 append, 不 mutate 已有 entry — correction 是新 entry); 与 mission_execution claims 形成双层: claims = runtime 强协议, ledger = design-time 软协议 (notes/blockers/handoff). **wave19-04 任务文件确实独立, 但代码文件被 wave19-02 commit 一并入 main**, 这是 git index 全局污染历史 — 功能落地正确, commit 归属错")

      ;; ── 区域 50 · machine-contract renderer dispatch brief v1 (wave 19 task 05) ──
      (section-entry
        :section-id "intent-layer.machine-contract.renderer-dispatch-brief-v1"
        :title "machine-contract renderer dispatch brief v1 — 4 新节 + agent-team literal 单实例条件 + verify command"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: renderer-dispatch-brief-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/render-claudecode-task.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"
           ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp"
           ".missiond/claudecode/wave19-00-machine-contract-pilot.md"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.task-verifier-v1"
                    "intent-layer.machine-contract.report-contract-v1"
                    "intent-layer.machine-contract.shared-memory-v1"]
        :wave "19 task 05 (commit c95eba8)"
        :note "renderer 升级 — task.lisp 渲染成 .md 给 ClaudeCode 时, 现包含完整 machine-contract 上下文: 4 新节 (Machine Contract: kind/status/owner/dispatch_strategy/depends_on, 后两 conditional 仅在存在时渲染 / Dispatch Note: 条件 - dispatch-strategy=agent-team 时渲染 '使用 agent-team提高效率' 字面恰好 1 次 / Shared Memory: 引用 .missiond/tasks/<wave>/shared-memory.lisp + ledger entry types 提示 / Report Contract: 引用 .missiond/tasks/<wave>/reports/<task-id>.report.lisp + status flow draft→in_progress→done) + Verify command (node scripts/verify-task-contract.mjs <task-lisp> --commit <hash>); agent-team literal 严格单实例 (重复检测, 防止重复渲染); 既有字段 backward-compatible byte-identical (仅新增节 + commit/render command, 既有字段无改动); 配合 wave19-06 plan emitter — emit + render 两步可在 plan execute 一次响应内完成 (response 含 task_contract_path + render_command)")

      ;; ── 区域 51 · plan task-contract emitter v0 (wave 19 task 06) ──
      (section-entry
        :section-id "intent-layer.plan-dag.task-contract-emitter-v0"
        :title "plan task-contract emitter v0 — emit_task_contract opt-in + .missiond/tasks/generated/<plan_id>/<node_id>.lisp + emit before dispatch"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: task-contract-emitter-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.renderer-dispatch-brief-v1"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "19 task 06 (commit 5d425e2)"
        :note "wave 19-05 renderer 让 task.lisp 可渲染成 brief, 但 task.lisp 之前都靠人工写; wave19-06 让 plan execute 自动 emit task.lisp: mission_plan(action=execute) 加 2 opt-in args (emit_task_contract bool default false 字节兼容; task_contract_mode enum off|emit|emit_and_render default off); 对 eligible workstation nodes (target=mission_task_delegate ∨ workstation_dispatch=true) 写 .missiond/tasks/generated/<plan_id>/<node_id>.lisp 用 task-contract v1 字段 (kind=workstation / status=ready / goal/objective from PLAN node :objective / write-scope from :owned-files / must-not-touch from :forbidden-files / acceptance from :acceptance / commit policy from scoped-commit handoff section / dispatch_strategy from :dispatch-strategy hint); response 加 task_contract_path + rendered_task_brief_path / render_command; **emit before dispatch** — 失败不进 inner handler (避免 dispatch 后才发现 contract 写不出 worker 已派出); default off 字节兼容 (legacy 路径不写 contract); 绝不调 Node — Rust 内本生 写文件; 不解析 PLAN.lisp 超出 wave 17/18 已有 hint parser 边界. 闭环 wave 18-06 autonomous PLAN field inference: inferred 字段 + emit task contract 形成 PLAN → contract → brief 全自动 (仍需 caller opt-in emit_task_contract; 完全 autonomous 仍 future)")

      ;; ── 区域 52 · workstation task-contract consumer v0 (wave 19 task 07) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.workstation-task-contract-consumer-v0"
        :title "workstation task-contract consumer v0 — overlay_contract + MalformedTaskContract SafeDescriptor + 绝不 fall back claude -p"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: workstation-task-contract-consumer-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.plan-dag.task-contract-emitter-v0"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.scoped-commit-handoff-brief-default-v1"]
        :wave "19 task 07 (commit bfc72b7)"
        :note "wave 19-06 plan emit 出 task.lisp 后, 工位端要会消费 — 否则 contract 仍是 dead letter. wave19-07 加 consumer: parse_task_contract (narrow Lisp 子集 helper, 仅读 wave19 task-contract-v1 字段, 不引入完整 reader); load_task_contract (从 path 读+解析+错误 hoist); WorkstationDispatchHints::overlay_contract (合并 contract 字段到 dispatch hints — contract 优先 ∵ contract 是 SSOT, 与 caller hint 冲突时 fail-fast 不静默 override); run_workstation_dispatch_with_contract(_, _, _, _, _, task_contract_path: Option<&Path>) 新签名 (legacy run_workstation_dispatch 仍存, 包装新签名 path=None); SafeDescriptorReason::MalformedTaskContract 新 variant (parse 失败 / required 字段缺失 / kind 不匹配 → 结构化失败); legacy brief byte-identical 当 task_contract_path absent (existing wave 17-07 brief default v1 路径不变); **绝不 fall back claude -p** — contract malformed 直接 SafeDescriptor (与 wave 15-05 SafeDescriptor 不静默 fallback prompt 契约一致). brief 内仍保留 wave 17-07 scoped-commit handoff section (brief → preflight → complete enforce 三段闭环不破)")

      ;; ── 区域 53 · execution task-contract completion verification v0 (wave 19 task 08) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.task-contract-completion-verification-v0"
        :title "execution task-contract completion verification v0 — 4 字段 + 2 新错误码 + claim scope ⊇ contract write-scope + daemon 仍 read-only"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: task-contract-completion-verification-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.report-contract-v1"
                    "intent-layer.machine-contract.task-verifier-v1"
                    "intent-layer.scoped-commit-enforce-v0"
                    "intent-layer.scoped-commit-worktree-preflight-v0"]
        :wave "19 task 08 (commit 405d13b)"
        :note "wave 19-06 plan emit + 19-07 workstation consume 后, 完成端要把 verifier 结果 / report path 锚回 mission_execution 协调面 — 否则 audit 看不到 contract 闭环. wave19-08 mission_execution(complete) 加 4 optional 字段 (task_contract_path / task_report_path / verifier_status / verifier_notes; default None 字节兼容); 2 新错误码: TASK_CONTRACT_REQUIRED_FOR_CONTRACT_VERIFICATION (传 task_report_path/verifier_status 但缺 task_contract_path) / MALFORMED_TASK_CONTRACT_PATH (path 越界或非 .missiond/tasks/) / COMMIT_HASH_REQUIRED_FOR_CONTRACT (enforce_scoped_commit=true + task_contract_path → 必须传 commit_hash); enforce_scoped_commit + task_contract_path → require claim scope coverage of contract write-scope (claim_scope ⊇ contract write-scope; 不足 → SCOPED_COMMIT_VIOLATION 复用 wave 16-06 错误码); **daemon 仍只 read-only** (grep proof 0 git mutating subcommand: agent_execution.rs 不调 git add/commit/reset/checkout/stash/push/merge/rebase, 只读 staged_files / commit_hash 由 caller 提供); preserve legacy complete byte-compat 当新字段 absent. brief → preflight → complete enforce + contract verify 四段闭环 (wave 17-07 + wave 18-08 + wave 19-08)")

      ;; ── 区域 54 · cross-plan distill auto-chain v1 (wave 19 task 09) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-chain-v1"
        :title "cross-plan distill auto-chain v1 — auto_chain opt-in + deterministic id 'chain:auto:wf-<sha256(...)>' + sonnet 仍仅显式"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: cross-plan-distill-auto-chain-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0"
                    "intent-layer.plan-dag-runtime-v2.finalize-distill-v0"
                    "memory.directive-layer.plan-evidence-sidecar"]
        :wave "19 task 09 (commit e57b56d)"
        :note "wave 18-05 cross-plan distill chain v0 要求 caller 显式传 distill_chain_id 串 chain — 但 distill chain 跨 plan 时 caller (plan-runner) 往往不知道上游 chain id (尤其 autonomous loop 场景). wave19-09 mission_workflow(action=distill) 加 2 opt-in args (auto_chain bool default false 字节兼容; auto_chain_name free-form display name optional); deterministic chain id 'chain:auto:wf-<sha256(project_root + plan_id + workflow_anchor + evidence_sha256)[..16]>' (hex 16); US (project_root) 隔离 prefix 防跨项目 hash 碰撞 (e.g. 同 plan_id 在不同 project_root 不同 chain); sonnet 仍仅显式 (auto_chain=true 不会自动 sonnet, 仍要求 caller 显式 distill_chain_mode=sonnet + parent succeeded + parent distill_on_success=true 5 rule 全过); 沿用 wave 18-05 sidecar shape (chain_id / mode / name / parent_plan_id / position_in_chain / created_at) + 5 skip 状态 + append-only 3 层保证; 无 SQL migration; default off 字节兼容 (legacy callers 仍需显式 distill_chain_id). 闭环 wave 18-05 deferred 的 '自动跨 plan 触发 chain (无需 caller 显式 chain_id)' 部分; sonnet 完全自动 (5 rule 自动满足) 仍 future")

      ;; ── 区域 55 · PLAN DAG forward compensate ref v0 (wave 19 task 10) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.forward-compensate-ref-v0"
        :title "PLAN DAG forward compensate ref v0 — :compensate-node / :compensate-ref 双拼写 + 4 拒绝条件 + union forward+reverse"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: forward-compensate-ref-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.cascade-rollback-v0"
                    "intent-layer.plan-dag-runtime-v2.rollback-policy-v0"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "19 task 10 (commit 246a675)"
        :note "wave 18-04 cascade rollback v0 :compensates 是 reverse 方向 (本节点是哪些节点的补偿); 节点 schema 一直缺 forward 引用 (本节点失败 → 哪个节点是它的 compensation). wave19-10 加 forward 引用: PLAN.lisp 节点新加 :compensate-node / :compensate-ref (双拼写 + underscore alias compensate_node / compensate_ref) — 单一 node-id 的 forward 引用 (本节点失败 → ref 节点入 ready-set); 与 wave 18-04 :compensates 互补; 4 拒绝条件 (parser-level structured error): (1) self reference (compensate-node 指本节点); (2) unknown compensation node (引用 plan 内未声明的 node-id); (3) empty (空字符串/空 list); (4) direction mismatch (forward 与 reverse 在两节点间同时存在但语义冲突 — 例如 A 声明 :compensate-node 'B' 但 B 声明 :compensates 不含 'A' 时 fail-fast); compute_compensation_order union forward + reverse 形成完整 cascade DAG (Kahn 排序仍 cycle-tolerant fallback); wave 18-04 5 safety triggers 完整保留 (workstation rollback prerequisite missing / target-project unresolved / unsafe-strategy / SafeDescriptor refusal / cascade graph cycle 时降级). 闭环 wave 18-04 deferred 的 'compensate-node :ref forward 引用' 项; cross-plan compensate 引用仍 future")

      ;; ── 区域 56 · ExecutionEvent::Opened dispatch metadata note (wave 19 task 11 NO-OP) ──
      (section-entry
        :section-id "event-bus.section.execution-event.opened-dispatch-metadata-note-wave19-11"
        :title "ExecutionEvent::Opened dispatch metadata wave 19-11 — NO-OP 验证, 历史已 implemented 自 wave 11 + wave 18-02"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar event-bus :: section execution-event :: opened-dispatch-metadata-note-wave19-11"
        :status code-aligned
        :compression-safe? true
        :implements
          ["crates/missiond-core/src/event/events/execution.rs (Opened variant 已含 dispatch_strategy/target_project/requested_cwd, serde defaults + skip_serializing_if)"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (mission_execution(open) 已写入 fields)"]
        :cross-ref ["event-bus.section.execution-event.dispatch-metadata-v1"
                    "event-bus.section.execution-event.plan-node-state-changed"
                    "intent-layer.plan-dag-runtime-v2.execution-event-decision"]
        :wave "19 task 11 (NO-OP — 字段已存在自 wave 11 commit fc58018 + wave 18-02 commit 2fbaa88)"
        :note "wave 19 task 11 task contract 要求把 ExecutionEvent::Opened 加 dispatch_strategy/target_project/requested_cwd; 进入 verification 时发现字段已存在 — wave 11 commit fc58018 + wave 14-02 commit 2e7789a + wave 18-02 commit 2fbaa88 三阶段联合早已在 Opened/Claimed/Completed 全部 variant 加 dispatch metadata (3 optional 字段 serde defaults + skip_serializing_if); round-trip tests 已存在 (Opened 含与不含字段两种 JSON 都可解析); wiring (mission_execution(open) 写入 fields) 已建. 任务文档已照 NO-OP 历史归档进 task contract; 无新 commit, 字段无新增. 本 entry 仅作 source-index 元数据声明 (区分于 wave 18-02 entry — 那是 Claimed+Completed; 本 entry 是 Opened 的 NO-OP 注解). event-bus pillar 仍 protected, 不动正文"))

    ;; ──────────────────────────────────────────────────
    ;; v1.1 (wave 20 task 10) — wave 20 machine-driven dispatch + governance backfill
    ;; ──────────────────────────────────────────────────
    ;; 目的:
    ;;   - 把 wave 20 task 01-09 的真实代码状态回填到 source-index
    ;;   - wave 20 主线: machine-driven dispatch (Lisp 真成 dispatch SSOT) +
    ;;     scoped-commit guard (script + pre-commit hook + brief renderer 集成) +
    ;;     execution preflight contract scope + cross-plan distill auto-trigger +
    ;;     LLM-augmented PLAN inference (sonnet_suggest opt-in) +
    ;;     review auto-answer policy + ExecutionEvent legacy metadata sweep
    ;;   - 不重复已有 section-id; 在 v1.0 baseline 上新增 9 anchor entry,
    ;;     升级现有 entry: review-automation-policy-v0 (partial → code-aligned with auto-answer),
    ;;     ExecutionEvent dispatch-metadata-v1 (note 扩 全 11 variants 覆盖完成)
    ;;   - 保留 R008 + R016 (section-id 不变, 改名走 :prev-id)
    ;;   - 11-stage scheduler 主线 + machine-contract 全闭环已 close;
    ;;     仍 partial 是: 完整 LLM auto-approve 全 4 v0 non-goal /
    ;;     autonomous workstation spawn 完全无 hint /
    ;;     git pre-commit hook 默认安装 (本 wave 加了 .githooks/pre-commit
    ;;     但要 git config core.hooksPath .githooks 才生效 — 默认未启用) /
    ;;     frontend lisp 仍未启动
    ;;
    ;; wave 20 已完成 commit (anchor):
    ;;   - 3f27681 chore(wave19): archive task contracts (task 00 — 13 wave19-*.lisp + 13 wave19-*.md 入库, 仅归档不进 source-index)
    ;;   - 1fc0fd6 feat(scope): enforce task scope on staged changes (task 01 — scripts/task-scope-guard.mjs staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发 + 9+3 fixtures + 0 mutating git; commit-mode delegates verify-task-contract)
    ;;   - b36cf6c feat(renderer): emit task scope guard step (task 02 — render-claudecode-task.mjs Commit 节加 task-scope-guard --mode staged 步骤 + MISSIOND_TASK_CONTRACT env prefix; 实战通过)
    ;;   - fe835e8 feat(execution): preflight task contract scope (task 03 — mission_execution preflight_commit 加 8 新字段 task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path; 无新错误码 信息性; grep proof 0 mutating git; legacy byte-compat)
    ;;   - 681c95d feat(plan): drive workstation dispatch from task contract (task 04 — DispatchContractMode {Rendered, Machine}, dispatch_contract_mode arg / render_markdown shorthand; Machine 模式接通 wave-19 emitter ↔ consumer; 加 task_contract_source_path field; unified_entry forward emitter knobs; Lisp 真成 dispatch SSOT)
    ;;   - d308fae test(execution): pin unified-entry machine loop (task 05 — 6 smoke tests s4→s6 machine-mode loop / Markdown non-load-bearing / malformed refusal; build_artifact_refs lift 8 个 machine-contract 字段; wave-15..19 byte-compat 钉死)
    ;;   - 3669ebc feat(plan): trigger cross-plan distill chains automatically (task 06 — auto_chain_trigger default never; 6 deterministic 触发 rule 全过才触发; sonnet 仍仅显式; default off byte-compat)
    ;;   - 6bb935a feat(plan): suggest plan fields with sonnet (task 07 — infer_plan_fields=sonnet_suggest mode opt-in / suggest only / applied=false 钉死; Sonnet 不可用 llm_unavailable no fallback; DAG mode 拒; default off byte-compat)
    ;;   - 8adb0a8 feat(review): apply auto-answer review policy (task 08 — auto_answer_policy off|deterministic_safe|dry_run; 5 inherited rules + 2 new destructive_action / caller_decision_present; 3 invariants I1 never reject / I2 destructive never auto-promote / I3 no LLM)
    ;;   - 6e01e3f feat(execution): complete dispatch metadata event coverage (task 09 — 8 legacy ExecutionEvent variants 全加 dispatch trio Heartbeat/Released/DeviationRecorded/DecisionRecorded/IssueRecorded/Audited/Repaired/StaleClaim; 从 companion log 自动继承; wave14-02 启动的 ExecutionEvent metadata 全部完成)
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.unified-entry-pipeline.review-automation-policy-v0      status code-aligned-partial → code-aligned (wave 20-08 加 auto_answer_policy 闭环 5+2 rule + 3 invariants; LLM 自动 approve 仍 v0 non-goal)
    ;;   - event-bus.section.execution-event.dispatch-metadata-v1               note 扩 (wave 20-09 完成 8 legacy variants 全覆盖; 11 variants 全 dispatch trio)
    ;;   - intent-layer.machine-contract.task-contract-v1                       note 扩 (wave 20-04 machine 模式接通后 Lisp 真正成为 dispatch SSOT — Markdown 仍存为兼容视图但 non-load-bearing)
    ;; ──────────────────────────────────────────────────
    (wave-20-backfill v1.1
      :date "2026-04-27"
      :decided-by "wave 20 / task 10 lisp backfill session"
      :scope "回填 wave 20 task 00-09 真实代码状态; 新增 9 anchor entry + 1 status upgrade (review-automation-policy-v0 由 partial → full); 不发明 wave20 没实现的架构"
      :non-goal "本任务不改 Rust / SQL / JS / Cargo / 任务文档; 不真正压缩主 Lisp; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp (continue postpone); pre-commit hook 默认未 git config core.hooksPath 启用 (照实记录, 不假装默认开)"
      :commits
        [(commit-1 :hash "1fc0fd6" :title "feat(scope): enforce task scope on staged changes"
                   :primary-targets ["scripts/task-scope-guard.mjs"
                                     ".githooks/pre-commit"]
                   :tests "task-scope-guard 双 mode (staged check 默认 / commit mode 借 verify-task-contract); 9 staged-mode fixtures + 3 commit-mode fixtures 覆盖 in-scope / forbidden touched / out-of-scope staged / unstaged in-scope warning / 空 staged / 无 contract; pre-commit hook 仅在 MISSIOND_TASK_CONTRACT env 设置时触发 (其它 commit 不阻塞); 全程 0 mutating git (grep proof: 不 add/commit/stash/reset/checkout/push/merge/rebase); commit-mode 直接 delegate scripts/verify-task-contract.mjs 逻辑")
         (commit-2 :hash "b36cf6c" :title "feat(renderer): emit task scope guard step"
                   :primary-targets ["scripts/render-claudecode-task.mjs"]
                   :tests "render-claudecode-task.mjs Commit 节追加 'Pre-commit guard (staged scope)' 子步: 'MISSIOND_TASK_CONTRACT=<task-lisp> node scripts/task-scope-guard.mjs --mode staged' (env prefix + 显式 mode, 不污染 user shell env); brief 实战通过 (wave20-02 自身的 brief 渲染了该步, 后续 wave20-03..09 commit 全程跑 staged guard 在 commit 前; 0 false positive); 与 wave19-05 既有 4 节 byte-compat (新加 1 子步, 不破既有 anchor)")
         (commit-3 :hash "fe835e8" :title "feat(execution): preflight task contract scope"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
                   :tests "mission_execution(action='preflight_commit') 加 8 新 response 字段 (task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path); 当 caller 传 task_contract_path 时, daemon 读 contract 解析 write-scope + must-not-touch, 与 git status --porcelain=v1 staged/unstaged 集合做交叉, 标 'in_scope' / 'out_of_scope' / 'forbidden_touched' / 'unstaged_in_scope_warning'; 无新错误码 (信息性 surface, 让 caller 决定是否 commit); grep proof 0 mutating git (仍只 git status --porcelain=v1); legacy byte-compat 当 task_contract_path absent (8 字段全 None / not_applicable)")
         (commit-4 :hash "681c95d" :title "feat(plan): drive workstation dispatch from task contract"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "DispatchContractMode { Rendered (default), Machine } 由 dispatch_contract_mode 字符串 allowlist 或 render_markdown=false 布尔 shorthand 解析; Machine 模式接通 wave-19/06 emitter ↔ wave-19/07 consumer (run_workstation_dispatch_with_contract): emit 产出 contract path 后, runner 直接交 path 给 consumer 读盘, Markdown brief 变 optional 兼容元数据; 加 task_contract_source_path field (machine + Dispatched only); brief 加 ## Source contract preamble 让 observer 看到是 Lisp 驱动; DAG path TaskContractDispatchCtx 在 scheduler 入口锁定 mode, propagate 给每节点; unified_entry forward 4 emitter/dispatch knobs (task_contract_mode / emit_task_contract / dispatch_contract_mode / render_markdown); 1246 daemon / 17 mcp / 302 core tests + workspace build + check-task-contract --all + check-architecture-lisp --all-v2 + git diff --check + verify-task-contract 全过. **此为 wave 20 主线: Lisp 真正成为 dispatch SSOT, Markdown brief 不再是 load-bearing**")
         (commit-5 :hash "d308fae" :title "test(execution): pin unified-entry machine loop"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
                   :tests "build_artifact_refs lift 8 个 machine-contract 字段 (task_contract_mode / task_contract_eligible / task_contract_path / task_contract_source_path / dispatch_contract_mode / render_command / task_contract_skip_reason / task_contract_error); 加 6 新 unit/smoke test: (1) artifact_refs lifts every machine-contract field on synthesised wave-20/04 inner payload; (2) artifact_refs omits all machine-contract keys when inner is silent (wave-15..19 byte-compat preserved); (3) fixture-based contract overlay smoke 跑 parse_task_contract + WorkstationDispatchHints::overlay_contract + build_task_brief_with_source 不 IO; (4) 完整 machine-mode canonical loop smoke s4 plan-compile dry_run → s6 plan-execute machine mode 断言 task_contract_source_path == task_contract_path SSOT invariant; (5) Markdown brief non-load-bearing assertion 证 task_brief_preview 仅在 inner payload 不在 artifact_refs; (6) malformed-contract refusal smoke 证 parse_task_contract 返 TaskContractParseError::MissingRequired('goal') 后 SafeDescriptor 落 workstation_dispatch_status=skipped_malformed_task_contract, 无 inner_result, 无 claude -p fallback artefact; 1252 daemon (1246 + 6 new) / 17 mcp / 302 core tests OK")
         (commit-6 :hash "3669ebc" :title "feat(plan): trigger cross-plan distill chains automatically"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
                   :tests "mission_workflow(distill) 加 auto_chain_trigger arg (默认 'never' = 字节兼容); 6 deterministic 触发 rule 必须全过才触发 auto chain (parent_plan_succeeded / parent_distill_on_success_true / evidence_sha256_present / project_root_resolvable / workflow_anchor_present / chain_id_not_already_supplied); sonnet 仍仅显式 (auto_chain_trigger 不会自动 sonnet, 仍要求 caller 显式 distill_chain_mode=sonnet); 复用 wave 19-09 deterministic id 'chain:auto:wf-<sha256(...)>'; default 'never' byte-compat (legacy callers 不受影响); auto_chain_trigger='deterministic_only' 才会按 6 rule 评估; 失败任一 rule → trigger_status='skipped_<rule_name>' 结构化 surface 不静默吞")
         (commit-7 :hash "6bb935a" :title "feat(plan): suggest plan fields with sonnet"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "infer_plan_fields=sonnet_suggest 新 mode 接 wave-18/06 deterministic 之上: 先跑 deterministic, 再 Sonnet 提议; **opt-in / suggest only / applied=false 永远钉死** (Sonnet 提议永不自动落, 仅 surface llm_proposals[] 给 caller 看); Sonnet 不可用 → llm_status='llm_unavailable' (no fallback, 不静默退到 deterministic-only); DAG mode 路径 refuse_llm_inference_in_dag_mode 拒 (sonnet_suggest 仅单节点 execute mode 可用); conflict_status 重新对账 caller args + deterministic high/medium tier; default off (字节兼容); 1230 daemon + 17 mcp + workspace build + architecture-lisp + git diff --check 全过")
         (commit-8 :hash "8adb0a8" :title "feat(review): apply auto-answer review policy"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
                   :tests "AutoAnswerPolicy { Off (default), DeterministicSafe, DryRun } 由 parse_auto_answer_policy 解析 (case-insensitive + trimmed; hyphenated aliases 接受; unknown collapses to Off); evaluate_auto_answer_policy 复用 wave-18/07 ReviewAutomationContext + safety inspector 加 2 新 guard: (G1) 5 inherited rules from wave-18/07 (caller_opt_in 必填 / no_unresolved_blockers / no_pending_questions_on_target / target_in_review_state / promotion_path_explicit); (G2) destructive-action gate (archive|supersede|remove case-insensitive, centralised in DESTRUCTIVE_REVIEW_ACTIONS) NEVER 自动 promote — pinned 为 dedicated AutoAnswerStatus::SkippedDestructiveAction; (G3) caller-decision guard 让显式 review_decision 永远 win; **3 hard invariants test-pinned**: I1 NEVER auto-reject (selected_decision 在任何 mode 下都不会 'rejected', 上游 Rejected 被 demote 到 NeedsChanges + audit entry 'rule:rejection_demoted'); I2 destructive actions NEVER auto-promote 即使 5 safety rule 全过; I3 完全 pure / sync / 不调 LLM (signature enforcement + 确定性测试); I4 skipped 时 response 带 policy_result / selected_decision / safety_rule_results[] / requires_human=true; dry_run 模式跑同 inspector + guards 但 ALWAYS requires_human=true; unified_entry forward auto_answer_policy 进 plan-execute key list verbatim (pipeline 不 re-validate, inner handler 拥 allowlist) + build_artifact_refs lift 5 outcome keys; default off byte-compat (lift 仅在 inner stamped 时 fire); **NO 新 MCP tool**; **NO 实时 LLM auto-approval**; **NO destructive auto-promotion**; 1288 daemon (1252 + 36) + 17 mcp + workspace build + architecture-lisp + git diff --check 全过")
         (commit-9 :hash "6e01e3f" :title "feat(execution): complete dispatch metadata event coverage"
                   :primary-targets ["crates/missiond-core/src/event/events/execution.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
                   :tests "Sweep 8 legacy ExecutionEvent variants (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) 全加 optional dispatch_strategy/target_project/requested_cwd serde-default skip-serializing fields, 与 Opened (wave-11) / Claimed+Completed (wave-18/02) / PlanNodeStateChanged (wave-14/02) 已有 projection 协议对齐; wire-shape 影响每 variant: Heartbeat 5→8, Released 5→8 (summary 仍为 mandatory key 在 legacy shape), DeviationRecorded 4→7, DecisionRecorded 4→7, IssueRecorded 4→7, Audited 4→7, Repaired 3→6, StaleClaim 4→7; 继承路径 — 每个 emit site 调 read_dispatch_metadata_from_log(&file) 读 post-write LogFile, forward 三元组; Audit 发 Audited 后 per-stale-claim StaleClaim, 共享 1 meta snapshot via clone; serde back-compat triple-pinned: (1) legacy_json_without_dispatch_metadata 解 None; (2) without_dispatch_metadata_serializes_byte_identical_to_legacy 锁 wire shape; (3) with_dispatch_metadata_round_trips verbatim; partial-metadata sweep 跨 Heartbeat/Audited/Repaired pin per-field skip-serialize; 327 core (302 baseline + 25 new) + 1304 daemon (1288 baseline + 16 new) + 17 mcp + workspace build + architecture-lisp + git diff --check + verify-task-contract 全过. **wave14-02 启动的 ExecutionEvent metadata 协议在 wave 20-09 全 11 variants 闭环**")]

      ;; ── 区域 57 · execution preflight task-contract scope v1 (wave 20 task 03) ──
      (section-entry
        :section-id "intent-layer.scoped-commit-worktree-preflight-task-contract-v1"
        :title "execution preflight task-contract scope v1 — 8 新字段 + 0 mutating git + legacy byte-compat"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: scoped-commit-worktree-preflight-task-contract-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["intent-layer.scoped-commit-worktree-preflight-v0"
                    "intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.scoped-commit-enforce-v0"
                    "intent-layer.machine-contract.task-verifier-v1"]
        :wave "20 task 03 (commit fe835e8)"
        :note "wave 18-08 加 mission_execution(preflight_commit) 只跑 git status 不 mutating; 但当时只 surface staged/unstaged/changed_files, caller 仍要自己对 task contract write-scope 做对账. wave20-03 在同一 preflight 加 8 新 response 字段把 contract 对账下沉进 daemon: task_contract_status (resolved | not_supplied | malformed | resolution_failed) + task_contract_scope (echo write-scope + must-not-touch from contract) + staged_out_of_scope (staged 文件 ∉ write-scope) + staged_forbidden (staged 文件 ∈ must-not-touch) + unstaged_in_scope (write-scope 文件未 stage 警告) + next_step 建议 (ready_to_commit / stage_required / unstage_out_of_scope / drop_forbidden / claim_scope_required) + task_contract_error (parse 失败原因) + task_contract_resolved_path (实际读 contract 文件路径). 无新错误码 (信息性 surface, daemon 不替 caller 决定 commit/abort). **grep proof 0 mutating git** — 仍只 git status --porcelain=v1, 不 add/commit/stash/reset/checkout/push/merge/rebase. Legacy byte-compat 当 task_contract_path absent (8 字段全 None 或 not_applicable). brief → preflight (含 contract 对账) → complete enforce + verifier 四段闭环加强 (wave 17-07 + wave 18-08 + wave 19-08 + wave 20-03)")

      ;; ── 区域 58 · machine-driven dispatch v0 (wave 20 task 04) ──
      (section-entry
        :section-id "intent-layer.machine-contract.dispatch-machine-mode-v0"
        :title "machine-driven dispatch v0 — DispatchContractMode {Rendered, Machine} + Lisp 真成 dispatch SSOT + Markdown 变 optional 兼容元数据"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: dispatch-machine-mode-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.plan-dag.task-contract-emitter-v0"
                    "intent-layer.unified-entry-pipeline.workstation-task-contract-consumer-v0"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"]
        :wave "20 task 04 (commit 681c95d)"
        :note "wave 19 全闭环 plan emit (19-06) → workstation consume (19-07) → execution verify (19-08), 但 dispatch 路径仍是: plan emit contract → renderer 出 Markdown brief → dispatch 用 brief; 即 Markdown 仍是 dispatch 表层. wave 20-04 把 wave-19/07 dormant consumer (run_workstation_dispatch_with_contract) 接进 active dispatch 路径: (1) DispatchContractMode { Rendered (default 兼容 wave-15..19 wire shape), Machine } 由 dispatch_contract_mode (字符串 allowlist 'rendered' / 'machine') 或 render_markdown=false (布尔 shorthand) 解析; (2) Machine 模式: emit 产出 contract path 后, dispatch 直接 forward path 给 consumer 读盘 (overlay onto brief, contract 是 SSOT); brief 加 ## Source contract preamble 让 observer 看到是 Lisp 驱动; (3) response surface task_contract_source_path field (machine + Dispatched only); (4) Markdown 变 optional 兼容元数据 (render_command 仍 surface 给 out-of-process 渲染); (5) DAG path TaskContractDispatchCtx 在 scheduler 入口锁定 dispatch_contract_mode, propagate 给每个 per-node dispatch (no per-node override); (6) unified_entry forward 4 knobs (wave-20: dispatch_contract_mode / render_markdown + wave-19: task_contract_mode / emit_task_contract — 后者之前未 forward). Malformed contract → SafeDescriptor (status='skipped_malformed_task_contract'); **绝不 fall back claude -p**. **此为 wave 20 主线: Lisp 真正成为 dispatch SSOT, Markdown brief 不再是 load-bearing** (但 ClaudeCode worker 仍在用 Markdown — Markdown 保留 ergonomics, machine 模式 caller 可关闭 render). 默认 Rendered, 不破 wave-15..19 任何 byte-shape")

      ;; ── 区域 59 · unified-entry machine-loop smoke v2 (wave 20 task 05) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.machine-loop-smoke-v2"
        :title "unified-entry machine-loop smoke v2 — 6 smoke tests + 8 machine-contract artifact_refs lift + Markdown non-load-bearing 钉死 + malformed refusal"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: machine-loop-smoke-v2"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.machine-contract.dispatch-machine-mode-v0"
                    "intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.unified-entry-pipeline.workstation-task-contract-consumer-v0"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"
                    "intent-layer.unified-entry-pipeline.e2e-smoke-v0"
                    "intent-layer.unified-entry-pipeline.autonomous-loop-smoke-v1"]
        :wave "20 task 05 (commit d308fae)"
        :note "wave 16-08 落 deterministic 4 hand-off no-LLM smoke (s1→s4→s6 + sidecar) ; wave 17-08 加 paused-resume e2e smoke; wave 18-09 加 autonomous loop smoke v1; wave 20-05 加 machine-loop smoke v2 — 把 wave 20-04 machine-driven dispatch loop 端到端 smoke 化: (1) build_artifact_refs lift wave-19/06 + wave-20/04 8 个 machine-contract keys (task_contract_mode / task_contract_eligible / task_contract_path / task_contract_source_path / dispatch_contract_mode / render_command / task_contract_skip_reason / task_contract_error) 进 unified envelope 让 downstream 不 re-parse inner JSON; (2) 6 新 test 覆盖: artifact_refs lift 全字段 (合成 wave-20/04 inner payload), artifact_refs omit 当 inner 沉默 (wave-15..19 byte-compat preserved), fixture-based contract overlay smoke (parse + overlay + brief render 不 IO), 完整 machine-mode canonical loop smoke (s4 plan-compile dry_run → s6 plan-execute machine 断言 task_contract_source_path == task_contract_path SSOT invariant), Markdown brief non-load-bearing assertion (task_brief_preview 仅在 inner payload 不在 artifact_refs), malformed-contract refusal smoke (parse_task_contract 返 MissingRequired('goal') 后 SafeDescriptor workstation_dispatch_status='skipped_malformed_task_contract', 无 inner_result, 无 claude -p fallback). **no LLM, no spawn, no shell**; 1252 daemon (1246 + 6) / 17 mcp / 302 core tests OK; wave-15..19 byte-compat 钉死 (artifact_refs 仅在 inner stamped machine keys 时 lift)")

      ;; ── 区域 60 · cross-plan distill auto-trigger v1 (wave 20 task 06) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-trigger-v1"
        :title "cross-plan distill auto-trigger v1 — auto_chain_trigger default never + 6 deterministic trigger rules + sonnet 仍仅显式"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: cross-plan-distill-auto-trigger-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-chain-v1"
                    "intent-layer.plan-dag-runtime-v2.finalize-distill-v0"
                    "memory.directive-layer.plan-evidence-sidecar"]
        :wave "20 task 06 (commit 3669ebc)"
        :note "wave 19-09 cross-plan distill auto-chain v1 加 deterministic id 但仍要求 caller 显式传 auto_chain=true 决定串 chain. wave20-06 加 mission_workflow(distill) auto_chain_trigger arg (default 'never' = 字节兼容, 'deterministic_only' = 按 6 rule 评估): 6 deterministic trigger rule 必须全过才触发 auto chain — (R1) parent_plan_succeeded (parent plan FSM = succeeded 或 succeeded-partial), (R2) parent_distill_on_success_true (parent plan / sidecar 标 distill_on_success=true), (R3) evidence_sha256_present (parent evidence sidecar 有可哈希内容), (R4) project_root_resolvable (US 隔离), (R5) workflow_anchor_present (workflow 已注册), (R6) chain_id_not_already_supplied (caller 没传 chain_id 显式覆盖); 任一 rule 失败 → trigger_status='skipped_<rule_name>' 结构化 surface 不静默吞; sonnet 仍仅显式 (auto_chain_trigger 不会自动 sonnet, 仍要求 caller 显式 distill_chain_mode=sonnet); 复用 wave 19-09 deterministic id 'chain:auto:wf-<sha256(...)>'; default 'never' byte-compat (legacy callers 不受影响). 闭环 wave 18-05 deferred 的 'auto-trigger 不需要 caller 显式 opt-in' 但仍 deterministic, sonnet 完全自动 (5+ rule 自动满足) 仍 future")

      ;; ── 区域 61 · LLM-augmented plan inference v0 (wave 20 task 07) ──
      (section-entry
        :section-id "intent-layer.plan-dag.llm-augmented-plan-field-inference-v0"
        :title "LLM-augmented plan inference v0 — infer_plan_fields=sonnet_suggest opt-in + applied=false 钉死 + Sonnet 不可用 llm_unavailable + DAG mode 拒"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: llm-augmented-plan-field-inference-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag.autonomous-plan-field-inference-v0"
                    "intent-layer.actor.plan-dag-scheduler"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"]
        :wave "20 task 07 (commit 6bb935a)"
        :note "wave 18-06 autonomous PLAN field inference v0 是 deterministic-only (3 confidence + apply_safe 仅 high; 无 LLM). wave20-07 加 infer_plan_fields=sonnet_suggest mode 把 LLM 引入推理但保持严格安全护栏: (1) 先跑 wave-18/06 deterministic 完整流程 (高/中/低 confidence), 再 Sonnet 提议 (额外字段 / 增强 confidence); (2) **opt-in only** — default off (字节兼容); (3) **suggest only / applied=false 永远钉死** — Sonnet 提议永不自动落, 仅 surface llm_proposals[] 给 caller 看 (caller 决定是否 manual apply); (4) Sonnet 不可用 → llm_status='llm_unavailable' (no fallback, 不静默退到 deterministic-only — 显式告诉 caller LLM 那段没跑); (5) DAG mode 路径 refuse_llm_inference_in_dag_mode 拒 (sonnet_suggest 仅单节点 execute mode 可用, 防 DAG 内多节点同时 LLM 推理炸 token); (6) conflict_status 重新对账 caller args + deterministic high/medium tier (LLM 提议与 caller 显式 args 冲突 → conflict_status='caller_wins'); 1230 daemon + 17 mcp + workspace build + architecture-lisp + git diff --check 全过. **status code-aligned-partial** — Sonnet suggest 已落但 applied=false, 完整 LLM-augmented apply (e.g. medium confidence + LLM corroboration → auto apply) 仍 v0 non-goal future")

      ;; ── 区域 62 · review auto-answer policy v0 (wave 20 task 08) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-auto-answer-policy-v0"
        :title "review auto-answer policy v0 — auto_answer_policy off|deterministic_safe|dry_run + 5+2 rules + 3 hard invariants (I1 never reject / I2 destructive never auto-promote / I3 no LLM)"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-auto-answer-policy-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-automation-policy-v0"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0"
                    "intent-layer.unified-entry-pipeline.run-pipeline-helper.v1"]
        :wave "20 task 08 (commit 8adb0a8)"
        :note "wave 18-07 review_automation_policy 是 caller-driven (manual / suggest / auto_safe) deterministic 但仍要求 caller 显式提供 review_decision; review-gate listener (wave 16-02 + 17-01) 自动接 QuestionEvent::Resolved 但仍要求 question 已被人 (or upstream) answer. wave20-08 加 auto_answer_policy 把 deterministic 自动 answer 引入 listener 路径但 3 invariants 永久护栏: AutoAnswerPolicy { Off (default), DeterministicSafe, DryRun } 由 parse_auto_answer_policy 解析 (case-insensitive + trimmed; hyphenated aliases 接受; unknown collapses to Off). evaluate_auto_answer_policy 复用 wave-18/07 ReviewAutomationContext + safety inspector 加 2 新 guard: G1 destructive-action gate (archive|supersede|remove case-insensitive, centralised in DESTRUCTIVE_REVIEW_ACTIONS) NEVER 自动 promote — pinned 为 dedicated AutoAnswerStatus::SkippedDestructiveAction; G2 caller-decision guard 让显式 review_decision 永远 win. **3 hard invariants test-pinned**: **I1 NEVER auto-reject** (selected_decision 在任何 mode 下都不会 'rejected', 上游 Rejected 被 demote 到 NeedsChanges + audit entry 'rule:rejection_demoted'); **I2 destructive actions NEVER auto-promote** 即使 5 safety rule 全过; **I3 完全 pure / sync / 不调 LLM** (signature enforcement + 确定性测试). I4 skipped 时 response 带 policy_result / selected_decision / safety_rule_results[] / requires_human=true. dry_run 模式跑同 inspector + guards 但 ALWAYS requires_human=true. unified_entry forward auto_answer_policy 进 plan-execute key list verbatim (pipeline 不 re-validate, inner handler 拥 allowlist) + build_artifact_refs lift 5 outcome keys (auto_answer_policy / policy_result / selected_decision / safety_rule_results / requires_human) 进 unified envelope. default off byte-compat (lift 仅在 inner stamped 时 fire). **NO 新 MCP tool**; **NO 实时 LLM auto-approval**; **NO destructive auto-promotion**. unified_entry doc-comment pivot: legacy 'auto-answer a review question' non-goal 收窄到 'auto-answer a review question via LLM' — listener-side deterministic promotion 在 opt-in deterministic_safe 下显式允许, LLM ban 仍持. 闭环 wave 16-02 / 17-01 / 18-07 留下的 'auto-answer 仍 caller-driven' 部分; LLM 自动 approve 仍 v0 non-goal")

      ;; ── 区域 63 · ExecutionEvent legacy metadata sweep v0 (wave 20 task 09) ──
      (section-entry
        :section-id "event-bus.section.execution-event.dispatch-metadata-legacy-sweep-v0"
        :title "ExecutionEvent legacy metadata sweep v0 — 8 legacy variants 全加 dispatch trio + 11 variants 闭环 + companion log 自动继承"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar event-bus :: section execution-event :: dispatch-metadata-legacy-sweep-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-core/src/event/events/execution.rs"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
        :cross-ref ["event-bus.section.execution-event.dispatch-metadata-v1"
                    "event-bus.section.execution-event.opened-dispatch-metadata-note-wave19-11"
                    "event-bus.section.execution-event.plan-node-state-changed"
                    "intent-layer.plan-dag-runtime-v2.execution-event-decision"]
        :wave "20 task 09 (commit 6e01e3f)"
        :note "wave 14-02 启动 ExecutionEvent dispatch metadata 协议 (PlanNodeStateChanged); wave 18-02 加 Claimed + Completed; wave 19-11 NO-OP 验证 Opened 已含; wave20-09 sweep 剩余 8 legacy variants 全加 optional dispatch_strategy/target_project/requested_cwd serde-default skip-serializing fields, 11 variants 全 dispatch trio 闭环: Heartbeat 5→8, Released 5→8 (summary 仍为 mandatory key 在 legacy shape), DeviationRecorded 4→7, DecisionRecorded 4→7, IssueRecorded 4→7, Audited 4→7, Repaired 3→6, StaleClaim 4→7. 继承路径 — 每个 emit site 调 read_dispatch_metadata_from_log(&file) 读 post-write LogFile, forward 三元组; Audit 发 Audited 后 per-stale-claim StaleClaim 共享 1 meta snapshot via clone. serde back-compat triple-pinned per variant: (1) legacy_json_without_dispatch_metadata 解 None; (2) without_dispatch_metadata_serializes_byte_identical_to_legacy 锁 wire shape; (3) with_dispatch_metadata_round_trips verbatim. partial-metadata sweep 跨 Heartbeat/Audited/Repaired pin per-field skip-serialize. 327 core (302 baseline + 25 new) + 84 agent_execution (68 baseline + 16 new) + 1304 daemon (1288 baseline + 16) + 17 mcp + workspace build OK. **wave14-02 启动的 ExecutionEvent metadata 协议在 wave 20-09 全 11 variants 闭环, 不再有 dispatch metadata gap**. event-bus pillar 仍 protected (本 entry 仅 source-index 元数据声明)")

      ;; ── 区域 64 · task-scope-index-guard v1 (wave 20 task 01) ──
      (section-entry
        :section-id "intent-layer.machine-contract.task-scope-index-guard-v1"
        :title "task-scope-index-guard v1 — staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发 + 0 mutating git"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: task-scope-index-guard-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["scripts/task-scope-guard.mjs"
           ".githooks/pre-commit"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.task-verifier-v1"
                    "intent-layer.scoped-commit-enforce-v0"
                    "intent-layer.scoped-commit-handoff-brief-default-v1"]
        :wave "20 task 01 (commit 1fc0fd6)"
        :note "wave 19 闭环 task-contract: schema (19-02) + verifier (19-02) + report (19-03) + shared-memory (19-04) + renderer (19-05) + plan emit (19-06) + workstation consume (19-07) + execution complete verify (19-08); 但 commit 前的 staged scope 检查仍要求 caller 手工对账. wave20-01 加 scripts/task-scope-guard.mjs 把对账下沉到 git 端: (1) 双 mode — staged mode 默认 (commit 前检查 git diff --cached --name-only 与 task contract write-scope/must-not-touch); commit mode (post-commit, delegate scripts/verify-task-contract.mjs 逻辑); (2) 9 staged-mode fixtures + 3 commit-mode fixtures 覆盖 in-scope / forbidden touched / out-of-scope staged / unstaged in-scope warning / 空 staged / 无 contract; (3) .githooks/pre-commit hook 仅在 MISSIOND_TASK_CONTRACT env 设置时触发 task-scope-guard --mode staged (其它 commit 不阻塞 — opt-in 不破现有 workflow); (4) 全程 0 mutating git (grep proof: 不 add/commit/stash/reset/checkout/push/merge/rebase). **status code-aligned-partial** — guard script + hook 已落, 但 git config core.hooksPath .githooks 默认未启用 (要 caller 显式 git config 或 task-renderer brief 指引才生效); enforce-by-default 切 git config 默认 + worktree-level scope enforce 仍 future")

      ;; ── 区域 65 · renderer scoped-commit guard v2 (wave 20 task 02) ──
      (section-entry
        :section-id "intent-layer.machine-contract.renderer-scoped-commit-guard-v2"
        :title "renderer scoped-commit guard v2 — Commit 节加 task-scope-guard --mode staged 子步 + MISSIOND_TASK_CONTRACT env prefix"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: renderer-scoped-commit-guard-v2"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/render-claudecode-task.mjs"]
        :cross-ref ["intent-layer.machine-contract.renderer-dispatch-brief-v1"
                    "intent-layer.machine-contract.task-scope-index-guard-v1"
                    "intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.scoped-commit-handoff-brief-default-v1"]
        :wave "20 task 02 (commit b36cf6c)"
        :note "wave 19-05 renderer 加 4 节 (Machine Contract / Dispatch Note / Shared Memory / Report Contract) + verify command. wave20-02 在 Commit 节追加 'Pre-commit guard (staged scope)' 子步: 'MISSIOND_TASK_CONTRACT=<task-lisp> node scripts/task-scope-guard.mjs --mode staged' (env prefix + 显式 mode, 不污染 user shell env, brief 渲染时直接打印整条命令让 worker copy-paste 可跑). 实战通过 — wave20-02 自身 brief 已含该步, 后续 wave20-03..09 commit 全程跑 staged guard 在 commit 前; 0 false positive (8 个 commit 全过). 与 wave19-05 既有 4 节 byte-compat (新加 1 子步, 不破既有 anchor; agent-team literal 单实例条件不动). **brief → preflight (wave 18-08 + wave 20-03 contract scope 对账) → staged guard (wave 20-02) → commit → verifier (wave 19-02) → execution complete verify (wave 19-08) 五段闭环**: brief 写 task scope; preflight + staged guard 在 commit 前对账; commit 落 hash; verifier + execution complete 在 commit 后再交叉对账")

      ;; ── 区域 66 · machine-contract task-contract-v1 status note extension (wave 20 task 04 note 扩) ──
      ;; 注: section-id 已存在 (区域 36), 本 entry 不新增 anchor; 仅在 wave-20-backfill 块内
      ;;     声明 note 扩展 (machine-mode dispatch 接通后 Lisp 真正成为 dispatch SSOT)
      (status-upgrade
        :section-id "intent-layer.machine-contract.task-contract-v1"
        :prev-status code-aligned
        :new-status code-aligned
        :note-extension "wave 20-04 (commit 681c95d) 接通 wave-19/07 dormant consumer (run_workstation_dispatch_with_contract) 进 active dispatch 路径: DispatchContractMode { Rendered (default 兼容), Machine } + dispatch_contract_mode arg / render_markdown shorthand; Machine 模式 dispatch 直接 forward contract path 给 consumer 读盘, Markdown brief 变 optional 兼容元数据 (render_command 仍 surface). **此为 wave 20 主线: Lisp 真正成为 dispatch SSOT, Markdown brief 不再 load-bearing** (但 ClaudeCode worker 仍可用 Markdown — Markdown 保留 ergonomics). 默认 Rendered, 不破 wave-15..19 任何 byte-shape. 见 anchor intent-layer.machine-contract.dispatch-machine-mode-v0")

      ;; ── 区域 67 · review-automation-policy-v0 status upgrade (wave 20 task 08 加 auto_answer_policy) ──
      ;; 注: section-id 已存在 (区域 wave-18-backfill), 本 entry 不新增 anchor; 仅在 wave-20-backfill 块内
      ;;     声明 status code-aligned-partial → code-aligned 升级 (wave 20-08 加 auto-answer-policy v0)
      (status-upgrade
        :section-id "intent-layer.unified-entry-pipeline.review-automation-policy-v0"
        :prev-status code-aligned-partial
        :new-status code-aligned
        :evidence-commits ["8adb0a8 (wave 20-08 review auto-answer policy v0)"]
        :rationale "wave 18-07 落 review_automation_policy v0 (manual|suggest|auto_safe) + 5 deterministic safety rule 但仍要求 caller 显式提供 review_decision; wave 20-08 加 auto_answer_policy (off|deterministic_safe|dry_run) 把 deterministic 自动 answer 引入 listener 路径并 pin 3 hard invariants (I1 never auto-reject / I2 destructive never auto-promote / I3 no LLM). 5 inherited rules from wave-18/07 + 2 new guards (destructive_action / caller_decision_present). LLM 自动 approve 仍 v0 non-goal future. 见 anchor intent-layer.unified-entry-pipeline.review-auto-answer-policy-v0")

      ;; ── 区域 68 · ExecutionEvent dispatch-metadata-v1 status note extension (wave 20 task 09 完成 11 variants 全覆盖) ──
      ;; 注: section-id 已存在 (区域 wave-18-backfill), 本 entry 不新增 anchor; 仅在 wave-20-backfill 块内
      ;;     声明 note 扩展 (8 legacy variants 全加 trio, 11 variants 全闭环)
      (status-upgrade
        :section-id "event-bus.section.execution-event.dispatch-metadata-v1"
        :prev-status code-aligned
        :new-status code-aligned
        :note-extension "wave 14-02 启动 dispatch metadata 协议 (PlanNodeStateChanged); wave 18-02 加 Claimed + Completed; wave 19-11 NO-OP 验证 Opened 已含; **wave 20-09 (commit 6e01e3f) sweep 剩余 8 legacy variants** (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) 全加 optional dispatch_strategy/target_project/requested_cwd serde-default skip-serializing fields, 从 companion log 自动继承 via read_dispatch_metadata_from_log helper. **11 variants 全 dispatch trio 闭环, 不再有 dispatch metadata gap**. 见 anchor event-bus.section.execution-event.dispatch-metadata-legacy-sweep-v0"))

    ;; ──────────────────────────────────────────────────
    ;; wave 21 task 09 · lisp-backfill-wave21-status v1.2
    ;; 范围: 回填 wave 21 task 00-08 真实代码状态 — 9 anchor entry + 1 status upgrade
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.unified-entry-pipeline.review-automation-policy-v0      note 扩 (wave 21-06 加 LLM auto-approve propose 第三 review knob, 与 wave-18/07 + wave-20/08 共存独立观测; LLM 自动 approve apply 仍 v0 non-goal)
    ;;   - intent-layer.machine-contract.task-contract-v1                       note 扩 (wave 21-02/03 加 三合一 run verifier + execution-side daemon-internal verified gate; 真正 6 段闭环 brief→preflight→staged guard→commit→verifier→execution complete verify)
    ;; ──────────────────────────────────────────────────
    (wave-21-backfill v1.2
      :date "2026-04-27"
      :decided-by "wave 21 / task 09 lisp backfill session"
      :scope "回填 wave 21 task 00-08 真实代码状态; 新增 9 anchor entry + 2 status upgrade (review-automation-policy-v0 note 扩 加 第三 review knob + task-contract-v1 note 扩 加 6 段闭环); 不发明 wave21 没实现的架构"
      :non-goal "本任务不改 Rust / SQL / JS / Cargo / 任务文档; 不真正压缩主 Lisp; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp (continue postpone); hooks installer 默认未 git config core.hooksPath 启用 (wave21-01 仍 opt-in repo-local only — 照实记录, 不假装 default-on); wave21-04 workstation propose 是 propose-only / applied=false / auto_spawn=false 永钉死 — 照实记录, 不当作完整 autonomous spawn; wave21-05 plan inference apply gate 是 caller approval-gated / persist_inference_applied=false 永钉死 — 照实记录, 不当作完整 persisted plan inference apply; wave21-06 LLM auto-approve 是 propose-only / applied=false / requires_human=true 永钉死 — 照实记录, 不当作完整 LLM auto-approve apply; wave21-07 sonnet distill chain 仍 dual opt-in required — 照实记录, 不当作完全自动 sonnet 接 chain"
      :commits
        [(commit-1 :hash "44c74df" :title "feat(tasks): add MissionD hooksPath installer"
                   :primary-targets ["scripts/install-missiond-hooks.mjs"
                                     "scripts/check-missiond-hooks.mjs"
                                     ".githooks/pre-commit"
                                     ".missiond/tasks/schema/task-contract-v1.lisp"]
                   :tests "install-missiond-hooks.mjs ships --check/--install/--json/--dry-fixture/--strict; check-missiond-hooks.mjs is read-only doctor alias (rejects --install/--dry-fixture). Installer is repo-local only: --install runs `git config --local core.hooksPath .githooks` exactly once and is no-op when already aligned; never touches --global/--system. 8 dry-fixtures all green; --check --json reports drift correctly when not yet opted in. .githooks/pre-commit remains MISSIOND_TASK_CONTRACT env-gated (其它 commit 不阻塞). Schema records hooks-installer-contract section. **opt-in repo-local only — 不擅自 default-on** (per task brief). Default behaviour stays opt-in; enable per clone via `node scripts/install-missiond-hooks.mjs --install`")
         (commit-2 :hash "1335fa7" :title "feat(tasks): verify complete task runs"
                   :primary-targets ["scripts/verify-task-run.mjs"
                                     "scripts/verify-task-contract.mjs"
                                     ".missiond/tasks/schema/report-contract-v1.lisp"
                                     ".missiond/tasks/schema/shared-memory-v1.lisp"]
                   :tests "scripts/verify-task-run.mjs 三合一 (task contract + report + shared-memory + commit) post-run proof: --task/--report/--memory/--commit/--json/--dry-fixture/--allow-missing-memory; delegates contract verification to verify-task-contract.verifyContract; checks report :task_id == contract id, report :commit_hash matches resolved git sha (full or >=7-hex prefix), shared-memory has a (completion :task <id>) entry. Missing --memory or non-existent ledger fails with structured diagnostic unless --allow-missing-memory is passed. Read-only by construction (assertReadOnlyGit allowlist documents the 14 forbidden git verbs; only verify-task-contract.readCommit's `git rev-parse | log | show` are reachable). 12 dry-fixtures + 7 helper cases all green. verify-task-contract.mjs main() now gated by import.meta.url so importers don't trigger CLI parsing. Schema files (report-contract-v1.lisp, shared-memory-v1.lisp) cross-link the new :run-verifier. Self-verify (dogfood) verify-task-run: ok=true, all 4 checks green against the new commit")
         (commit-3 :hash "308426e" :title "feat(execution): record task run verification status"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
                   :tests "action=complete 加 4 新字段 (task_run_verifier_status enum passed|failed|skipped|unknown / shared_memory_path / verifier_diagnostics free-form / verified bool); verified=true 触发 daemon-internal enforce_verified_completion gate (要求 enforce_scoped_commit=true + task_contract_path + task_report_path + commit_hash) + 加载 report file off disk via local sexp module (无新 dep) + 断言 :schema=missiond.report-contract.v1 + :task_id matches contract head id + :commit_hash matches supplied hash with short<->long sha prefix overlap. 4 structured error codes (VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH + TASK_REPORT_REQUIRED/MALFORMED + TASK_REPORT_TASK_ID_MISMATCH + TASK_REPORT_COMMIT_HASH_MISMATCH + TASK_CONTRACT_MALFORMED) — 全在 companion log mutation 之前 fail-fast. 4 字段持久化为 Lisp keyword 进 completion entry; CompletionRecord struct 加 4 Option fields; legacy companion logs 不带新字段 byte-identical preserved. action=preflight_commit 也 echoes task_report_path/shared_memory_path advisory hints. **daemon_never_invokes_mutating_git unit-pinned** (Command::new(<git>) 在 agent_execution.rs returns ONE hit — wave18-08 status read; zero mutating git argv strings introduced; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威). MCP schema gains 4 new properties + tool description appended with wave21-03 paragraph. cargo test agent_execution::tests 102/102 (17 new); daemon 1370/1370; mcp 17/17; build clean")
         (commit-4 :hash "68b84f1" :title "feat(workstation): propose autonomous dispatch with Sonnet"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "workstation_inference_mode=off|sonnet_suggest opt-in (default off preserves wave-15..20 byte-shape). When opted in AND caller / PLAN supplied no signals (gated by WorkstationProposalGate: caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in 全 false), runner asks Sonnet to PROPOSE values for 4 core workstation knobs (target / dispatch_strategy / objective / scope). Proposals validated into structured records (field, value, confidence, evidence, safety_status); cap 6; surfaced under workstation_proposals[] with bundle-level auto_spawn=false and per-proposal applied=false; **NEVER auto-spawn / auto-apply**. Conservative whitelists: target ∈ {mission_execution, mission_task_delegate, mission_flow_run}; dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} (prompt-fallback / unknown excluded). Sonnet unavailable surfaces status='llm_unavailable' + unavailable_reason text pinning 'no fallback to claude -p / prompt mode in v0'; gate-suppressed surfaces status='plan_hints_present' + signal_summary listing which slots fired. DAG mode (scheduler_mode=dag_v1) preflight rejects sonnet_suggest with INVALID_PARAM. attach_workstation_proposals_block splices the bundle onto every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge); errors and pre-existing keys preserved untouched. cargo test workstation_dispatch::tests 109 passed / plan::tests 249 passed / mcp 17/17 / build clean")
         (commit-5 :hash "a18200b" :title "feat(plan): gate applying inferred PLAN fields"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "新 apply gate v1: apply_inferred_fields (bool, default false) + llm_caller_approved (object {field: bool} or array of field strings) + persist_inference (bool, default false) — all strict-shape validated pre-flight (string \"true\" rejected with INVALID_PARAM). Default suggest-only ⇒ wave-18..20 byte-shape preserved (wave-18 ApplySafe still auto-fills high-confidence slots when gate flag absent for back-compat). Opt-in (apply_inferred_fields=true) promotes ONLY: (a) deterministic high-confidence + no-conflict + caller-slot-empty fields; (b) wave-20 LLM proposals where caller named the field in llm_caller_approved AND confidence ∈ {high, medium} AND conflict_status=none AND per-field safety check passes (target ∈ {mission_execution, mission_task_delegate, mission_flow_run}; dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} — prompt-fallback/unknown excluded; acceptance_mode via canonicalize_acceptance_mode; owned_files non-empty string array; target_project non-empty; workstation_dispatch bool). Caller-vs-inferred conflicts NEVER apply (land on conflict_fields[]). Sub-threshold suggestions never apply (skipped reason `below_apply_threshold`). Response surfaces apply_gate block on every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge / preview short-circuit / sonnet_suggest short-circuit) with stable shape: {requested, persist_inference_requested, persist_inference_applied (always false in v1 — hard-pinned invariant), applied_fields[] (each {field, value, source, origin ∈ {deterministic_inferred, llm_proposal}}), skipped_fields[] (each {field, reason ∈ 8 canonical, origin, [detail]}), conflict_fields[], resulting_plan_preview (caller args ∪ applied)}. **persisted plan.sexp_text 永不 mutate** persist_inference_applied=false 永钉死; persist_inference flag echoed for future-wave wiring. attach_apply_gate_block mirrors attach_inference_block / attach_workstation_proposals_block: errors / pre-existing keys preserved untouched. 22 new unit tests added (all pure / no AppState). cargo test plan_dag::tests 249/249 / plan::tests 271/271 (22 new) / daemon 1392/1392 / mcp 17/17 / build clean")
         (commit-6 :hash "e140773" :title "feat(review): propose LLM auto approvals"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/directive.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "新 auto_approve_mode 知识 (off | sonnet_suggest, default off preserves wave-18..20 byte-shape). When opted in, asks Sonnet to PROPOSE structured review decision and surfaces it under llm_auto_approve_proposal* on every dispatch branch (legacy / wave-15 explicit / wave-18 policy-only). Bundle shape: {mode, status (not_invoked|llm_unavailable|suggested|destructive_blocked|no_suggestion), proposal {decision, confidence, evidence, non_goal_check, destructive_check, requires_human, applied=false}, proposal_warnings[], unavailable_reason, action, request_caller, model}. Wired into directive.action_approve / action_archive / both _with_resolution / both _with_policy_only AND plan.action_approve / action_mark / action_supersede + their _with_resolution + _with_policy_only sister functions (10 total branch sites). **5 strict invariants pinned by 22 new pure unit tests** (review_gate.rs:tests::auto_approve_*): (I1) proposal NEVER carries decision=rejected; rejected from model demoted to needs_changes with proposal_warnings[] entry; (I2) destructive actions (archive | supersede | remove, case-insensitive) short-circuit to destructive_blocked WITHOUT calling Sonnet — proposal=None, requires_human=true pinned; (I3) applied=false pinned on EVERY proposal; requires_human=true forced even on non-destructive in v0 (propose-only); (I4) Sonnet unavailable surfaces status=llm_unavailable + unavailable_reason; NO fallback to deterministic; (I5) destructive_check ALWAYS sourced from is_destructive_review_action(action), overwriting model output via enforce_proposal_invariants helper. Strict-enum parse: typo `auto_approve_mode=\"auto\"` fails fast with INVALID_PARAM (parse_proposer_mode_or_error) BEFORE any DB read. Caller-supplied review_decision ALWAYS wins. MCP descriptors updated on directive.rs + plan.rs with full invariant table + paragraph. cargo test review_gate::tests 180/180 (22 new); directive::tests 43/43; plan::tests 271/271; daemon 1420/1420 (+28 from 1392); mcp 17/17; build clean. ORTHOGONAL to wave-18/07 + wave-20/08 — 三 review knob 共存 (review_automation_policy + auto_answer_policy + auto_approve_mode) 独立观测")
         (commit-7 :hash "4d494db" :title "feat(workflow): gate Sonnet distill chain auto-apply"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
                   :tests "新 auto_sonnet apply-gate v1 (default false → wave-20 byte-shape preserved). When opted in alongside wave-20 auto_chain_trigger=auto_safe AND ALL 6 deterministic safety rules pass AND caller attests auto_sonnet_approved=true AND distill_mode is not already sonnet, daemon auto-promotes inner distill from dry_run to sonnet via direct call to action_distill_sonnet and replaces outer payload (carrying forward wave-19 auto_chain + wave-20 auto_trigger receipts). **8-status taxonomy**: not_requested | disabled | skipped_no_trigger | skipped_rules_failed | skipped_caller_approval_missing | skipped_already_sonnet | skipped_inner_error | applied_sonnet. **4-status model_call_status taxonomy**: not_invoked | invoked | failed | invalid_output. Block shape: {requested, status, applied, review_required, model_call_status, safety_rule_results, caller_approval, caller_distill_mode?, model_call_error?, sidecar?, chain_id?} + top-level shortcut auto_sonnet_status. **7 conservative invariants enforced**: I1 default-off; I2 dual opt-in required (single typo cannot escalate); I3 ALL 6 wave-20 rules must pass (gate REUSES trigger outcomes); I4 caller-already-sonnet refusal; I5 Sonnet failure PRESERVES inner payload (handler error → status=applied_sonnet + model_call_status=failed; structured error envelope from sonnet → model_call_status=invalid_output); I6 review_required=true PINNED on every successful auto-apply (receipt-only, no DB transition); I7 wave-19/20 blocks UNCHANGED (purely additive). Strict shape validation: workflow.rs::validate_auto_sonnet_args (front-line at action entry) + plan.rs::validate_distill_chain_args defense-in-depth — typo `auto_sonnet=\"true\"` (string) / `auto_sonnet_approved=1` (number) fails fast INVALID_PARAM with shape-label diagnostic. plan.rs::apply_distill_chain forwards auto_sonnet args into workflow.distill sub-call so plan-side callers reach the same gate. MCP descriptor on workflow.rs gains auto_sonnet + auto_sonnet_approved properties. 16 new workflow.rs + 4 new plan.rs unit tests. cargo test workflow::tests 142/142 (16 new); plan::tests 275/275 (4 new); daemon 1439/1439 (+19 from 1420); mcp 17/17; build clean")
         (commit-8 :hash "8ba8723" :title "test(intent): cover autonomous machine-contract loop"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
                   :tests "Deterministic end-to-end smoke covering wave21 machine-contract autonomous loop across 4 handler files in :write-scope. **15 new tests grouped by handler**: unified_entry.rs adds 3 (smoke_wave21_machine_contract_autonomous_loop_envelope_pins_every_invariant — single envelope round-trip pinning row-id pointers + machine-contract splice (task_contract_path == task_contract_source_path SSOT) + wave21-04 workstation_proposals applied=false / auto_spawn=false / I3-04 + wave21-06 LLM auto-approve decision != rejected / requires_human=true / applied=false / I1-06 + I3-06 + wave21-07 default-off byte-shape (no auto_sonnet block when not requested) / I1-07 + wave-19/20 distill_chain triggered=true status=recorded chain_mode=record_only / I3-07 + v0 non-goals loud; smoke_wave21_llm_auto_approve_destructive_action_pins_requires_human — destructive action archive pins requires_human=true regardless of model output / I2-06 + destructive_check sourced from is_destructive_review_action / I5-06; smoke_wave21_machine_contract_loop_proof_markdown_is_non_load_bearing — task_brief_preview MUST stay on inner payload, NOT in artifact_refs (machine-mode brief is non-load-bearing); brief preview still surfaces ## Source contract preamble on inner payload). plan.rs adds 3 (distill_chain_block_pins_skip_status / distill_chain_block_surfaces_inner_distill_warning / machine_dispatch_response_pins_source_path_invariant). workstation_dispatch.rs adds 3 (workstation_proposals_pin_applied_false_invariant_across_safety_spectrum — fixture spans Safe + InvalidStrategy + Safe; applied=false on every proposal + bundle auto_spawn=false; classify_proposal_safety MUST tag UnsupportedTarget; workstation_proposal_unavailable_pins_no_silent_fallback — Sonnet unavailable surfaces status=llm_unavailable + reason naming `no fallback` / `claude -p` / `prompt mode`; auto_spawn=false pinned even on llm_unavailable; machine_contract_overlay_drives_brief_renderer — parse_task_contract → overlay_contract → build_task_brief_with_source: contract :goal beats stale caller objective + :scope wins + :write-scope replaces stale owned_files + :must-not-touch surfaces verbatim + :commit :policy reaches hints + :dispatch-strategy beats caller fresh-code-alignment + brief carries ## Source contract preamble + agent-team hint exactly once). agent_execution.rs adds 6 (machine_contract_autonomous_loop_verifier_accepts_aligned_triple — wave21-03 happy path: aligned (contract, report, hash) triple passes verifier + summary echoes resolved paths + :checked surfaces all 5 wave21-03 rules; malformed_report_task_id_yields_structured_failure → TASK_REPORT_TASK_ID_MISMATCH; malformed_task_contract_yields_structured_failure → TASK_CONTRACT_MALFORMED; missing_report_yields_structured_failure → TASK_REPORT_REQUIRED; mismatched_commit_hash_yields_structured_failure → TASK_REPORT_COMMIT_HASH_MISMATCH; report_and_contract_projectors_extract_required_fields — read_report_summary + read_task_contract_id cross-check anchor). cargo test unified_entry::tests 67/67 (+3); agent_execution::tests 108/108 (+6); daemon 1454/1454 (+15 from 1439); build clean. **Cross-wave invariants pinned**: I1-06 / I2-06 / I3-06 / I5-06 / I3-04 / I1-07 / I3-07 / I7-07 / wave-19 task-contract / wave-20 dispatch-contract / wave21-03 verifier 5-rule cross-check. Markdown is non-load-bearing — task_brief_preview NEVER projected into artifact_refs (二度钉死). no LLM, no spawn, no shell")]

      ;; ── 区域 69 · hooks-path installer v1 (wave 21 task 01) ──
      (section-entry
        :section-id "intent-layer.machine-contract.hooks-path-installer-v1"
        :title "hooks-path installer v1 — explicit installer/doctor for repo-local core.hooksPath .githooks + opt-in only (不擅自 default-on)"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: hooks-path-installer-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["scripts/install-missiond-hooks.mjs"
           "scripts/check-missiond-hooks.mjs"
           ".githooks/pre-commit"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.task-scope-index-guard-v1"
                    "intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.renderer-scoped-commit-guard-v2"]
        :wave "21 task 01 (commit 44c74df)"
        :note "wave 20-01 落 task-scope-guard.mjs + .githooks/pre-commit env-gated 但要求 caller 显式 git config 才生效. wave21-01 加显式 installer + doctor: scripts/install-missiond-hooks.mjs (--check/--install/--json/--dry-fixture/--strict) + scripts/check-missiond-hooks.mjs (read-only doctor alias, rejects --install/--dry-fixture). --install 跑 `git config --local core.hooksPath .githooks` 一次 + already-aligned no-op + never --global/--system. --check 报告 drift (current vs expected). --json 出结构化 status. 8 dry-fixtures all green. .githooks/pre-commit 仍 MISSIOND_TASK_CONTRACT env-gated 不破现有 workflow. Schema records hooks-installer-contract section. **status code-aligned-partial** — installer + doctor 已落, 但 git config core.hooksPath .githooks 默认未启用 (per task brief: 'Do not enable hooks globally; repo-local only'); enable per clone via 显式 install. **opt-in repo-local only — 不擅自 default-on**; default default-on + worktree-level scope enforce 仍 future")

      ;; ── 区域 70 · task-run verifier v1 (wave 21 task 02) ──
      (section-entry
        :section-id "intent-layer.machine-contract.task-run-verifier-v1"
        :title "task-run verifier v1 — 三合一 (task contract + report + memory completion + commit) post-run proof + 14 forbidden git verb proof + dogfood self-verify"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: task-run-verifier-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/verify-task-run.mjs"
           "scripts/verify-task-contract.mjs"
           ".missiond/tasks/schema/report-contract-v1.lisp"
           ".missiond/tasks/schema/shared-memory-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.task-verifier-v1"
                    "intent-layer.machine-contract.task-scope-index-guard-v1"
                    "intent-layer.machine-contract.hooks-path-installer-v1"]
        :wave "21 task 02 (commit 1335fa7)"
        :note "wave 19 全闭环 task-contract: schema (19-02) + verifier (19-02) + report (19-03) + shared-memory (19-04) + renderer (19-05) + plan emit (19-06) + workstation consume (19-07) + execution complete verify (19-08); 但每个工件单独走自己 checker — 没有一个 single proof. wave21-02 加 scripts/verify-task-run.mjs 三合一 post-run proof: --task <task.lisp> + --report <report.lisp> + --memory <shared-memory.lisp> + --commit <hash> + --json + --dry-fixture + --allow-missing-memory; delegates contract verification to verify-task-contract.verifyContract (复用 verifier 不重写). 4 cross-checks: (1) task contract passes wave-19/02 verifier; (2) report :task_id == contract head id; (3) report :commit_hash matches resolved git sha (full or >=7-hex prefix overlap); (4) shared-memory has a (completion :task <id>) entry. Missing --memory or non-existent ledger fails with structured diagnostic unless --allow-missing-memory is passed. **read-only by construction** — assertReadOnlyGit allowlist documents the 14 forbidden git verbs (add / commit / stash / reset / checkout / push / merge / rebase / cherry-pick / pull / fetch / branch / tag / clean); only verify-task-contract.readCommit's `git rev-parse | log | show` are reachable. 12 dry-fixtures + 7 helper cases all green. **Important**: verify-task-contract.mjs main() now gated by `import.meta.url === pathToFileURL(process.argv[1]).href` so importers don't trigger CLI parsing (key fix to enable script-as-library use). Schema files (report-contract-v1.lisp, shared-memory-v1.lisp) cross-link the new :run-verifier. Self-verify (dogfood): verify-task-run --task ... --report ... --memory ... --commit 1335fa7 → ok=true, all 4 checks green against the new commit (即, 此 verifier proves itself just-committed)")

      ;; ── 区域 71 · execution report-verifier integration v1 (wave 21 task 03) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.execution-report-verifier-integration-v1"
        :title "execution report-verifier integration v1 — mission_execution(complete) 加 4 新字段 + verified=true daemon-internal sexp cross-check + 4 structured error codes + daemon_never_invokes_mutating_git unit-pinned"
        :source-file ".missiond/v2/intent-execution-governance.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: execution-report-verifier-integration-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["intent-layer.machine-contract.task-run-verifier-v1"
                    "intent-layer.unified-entry-pipeline.task-contract-completion-verification-v0"
                    "intent-layer.scoped-commit-worktree-preflight-task-contract-v1"
                    "intent-layer.scoped-commit-enforce-v0"
                    "intent-layer.machine-contract.task-contract-v1"]
        :wave "21 task 03 (commit 308426e)"
        :note "wave 19-08 落 execution task-contract completion verification v0 (mission_execution(complete) 加 task_contract_path/task_report_path/verifier_status/verifier_notes 4 字段); wave 21-02 落 scripts/verify-task-run.mjs 三合一 post-run proof — 但 daemon-side 仍仅 store caller-supplied verifier 状态字, 不主动 cross-check report file. wave21-03 加 daemon-internal verified gate 把验证下沉到 daemon: action=complete 加 4 新字段 (task_run_verifier_status enum passed|failed|skipped|unknown / shared_memory_path string / verifier_diagnostics free-form / verified bool); verified=true 触发 daemon-internal enforce_verified_completion gate 要求 (a) 前提条件 (enforce_scoped_commit=true + task_contract_path + task_report_path + commit_hash), (b) 加载 report file off disk via local sexp module (无新 dep), (c) 断言 :schema=missiond.report-contract.v1 + :task_id matches contract head id + :commit_hash matches supplied hash with short<->long sha prefix overlap. **4 structured error codes**: VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH (前提条件缺) + TASK_REPORT_REQUIRED/MALFORMED + TASK_REPORT_TASK_ID_MISMATCH + TASK_REPORT_COMMIT_HASH_MISMATCH + TASK_CONTRACT_MALFORMED — 全在 companion log mutation 之前 fail-fast (绝不污染 history). 持久化为 Lisp keyword (`:task-run-verifier-status` / `:shared-memory-path` / `:verifier-diagnostics` / `:verified`); `verified` writes as bare true/false atom. CompletionRecord struct extended with 4 Option fields; legacy companion logs omitting them keep byte-identical shape. action=preflight_commit 也 echoes task_report_path/shared_memory_path advisory hints. **daemon NEVER spawns Node** — wave21-02 verifier 仍 out-of-process 权威; daemon 只用 in-tree sexp parser 做 cross-check. **daemon_never_invokes_mutating_git unit-pinned** (Command::new(<git>) 在 agent_execution.rs returns ONE hit — wave18-08 status read; zero mutating git argv strings introduced). MCP schema gains 4 new properties + tool description appended. cargo test agent_execution::tests 102/102 (17 new); daemon 1370/1370; mcp 17/17; build clean. **brief → preflight (含 contract scope) → staged guard → commit → verifier (post-commit) → execution complete verify (daemon-internal verified gate) 六段闭环**")

      ;; ── 区域 72 · autonomous workstation LLM proposal v0 (wave 21 task 04) ──
      (section-entry
        :section-id "intent-layer.workstation.autonomous-workstation-llm-proposal-v0"
        :title "autonomous workstation LLM proposal v0 — workstation_inference_mode opt-in + 4 propose 字段 × 3 confidence × 4 safety + propose only never auto-spawn (applied=false / auto_spawn=false 永钉死)"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: autonomous-workstation-llm-proposal-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.plan-dag.autonomous-plan-field-inference-v0"
                    "intent-layer.plan-dag.llm-augmented-plan-field-inference-v0"
                    "intent-layer.machine-contract.dispatch-machine-mode-v0"]
        :wave "21 task 04 (commit 68b84f1)"
        :note "wave 15-05 + wave 16-03 落 workstation-dispatch v0 + auto-inference v1 (deterministic 5 rule); wave 18-06 加 autonomous PLAN field inference v0 deterministic; wave 20-07 加 LLM-augmented plan inference v0 sonnet_suggest. **但 workstation dispatch 自身仍要求 PLAN hint 或 caller 显式 args, 没有 LLM 完全 propose target/strategy/objective/scope 路径**. wave21-04 加 workstation_inference_mode=off|sonnet_suggest opt-in (default off preserves wave-15..20 byte-shape). When opted in AND caller / PLAN supplied no signals (gated by **WorkstationProposalGate**: caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in 全 false), runner asks Sonnet to PROPOSE values for 4 core workstation knobs (target / dispatch_strategy / objective / scope). Proposals validated into structured records (field, value, confidence ∈ {high, medium, low}, evidence, safety_status ∈ {Safe, InvalidTarget, InvalidStrategy, UnsupportedTarget}); **cap 6**; surfaced under workstation_proposals[] with bundle-level **auto_spawn=false** and per-proposal **applied=false**; **NEVER auto-spawn / auto-apply — applied=false / auto_spawn=false 永钉死**. Conservative whitelists: target ∈ {mission_execution, mission_task_delegate, mission_flow_run}; dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} (prompt-fallback / unknown excluded). Sonnet unavailable surfaces status='llm_unavailable' + unavailable_reason 钉 'no fallback to claude -p / prompt mode in v0'; gate-suppressed surfaces status='plan_hints_present' + signal_summary listing which slots fired. **DAG mode (scheduler_mode=dag_v1) preflight rejects sonnet_suggest with INVALID_PARAM** (sonnet_suggest 仅单节点 execute mode 可用, 防 DAG 内多节点同时 LLM propose 炸 token). attach_workstation_proposals_block splices the bundle onto every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge); errors and pre-existing keys preserved untouched. **status code-aligned-partial** — propose 已落但永不 auto-spawn; 完整 autonomous workstation true spawn (propose → caller 显式 approve → 真 spawn) 仍 v0 non-goal future")

      ;; ── 区域 73 · plan inference apply gate v1 (wave 21 task 05) ──
      (section-entry
        :section-id "intent-layer.plan-dag.plan-inference-apply-gate-v1"
        :title "plan inference apply gate v1 — apply_inferred_fields=true opt-in apply gate + 6 道严格 gate + 8 skip reason canonical + persisted plan.sexp_text 永不 mutate (persist_inference_applied=false 永钉死)"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: plan-inference-apply-gate-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag.autonomous-plan-field-inference-v0"
                    "intent-layer.plan-dag.llm-augmented-plan-field-inference-v0"
                    "intent-layer.workstation.autonomous-workstation-llm-proposal-v0"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "21 task 05 (commit a18200b)"
        :note "wave 18-06 落 autonomous PLAN field inference v0 deterministic-only (3 confidence + apply_safe 仅 high 自动落); wave 20-07 加 sonnet_suggest 但 applied=false 永钉死. wave21-05 加 explicit apply gate v1 把 apply 升级到 caller-controlled: apply_inferred_fields (bool, default false) + llm_caller_approved (object {field: bool} or array of field strings) + persist_inference (bool, default false) — all strict-shape validated pre-flight (string \"true\" rejected with INVALID_PARAM). Default suggest-only ⇒ wave-18..20 byte-shape preserved (wave-18 ApplySafe still auto-fills high-confidence slots when gate flag absent for back-compat). Opt-in (apply_inferred_fields=true) promotes ONLY: (a) deterministic high-confidence + no-conflict + caller-slot-empty fields; (b) wave-20 LLM proposals where caller named the field in llm_caller_approved AND confidence ∈ {high, medium} AND conflict_status=none AND per-field safety check passes (target ∈ {mission_execution, mission_task_delegate, mission_flow_run}; dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} — prompt-fallback/unknown excluded; acceptance_mode via canonicalize_acceptance_mode; owned_files non-empty string array; target_project non-empty; workstation_dispatch bool). **6 道严格 gate**: caller_approval (llm_caller_approved 显式) / master_flag (apply_inferred_fields=true) / confidence ∈ {high (deterministic) | high+medium (LLM)} / conflict_status=none / per-field safety / slot availability (caller-empty). Caller-vs-inferred conflicts NEVER apply (land on conflict_fields[]). Sub-threshold suggestions never apply (skipped reason `below_apply_threshold`). **8 skip reason canonical**: apply_gate_not_requested / caller_value_already_set / caller_value_conflict / below_apply_threshold / llm_not_caller_approved / llm_confidence_too_low / llm_conflict_present / llm_safety_check_failed / deterministic_inferred_already_applied. Response surfaces apply_gate block on every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge / preview short-circuit / sonnet_suggest short-circuit) with stable shape: {requested, persist_inference_requested, persist_inference_applied (always false in v1 — hard-pinned invariant), applied_fields[] (each {field, value, source, origin ∈ {deterministic_inferred, llm_proposal}}), skipped_fields[] (each {field, reason ∈ 8 canonical, origin, [detail]}), conflict_fields[], resulting_plan_preview (caller args ∪ applied)}. **persisted plan.sexp_text 永不 mutate** persist_inference_applied=false 永钉死 — persist_inference flag echoed for future-wave wiring (a future wave will land the persisted plan write under existing `persist=true` action arg or this explicit flag). attach_apply_gate_block mirrors attach_inference_block / attach_workstation_proposals_block: errors / pre-existing keys preserved untouched. plan_dag.rs unchanged in this commit (in scope but no DAG-side wiring needed — DAG path already rejects sonnet_suggest at preflight, and the wave-18 ApplySafe back-compat is preserved for the dag_v1 inference branch). **status code-aligned-partial** — apply gate 已落但 persisted plan.sexp_text 永不被 mutate; 完整 persisted plan inference apply (sonnet_suggest 升级到 真改 plan.sexp_text 落盘) 仍 v0 non-goal future")

      ;; ── 区域 74 · LLM auto-approve proposal v0 (wave 21 task 06) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.llm-auto-approve-proposal-v0"
        :title "LLM auto-approve proposal v0 — auto_approve_mode opt-in for directive/plan review surfaces + 5 invariants (I1 never reject / I2 destructive blocked WITHOUT LLM call / I3 applied=false+requires_human=true / I4 no fallback / I5 destructive_check from helper) + 22 unit tests + 10 dispatch branch sites + ORTHOGONAL to wave-18/07 + wave-20/08 (3 review knob 共存)"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: llm-auto-approve-proposal-v0"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-resolution-v0"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0"
                    "intent-layer.unified-entry-pipeline.review-automation-policy-v0"
                    "intent-layer.unified-entry-pipeline.review-auto-answer-policy-v0"
                    "intent-layer.machine-contract.task-contract-v1"]
        :wave "21 task 06 (commit e140773)"
        :note "wave 14-04 unified-entry pipeline v1 留 4 v0 non-goal: auto_approve_directive / auto_approve_plan / auto_answer_review_question / autonomous_workstation_dispatch. wave 16-02 + 17-01 落 review-gate listener subscriber v0 + plan-node action 自动 route. wave 18-07 落 review_automation_policy v0 (manual|suggest|auto_safe) + 5 deterministic safety rule, 但仍要求 caller 显式提供 review_decision. wave 20-08 落 auto_answer_policy (off|deterministic_safe|dry_run) 把 deterministic 自动 answer 引入 listener 路径 + 3 hard invariants (I1 never auto-reject / I2 destructive never auto-promote / I3 no LLM). **但完整 LLM 路径 (Sonnet 提议 review decision) 仍是 v0 non-goal**. wave21-06 加 auto_approve_mode (off|sonnet_suggest, default off preserves wave-18..20 byte-shape) 把 LLM 引入 review/approve 路径但保持严格安全护栏: When opted in, asks Sonnet to PROPOSE structured review decision and surfaces it under `llm_auto_approve_proposal*` on every dispatch branch (legacy / wave-15 explicit / wave-18 policy-only). Bundle shape: {mode, status (not_invoked|llm_unavailable|suggested|destructive_blocked|no_suggestion), proposal {decision, confidence, evidence, non_goal_check, destructive_check, requires_human, applied=false}, proposal_warnings[], unavailable_reason, action, request_caller, model}. **Wired into directive.action_approve / action_archive / both _with_resolution / both _with_policy_only AND plan.action_approve / action_mark / action_supersede + their _with_resolution + _with_policy_only sister functions (10 total branch sites)**. **5 strict invariants pinned by 22 new pure unit tests** (review_gate.rs:tests::auto_approve_*): **(I1)** proposal NEVER carries decision=rejected; rejected from model demoted to needs_changes with proposal_warnings[] entry; **(I2)** destructive actions (archive | supersede | remove, case-insensitive) short-circuit to destructive_blocked WITHOUT calling Sonnet — proposal=None, requires_human=true pinned; **(I3)** applied=false pinned on EVERY proposal; requires_human=true forced even on non-destructive in v0 (propose-only); **(I4)** Sonnet unavailable surfaces status=llm_unavailable + unavailable_reason; NO fallback to deterministic; **(I5)** destructive_check ALWAYS sourced from is_destructive_review_action(action), overwriting model output via enforce_proposal_invariants helper. Strict-enum parse: typo `auto_approve_mode=\"auto\"` fails fast with INVALID_PARAM (parse_proposer_mode_or_error) BEFORE any DB read. Caller-supplied review_decision ALWAYS wins (proposal NEVER overrides explicit human decisions or deterministic policy outcome). MCP descriptors updated on directive.rs + plan.rs with full invariant table + paragraph in tool-level Chinese description. **ORTHOGONAL to wave-18/07 review_automation_policy AND wave-20/08 auto_answer_policy — 三 review knob 共存 (review_automation_policy + auto_answer_policy + auto_approve_mode) 独立观测**. **status code-aligned-partial** — propose 已落但 applied=false / requires_human=true 永钉死; 完整 LLM auto-approve apply (proposal 升级到 真落 review row state transition) 仍 v0 non-goal future. workflow.rs (wave21-07 scope) deliberately untouched")

      ;; ── 区域 75 · sonnet distill chain auto-apply v1 (wave 21 task 07) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.sonnet-distill-chain-auto-apply-v1"
        :title "sonnet distill chain auto-apply v1 — auto_sonnet 双 opt-in apply-gate + 7 重 gate (双 opt-in + auto_chain_trigger=auto_safe + ALL 6 wave-20 rule + caller distill_mode != sonnet) + 8 status taxonomy + 7 invariants I1-I7"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: sonnet-distill-chain-auto-apply-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-chain-v1"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-trigger-v1"
                    "intent-layer.plan-dag-runtime-v2.finalize-distill-v0"
                    "memory.directive-layer.plan-evidence-sidecar"]
        :wave "21 task 07 (commit 4d494db)"
        :note "wave 18-05 落 cross-plan distill chain v0 (3 modes record_only|dry_run|sonnet, sonnet 仅显式); wave 19-09 加 cross-plan distill auto-chain v1 (auto_chain opt-in + deterministic id 'chain:auto:wf-<sha256(...)>'); wave 20-06 加 cross-plan distill auto-trigger v1 (auto_chain_trigger default 'never' / 'deterministic_only' + 6 deterministic trigger rule). **但 sonnet 仍要求 caller 显式 distill_mode=sonnet** — 即 sonnet 真正落仍是 caller 决策. wave21-07 加 auto_sonnet apply-gate v1 把 sonnet auto-apply 引入但保持严格保守: auto_sonnet=true + auto_sonnet_approved=true 双 opt-in (default off → wave-20 byte-shape preserved). When opted in alongside wave-20 auto_chain_trigger=auto_safe AND ALL 6 deterministic safety rules pass AND caller attests auto_sonnet_approved=true AND distill_mode is not already sonnet, daemon auto-promotes inner distill from dry_run to sonnet via direct call to action_distill_sonnet and replaces outer payload (carrying forward wave-19 auto_chain + wave-20 auto_trigger receipts). **7 重 gate**: 双 opt-in (auto_sonnet=true + auto_sonnet_approved=true) + auto_chain_trigger=auto_safe + ALL 6 wave-20 deterministic safety rule (parent_plan_succeeded + parent_distill_on_success_true + evidence_sha256_present + project_root_resolvable + workflow_anchor_present + chain_id_not_already_supplied) + caller distill_mode != sonnet. **8-status taxonomy**: not_requested | disabled | skipped_no_trigger | skipped_rules_failed | skipped_caller_approval_missing | skipped_already_sonnet | skipped_inner_error | applied_sonnet. **4-status model_call_status taxonomy**: not_invoked | invoked | failed | invalid_output. Block shape: {requested, status, applied, review_required, model_call_status, safety_rule_results, caller_approval, caller_distill_mode?, model_call_error?, sidecar?, chain_id?} + top-level shortcut auto_sonnet_status. **7 conservative invariants enforced**: **(I1)** default-off byte-shape; **(I2)** dual opt-in required (single typo cannot escalate); **(I3)** ALL 6 wave-20 rules must pass (gate REUSES trigger outcomes — never relaxes); **(I4)** caller-already-sonnet refusal (no double-call); **(I5)** Sonnet failure PRESERVES inner payload (handler error → status=applied_sonnet + model_call_status=failed; structured error envelope from sonnet → model_call_status=invalid_output); **(I6)** review_required=true PINNED on every successful auto-apply (receipt-only, no DB transition); **(I7)** wave-19/20 blocks UNCHANGED (auto_sonnet purely additive). Strict shape validation: workflow.rs::validate_auto_sonnet_args (front-line at action entry) + plan.rs::validate_distill_chain_args defense-in-depth — typo `auto_sonnet=\"true\"` (string) / `auto_sonnet_approved=1` (number) fails fast INVALID_PARAM with shape-label diagnostic. plan.rs::apply_distill_chain forwards auto_sonnet args into workflow.distill sub-call so plan-side callers reach the same gate. MCP descriptor on workflow.rs gains auto_sonnet + auto_sonnet_approved properties. 16 new workflow.rs + 4 new plan.rs unit tests. **status code-aligned-partial** — auto-apply 已落但仍 dual opt-in required (单 typo 不能 escalate); 完全自动 sonnet 接 chain (无需 caller dual opt-in, plan-runner 全局推断) 仍 future")

      ;; ── 区域 76 · machine-contract autonomous loop smoke v3 (wave 21 task 08) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.machine-contract-autonomous-loop-smoke-v3"
        :title "machine-contract autonomous loop smoke v3 — 15 deterministic e2e tests across 4 handler files + cross-wave invariants pinned + Markdown non-load-bearing 二度钉死 + machine dispatch SSOT 钉死 + no LLM/spawn/shell"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: machine-contract-autonomous-loop-smoke-v3"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.machine-loop-smoke-v2"
                    "intent-layer.unified-entry-pipeline.execution-report-verifier-integration-v1"
                    "intent-layer.workstation.autonomous-workstation-llm-proposal-v0"
                    "intent-layer.unified-entry-pipeline.llm-auto-approve-proposal-v0"
                    "intent-layer.plan-dag-runtime-v2.sonnet-distill-chain-auto-apply-v1"
                    "intent-layer.machine-contract.dispatch-machine-mode-v0"
                    "intent-layer.machine-contract.task-contract-v1"]
        :wave "21 task 08 (commit 8ba8723)"
        :note "wave 16-08 落 deterministic 4 hand-off no-LLM smoke; wave 17-08 加 paused-resume e2e smoke; wave 18-09 加 autonomous loop smoke v1; wave 20-05 加 machine-loop smoke v2 (6 smoke + 8 machine-contract artifact_refs lift + Markdown non-load-bearing 钉死). wave21-08 加 v3 deterministic end-to-end smoke covering wave21-03..07 machine-contract autonomous loop across 4 handler files in :write-scope. **15 new tests grouped by handler**: unified_entry.rs adds 3 (smoke_wave21_machine_contract_autonomous_loop_envelope_pins_every_invariant — single envelope round-trip pinning row-id pointers + machine-contract splice (task_contract_path == task_contract_source_path SSOT) + wave21-04 workstation_proposals applied=false / auto_spawn=false / I3-04 + wave21-06 LLM auto-approve decision != rejected / requires_human=true / applied=false / I1-06 + I3-06 + wave21-07 default-off byte-shape (no auto_sonnet block when not requested) / I1-07 + wave-19/20 distill_chain triggered=true status=recorded chain_mode=record_only / I3-07 + v0 non-goals loud; smoke_wave21_llm_auto_approve_destructive_action_pins_requires_human — destructive action archive pins requires_human=true regardless of model output / I2-06 + destructive_check sourced from is_destructive_review_action / I5-06; smoke_wave21_machine_contract_loop_proof_markdown_is_non_load_bearing — task_brief_preview MUST stay on inner payload, NOT in artifact_refs (machine-mode brief is non-load-bearing); brief preview still surfaces ## Source contract preamble on inner payload). plan.rs adds 3 (smoke_wave21_distill_chain_block_pins_skip_status_when_trigger_short_circuits — wave21-07 I3 the gate REUSES wave-20 trigger outcomes; skip_plan_not_succeeded MUST NOT fabricate evidence_path / chain_index_in_plan / distill_result; smoke_wave21_distill_chain_block_surfaces_inner_distill_warning — recorded_with_distill_warning surfaces both inner result + warning; wave21-07 I7 additive contract; smoke_wave21_machine_dispatch_response_pins_source_path_invariant — build_workstation_dispatch_response in machine mode surfaces task_contract_source_path verbatim + brief preview carries ## Source contract preamble + wave21-04 dispatch_contract_mode marker is load-bearing). workstation_dispatch.rs adds 3 (smoke_wave21_workstation_proposals_pin_applied_false_invariant_across_safety_spectrum — fixture spans Safe (target=mission_task_delegate) + InvalidStrategy (vibes-based) + Safe objective; applied=false on every proposal + bundle auto_spawn=false; classify_proposal_safety(target, mission_unknown) MUST tag UnsupportedTarget; wave21-04 I3 + I4 + I5; smoke_wave21_workstation_proposal_unavailable_pins_no_silent_fallback — Sonnet unavailable surfaces status=llm_unavailable + reason naming `no fallback` / `claude -p` / `prompt mode`; auto_spawn=false pinned even on llm_unavailable; smoke_wave21_machine_contract_overlay_drives_brief_renderer — parse_task_contract → overlay_contract → build_task_brief_with_source: contract :goal beats stale caller objective + :scope wins + :write-scope replaces stale owned_files + :must-not-touch surfaces verbatim + :commit :policy reaches hints + :dispatch-strategy beats caller fresh-code-alignment + brief carries ## Source contract preamble + agent-team hint exactly once). agent_execution.rs adds 6 (smoke_wave21_machine_contract_autonomous_loop_verifier_accepts_aligned_triple — wave21-03 happy path: aligned (contract, report, hash) triple passes verifier + summary echoes resolved paths + :checked surfaces all 5 wave21-03 rules; smoke_wave21_malformed_report_task_id_yields_structured_failure → TASK_REPORT_TASK_ID_MISMATCH; smoke_wave21_malformed_task_contract_yields_structured_failure → TASK_CONTRACT_MALFORMED; smoke_wave21_missing_report_yields_structured_failure → TASK_REPORT_REQUIRED; smoke_wave21_mismatched_commit_hash_yields_structured_failure → TASK_REPORT_COMMIT_HASH_MISMATCH; smoke_wave21_report_and_contract_projectors_extract_required_fields — read_report_summary + read_task_contract_id cross-check anchor: contract head id == report :task_id). cargo test unified_entry::tests 67/67 (+3); agent_execution::tests 108/108 (+6); daemon 1454/1454 (+15 from 1439); build clean. **Cross-wave invariants pinned**: I1-06 (LLM auto-approve NEVER rejected) / I2-06 (destructive_blocked) / I3-06 (applied=false + requires_human=true) / I5-06 (destructive_check sourced from is_destructive_review_action) / I3-04 (workstation proposals applied=false + bundle auto_spawn=false) / I1-07 (auto_sonnet default-off byte-shape) / I3-07 (chain reuses wave-20 trigger outcomes) / I7-07 (chain block additive) / wave-19 task-contract / wave-20 dispatch-contract / wave21-03 verifier 5-rule cross-check. **Markdown is non-load-bearing — task_brief_preview NEVER projected into artifact_refs (二度钉死, 紧接 wave 20-05 一度钉死)**. machine dispatch SSOT task_contract_path == task_contract_source_path 钉死. **no LLM, no spawn, no shell**")

      ;; ── 区域 77 · review-automation-policy-v0 status note extension (wave 21 task 06 加 第三 review knob) ──
      ;; 注: section-id 已存在 (区域 wave-18-backfill / wave-20-backfill), 本 entry 不新增 anchor; 仅在 wave-21-backfill 块内
      ;;     声明 note 扩展 (LLM auto-approve propose 与 wave-18/07 + wave-20/08 共存独立观测)
      (status-upgrade
        :section-id "intent-layer.unified-entry-pipeline.review-automation-policy-v0"
        :prev-status code-aligned
        :new-status code-aligned
        :note-extension "wave 18-07 落 review_automation_policy v0 (manual|suggest|auto_safe) + 5 deterministic safety rule (caller-driven, deterministic 不调 LLM); wave 20-08 加 auto_answer_policy (off|deterministic_safe|dry_run) 把 deterministic 自动 answer 引入 listener 路径 + 3 hard invariants. **wave 21-06 (commit e140773) 加 第三 review knob auto_approve_mode (off|sonnet_suggest)** 把 LLM Sonnet propose 引入 review/approve 路径但 5 invariants 永久护栏 (I1 never auto-reject / I2 destructive blocked WITHOUT LLM call / I3 applied=false + requires_human=true 永钉死 / I4 sonnet unavailable no fallback / I5 destructive_check ALWAYS from is_destructive_review_action helper); 22 unit tests; 10 dispatch branch sites. **3 review knob 共存独立观测** (review_automation_policy + auto_answer_policy + auto_approve_mode) — 各管自己语义不互相 overlap; persisted directive/plan/review state v0 永不被自动 mutate. LLM 自动 approve apply (proposal 升级到 真落 review row state transition) 仍 v0 non-goal future. 见 anchor intent-layer.unified-entry-pipeline.llm-auto-approve-proposal-v0")

      ;; ── 区域 78 · machine-contract task-contract-v1 status note extension (wave 21 task 02/03 加 真正 6 段闭环) ──
      ;; 注: section-id 已存在 (区域 36 / wave-19-backfill / wave-20-backfill), 本 entry 不新增 anchor; 仅在 wave-21-backfill 块内
      ;;     声明 note 扩展 (run-verifier 三合一 + execution-side daemon-internal verified gate, 真正 6 段闭环)
      (status-upgrade
        :section-id "intent-layer.machine-contract.task-contract-v1"
        :prev-status code-aligned
        :new-status code-aligned
        :note-extension "wave 19 task 02-08 落 task-contract v1 schema + 各 checker; wave 20 task 01-04 加 staged guard + machine-driven dispatch (Lisp 真成 dispatch SSOT). **wave 21-02 (commit 1335fa7) 加 task-run verifier v1**: scripts/verify-task-run.mjs 三合一 (task contract + report + memory + commit) post-run proof + 14 forbidden git verb proof + dogfood self-verify. **wave 21-03 (commit 308426e) 加 execution report-verifier integration v1**: mission_execution(complete) 加 4 新字段 + verified=true 触发 daemon-internal sexp cross-check (无新 dep) + 4 structured error codes + daemon NEVER spawns Node + daemon_never_invokes_mutating_git unit-pinned. **brief → preflight → staged guard → commit → verifier (post-commit) → execution complete verify (daemon-internal verified gate) 真正 6 段闭环**. 见 anchor intent-layer.machine-contract.task-run-verifier-v1 + intent-layer.unified-entry-pipeline.execution-report-verifier-integration-v1"))

    ;; ──────────────────────────────────────────────────
    ;; wave 22 task 08 · lisp-backfill-wave22-status v1.3
    ;; 范围: 回填 wave 22 task 00-07 真实代码状态 — 7 anchor entry + 1 status upgrade
    ;;
    ;; 状态升级摘要 (本批次直接修改的现有 entry, 见各 entry note 末尾):
    ;;   - intent-layer.machine-contract.task-contract-v1                       note 扩 (wave 22-01 加 hooks default-on doctor v2 + wave 22-02 加 daemon-internal auto-run-verifier 8 cross-checks; brief→preflight→staged guard→commit→verifier→execution complete (daemon-internal auto-verifier) 真正 6 段闭环升级到 daemon 自跑 verifier)
    ;; ──────────────────────────────────────────────────
    (wave-22-backfill v1.3
      :date "2026-04-27"
      :decided-by "wave 22 / task 08 lisp backfill session"
      :scope "回填 wave 22 task 00-07 真实代码状态; 新增 7 anchor entry + 1 status upgrade (task-contract-v1 note 扩 加 daemon-internal auto-run-verifier + hooks default-on doctor v2); 不发明 wave22 没实现的架构"
      :non-goal "本任务不改 Rust / SQL / JS / Cargo / 任务文档; 不真正压缩主 Lisp; 不动 event-bus.lisp / intent-mcp-defs.lisp; 不启动 frontend Lisp (continue postpone); wave22-01 hooks default-on doctor v2 仍是 read-only doctor (--check 默认), --install 仍唯一 mutation entry, git config core.hooksPath .githooks 默认未启用 — 照实记录, 不假装 default-on real install; wave22-02 daemon-internal auto-verifier 仅在 caller 提供 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时触发, legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' 不硬拒 — 照实记录, 不当作完全 caller 无 hint 的 autonomous 验证; wave22-03 review LLM approve apply gate v1 是 4 opt-in + 6 道 gate + caller_approved=true 显式 — 照实记录, 不当作完全 LLM 自主 apply; wave22-04 persisted plan inference apply v2 是 4 opt-in + plan_supersede rollback handle, wave21-05 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死 (v2 用 SEPARATE persisted_apply block surface) — 照实记录, 不当作合并 v1 字段; wave22-05 autonomous workstation true spawn v1 是 4 opt-in + 12-rule gate matrix + mission_task_delegate substrate (绝不 claude -p) — 照实记录, 不当作 unconditional autonomy; wave22-06 distill chain policy auto-sonnet v2 是 policy 选择 (single explicit enum) 即 attestation, dual opt-in 移除但 legacy auto_sonnet=true + auto_sonnet_approved=true 双 opt-in 路径仍 back-compat coexists — 照实记录, 不当作真无任何 attestation; wave22-07 smoke v4 是 deterministic + no LLM/spawn/mutating-git — 照实记录, 不当作真 spawn/真 LLM smoke"
      :commits
        [(commit-1 :hash "49555c4" :title "feat(tasks): surface hooksPath as default preflight"
                   :primary-targets ["scripts/install-missiond-hooks.mjs"
                                     "scripts/check-missiond-hooks.mjs"
                                     "scripts/render-claudecode-task.mjs"
                                     ".missiond/tasks/schema/task-contract-v1.lisp"]
                   :tests "install-missiond-hooks.mjs default mode (无 flag) 升级到 = --check 只读 doctor (read-only); --install 仍唯一 mutation entry; doctor JSON payload 加 :severity (ok|preflight-drift), :reason (4 codes: aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing), :advice, :install_command (可 copy-paste 跑); renderer renderHooksDoctorPreflight() 块在每 :commit :required brief commit section 上方 (check-missiond-hooks --json + install-missiond-hooks --install 双指令); dry-fixture 8→11 covers 4 doctor states + install-refuses-when-hook-file-missing + adviceFor() install-command surface assertions; **opt-in repo-local only — git config 真 mutation 仍 caller 显式 --install** (per task brief). check-missiond-hooks.mjs 仍 read-only alias (rejects --install/--dry-fixture). schema 更新 hooks-installer-contract section. Default behaviour 是 doctor 状态报告 — git config core.hooksPath .githooks 默认仍未启用; enable per clone via 显式 install")
         (commit-2 :hash "02ac627" :title "feat(execution): auto-verify task run completion"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
                   :tests "daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process: (1) task_contract_loadable; (2) task_report_loadable; (3) task_report_schema=missiond.report-contract.v1; (4) task_id_matches_contract; (5) commit_hash_matches_report (with short<->long sha prefix overlap); (6) shared_memory_loadable; (7) shared_memory_schema=missiond.shared-memory.v1; (8) shared_memory_completion_for_task ((completion :task <id>) entry exists). 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 cross-check (read-only file inspection — 绝不 spawn Node/shell/mutating git); verification_source='daemon-auto-verifier' 主路径; legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' (verifier_status=unknown, verifier_diagnostics lists missing paths) 不硬拒 (back-compat); 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK; wave21-03 enforce_verified_completion helper 仍保留 (legacy verified=true 路径仍 honored, 不再 routed); 8 new tests; cargo test agent_execution::tests pass; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威; daemon_never_invokes_mutating_git unit-pinned (Command::new(<git>) 仍 ONE hit 在 wave18-08 status read)")
         (commit-3 :hash "4b55cb4" :title "feat(review): gate applying LLM approval proposals"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/directive.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "新 apply_llm_auto_approve=true + proposal_hash (32-hex deterministic SHA-256) + caller_approved=true 4 opt-in (legacy quiet `action=approve` 路径触发 DB transition). compute_proposal_hash 32-hex SHA-256 over action+artifact+version+decision+confidence+destructive prefix (deterministic, no clock/randomness). **6 道严格 gate**: G1 apply flag (apply_llm_auto_approve=true) / G2 proposal_hash deterministic match (compute_proposal_hash 32-hex SHA-256) / G3 caller_approved=true / G4 deterministic non-destructive (is_destructive_review_action=false) / G5 decision=approved / G6 confidence=high. 3 新错误码 APPLY_GATE_MISSING_PROPOSAL_HASH / APPLY_GATE_PROPOSAL_HASH_MISMATCH / APPLY_GATE_INVALID_PARAM (hash mismatch fail-fast BEFORE DB mutation); response surface: `llm_auto_approve_proposal_hash` (always stamped when bundle carries proposal) + `llm_approve_apply_gate` block (apply_status, applied_decision, proposal_hash_status, computed_proposal_hash, supplied_proposal_hash, caller_approved, safety_rule_results[]). Explicit review_decision 路径 gate 仍 informational only (no DB mutation); destructive actions (archive/supersede) ALWAYS skip with SkippedDestructiveAction. **wave-21/06 5 invariants 全 preserved 5 dedicated tests** (I1 NeedsChanges skipped non-approved / I2 archive/supersede/remove destructive skip / I3 proposal applied=false BEFORE+AFTER gate / I4 unavailable no fallback / I5 model-lied destructive_check overridden by deterministic verdict). cargo test review_gate::tests pass; build clean. ORTHOGONAL to wave-18/07 + wave-20/08 — 三 review knob 共存 (review_automation_policy + auto_answer_policy + auto_approve_mode) 独立观测; **caller_approved=true 显式必填 — 不当作完全 LLM 自主 apply**")
         (commit-4 :hash "fee6567" :title "feat(plan): persist inferred PLAN fields with audit"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "persist_inference=true + caller_approved=true + proposal_hash + apply_inferred_fields=true 4 opt-in apply gate (在 wave-21/05 v1 之上加 persisted layer). compute_inference_proposal_hash 32-hex SHA-256 over (plan_id, original_sexp_hash, sorted applied_fields). execute_persisted_apply DB path: plan_list_by_task → plan_insert(next-version) with original sexp 加 (plan-inference-applied :inference-version v2 :proposal-hash ... :persisted-at ...) annotation → plan_supersede(old_id) → append_plan_evidence_entry typed source/kind=plan_inference_persisted_apply with rollback_plan_id=predecessor (rollback handle: 通过 predecessor 重 supersede 即可回滚). 2 新错误码 PERSIST_APPLY_MISSING_PROPOSAL_HASH / PERSIST_APPLY_PROPOSAL_HASH_MISMATCH (hash mismatch fail-fast BEFORE DB mutation). attach_persisted_apply_block stable wire surface (mirrors apply gate block). **wave-21/05 6 invariants 全 preserved 7 dedicated tests** (apply_gate_v1_byte_shape_when_off / conflicts_never_persist / suggestions_never_persist / llm_unapproved_never_persists / strict_bool_shape / persist_inference_applied_field_intact / was_applied_only_for_applied). **I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死** (v2 用 SEPARATE `persisted_apply` block surface 状态, 不动 v1 字段 — 保 wave-21/05 byte-shape). cargo test plan::tests pass; daemon build clean")
         (commit-5 :hash "162a303" :title "feat(workstation): gate autonomous task dispatch"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
                   :tests "auto_spawn=true + workstation_caller_approved=true + preflight_status_acceptable=true + workstation_proposal_hash 4 opt-in (mode=sonnet_suggest 是前置必填). **12-rule gate matrix**: G1 auto_spawn opt-in / G2 bundle Suggested / G3 hash matches (compute_workstation_proposal_hash 32-hex SHA-256 over v1 sentinel + bundle status + each proposal field|value|confidence|safety_status semicolon-joined, evidence text excluded) / G4 ALL proposals safety_status=safe / G5 ALL proposals confidence=high / G6 caller_approved=true / G7 preflight_acceptable=true / G8 task_contract_path supplied / G9 contract loads ok / G10 :write-scope non-empty / G11 :write-scope non-overlap with :must-not-touch / G12 proposed target=mission_task_delegate. 走 `mission_task_delegate` substrate via wave-15 run_workstation_dispatch_with_contract; **绝不 `claude -p`** (substrate's existing SafeDescriptorReason::UnsupportedTarget 拒非 mission_task_delegate up-front + G12 也拒). 3 新错误码 AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH (hash mismatch / missing fail-fast BEFORE substrate dispatch). 15 status taxonomy (not_requested|spawned|skipped_unavailable|skipped_no_proposals|skipped_unsafe_proposal|skipped_confidence_too_low|skipped_caller_not_approved|skipped_missing_task_contract_path|skipped_malformed_task_contract|skipped_empty_write_scope|skipped_forbidden_scope_overlap|skipped_preflight_unacceptable|skipped_unsupported_target|skipped_substrate_refused|skipped_substrate_inner_error). **wave-21/04 4 invariants 全 preserved 4 dedicated tests** (default off / Sonnet unavailable no fallback / DAG mode rejects / propose-only fields preserved on wave-21/04 surface — workstation_proposals.auto_spawn=false + 每 proposal applied=false 在 wave-21/04 surface 仍 hard-pinned). cargo test workstation_dispatch::tests pass; daemon build clean")
         (commit-6 :hash "2423d4b" :title "feat(workflow): policy-gate automatic Sonnet distill"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
                   :tests "auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} 单一 policy 选择即 attestation. **dual opt-in 移除** — policy 选择 (单一显式 enum) 即 explicit operator attestation, 不需 auto_sonnet_approved 第二 flag. legacy auto_sonnet=true + auto_sonnet_approved=true 双 opt-in 路径仍 back-compat coexists (additive blocks 同 call 可同时 surface). safe_after_rules 触发要 ALL 6 wave-20 deterministic rule pass + trigger=auto_safe + distill_mode != sonnet (即重用 wave-21/07 7-gate 中的 6 项, 仅 attestation 来源换). dry_run 完整 evaluate 仅 surface 不 spawn Sonnet. 8 AUTO_SONNET_POLICY_STATUS_* wire-string constants (not_requested|off|safe_after_rules_applied|safe_after_rules_dry_run|skipped_no_trigger|skipped_rules_failed|skipped_already_sonnet|skipped_inner_error|invalid_param). strict closed-enum parse rejects 10 typo / camelCase / case-mismatch / shape-mismatch inputs at action entry (single typo cannot escalate). **wave-21/07 7 invariants 全 preserved 7 dedicated tests** (default off byte shape / strict shape no typo escalation / rules must pass no relax / already sonnet refuses double call / Sonnet failure preserves inner / review_required PINNED 7 outcomes / wave-19/20/21 blocks UNCHANGED 4-block coexistence). cargo test workflow::tests + plan::tests pass; daemon build clean")
         (commit-7 :hash "6b2125c" :title "test(intent): cover autonomous apply gates"
                   :primary-targets ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                                     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]
                   :tests "9 new deterministic smoke tests across 5 in-scope handler files covering wave22-02/03/04/05/06: unified_entry +1 envelope smoke pinning all 22 cross-wave invariants + Markdown task_brief_preview NEVER 进 artifact_refs 三度钉死 (across 10 forbidden artifact_refs keys); agent_execution +2 verifier failure paths SHARED_MEMORY_NO_COMPLETION_FOR_TASK + TASK_REPORT_COMMIT_HASH_MISMATCH; plan +2 persisted apply gate fixture hash accept + missing hash reject + wave-21/05 6 invariants pinned + wave-22/04 v2 persisted_apply block separate from v1 apply_gate field; workstation_dispatch +2 auto-spawn gate fixture hash accept + missing hash reject + wave-21/04 4 invariants pinned + wave-22/05 12-rule gate matrix; review_gate +2 review apply gate fixture hash accept + missing hash reject + wave-21/06 5 invariants pinned + wave-22/03 6 道 gate. **22 cross-wave invariants pinned** (wave21-04 4 + wave21-05 6 + wave21-06 5 + wave21-07 7 = 22). no real LLM (synthesised LlmAutoApproveProposalBundle / WorkstationProposalBundle / PlanFieldInference / AppliedField fixtures, NO Sonnet gateway initialized). no real spawn (workstation evaluator is PURE function ending at WorkstationAutoSpawnStatus::Spawned without calling substrate). no mutating git (verifier helpers do read-only file inspection on tempfile-backed paths only). cargo test unified_entry::tests / agent_execution::tests / plan::tests / workstation_dispatch::tests / review_gate::tests pass; daemon build clean")]

      ;; ── 区域 79 · hooks default-on doctor v2 (wave 22 task 01) ──
      (section-entry
        :section-id "intent-layer.machine-contract.hooks-default-on-doctor-v2"
        :title "hooks default-on doctor v2 — install-missiond-hooks.mjs default = --check 只读 doctor + 4 reason codes + renderer renderHooksDoctorPreflight() 块 + 11 dry-fixtures + opt-in repo-local only (real install 仍 caller 显式)"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: hooks-default-on-doctor-v2"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["scripts/install-missiond-hooks.mjs"
           "scripts/check-missiond-hooks.mjs"
           "scripts/render-claudecode-task.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.hooks-path-installer-v1"
                    "intent-layer.machine-contract.task-scope-index-guard-v1"
                    "intent-layer.machine-contract.renderer-scoped-commit-guard-v2"
                    "intent-layer.machine-contract.task-contract-v1"]
        :wave "22 task 01 (commit 49555c4)"
        :note "wave 21-01 落 install-missiond-hooks.mjs (--check/--install/--json/--dry-fixture/--strict) + check-missiond-hooks.mjs (read-only doctor alias) + .githooks/pre-commit MISSIOND_TASK_CONTRACT env-gated; opt-in repo-local only (--install 跑 git config --local core.hooksPath .githooks 一次). 但 default behaviour (no flag) 仍未明确, caller 必须显式 --check 才看 status. wave22-01 把 default mode (无 flag) 升级到 = --check 只读 doctor (read-only doctor): doctor JSON payload 加 :severity (ok|preflight-drift), :reason (4 codes: aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing), :advice 文本说明 + :install_command (可 copy-paste 跑); --install 仍唯一 mutation entry (default off-by-flag, opt-in by --install). renderer renderHooksDoctorPreflight() 块在每 :commit :required brief commit section 上方 (check-missiond-hooks --json + install-missiond-hooks --install 双指令); brief 渲染时直接打印整条命令让 worker copy-paste 可跑. dry-fixture 8 → 11 covers 4 doctor states (aligned / hooks-path-unset / hooks-path-wrong / hook-file-missing) + install-refuses-when-hook-file-missing + adviceFor() install-command surface assertions. **opt-in repo-local only — git config 真 mutation 仍 caller 显式 --install** (per task brief: 'default-on doctor only, real install 仍 caller 显式'). check-missiond-hooks.mjs 仍 read-only alias (rejects --install/--dry-fixture). schema 更新 hooks-installer-contract section 加 doctor-default-on 备注. **status code-aligned-partial** — doctor default-on 已落, 但 git config core.hooksPath .githooks 默认仍未启用 (real install 仍要求 caller 显式 --install); enforce-by-default real install (包含 git config core.hooksPath .githooks 默认安装) 仍 future")

      ;; ── 区域 80 · execution auto-run-verifier v2 (wave 22 task 02) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.execution-auto-run-verifier-v2"
        :title "execution auto-run-verifier v2 — daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process + verification_source taxonomy + 3 新错误码 + daemon NEVER spawns Node + back-compat downgrade for legacy verified=true"
        :source-file ".missiond/v2/intent-execution-governance.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: execution-auto-run-verifier-v2"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.execution-report-verifier-integration-v1"
                    "intent-layer.machine-contract.task-run-verifier-v1"
                    "intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.scoped-commit-worktree-preflight-task-contract-v1"]
        :wave "22 task 02 (commit 02ac627)"
        :note "wave 21-02 落 scripts/verify-task-run.mjs 三合一 post-run proof (out-of-process 权威 CLI); wave 21-03 落 mission_execution(complete) 加 4 新字段 + verified=true 触发 daemon-internal sexp cross-check (但仍要求 caller 提供 verifier_status, daemon 不主动跑 verifier). wave22-02 加 daemon-internal `auto_run_task_run_verifier` 把 wave21-02 verifier 8 cross-checks 完全 lift 到 daemon in-process: (1) task_contract_loadable; (2) task_report_loadable; (3) task_report_schema=missiond.report-contract.v1; (4) task_id_matches_contract (report :task_id == contract head id); (5) commit_hash_matches_report (with short<->long sha prefix overlap); (6) shared_memory_loadable; (7) shared_memory_schema=missiond.shared-memory.v1; (8) shared_memory_completion_for_task ((completion :task <id>) entry exists). 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 cross-check (read-only file inspection — 绝不 spawn Node/shell/mutating git, 用 in-tree sexp parser 做 cross-check); verification_source='daemon-auto-verifier' 主路径 (verifier_status / verified_scope_summary / verifier_diagnostics 全 daemon 计算). legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' (verifier_status=unknown, verifier_diagnostics lists missing paths) 不硬拒 (back-compat for wave21-03 callers). 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK (re-uses wave19-08 / wave21-03 TASK_CONTRACT_* and TASK_REPORT_* vocabulary 不重发明). wave21-03 enforce_verified_completion helper 仍保留 (legacy verified=true 路径仍 honored, 不再 routed by action_complete; 留 direct test coverage). 8 new tests; cargo test agent_execution::tests pass; build clean. **daemon NEVER spawns Node** — wave21-02 verifier 仍 out-of-process 权威 CLI (caller-driven proof); daemon 只用 in-tree sexp parser 做 cross-check. **daemon_never_invokes_mutating_git unit-pinned** (Command::new(<git>) 仍 ONE hit 在 wave18-08 status read; 0 mutating git argv strings introduced). brief → preflight → staged guard → commit → out-of-process verifier (wave21-02) → execution complete + daemon-internal auto-verifier (wave22-02) → 真正 6 段闭环 升级到 daemon 自跑 verifier. caller 必须仍提供 4 路径才触发 daemon-internal verifier — 完全无 hint autonomous spawn / report-contract checker auto-invoke 仍 future")

      ;; ── 区域 81 · review LLM approve apply gate v1 (wave 22 task 03) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.review-llm-approve-apply-gate-v1"
        :title "review LLM approve apply gate v1 — apply_llm_auto_approve + proposal_hash + caller_approved 4 opt-in + 6 道严格 gate + 3 新错误码 + hash mismatch fail-fast BEFORE DB mutation + wave-21/06 5 invariants 全 preserved"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: review-llm-approve-apply-gate-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
           "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/directive.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.llm-auto-approve-proposal-v0"
                    "intent-layer.unified-entry-pipeline.review-automation-policy-v0"
                    "intent-layer.unified-entry-pipeline.review-auto-answer-policy-v0"
                    "intent-layer.unified-entry-pipeline.review-gate-resolution-v0"]
        :wave "22 task 03 (commit 4b55cb4)"
        :note "wave 21-06 落 LLM auto-approve proposal v0 (auto_approve_mode=off|sonnet_suggest opt-in for directive/plan review; **propose only / applied=false / requires_human=true 永钉死**, 5 invariants pinned). 但 propose 升级到 真落 review row state transition 仍是 wave21-06 v0 non-goal future. wave22-03 加 apply gate v1 把 LLM auto-approve 升级到 caller-controlled apply: apply_llm_auto_approve=true + proposal_hash (32-hex deterministic SHA-256) + caller_approved=true 4 opt-in (legacy quiet `action=approve` 路径触发 DB transition). compute_proposal_hash 32-hex SHA-256 over action+artifact+version+decision+confidence+destructive prefix (deterministic, no clock/randomness). **6 道严格 gate**: G1 apply flag (apply_llm_auto_approve=true) / G2 proposal_hash deterministic match (compute_proposal_hash) / G3 caller_approved=true / G4 deterministic non-destructive (is_destructive_review_action=false) / G5 decision=approved (proposal carries decision='approved' deterministically — wave21-06 I1 NEVER auto-reject 仍硬钉) / G6 confidence=high. 3 新错误码 APPLY_GATE_MISSING_PROPOSAL_HASH / APPLY_GATE_PROPOSAL_HASH_MISMATCH / APPLY_GATE_INVALID_PARAM (hash mismatch fail-fast BEFORE DB mutation — fail-fast 准则). Response surface: `llm_auto_approve_proposal_hash` (always stamped when bundle carries proposal) + `llm_approve_apply_gate` block (apply_status, applied_decision, proposal_hash_status, computed_proposal_hash, supplied_proposal_hash, caller_approved, safety_rule_results[]) — stable wire shape. Explicit review_decision 路径 gate 仍 informational only (no DB mutation, caller-supplied review_decision ALWAYS wins). Destructive actions (archive/supersede) ALWAYS skip with SkippedDestructiveAction (wave21-06 I2 destructive blocked WITHOUT calling Sonnet 仍硬钉 — wave22-03 apply gate 不 relax destructive ban). **wave-21/06 5 invariants 全 preserved 5 dedicated tests**: I1 NEVER auto-reject (proposal applied 路径仍 demote rejected → needs_changes); I2 archive/supersede/remove destructive skip; I3 proposal applied=false BEFORE+AFTER gate (apply gate 路径不动 propose-only surface); I4 sonnet unavailable no fallback; I5 model-lied destructive_check overridden by deterministic verdict (enforce_proposal_invariants 仍 sourced from is_destructive_review_action helper). cargo test review_gate::tests pass; build clean. ORTHOGONAL to wave-18/07 + wave-20/08 + wave-21/06 — 三 review knob (review_automation_policy + auto_answer_policy + auto_approve_mode) + apply gate 共存独立观测. **status code-aligned-partial** — apply gate 已落但 caller_approved=true 显式必填; 完全 LLM 自主无任何 caller opt-in apply 仍 future")

      ;; ── 区域 82 · persisted plan inference apply v2 (wave 22 task 04) ──
      (section-entry
        :section-id "intent-layer.plan-dag.persisted-plan-inference-apply-v2"
        :title "persisted plan inference apply v2 — persist_inference + caller_approved + proposal_hash 4 opt-in + plan_insert(version=max+1) + plan_supersede(old) 真改 plan.sexp_text + rollback handle (predecessor 重 supersede) + wave-21/05 6 invariants 全 preserved + I6 v1 apply_gate.persist_inference_applied=false 仍硬钉死 (v2 用 SEPARATE persisted_apply block)"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: persisted-plan-inference-apply-v2"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.plan-dag.plan-inference-apply-gate-v1"
                    "intent-layer.plan-dag.autonomous-plan-field-inference-v0"
                    "intent-layer.plan-dag.llm-augmented-plan-field-inference-v0"
                    "intent-layer.actor.plan-dag-scheduler"]
        :wave "22 task 04 (commit fee6567)"
        :note "wave 21-05 落 plan inference apply gate v1 (apply_inferred_fields=true + llm_caller_approved + persist_inference 4 opt-in apply gate; **persisted plan.sexp_text 永不 mutate** — apply_gate.persist_inference_applied=false 永钉死, persist_inference flag 仅 echoed for future-wave wiring). wave22-04 加 v2 把 persisted layer 落: persist_inference=true + caller_approved=true + proposal_hash + apply_inferred_fields=true 4 opt-in apply gate (在 wave-21/05 v1 之上加 persisted layer). compute_inference_proposal_hash 32-hex SHA-256 over (plan_id, original_sexp_hash, sorted applied_fields). execute_persisted_apply DB path: plan_list_by_task → plan_insert(next-version) with original sexp 加 (plan-inference-applied :inference-version v2 :proposal-hash ... :persisted-at ...) annotation → plan_supersede(old_id) → append_plan_evidence_entry typed source/kind=plan_inference_persisted_apply with rollback_plan_id=predecessor. **rollback handle**: 通过 predecessor 重 supersede 即可回滚 (audit entry 永不丢, plan_id 链可 traverse). 2 新错误码 PERSIST_APPLY_MISSING_PROPOSAL_HASH / PERSIST_APPLY_PROPOSAL_HASH_MISMATCH (hash mismatch fail-fast BEFORE DB mutation). attach_persisted_apply_block stable wire surface (mirrors apply gate block). **wave-21/05 6 invariants 全 preserved 7 dedicated tests** (apply_gate_v1_byte_shape_when_off / conflicts_never_persist / suggestions_never_persist / llm_unapproved_never_persists / strict_bool_shape / persist_inference_applied_field_intact / was_applied_only_for_applied). **I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死** (v2 用 SEPARATE `persisted_apply` block surface 状态, 不动 v1 字段 — 保 wave-21/05 byte-shape; v2 surface 是 NEW block, 不污染 v1 channel). cargo test plan::tests pass; daemon build clean. **rollback policy**: predecessor (plan_id N-1) 仍 alive 在 plan_list_by_task; 后续若回滚, caller 用 plan_supersede(plan_id N) → predecessor 路径; sidecar evidence 一致 (rollback_plan_id 字段在 evidence entry 已写, 且未来 wave 可加 cli/daemon helper 驱动 rollback). 完全无 caller_approved opt-in 的 LLM 自主 persisted apply 仍 future")

      ;; ── 区域 83 · autonomous workstation true spawn v1 (wave 22 task 05) ──
      (section-entry
        :section-id "intent-layer.workstation.autonomous-workstation-true-spawn-v1"
        :title "autonomous workstation true spawn v1 — auto_spawn + workstation_caller_approved + preflight_acceptable + proposal_hash 4 opt-in + 12-rule gate matrix + mission_task_delegate substrate (绝不 claude -p) + 15 status taxonomy + wave-21/04 4 invariants 全 preserved"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: autonomous-workstation-true-spawn-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.workstation.autonomous-workstation-llm-proposal-v0"
                    "intent-layer.unified-entry-pipeline.workstation-dispatch-v0"
                    "intent-layer.machine-contract.dispatch-machine-mode-v0"
                    "intent-layer.machine-contract.task-contract-v1"]
        :wave "22 task 05 (commit 162a303)"
        :note "wave 21-04 落 autonomous workstation LLM proposal v0 (workstation_inference_mode=off|sonnet_suggest opt-in; **propose only never auto-spawn — applied=false / auto_spawn=false 永钉死**, 4 invariants pinned). 但 propose 升级到 真 spawn 仍是 wave21-04 v0 non-goal future. wave22-05 加 v1 把 workstation propose 升级到 conditional 真 spawn: auto_spawn=true + workstation_caller_approved=true + preflight_status_acceptable=true + workstation_proposal_hash 4 opt-in (mode=sonnet_suggest 是前置必填). **12-rule gate matrix**: G1 auto_spawn opt-in / G2 bundle Suggested / G3 hash matches (compute_workstation_proposal_hash 32-hex SHA-256 over v1 sentinel + bundle status + each proposal field|value|confidence|safety_status semicolon-joined, evidence text excluded — deterministic, evidence 排除以避免无关变化破坏 hash) / G4 ALL proposals safety_status=safe / G5 ALL proposals confidence=high / G6 caller_approved=true / G7 preflight_acceptable=true / G8 task_contract_path supplied / G9 contract loads ok (parse 无 error) / G10 :write-scope non-empty / G11 :write-scope non-overlap with :must-not-touch / G12 proposed target=mission_task_delegate. 走 `mission_task_delegate` substrate via wave-15 run_workstation_dispatch_with_contract; **绝不 `claude -p`** (substrate's existing SafeDescriptorReason::UnsupportedTarget 拒非 mission_task_delegate up-front + G12 也拒). 3 新错误码 AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH (hash mismatch / missing fail-fast BEFORE substrate dispatch). **15 status taxonomy** (not_requested|spawned|skipped_unavailable|skipped_no_proposals|skipped_unsafe_proposal|skipped_confidence_too_low|skipped_caller_not_approved|skipped_missing_task_contract_path|skipped_malformed_task_contract|skipped_empty_write_scope|skipped_forbidden_scope_overlap|skipped_preflight_unacceptable|skipped_unsupported_target|skipped_substrate_refused|skipped_substrate_inner_error). **wave-21/04 4 invariants 全 preserved 4 dedicated tests** (default off ⇒ wave-21/04 byte-shape preserved exactly; Sonnet unavailable ⇒ no fallback (gate inherits the wave-21 invariant — SkippedUnavailable status text pins no-fallback verbatim); propose-only proposal.applied=false / bundle.auto_spawn=false intact on wave-21/04 surface; DAG mode rejects sonnet_suggest at preflight single-node-only). cargo test workstation_dispatch::tests pass; daemon build clean. **status code-aligned-partial** — true spawn 已落但 4 opt-in 显式必填; 完全 LLM 自主无任何 caller opt-in 的 workstation spawn 仍 future. workstation_proposals.auto_spawn=false 在 wave-21/04 surface 仍 hard-pinned (即默认 off 时 byte-shape 不变, opt-in 时 NEW workstation_auto_spawn_gate block surface 状态)")

      ;; ── 区域 84 · distill chain policy auto-sonnet v2 (wave 22 task 06) ──
      (section-entry
        :section-id "intent-layer.plan-dag-runtime-v2.distill-chain-policy-auto-sonnet-v2"
        :title "distill chain policy auto-sonnet v2 — auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} 单一 policy 选择即 attestation + dual opt-in 移除 + legacy back-compat coexists + 8 wire-string status constants + 10 strict closed-enum parse rejection + wave-21/07 7 invariants 全 preserved"
        :source-file ".missiond/v2/intent-plan-dag.lisp"
        :local-path "pillar intent-layer :: section action-instruction-actor :: actor plan-dag-scheduler :: runtime v2 :: distill-chain-policy-auto-sonnet-v2"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
        :cross-ref ["intent-layer.plan-dag-runtime-v2.sonnet-distill-chain-auto-apply-v1"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-trigger-v1"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-auto-chain-v1"
                    "intent-layer.plan-dag-runtime-v2.cross-plan-distill-chain-v0"]
        :wave "22 task 06 (commit 2423d4b)"
        :note "wave 21-07 落 sonnet distill chain auto-apply v1 (auto_sonnet=true + auto_sonnet_approved=true 双 opt-in apply-gate; 7 invariants pinned). 但 dual opt-in 是 'caller 显式 opt-in 之上再加 attestation flag', 实战中 caller 多次 surface 觉得冗余 — 单 opt-in 还要再 approve 一次. wave22-06 加 v2 把 dual opt-in 改成 policy-driven: auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} 单一 policy 选择即 attestation. **dual opt-in 移除** — policy 选择 (单一显式 enum) 即 explicit operator attestation, 不需 auto_sonnet_approved 第二 flag. legacy auto_sonnet=true + auto_sonnet_approved=true 双 opt-in 路径仍 back-compat coexists (additive blocks 同 call 可同时 surface — 不破 wave-21/07 byte-shape). safe_after_rules 触发要 ALL 6 wave-20 deterministic safety rule pass + trigger=auto_safe + distill_mode != sonnet (即重用 wave-21/07 7-gate 中的 6 项, 仅 attestation 来源换 — 'rules pass + policy=safe_after_rules' 等价于 'rules pass + double opt-in', deterministic 部分不放松). dry_run 完整 evaluate 仅 surface 不 spawn Sonnet (deterministic preview, 让 caller 验证 6 rule 全通过再切 safe_after_rules). 8 AUTO_SONNET_POLICY_STATUS_* wire-string constants (not_requested|off|safe_after_rules_applied|safe_after_rules_dry_run|skipped_no_trigger|skipped_rules_failed|skipped_already_sonnet|skipped_inner_error|invalid_param). **strict closed-enum parse rejects 10 typo / camelCase / case-mismatch / shape-mismatch inputs at action entry** (single typo cannot escalate — 'safeAfterRules' / 'SAFE_AFTER_RULES' / 'safe-after-rule' 全 INVALID_PARAM). **wave-21/07 7 invariants 全 preserved 7 dedicated tests** (default off byte shape / strict shape no typo escalation / rules must pass no relax / already sonnet refuses double call / Sonnet failure preserves inner / review_required PINNED 7 outcomes / wave-19/20/21 blocks UNCHANGED 4-block coexistence — wave-21/07 dual opt-in path / wave-22/06 policy path / wave-19 auto_chain / wave-20 auto_chain_trigger 4 块同 call 可同时 surface). cargo test workflow::tests + plan::tests pass; daemon build clean. **policy 选择即 attestation** — 单 enum value 替代 dual flag 但 deterministic safety rule 不放松 (6 rule 仍硬钉). 完全自动 sonnet 接 chain (无任何 attestation, plan-runner 全局推断) 仍 future")

      ;; ── 区域 85 · autonomous loop apply smoke v4 (wave 22 task 07) ──
      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.autonomous-loop-apply-smoke-v4"
        :title "autonomous loop apply smoke v4 — 9 new deterministic smoke tests across 5 in-scope handler files + 22 cross-wave invariants pinned + Markdown task_brief_preview NEVER 进 artifact_refs 三度钉死 + no LLM/spawn/mutating-git"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: autonomous-loop-apply-smoke-v4"
        :status code-aligned
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]
        :cross-ref ["intent-layer.unified-entry-pipeline.machine-contract-autonomous-loop-smoke-v3"
                    "intent-layer.unified-entry-pipeline.machine-loop-smoke-v2"
                    "intent-layer.unified-entry-pipeline.execution-auto-run-verifier-v2"
                    "intent-layer.unified-entry-pipeline.review-llm-approve-apply-gate-v1"
                    "intent-layer.plan-dag.persisted-plan-inference-apply-v2"
                    "intent-layer.workstation.autonomous-workstation-true-spawn-v1"
                    "intent-layer.plan-dag-runtime-v2.distill-chain-policy-auto-sonnet-v2"]
        :wave "22 task 07 (commit 6b2125c)"
        :note "wave 16-08 落 deterministic 4 hand-off no-LLM smoke; wave 17-08 加 paused-resume e2e smoke; wave 18-09 加 autonomous loop smoke v1; wave 20-05 加 machine-loop smoke v2 (Markdown non-load-bearing 一度钉死); wave 21-08 加 machine-contract autonomous loop smoke v3 (15 deterministic e2e tests + 二度钉死 Markdown). wave22-07 加 v4 deterministic end-to-end smoke covering wave22-02/03/04/05/06 apply-gate paradigm: **9 new deterministic smoke tests across 5 in-scope handler files**: unified_entry +1 envelope smoke pinning all 22 cross-wave invariants + Markdown task_brief_preview NEVER 进 artifact_refs 三度钉死 (across 10 forbidden artifact_refs keys 全部 audit); agent_execution +2 verifier failure paths SHARED_MEMORY_NO_COMPLETION_FOR_TASK + TASK_REPORT_COMMIT_HASH_MISMATCH (wave22-02 daemon-internal verifier 失败路径 fixture); plan +2 persisted apply gate fixture hash accept + missing hash reject + wave-21/05 6 invariants pinned + wave-22/04 v2 persisted_apply block separate from v1 apply_gate field (I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死 跨 v2 path); workstation_dispatch +2 auto-spawn gate fixture hash accept + missing hash reject + wave-21/04 4 invariants pinned + wave-22/05 12-rule gate matrix; review_gate +2 review apply gate fixture hash accept + missing hash reject + wave-21/06 5 invariants pinned + wave-22/03 6 道 gate. **22 cross-wave invariants pinned** (wave21-04 4 + wave21-05 6 + wave21-06 5 + wave21-07 7 = 22). no real LLM (synthesised LlmAutoApproveProposalBundle / WorkstationProposalBundle / PlanFieldInference / AppliedField fixtures, NO Sonnet gateway initialized). no real spawn (workstation evaluator is PURE function ending at WorkstationAutoSpawnStatus::Spawned without calling substrate). no mutating git (verifier helpers do read-only file inspection on tempfile-backed paths only). cargo test unified_entry::tests / agent_execution::tests / plan::tests / workstation_dispatch::tests / review_gate::tests pass; daemon build clean. **Cross-wave apply-gate paradigm 钉死**: wave22-02 daemon-internal verifier / wave22-03 review apply gate / wave22-04 persisted apply v2 / wave22-05 workstation true spawn / wave22-06 distill policy 全 fixture-tested without real LLM/spawn/mutating-git")

      ;; ── 区域 86 · session-trace v1 (wave 23 tasks 01/04/05/06) ──
      (section-entry
        :section-id "intent-layer.machine-contract.session-trace-v1"
        :title "session-trace v1 — append-only factual telemetry schema/checker + daemon append + plan/workstation propagation + descriptive analyzer"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: session-trace-v1"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/session-trace-v1.lisp"
           "scripts/check-session-trace.mjs"
           "scripts/analyze-session-trace.mjs"
           "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.shared-memory-v1"
                    "worker.section.trace-derived-router-policy"]
        :wave "23 tasks 01/04/05/06 (commits 2f173ee / 9096085 / 0c81611 / e7ab99d)"
        :note "session-trace.lisp 是事实遥测, 不是报告解释也不是 conversation log. wave23-01 加 schema + checker; wave23-04 mission_execution(open/preflight/complete) 直接 Rust append trace-event, 不 spawn Node/shell; wave23-05 plan/workstation 透传 session_trace_path / :session-trace-path overlay; wave23-06 analyzer 只产 bottleneck descriptors, 不做模型选择。:session-trace-writable default false, 只有任务 contract 显式 opt-in 才允许 worker 写 trace ledger。")

      (section-entry
        :section-id "intent-layer.machine-contract.report-trace-explanation-fields-v1"
        :title "report-contract trace explanation fields — time_sinks / major_decisions / unexpected_work / blockers / trace_refs"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: report-contract-v1 :: trace explanation fields"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/report-contract-v1.lisp"
           "scripts/check-task-report.mjs"
           "scripts/render-claudecode-task.mjs"]
        :cross-ref ["intent-layer.machine-contract.session-trace-v1"
                    "intent-layer.machine-contract.task-contract-v1"]
        :wave "23 task 02 (commits 7c6ea9a + a005a45)"
        :note "report-contract 增加五个 explanation 字段用于人工解释耗时/决策/意外工作/阻塞/trace refs; facts 仍以 session-trace.lisp 为准, report 字段不成为 telemetry SSOT。renderer 会在 sibling session-trace.lisp 存在时渲染 Session Trace section, 并根据 :session-trace-writable 明确 worker 是否可写。")

      (section-entry
        :section-id "worker.section.trace-derived-router-policy"
        :title "trace-derived router policy — backend selection draft for ClaudeCode / missiond-llm-router / checker / patch / verifier workers"
        :source-file ".missiond/v2/intent-workstation-policy.lisp"
        :local-path "pillar worker :: section trace-derived-router-policy"
        :status architecture-designed
        :compression-safe? false
        :implements []
        :cross-ref ["intent-layer.machine-contract.session-trace-v1"
                    "intent-layer.machine-contract.trace-derived-router-policy"
                    "worker.section.claudecode-workstation-orchestration"
                    "intent-layer.workstation.autonomous-workstation-true-spawn-v1"]
        :wave "23 task 07 (Codex architecture draft)"
        :note "router policy 只完成蓝图: backend classes = claudecode / missiond-llm-router / deterministic-checker / patch-worker / verifier-worker; selection inputs = contract shape + trace history + scope/risk + acceptance/verifier needs。wave23 不替换 ClaudeCode, 不从单次 trace 推断模型, 不自动 spawn 新 router。")

      (section-entry
        :section-id "intent-layer.machine-contract.trace-derived-router-policy"
        :title "machine-contract trace-derived router policy — analyzer descriptors feed future policy, not runtime routing"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: trace-derived-router-policy"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/router-policy-v1.lisp"
           ".missiond/router/router-policy-v1.lisp"
           "scripts/check-router-policy.mjs"
           "scripts/build-session-trace-index.mjs"
           "scripts/recommend-task-backend.mjs"]
        :cross-ref ["intent-layer.machine-contract.session-trace-v1"
                    "worker.section.trace-derived-router-policy"
                    "intent-layer.machine-contract.router-policy-v1"
                    "intent-layer.machine-contract.router-recommendation-cli-v0"]
        :wave "23 task 07 (Codex architecture draft) + 24 tasks 01/02/03 (commits 988f7d8 / e61088b / d6d8e10+ef8fc5f)"
        :note "machine-contract 侧声明 trace 是 router policy 的事实输入: analyzer 输出 bottleneck tags / durations / retries / tool-counts。wave24 加 policy schema/checker/seed + trace corpus indexer + recommendation CLI, 把 architecture draft 推到 advisory dry-run code-aligned partial; runtime backend replacement 仍禁止。")

      ;; ── 区域 87 · router-policy dry-run chain (wave 24 tasks 01-06) ──
      (section-entry
        :section-id "intent-layer.machine-contract.router-policy-v1"
        :title "router-policy v1 schema/checker/seed — dry-run-only backend policy"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-policy-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/router-policy-v1.lisp"
           ".missiond/router/router-policy-v1.lisp"
           "scripts/check-router-policy.mjs"]
        :cross-ref ["intent-layer.machine-contract.trace-derived-router-policy"
                    "worker.section.trace-derived-router-policy"]
        :wave "24 task 01 (commit 988f7d8)"
        :note "schema missiond.router-policy.v1 + checker + seed policy 3 rule. Valid policy must set :dry-run-only true and :runtime-replacement false; runtime-replacement=true is rejected. Seed maps docs→claudecode, code-alignment+scripts/check-*.mjs→deterministic-checker, review/smoke→verifier-worker.")

      (section-entry
        :section-id "intent-layer.machine-contract.trace-corpus-index-v0"
        :title "trace corpus index v0 — deterministic aggregate over session-trace ledgers"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: trace-corpus-index-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/build-session-trace-index.mjs"
           ".missiond/tasks/schema/session-trace-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.session-trace-v1"
                    "intent-layer.machine-contract.router-policy-v1"]
        :wave "24 task 02 (commit e61088b)"
        :note "read-only indexer over .missiond/tasks/**/session-trace.lisp. Output top-level keys are bottleneck_tags / by_backend / by_task / by_wave / schema / source_files / thresholds / totals. It reuses wave23 analyzer thresholds and does not recommend backend.")

      (section-entry
        :section-id "intent-layer.machine-contract.router-recommendation-cli-v0"
        :title "router recommendation CLI v0 — deterministic advisory backend recommendation"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-recommendation-cli-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/recommend-task-backend.mjs"
           "scripts/build-session-trace-index.mjs"
           "scripts/check-router-policy.mjs"
           "scripts/lib/missiond_lisp.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-policy-v1"
                    "intent-layer.machine-contract.trace-corpus-index-v0"]
        :wave "24 task 03 (commits d6d8e10 + ef8fc5f)"
        :note "CLI emits schema missiond.router-recommendation.v0. Guarantees: dry_run_only=true, read-only deterministic, no LLM/spawn/git/HTTP; no match falls back to claudecode confidence=low reason insufficient_trace_history; policies with runtime_replacement=true or dry_run_only!=true are rejected.")

      (section-entry
        :section-id "tools.surface.plan-router-policy-dry-run-v0"
        :title "mission_plan execute router_policy_mode=dry_run surface"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_plan :: router-policy dry-run args"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.router-recommendation-cli-v0"
                    "intent-layer.plan-dag-runtime-v2"
                    "worker.section.trace-derived-router-policy"]
        :wave "24 task 04 (commit b8721ab)"
        :note "mission_plan execute adds optional router_policy_mode/router_policy_path. absent/off preserves legacy shape; dry_run returns router_recommendation with applied=false literal Value::Bool(false); apply/auto/unknown reject INVALID_PARAM before plan lookup. Daemon implementation is independent Rust parser/glob logic and does not spawn Node CLI.")

      (section-entry
        :section-id "intent-layer.machine-contract.renderer-router-context-v0"
        :title "renderer Router Policy (advisory) block + :router-policy-path task field"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: renderer-router-context-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/render-claudecode-task.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.router-policy-v1"]
        :wave "24 task 05 (commit 294a92a)"
        :note "task contract gains optional :router-policy-path. Renderer auto-detects .missiond/router/router-policy-v1.lisp and renders a Router Policy (advisory) section containing literal 'advisory' + 'dry-run only'; it never shells out to recommendation CLI and never instructs backend switching. Wave23 Session Trace section byte-identical.")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.router-dry-run-smoke-v0"
        :title "router dry-run full-chain smoke — Node/Rust/renderer cross-check"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: router-dry-run-smoke-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/check-router-policy.mjs"
           "scripts/build-session-trace-index.mjs"
           "scripts/recommend-task-backend.mjs"
           "scripts/render-claudecode-task.mjs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.router-policy-v1"
                    "tools.surface.plan-router-policy-dry-run-v0"
                    "intent-layer.machine-contract.renderer-router-context-v0"]
        :wave "24 task 06 (commit 6afe541)"
        :note "full-chain smoke pins the Node policy/recommendation path, independent Rust dry-run surface, renderer advisory block, applied=false literal, no backend replacement, no LLM/spawn/mutating git, and no loss of Wave23 Session Trace rendering.")

      (status-upgrade
        :section-id "worker.section.trace-derived-router-policy"
        :prev-status architecture-designed
        :new-status code-aligned-partial
        :note-extension "wave 24 tasks 01-06 turn the wave23 router draft into an advisory dry-run chain: router-policy v1 schema/checker/seed, trace corpus index, recommendation CLI, mission_plan dry-run surface, renderer advisory block, and full-chain smoke. Runtime backend replacement remains pending; :runtime-replacement false / applied=false are hard boundaries.")

      (status-upgrade
        :section-id "intent-layer.machine-contract.task-contract-v1"
        :prev-status code-aligned
        :new-status code-aligned
        :note-extension "wave 24 task 05 adds optional :router-policy-path to task-contract-v1 and renderer support for Router Policy (advisory). This is context for dry-run recommendation only; Markdown remains a view and backend choice is not controlled by rendered text.")

      ;; ── 区域 88 · router-policy measurement loop (wave 25 tasks 01-05) ──
      (section-entry
        :section-id "intent-layer.machine-contract.router-policy-corpus-evaluator-v0"
        :title "router-policy corpus evaluator v0 — measure recommendations over task contracts"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-policy-corpus-evaluator-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/evaluate-router-policy-corpus.mjs"
           "scripts/build-session-trace-index.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-policy-v1"
                    "intent-layer.machine-contract.trace-corpus-index-v0"
                    "worker.section.trace-derived-router-policy"]
        :wave "25 task 01 (commit 8dbe85f)"
        :note "CLI emits schema missiond.router-policy-evaluation.v0. Real corpus: 67 task contracts; backend counts claudecode=49 / deterministic-checker=14 / verifier-worker=4; confidence high=6 / medium=5 / low=56; fallback_count=43, rejected_count=0. It is read-only, never spawns Node child commands, never uses git/LLM/HTTP, and skips schema/report/shared-memory/session-trace/index artifacts.")

      (section-entry
        :section-id "intent-layer.machine-contract.router-recommendation-report-fields-v0"
        :title "report-contract router recommendation fields — dry-run evidence in completion reports"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-recommendation-report-fields-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/report-contract-v1.lisp"
           "scripts/check-task-report.mjs"]
        :cross-ref ["intent-layer.machine-contract.task-contract-v1"
                    "intent-layer.machine-contract.router-recommendation-cli-v0"
                    "intent-layer.machine-contract.router-policy-corpus-evaluator-v0"]
        :wave "25 task 02 (commit 7709031)"
        :note "report-contract v1 adds seven optional flat fields: recommended_backend, router_confidence, router_policy_path, router_dry_run_only, router_applied, router_reasons, router_trace_index_path. checker enforces backend/confidence enums, repo-relative paths, non-empty reason strings, and atom-only booleans; strings like \"false\" do not pass.")

      (section-entry
        :section-id "tools.surface.plan-router-policy-trace-index-confidence-v1"
        :title "mission_plan router_policy_trace_index_path — dry-run confidence from measured trace index"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_plan :: router-policy trace-index confidence"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["tools.surface.plan-router-policy-dry-run-v0"
                    "intent-layer.machine-contract.trace-corpus-index-v0"
                    "intent-layer.machine-contract.router-policy-corpus-evaluator-v0"]
        :wave "25 task 03 (commit bd2b5a3)"
        :note "mission_plan execute adds optional router_policy_trace_index_path, read only in router_policy_mode=dry_run. Status values: used / missing / unreadable / malformed; path absent omits trace_index fields; mode=off/default performs no I/O and keeps wave24 byte-shape. Rich trace threshold mirrors CLI: matched recommendation + max(by_task, by_backend) >=5 => high, 1..4 => medium, 0/no-match => low. Failures are non-fatal warnings.")

      (section-entry
        :section-id "intent-layer.machine-contract.renderer-router-recommendation-command-v1"
        :title "renderer router recommendation command — advisory CLI invocation in task brief"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: renderer-router-recommendation-command-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/render-claudecode-task.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.renderer-router-context-v0"
                    "intent-layer.machine-contract.router-recommendation-report-fields-v0"]
        :wave "25 task 04 (commit e1fdbe4)"
        :note "Renderer now prints a parameterized command: node scripts/recommend-task-backend.mjs --task <relSource> --policy <routerPolicyPath> --json. Existing check-router-policy command stays byte-identical; Report Contract section lists the seven router fields with MAY language. Renderer still emits text only and does not shell out.")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.router-policy-measurement-smoke-v1"
        :title "router-policy measurement smoke v1 — CLI/Rust/report invariants"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: router-policy-measurement-smoke-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/evaluate-router-policy-corpus.mjs"
           "scripts/recommend-task-backend.mjs"
           "scripts/check-task-report.mjs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.router-policy-corpus-evaluator-v0"
                    "tools.surface.plan-router-policy-trace-index-confidence-v1"
                    "intent-layer.machine-contract.router-recommendation-report-fields-v0"]
        :wave "25 task 05 (commit 0f5d857)"
        :note "Smoke pins the four-layer loop: recommend CLI confidence buckets, evaluator projection, Rust mission_plan parity for rich trace evidence, and report checker valid router block. It locks eight cross-wave invariants: runtime_replacement=false, dry_run_only=true, applied=false bool literal, advisory renderer, rejected true/false router report atoms, off/default byte-shape, CLI/Rust parity at (5,5), and no shell/LLM/git/network/runtime replacement.")

      (status-upgrade
        :section-id "worker.section.trace-derived-router-policy"
        :prev-status code-aligned-partial
        :new-status code-aligned-partial
        :note-extension "wave 25 tasks 01-05 add measurement over the dry-run router chain: corpus evaluator, report fields, mission_plan trace-index confidence scoring, renderer recommendation command, and CLI/Rust parity smoke. This raises observability and confidence only; runtime backend replacement remains pending.")

      (status-upgrade
        :section-id "intent-layer.machine-contract.trace-derived-router-policy"
        :prev-status code-aligned-partial
        :new-status code-aligned-partial
        :note-extension "wave 25 turns advisory router policy from static dry-run into measurable dry-run: real corpus evaluator (67 tasks) + trace-index confidence path + report echo fields. It still forbids runtime replacement; runtime_replacement=false / dry_run_only=true / applied=false remain hard boundaries.")

      ;; ── 区域 89 · router backend readiness loop (wave 26 tasks 01-06) ──
      (section-entry
        :section-id "intent-layer.machine-contract.router-backend-registry-v1"
        :title "router backend readiness registry v1 — backend runtime readiness and apply blockers"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-backend-registry-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/router-backend-registry-v1.lisp"
           ".missiond/router/router-backend-registry-v1.lisp"
           "scripts/check-router-backend-registry.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-policy-v1"
                    "worker.section.trace-derived-router-policy"]
        :wave "26 task 01 (commit 2ac6a5b)"
        :note "Adds schema + seed + checker for 5 backend ids aligned with router-policy v1: claudecode / missiond-llm-router / deterministic-checker / patch-worker / verifier-worker. Readiness statuses: current-default / advisory-only / runtime-ready / unavailable plus unknown sentinel for consumers. Seed marks claudecode current-default runtime_allowed=true, non-claudecode backends advisory-only/unavailable, and no backend as runtime-ready; checker has 19 fixtures across 13 categories.")

      (section-entry
        :section-id "intent-layer.machine-contract.router-recommendation-readiness-v1"
        :title "router recommendation readiness annotations v1 — Node CLI apply blockers"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-recommendation-readiness-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/recommend-task-backend.mjs"
           "scripts/evaluate-router-policy-corpus.mjs"
           "scripts/check-router-backend-registry.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-backend-registry-v1"
                    "intent-layer.machine-contract.router-policy-corpus-evaluator-v0"
                    "intent-layer.machine-contract.router-recommendation-cli-v0"]
        :wave "26 task 02 (commit ad8ec04)"
        :note "recommend/evaluate CLIs accept --backend-registry and add backend_readiness_status / backend_runtime_allowed / router_apply_eligible / router_apply_blockers / backend_registry_path. Node gate has 7 conditions (policy dry-run invariants split, status=computed, high confidence, backend present, runtime_allowed=true, readiness_status=runtime-ready, no blockers). Real corpus: 75 tasks; by_backend claudecode=54 / deterministic-checker=16 / verifier-worker=5; by_backend_readiness current-default=54 / advisory-only=21; apply_eligible_count=0.")

      (section-entry
        :section-id "tools.surface.plan-router-backend-readiness-v1"
        :title "mission_plan router_backend_registry_path — dry-run backend readiness surface"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_plan :: router backend readiness"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["tools.surface.plan-router-policy-dry-run-v0"
                    "tools.surface.plan-router-policy-trace-index-confidence-v1"
                    "intent-layer.machine-contract.router-backend-registry-v1"]
        :wave "26 task 03 (commit fcd937a)"
        :note "mission_plan execute adds optional router_backend_registry_path, read only under router_policy_mode=dry_run. Response fields: backend_registry_path / backend_registry_status / backend_readiness_status / backend_runtime_allowed / router_apply_eligible / router_apply_blockers. Rust gate has 6 conditions (status=computed, confidence=high, backend in registry, runtime_allowed=true, readiness_status=runtime-ready, blockers empty). Off/default with both registry and trace-index paths remains byte-identical and performs no file I/O; applied remains literal false.")

      (section-entry
        :section-id "intent-layer.machine-contract.router-readiness-report-fields-v0"
        :title "report-contract router readiness fields — apply blocker evidence"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-readiness-report-fields-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/report-contract-v1.lisp"
           "scripts/check-task-report.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-recommendation-report-fields-v0"
                    "intent-layer.machine-contract.router-recommendation-readiness-v1"]
        :wave "26 task 04 (commit 44ddf9d)"
        :note "report-contract v1 adds five optional flat fields: router_backend_readiness_status, router_backend_runtime_allowed, router_apply_eligible, router_apply_blockers, router_backend_registry_path. Checker fixtures 23→31 after smoke; rejects invalid readiness enum, string booleans, absolute registry path, and empty blocker strings. Legacy reports remain valid.")

      (section-entry
        :section-id "intent-layer.machine-contract.renderer-router-readiness-context-v1"
        :title "renderer router readiness context — backend-registry command and MUST NOT switch backend"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: renderer-router-readiness-context-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/render-claudecode-task.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.renderer-router-recommendation-command-v1"
                    "intent-layer.machine-contract.router-backend-registry-v1"
                    "intent-layer.machine-contract.router-readiness-report-fields-v0"]
        :wave "26 task 05 (commit 43df6230)"
        :note "task contract gains optional :router-backend-registry-path / :router_backend_registry_path. Renderer auto-detects .missiond/router/router-backend-registry-v1.lisp, emits check-router-backend-registry command, appends --backend-registry to recommend-task-backend when registry resolves, and always renders an explicit MUST NOT switch backend bullet. Renderer still emits text only and never shells out.")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.router-readiness-smoke-v1"
        :title "router readiness smoke v1 — 5-layer readiness/apply-blocker invariant suite"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: router-readiness-smoke-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/recommend-task-backend.mjs"
           "scripts/evaluate-router-policy-corpus.mjs"
           "scripts/check-task-report.mjs"
           "scripts/render-claudecode-task.mjs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.router-backend-registry-v1"
                    "intent-layer.machine-contract.router-recommendation-readiness-v1"
                    "tools.surface.plan-router-backend-readiness-v1"]
        :wave "26 task 06 (commit 4bfa710)"
        :note "5-layer smoke pins 9 invariants: router-policy dry-run invariants, applied=false ownership, apply_eligible only for runtime-ready (current-default blocked), real-corpus seed apply_eligible_count=0, CLI/Rust parity for seed current-default registry, off/default no-I/O byte shape, renderer advisory/dry-run/MUST NOT text, report checker strict literals, and zero shell/LLM/git/network in active readiness paths. Daemon tests 1659→1661; mcp 17 unchanged.")

      (status-upgrade
        :section-id "worker.section.trace-derived-router-policy"
        :prev-status code-aligned-partial
        :new-status code-aligned-partial
        :note-extension "wave 26 tasks 01-06 add backend readiness/apply-blocker metadata on top of the measurable dry-run loop. Seed registry keeps apply_eligible_count=0 across the real corpus and treats current-default as insufficient for apply eligibility. Runtime backend replacement remains pending.")

      (status-upgrade
        :section-id "intent-layer.machine-contract.trace-derived-router-policy"
        :prev-status code-aligned-partial
        :new-status code-aligned-partial
        :note-extension "wave 26 introduces router-backend-registry v1 and Node/Rust readiness annotations. router_apply_eligible is advisory metadata only; no runtime consumer applies it, and applied=false remains pinned.")

      ;; ── 区域 90 · router dispatch descriptor handoff loop (wave 27 tasks 01-06) ──
      (section-entry
        :section-id "intent-layer.machine-contract.router-dispatch-descriptor-v1"
        :title "router dispatch descriptor v1 — no-execution handoff schema/checker"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-dispatch-descriptor-v1"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/router-dispatch-descriptor-v1.lisp"
           "scripts/check-router-dispatch-descriptor.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-backend-registry-v1"
                    "intent-layer.machine-contract.router-recommendation-cli-v0"
                    "worker.section.trace-derived-router-policy"]
        :wave "27 task 01 (commit f451b04)"
        :note "schema missiond.router-dispatch-descriptor.v1 + checker. The descriptor requires recommendation/readiness provenance plus locked literal bools: dry_run_only=true, runtime_replacement=false, no_execution=true. router_apply_eligible=true is rejected unless readiness is runtime-ready, runtime_allowed=true, confidence=high, and blockers are empty.")

      (section-entry
        :section-id "intent-layer.machine-contract.router-dispatch-descriptor-cli-v0"
        :title "router dispatch descriptor CLI v0 — build ephemeral descriptor from task/policy/registry"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-dispatch-descriptor-cli-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/build-router-dispatch-descriptor.mjs"
           "scripts/recommend-task-backend.mjs"
           "scripts/check-router-backend-registry.mjs"
           "scripts/check-router-dispatch-descriptor.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-dispatch-descriptor-v1"
                    "intent-layer.machine-contract.router-recommendation-readiness-v1"]
        :wave "27 task 02 (commits 14fdf5a + 752fe40)"
        :note "CLI emits Lisp by default and JSON with --json. It imports existing recommendation/readiness helpers, hard-codes the three descriptor bool invariants, validates before emit, handles missing registry as blocked descriptor evidence, and does not shell out / call git / use LLM or network.")

      (section-entry
        :section-id "tools.surface.plan-router-dispatch-descriptor-v0"
        :title "mission_plan router_dispatch_descriptor — dry-run descriptor surface"
        :source-file ".missiond/v2/intent-tools.lisp"
        :local-path "pillar tools :: section mcp-surface-lifecycle :: implemented-surface mission_plan :: router dispatch descriptor"
        :status code-aligned-partial
        :compression-safe? false
        :implements
          ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
           "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
        :cross-ref ["tools.surface.plan-router-backend-readiness-v1"
                    "intent-layer.machine-contract.router-dispatch-descriptor-v1"]
        :wave "27 task 03 (commit 6e4f14d)"
        :note "mission_plan execute adds optional router_dispatch_descriptor bool. The arg is honored only in router_policy_mode=dry_run; absent, false, off/default, or malformed values keep legacy shape/no-I/O. When registry resolves, response carries router_dispatch_descriptor; with no registry path, descriptor_status=registry_missing is surfaced and descriptor body is omitted.")

      (section-entry
        :section-id "intent-layer.machine-contract.router-dispatch-descriptor-report-fields-v0"
        :title "report-contract router dispatch descriptor fields — no-execution proof"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: router-dispatch-descriptor-report-fields-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          [".missiond/tasks/schema/report-contract-v1.lisp"
           "scripts/check-task-report.mjs"]
        :cross-ref ["intent-layer.machine-contract.router-dispatch-descriptor-v1"
                    "intent-layer.machine-contract.router-readiness-report-fields-v0"]
        :wave "27 task 04 (commit afb5ffb)"
        :note "report-contract v1 adds six optional flat fields: router_dispatch_descriptor_path, router_dispatch_descriptor_status, router_dispatch_backend, router_dispatch_eligible, router_dispatch_no_execution, router_dispatch_blockers. Checker rejects strings for booleans and accepts router_dispatch_no_execution only as literal atom true.")

      (section-entry
        :section-id "intent-layer.machine-contract.renderer-router-dispatch-descriptor-context-v0"
        :title "renderer router dispatch descriptor context — build/check commands, no execution"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section machine-contract-layer :: renderer-router-dispatch-descriptor-context-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/render-claudecode-task.mjs"
           ".missiond/tasks/schema/task-contract-v1.lisp"]
        :cross-ref ["intent-layer.machine-contract.router-dispatch-descriptor-cli-v0"
                    "intent-layer.machine-contract.router-dispatch-descriptor-report-fields-v0"
                    "intent-layer.machine-contract.renderer-router-readiness-context-v1"]
        :wave "27 task 05 (commit 17cb401)"
        :note "Renderer appends build-router-dispatch-descriptor and pipe-to-check-router-dispatch-descriptor commands only when both policy and registry resolve. It documents that descriptors are generated on demand and intentionally does not add a static :router-dispatch-descriptor-path task field. Brief text contains advisory / dry-run only / no execution / MUST NOT switch backend.")

      (section-entry
        :section-id "intent-layer.unified-entry-pipeline.router-dispatch-descriptor-smoke-v0"
        :title "router dispatch descriptor smoke v0 — 5-layer no-execution invariant suite"
        :source-file ".missiond/v2/intent-machine-contract.lisp"
        :local-path "pillar intent-layer :: section unified-entry-pipeline :: router-dispatch-descriptor-smoke-v0"
        :status code-aligned
        :compression-safe? false
        :implements
          ["scripts/check-router-dispatch-descriptor.mjs"
           "scripts/build-router-dispatch-descriptor.mjs"
           "scripts/check-task-report.mjs"
           "scripts/render-claudecode-task.mjs"
           "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
        :cross-ref ["intent-layer.machine-contract.router-dispatch-descriptor-v1"
                    "tools.surface.plan-router-dispatch-descriptor-v0"
                    "intent-layer.machine-contract.renderer-router-dispatch-descriptor-context-v0"]
        :wave "27 task 06 (commit 7f65f05)"
        :note "5-layer smoke pins checker fixtures, CLI descriptor builds, Rust mission_plan descriptor literals, report checker descriptor block, and renderer literals. It keeps dispatch fields byte-identical with vs without descriptor flag and proves no shell/LLM/git/network/runtime backend replacement enters the path.")

      (status-upgrade
        :section-id "worker.section.trace-derived-router-policy"
        :prev-status code-aligned-partial
        :new-status code-aligned-partial
        :note-extension "wave 27 tasks 01-06 add a router dispatch descriptor layer after readiness: recommendation + readiness are projected into an ephemeral no-execution S-expression handoff. This improves machine readability for future runner/router work but still does not replace ClaudeCode or mission_task_delegate at runtime.")

      (status-upgrade
        :section-id "intent-layer.machine-contract.trace-derived-router-policy"
        :prev-status code-aligned-partial
        :new-status code-aligned-partial
        :note-extension "wave 27 adds router-dispatch-descriptor v1, build CLI, mission_plan descriptor surface, report fields, renderer context, and smoke. Descriptor facts are advisory/no-execution evidence only; runtime_replacement=false, dry_run_only=true, no_execution=true, and applied=false remain pinned.")

      ;; ── 区域 91 · machine-contract task-contract-v1 status note extension (wave 22 task 01/02 加 hooks default-on doctor + daemon-internal auto-verifier) ──
      ;; 注: section-id 已存在 (区域 36 / wave-19-backfill / wave-20-backfill / wave-21-backfill), 本 entry 不新增 anchor; 仅在 wave-22-backfill 块内
      ;;     声明 note 扩展 (hooks default-on doctor v2 + daemon-internal auto-run-verifier 8 cross-checks)
      (status-upgrade
        :section-id "intent-layer.machine-contract.task-contract-v1"
        :prev-status code-aligned
        :new-status code-aligned
        :note-extension "wave 19 task 02-08 落 task-contract v1 schema + 各 checker; wave 20 task 01-04 加 staged guard + machine-driven dispatch (Lisp 真成 dispatch SSOT); wave 21-02/03 加 task-run verifier v1 三合一 + execution-side daemon-internal verified gate (但仍要求 caller 提供 verifier_status). **wave 22-01 (commit 49555c4) 加 hooks default-on doctor v2**: install-missiond-hooks.mjs default mode (无 flag) = --check 只读 doctor + 4 reason codes (aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing) + renderer renderHooksDoctorPreflight() 块在 commit section 上方提示 caller 跑 --install. **wave 22-02 (commit 02ac627) 加 execution auto-run-verifier v2**: daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process (task_contract_loadable / task_report_loadable / task_report_schema / task_id_matches_contract / commit_hash_matches_report / shared_memory_loadable / shared_memory_schema / shared_memory_completion_for_task); 当 4 路径全提供 daemon 自跑 (verification_source='daemon-auto-verifier'); legacy verified=true 降级 'legacy-caller-claim'. **brief → preflight → staged guard → commit → out-of-process verifier (wave21-02) → execution complete + daemon-internal auto-verifier (wave22-02) → 真正 6 段闭环 升级到 daemon 自跑 verifier**. 见 anchor intent-layer.machine-contract.hooks-default-on-doctor-v2 + intent-layer.unified-entry-pipeline.execution-auto-run-verifier-v2"))

    ;; ── 已声明但本次未细化的 section, 后续再补 ──
    (deferred-coverage
      :reason "v0.2 baseline 覆盖 7 pillar 顶层; v0.3 (wave 12 task 06) 扩了 7 高变动语义区; v0.4 (wave 13 task 04) 回填 evidence-collector / PLAN DAG runtime v2 / unified-entry pipeline v0 共 +11 entry; v0.5 (wave 14 task 07) 回填 file-first writer integration / PlanNodeStateChanged + live EventRef / review-gate auto-create v1 / unified-entry v1 / source-index checker R015+R016 共 +13 entry; v0.6 (wave 15 task 06) 回填 extensible domain count test / L2 shard split executed / shard-aware checker R017+R018 + auto-discovery / review-gate resolution v0 / workstation dispatch v0 共 +5 entry; v0.7 (wave 16 task 09) 回填 workflow review-resolution / review-gate listener / workstation auto-inference v1 / plan paused 7th + review-gate question-event trigger / plan retry policy v0 / scoped commit enforce v0 / evidence subscriber 三档 / unified-entry e2e smoke 共 +6 entry; v0.8 (wave 17 task 09) 回填 paused-resume hook v0 / claim-lease v0 + Claimed 第 8 态 / acceptance evaluator v0 三模式 / rollback policy v0 三模式 + 5 safety gates / finalize+distill trigger v0 / event-log query path v0 三档 resolver / workstation scoped-commit brief default v1 / unified-entry paused-resume e2e smoke 共 +7 entry; v0.9 (wave 18 task 10) 回填 event-log query API v1 / ExecutionEvent dispatch metadata v1 / cross-node acceptance fan-in v0 / cascade rollback v0 / cross-plan distill chain v0 / autonomous PLAN field inference v0 deterministic / review automation policy v0 / scoped-commit worktree preflight v0 / unified-entry autonomous loop smoke v1 共 +9 entry; v1.0 (wave 19 task 12) 回填 task-verifier v1 / report-contract v1 / shared-memory v1 / renderer dispatch brief v1 / plan task-contract emitter v0 / workstation task-contract consumer v0 / execution task-contract completion verification v0 / cross-plan distill auto-chain v1 / forward compensate ref v0 / Opened dispatch metadata NO-OP 注解 共 +9 entry + 1 status-upgrade; v1.1 (wave 20 task 10) 回填 task-scope-index-guard v1 / renderer scoped-commit guard v2 / execution preflight contract scope v1 / machine-driven dispatch v0 / unified-entry machine loop smoke v2 / cross-plan distill auto-trigger v1 / LLM-augmented plan inference v0 sonnet_suggest / review auto-answer policy v0 / ExecutionEvent legacy metadata sweep v0 共 +9 entry + 3 status-upgrade; v1.2 (wave 21 task 09) 回填 hooks-path installer v1 / task-run verifier v1 / execution report-verifier integration v1 / autonomous workstation LLM proposal v0 / plan inference apply gate v1 / LLM auto-approve proposal v0 / sonnet distill chain auto-apply v1 / machine-contract autonomous loop smoke v3 共 +8 entry + 2 status-upgrade; v1.3 (wave 22 task 08) 回填 explicit-gate-promotion + auto-verifier + smoke v4 共 +7 entry + 1 status-upgrade; v1.4 (wave 23 Codex) 回填 session-trace / trace analyzer / router draft 共 +4 entry; v1.5 (wave 24 Codex) 回填 router-policy dry-run chain 共 +6 entry + 2 status-upgrade; v1.6 (wave 25 Codex) 回填 router-policy measurement loop 共 +5 entry + 2 status-upgrade; v1.7 (wave 26 Codex) 回填 router backend readiness loop 共 +6 entry + 2 status-upgrade; **v1.8 (wave 27 Codex) 回填 router dispatch descriptor handoff loop 共 +6 entry + 2 status-upgrade**. 仍有以下未细化项, 等后续 wave 再补"
      :latest-backfill "v1.8 (wave 27 Codex) 回填 router dispatch descriptor handoff loop 共 +6 entry + 2 status-upgrade; reason 文本保留历史串, 本字段记录最新收口"
      :scope-deferred
        ["pillar memory 内 cross-cutting / pillar-interfaces 的 5 surface 矩阵"
         "pillar worker section workers 内 19 worker 的 per-worker entry"
         "pillar tools 83 tool 的 per-tool section-id (现仅按 section 分组, capability_usage / mission_plan record_evidence / mission_directive write_file/review_gate / mission_plan write_file/review_gate / mission_workflow write_file/review_gate / mission_directive review-resolution / mission_plan review-resolution / mission_workflow review-resolution / mission_plan workstation-dispatch / mission_execution enforce_scoped_commit / mission_plan resume_review_* / mission_plan claim_lease_* / mission_plan acceptance_* / mission_plan rollback_* / mission_plan finalize_plan / distill_on_success / distill_mode 已有专项)"
         "pillar intent-layer 各 actor 内部 step (directive-compiler / plan-compiler / workflow-distiller 内部)"
         "pillar event-bus 4 表内部字段索引 (frozen 文件, 不强行细化; PlanNodeStateChanged variant 已 code-aligned, 详见 event-bus.section.execution-event.plan-node-state-changed; domain count extensibility test 已 code-aligned; QuestionEvent::Resolved subscriber 接入由 wave 16 task 02 落 + wave 17 task 01 plan-node action 自动 route, 详见 intent-layer.unified-entry-pipeline.review-gate-resolution-listener-v0)"
         "pillar flow 其他 ~17 个非主线 flow (F9-project-init / F-incident-reaction / F-execution-log-governance 等)"
         "scoped-commit-handoff daemon enforce — wave 16 task 06 已落 v0 (enforce_scoped_commit opt-in default false; 4 错误码; gate 在 allocate_id 之前; 不跑 git); wave 17 task 07 加 brief-layer enforce 默认开 (workstation_dispatch.rs 任务 brief section 7); wave 18 task 08 加 read-only preflight tool (mission_execution action='preflight_commit' 只 git status --porcelain=v1, 0 mutating subcommand); daemon legacy mission_execution(action=complete) callers 仍 enforce_scoped_commit default false 字节兼容; 后续 enforce-by-default 切默认 + git 仓库挂钩 (pre-commit hook scope 校验) / scope-overlap 跨多 plan 全局视图 仍 pending"
         "PLAN DAG scheduler 完整 11-stage 的 per-stage entry — runtime v2 已覆盖 dispatch/lifecycle/failure-policy/condition-gate, wave 16 task 04 加 paused 7th lifecycle + review-gate question-event trigger, wave 16 task 05 加 per-node retry policy v0; wave 17 大量 close: s5 claim-lease v0 (task 02, opt-in enforce_claims default false) ✓ / s7 acceptance evaluator v0 (task 03, 三模式) ✓ / s8 paused-resume hook (task 01) ✓ / s9 rollback policy v0 (task 04, 三模式 + 5 safety gates) ✓ / s10 mark-plan-final (task 05) ✓ / s11 trigger-record-execution-distill (task 05) ✓; wave 18 闭环更多: cross-node acceptance fan-in v0 (task 03 :acceptance-depends-on / :acceptance-requires + 4 validator rejection) ✓ / cascade rollback v0 (task 04 :compensates / :rollback-cascade plan|dispatch-safe + Kahn 排序 + 5 safety triggers) ✓ / cross-plan distill chain v0 (task 05 distill_chain_id + 3 modes record_only|dry_run|sonnet, sidecar 无 migration) ✓ / autonomous PLAN field inference v0 (task 06 6 字段 + 3 confidence + apply_safe 仅 high, 无 LLM) ✓; 仍 partial 的 11-stage 子项: 完全自动 enforce (claim-lease default true / 切 default enforce) 仍 pending; sonnet 自动接 chain (无需 caller chain_id) 仍 future; LLM-augmented PLAN inference 仍 future; compensate-node :ref forward 引用 (与 :compensates reverse 方向相对) 仍 future"
         "autonomous workstation dispatch — wave 15 task 05 v0 是 PLAN-hint opt-in, wave 16 task 03 加 5 inference rules (target=mission_task_delegate scoped node); wave 17 task 07 加 brief-layer scoped-commit enforce 默认; wave 18 task 06 加 deterministic PLAN field inference (workstation_dispatch 是其中一个推断字段, 仍 caller-driven apply_safe high-confidence only); 完全 autonomous spawn (plan-runner 不需要任何 hint, 全局推断 + 扩展到 mission_execution path) 仍 surface 不实现"
         "autonomous PLAN.lisp 推理 — wave 16 task 03 仅 workstation dispatch 推断; wave 18 task 06 (commit 91f8fa1) 把推理扩到完整 PLAN.lisp 6 字段子集 (target / dispatch_strategy / target_project / owned_files / acceptance_mode / workstation_dispatch) deterministic heuristics + 3 confidence + apply_safe 仅 high 落; **完全无 LLM**; 仍 partial: failure-policy / retry-policy / rollback-policy / acceptance-depends-on 等 PLAN 字段仍 caller hint only; LLM-augmented inference (从 directive 历史语义推) 仍 future; conflict resolution 现仅 fail-fast"
         "unified-entry pipeline 升级到 actor 后的 4 自动化 non-goal 实现 (auto_approve_directive / auto_approve_plan / auto_answer_review_question / autonomous_workstation_dispatch) — wave 14 task 04 仍 surface; wave 15-18 task 部分缓解 (review-resolution 显式 by wave 15-04 / workstation dispatch opt-in + auto-inference by wave 15-05 + 16-03 / review listener plan-node action 自动 re-dispatch by wave 17-01 但仍是 caller 显式 resolution drives, 不替人答 / acceptance manual_required / rollback workstation 都仍 caller 显式 opt-in / wave 18 task 07 review_automation_policy auto_safe 5 deterministic safety rule + caller_opt_in 必填 + 绝不 auto_reject + destructive transitions 永不 auto-promote, 仍是 caller-driven deterministic 不是 LLM); 完整 LLM 自动 approve / 自动 answer review question 仍 surface"
         "ExecutionEvent dispatch_strategy/target_project/requested_cwd 完整字段扩展 — wave 14 task 02 已扩 PlanNodeStateChanged variant; wave 18 task 02 (commit 2fbaa88) 闭环 ExecutionEvent::Claimed + Completed 同字段 (3 variants 现在都含 dispatch metadata, 不要求 caller 重传 — handler 从 companion log meta 读); ExecutionEvent::Opened 等更早 variant 仍未扩同字段 (Opened 写在 wave 13 dispatch_strategy 协议之前) 仍 future"
         "event-log query path — wave 16 task 07 加 passive subscriber cache (cap 1024 FIFO); wave 17 task 06 加 ad-hoc resolver 三档 (live cache → log query → unavailable); wave 18 task 01 (commit 016487f) 升级到 typed query API v1: EventLogQuery typed builder + EventLogQueryable trait + blanket impl + EVENT_LOG_QUERY_LIMIT_CAP=512 硬上限 + EventRefProvenance 4 值 enum (live | passive_cache | event_log_query | unavailable, 把 wave-17 三档展开); 系统化按 plan_id / node_id / time-range / event-id query API 已通; dedicated store method (例如 by_topic 索引下推) 仍 future"
         "PLAN DAG node FSM 完整 enum 10 主态 (pending/ready/claimed/running/succeeded/failed/skipped/retrying/rolling-back/paused) — wave 13 已 6 主 + 3 skip 子分类 落 evidence; wave 16 task 04 加 paused 第 7 主态; wave 17 task 02 加 Claimed 第 8 主态; retrying / rolling-back 仍未独立 lifecycle 状态 (retry 通过每 attempt 写独立 evidence 表达; rollback 通过 failed→rollback_* evidence + node_results[].rollback 块 + wave 18-04 rollback_cascade 块表达, 不进 FSM)"
         "git pre-commit hook + worktree-level scope enforcement — wave 17 task 07 brief 强制 worker 显式 commit; wave 18 task 08 加 read-only preflight tool 帮 worker 看 worktree (mission_execution action='preflight_commit' 只 git status); 但 daemon 仍不跑 git mutating commands; 真正 git hook 执行 scope check (pre-commit / pre-push) 仍 future"
         "review-gate / workstation dispatch / acceptance / rollback UI panel — 当前仅 MCP response + sidecar; 前端 review/dispatch/acceptance/rollback 面板 pending (frontend Lisp 仍 postpone 直到 MissionD loop 稳)"
         "frontend Lisp 系列 — 全部 postpone 至 MissionD loop 稳定后启动 (本 wave 不开 frontend)"]))

  ;; ──────────────────────────────────────────────────
  ;; Part 3 · 当前判断与下一步路径
  ;; ──────────────────────────────────────────────────
  (judgement-now
    :date "2026-04-27"
    :decided-by "wave 11 lisp-source-index-precompression + wave 12 task 06 source-index-expansion + wave 13 task 04 execution-status-backfill + wave 14 task 07 lisp-backfill-and-l2-shard-plan + wave 15 task 06 lisp-backfill-wave15-status + wave 16 task 09 lisp-backfill-wave16-status + wave 17 task 09 lisp-backfill-wave17-status + wave 18 task 10 lisp-backfill-wave18-status + wave 19 task 12 lisp-backfill-wave19-status + wave 20 task 10 lisp-backfill-wave20-status + wave 21 task 09 lisp-backfill-wave21-status + wave 22 task 08 lisp-backfill-wave22-status sessions"
    :wave22-task-08-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 22 没实现的架构 — 只反映 committed implementation truth (7 wave-22 commits 真实状态)"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"
       "wave 22-01 hooks default-on doctor v2 仍是 read-only doctor (--check 默认), --install 仍唯一 mutation entry — 照实记录, 不假装 default-on real install (git config core.hooksPath .githooks 默认未启用)"
       "wave 22-02 daemon-internal auto-verifier 仅在 caller 提供 4 路径 (task_contract_path + task_report_path + shared_memory_path + commit_hash) 全提供时触发 — 照实记录, 不当作完全无 hint 的 autonomous 验证"
       "wave 22-03 review LLM approve apply gate v1 是 4 opt-in + 6 道 gate + caller_approved=true 显式 — 照实记录, 不当作完全 LLM 自主 apply"
       "wave 22-04 persisted plan inference apply v2 是 4 opt-in + plan_supersede rollback handle, wave21-05 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死 (v2 用 SEPARATE persisted_apply block) — 照实记录, 不当作合并 v1 字段"
       "wave 22-05 autonomous workstation true spawn v1 是 4 opt-in + 12-rule gate matrix + mission_task_delegate substrate (绝不 claude -p) — 照实记录, 不当作 unconditional autonomy"
       "wave 22-06 distill chain policy auto-sonnet v2 是 policy 选择 (single explicit enum) 即 attestation, dual opt-in 移除但 legacy 双 opt-in 路径仍 back-compat coexists — 照实记录, 不当作真无任何 attestation"
       "wave 22-07 smoke v4 是 deterministic + no LLM/spawn/mutating-git — 照实记录, 不当作真 spawn/真 LLM smoke"]
    :wave21-task-09-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 21 没实现的架构 — 只反映 committed implementation truth (8 wave-21 commits 真实状态)"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"
       "wave 21-01 hooks-path installer 是 opt-in repo-local only — 照实记录, 不假装 default-on"
       "wave 21-04 workstation propose 是 propose-only / applied=false / auto_spawn=false 永钉死 — 照实记录, 不当作完整 autonomous spawn"
       "wave 21-05 plan inference apply gate 是 caller approval-gated / persist_inference_applied=false 永钉死 — 照实记录, 不当作完整 persisted plan inference apply"
       "wave 21-06 LLM auto-approve 是 propose-only / applied=false / requires_human=true 永钉死 — 照实记录, 不当作完整 LLM auto-approve apply"
       "wave 21-07 sonnet distill chain 仍 dual opt-in required — 照实记录, 不当作完全自动 sonnet 接 chain"
       "wave 21-08 smoke 是 deterministic + no LLM/spawn/shell — 照实记录, 不当作真 spawn smoke"]
    :wave20-task-10-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 20 没实现的架构 — 只反映 committed implementation truth"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"
       "wave 20-01 task-scope-guard hook 默认未 git config core.hooksPath .githooks 启用 — 照实记录, 不假装默认开"
       "wave 20-07 sonnet_suggest 是 suggest-only / applied=false 永远钉死 — 照实记录, 不当作完整 LLM-augmented apply"]
    :wave19-task-12-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 19 没实现的架构 — 只反映 committed implementation truth"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"
       "wave 19-11 NO-OP (历史已 implemented 自 wave 11 + wave 18-02) 照实记录, 不假装新做"
       "wave 19-04 commit 归属错 (被 wave 19-02 一并入 commit 77f1f2b) 照实记录"]
    :wave18-task-10-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 18 没实现的架构 — 只反映 committed implementation truth"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"]
    :wave17-task-09-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 17 没实现的架构 — 只反映 committed implementation truth"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"]
    :wave16-task-09-non-goal
      ["本任务不改 Rust / SQL / JS / Cargo / 任务文档"
       "本任务不真正压缩主 Lisp"
       "本任务不发明 wave 16 没实现的架构 — 只反映 committed implementation truth"
       "不动 .missiond/v2/intent-event-bus.lisp 正文 (frozen) / .missiond/intent-mcp-defs.lisp"
       "不删 anchor / 不合并不相关 sections / 不重写 event-bus protected"
       "不启动 frontend Lisp (continue postpone 直到 MissionD loop 稳)"]
    :why-no-main-compression-yet
      ["主大 lisp 正文仍是 future code wave 的 anchor"
       "L1 压缩在 wave 13 task 05 已部分完成, L2 物理拆分已在 wave 15 task 02 完成 (5 shard 创建; 主 Lisp 减约 3000 行)"
       "压缩需要的 section-id / status taxonomy / split rule / shard plan 必须先冻结 — 这正是 wave 11 + wave 12 task 06 + wave 13 task 04 + wave 14 task 07 + wave 15 task 02/03/06 + wave 16 task 09 + wave 17 task 09 + wave 18 task 10 的工作"]
    :pre-compression-checklist
      ["section-id 在 source index 已落 (wave 11 完成 7 pillar baseline; wave 12 task 06 扩 7 高变动语义区 +22 entry; wave 13 task 04 扩 +11 entry; wave 14 task 07 扩 +13 entry; wave 15 task 06 扩 +5 entry; wave 16 task 09 扩 +6 entry; wave 17 task 09 扩 +7 entry; wave 18 task 10 扩 +9 entry; wave 19 task 12 扩 +9 entry; wave 20 task 10 扩 +9 entry; wave 21 task 09 扩 +8 entry; wave 22 task 08 扩 +7 entry)"
       "status-taxonomy 已在 architecture-dsl.lisp 冻结 7 值"
       "split-policy 已写明 wait-for-conditions"
       "compression-policy 已写明 forbidden 红线 (ingress/logic-core/egress 不动)"
       "frozen 文件 (event-bus / event-bus-execution) 在本 index 标 protected, 不参与压缩"
       "checker phase-3-precompression 已写入 architecture-dsl.lisp"
       "checker phase-3.1 R015+R016 已 IMPLEMENTED (wave 14 task 05 commit 5c60f82)"
       "checker phase-3.2 R017+R018 + shard auto-discovery 已 IMPLEMENTED (wave 15 task 03 commit b861b9a)"
       "L2 shard split plan 已写入 architecture-dsl.lisp :: l2-shard-split-plan (wave 14 task 07)"
       "L2 shard split 已 EXECUTED — 5 shard 创建 + 28 source-index 重定向 (wave 15 task 02 commit 3f37d32; section-id 全保 R008 + R016)"]
    :unblock-conditions-for-real-compression
      ["条件 1 (wave 14 升级) — file-first writer 已 code-aligned (wave 14 task 01 commit 00cbc1d): 三类 artifact (directive alignment / PLAN / workflow methodology) 全走统一 helper file_artifacts::attempt_artifact_write; resolve_target_project_root; partial 语义; 6 file_* 响应字段; 写者主路径 stable"
       "条件 2 (wave 14 + wave 15 task 04 + wave 16 task 01 + wave 18 task 07) — review gate auto-create v1 + resolution bridge v0 + workflow handler 接入 + review automation policy v0: review_gate policy enum (manual|emit_question|off) + 显式 resolution input (review_decision + review_actor + review_note) + envelope validator 5 fail-fast 错误码; 接到 directive (approve/archive) + plan (approve/mark/supersede) + workflow (resolve_review with stamp-only on workflow row, methodology receipt-only); wave 18 task 07 加 review_automation_policy (manual|suggest|auto_safe) + 5 deterministic safety rule + caller_opt_in 必填 + 绝不 auto_reject + destructive transitions 永不 auto-promote; 3 surface 全接 + automation policy → code-aligned (留 LLM 自动 approve / 自动 answer review question 仍 v0 non-goal)"
       "条件 3 (wave 14 + wave 16 task 04/05 + wave 17 task 01/02/03/04/05 + wave 18 task 03/04/05/06) — PlanNodeStateChanged variant + live EventRef 三层策略 + paused 7th lifecycle + per-node retry policy v0 + paused-resume hook v0 + claim-lease v0 + acceptance evaluator v0 三模式 + rollback policy v0 三模式 + finalize/distill trigger v0 + cross-node acceptance fan-in v0 + cascade rollback v0 + cross-plan distill chain v0 + autonomous PLAN field inference v0: 11-stage scheduler 几乎全部 close (s5 claim ✓ / s7 acceptance + cross-node fan-in ✓ / s8 paused-resume ✓ / s9 rollback + cascade ✓ / s10 mark-final ✓ / s11 distill 触发 + cross-plan chain ✓); 仍 partial 是因为: enforce_claims default false (opt-in 兼容期) / sonnet 自动接 chain (无需 caller chain_id) / LLM-augmented PLAN inference / compensate-node :ref forward 引用 仍 future"
       "条件 4 (wave 14 升级 + wave 17 task 08 + wave 18 task 09) — unified-entry pipeline v1 已 code-aligned + wave 16 task 08 e2e smoke + wave 17 task 08 paused-resume e2e smoke + wave 18 task 09 autonomous loop smoke v1: build_plan_execute_args forward 4 wave-18 keys (distill_chain_* + infer_plan_fields), 5 stage smoke 走通 wave17/18 多任务 (no LLM no spawn no shell); 不新增 MCP tool (仍 83); 4 v0 non-goal 中 review-resolution / workstation-dispatch / paused-resume listener / review automation policy / inference 已缓解; auto_approve_directive / auto_approve_plan / autonomous_workstation_dispatch 完整 LLM 版 + 自动 answer review question 仍 surface"
       "条件 5 (wave 14 + wave 16 task 07 + wave 17 task 06 + wave 18 task 01) — evidence collector live event ref 三层策略 + passive subscriber cache + event-log query path v0 三档 resolver + event-log query API v1 typed builder + EventRefProvenance 4 值: 现可 (1) live → live id; (2) passive_cache 命中 → cache id; (3) event_log_query 命中 → log id; (4) unavailable + reason 兜底; EVENT_LOG_QUERY_LIMIT_CAP=512 硬上限; primary event_ref_status / event_ref_source / event_ref_warning 加在 EvidenceEntry surface; 系统化 query API typed 已通; dedicated store method (例如 by_topic 索引下推) 仍 future"
       "条件 6 (wave 15 task 02) — L2 shard split executed: 5 shard 文件 (intent-execution-governance / intent-directive-artifacts / intent-plan-dag / intent-capability-governance / intent-workstation-policy); section-id 全保; 内容 byte-identical"
       "条件 7 (wave 15 task 03) — shard-aware checker: R017 (source-file 必存在) + R018 (source-file 必 .missiond/v2/) + 自动 shard 发现 (data-driven); --dry-fixture 5 → 10; --all-v2 20 文件 OK (含 wave 19 machine-contract layer)"
       "条件 8 (wave 15 task 05 + wave 16 task 03 + wave 18 task 06) — workstation-dispatch v0 opt-in + auto-inference v1 + autonomous PLAN field inference deterministic: PLAN node :workstation-dispatch true 触发 mission_task_delegate; 5 inference rules 对 mission_task_delegate scoped node 自动推断; workstation_dispatch_source 5 值 surface; wave 18 task 06 把推理扩到 6 字段 (含 workstation_dispatch) + 3 confidence + apply_safe 仅 high (无 LLM); 完全 autonomous spawn (无任何 hint, 全局 LLM 推断) 仍 pending"
       "条件 9 (wave 16 task 02 + wave 17 task 01) — review-gate QuestionEvent::Resolved 订阅 listener v0 + plan-node action 自动 route: spawn_review_resolution_sub 与 spawn_decision_sub 并行; conservative vocabulary mapping; ack 后 ignore 非 review id; wave 17 task 01 加 plan-node action 自动调 paused-node resume helper (approved → 重 dispatch / rejected/needs_changes → 写 evidence keep paused); 仍是 caller-driven resolution drives, 不替人答 (auto_answer_review_question 仍 surface)"
       "条件 10 (wave 16 task 06 + wave 17 task 07 + wave 18 task 08) — scoped commit handoff daemon enforce v0 (opt-in default false 字节兼容; 4 错误码; gate 在 allocate_id 之前; 不跑 git mutating) + brief-layer enforce 默认开 (workstation_dispatch.rs 任务 brief section 7) + read-only preflight tool (mission_execution action='preflight_commit' 只调 git status --porcelain=v1 0 mutating subcommand, 返结构化 ok/changed_files/staged_files/out_of_scope_files/expected_*/next_step); brief → preflight → complete enforce 三段闭环; daemon 仍不跑 git mutating commands; 后续 enforce-by-default 切默认 + git pre-commit hook 执行 scope check 仍 pending"
       "条件 11 (wave 18 task 02) — ExecutionEvent dispatch metadata v1: ExecutionEvent::Claimed + ExecutionEvent::Completed 加 dispatch_strategy / target_project / requested_cwd (serde-bw-compat, 不要求 caller 重传, 从 companion log meta 读); 闭环 wave 14-02 留下的 'PlanNodeStateChanged 之外的 variants 仍未扩同字段' 部分 (Claimed + Completed 闭, ExecutionEvent::Opened 仍未扩同字段)"]
    :wave-18-status-summary
      ["条件 1 → code-aligned (file-first writer integration; wave 14)"
       "条件 2 → code-aligned (review gate auto-create v1 + resolution bridge v0 + workflow handler 接入 + review automation policy v0 by wave 18 task 07; LLM 自动 approve / 自动 answer review 仍 v0 non-goal)"
       "条件 3 → code-aligned (PlanNodeStateChanged variant + live ref + paused 7th + retry v0 + paused-resume + claim-lease + acceptance + rollback + finalize/distill + cross-node acceptance fan-in + cascade rollback + cross-plan distill chain + autonomous PLAN field inference 已落; 留 enforce_claims default / sonnet 自动接 chain / LLM PLAN inference / compensate-node :ref forward 引用 仍 future)"
       "条件 4 → code-aligned (unified-entry pipeline v1 + wave 16 task 08 e2e smoke + wave 17 task 08 paused-resume e2e smoke + wave 18 task 09 autonomous loop smoke v1; LLM 自动 approve/answer 4 v0 non-goal 仍 surface)"
       "条件 5 → code-aligned (live event ref 三层策略 + subscriber 三档 + cap 1024 FIFO cache + log query 三档 resolver + event-log query API v1 typed builder + EventRefProvenance 4 值 + 512 cap by wave 18 task 01)"
       "条件 6 → code-aligned (L2 shard split executed)"
       "条件 7 → code-aligned (R015+R016 + R017+R018 + shard auto-discovery; --all-v2 20 files OK)"
       "条件 8 → code-aligned (workstation dispatch v0 opt-in + auto-inference v1 + autonomous PLAN field inference deterministic 6 字段 by wave 18 task 06; 完全 autonomous LLM 推断 spawn 仍 pending)"
       "条件 9 → code-aligned (review-gate listener subscriber v0 + plan-node action 自动 route; auto_answer_review_question 仍 surface)"
       "条件 10 → code-aligned (scoped commit daemon enforce v0 opt-in + brief-layer enforce 默认开 + read-only preflight tool by wave 18 task 08; daemon 仍不跑 git mutating; git pre-commit hook 仍 pending)"
       "条件 11 → code-aligned (ExecutionEvent dispatch metadata v1 by wave 18 task 02: Claimed + Completed 闭环 wave 14-02 留下的 variants 部分; ExecutionEvent::Opened 仍 future)"
       "extensible domain count test → code-aligned (wave 15 task 01 — 不 hardcode count, 走 contains+floor)"
       "unified-entry e2e smoke → operational-practice (wave 16 task 08 deterministic 4 hand-off no-LLM + wave 17 task 08 paused-resume 6 步 round-trip + wave 18 task 09 autonomous loop 5 stage)"
       "PLAN DAG cross-node acceptance fan-in v0 → code-aligned (wave 18 task 03)"
       "PLAN DAG cascade rollback v0 → code-aligned (wave 18 task 04)"
       "cross-plan distill chain v0 → code-aligned (wave 18 task 05; sidecar 无 SQL migration)"
       "autonomous PLAN field inference v0 → code-aligned-partial (wave 18 task 06; deterministic 6 字段, LLM-augmented 仍 future)"
       "review automation policy v0 → code-aligned-partial (wave 18 task 07; deterministic 5 safety rule, LLM 自动 approve 仍 v0 non-goal)"
       "scoped-commit worktree preflight v0 → code-aligned (wave 18 task 08; mission_execution(action='preflight_commit') 0 mutating subcommand)"
       "evidence event-log query API v1 → code-aligned (wave 18 task 01; typed builder + EventRefProvenance 4 值 + 512 cap)"
       "ExecutionEvent dispatch metadata v1 → code-aligned (wave 18 task 02; Claimed + Completed 加 3 字段 serde-bw-compat)"]
    :wave-19-status-summary
      ["machine-contract task-contract-v1 → code-aligned (wave 19 task 02-08 全 close: verifier + report + shared-memory + renderer + plan emitter + workstation consumer + execution completion)"
       "machine-contract task-verifier v1 → code-aligned (wave 19 task 02 commit 77f1f2b; 5 项检查 + --commit/--json/--dry-fixture; read-only 0 mutating git)"
       "machine-contract report-contract v1 → code-aligned (wave 19 task 03 commit ba58f20; 7 字段 + 10 fixtures + sample report)"
       "machine-contract shared-memory v1 → code-aligned (wave 19 task 04 commit 77f1f2b 吸入 wave19-02 commit; 6 entry types + 13 fixtures + seed)"
       "machine-contract renderer dispatch brief v1 → code-aligned (wave 19 task 05 commit c95eba8; 4 新节 + agent-team literal 单实例 + verify command)"
       "plan task-contract emitter v0 → code-aligned (wave 19 task 06 commit 5d425e2; emit_task_contract opt-in + .missiond/tasks/generated/<plan_id>/<node_id>.lisp + emit before dispatch + default off byte-compat)"
       "workstation task-contract consumer v0 → code-aligned (wave 19 task 07 commit bfc72b7; overlay_contract + MalformedTaskContract SafeDescriptor + 绝不 fall back claude -p)"
       "execution task-contract completion verification v0 → code-aligned (wave 19 task 08 commit 405d13b; 4 字段 + 2 新错误码 + claim scope ⊇ contract write-scope + daemon 仍 read-only)"
       "cross-plan distill auto-chain v1 → code-aligned (wave 19 task 09 commit e57b56d; auto_chain opt-in + deterministic id 'chain:auto:wf-<sha256(...)>' + sonnet 仍仅显式)"
       "PLAN DAG forward compensate ref v0 → code-aligned (wave 19 task 10 commit 246a675; :compensate-node / :compensate-ref 双拼写 + 4 拒绝条件 + union forward+reverse)"
       "ExecutionEvent::Opened dispatch metadata wave 19-11 → code-aligned NO-OP (历史已 implemented 自 wave 11 commit fc58018 + wave 18-02 commit 2fbaa88; 字段 + wiring + round-trip tests 全在; 无新 commit)"
       "wave-19-04 commit 归属错 (被 wave 19-02 commit 77f1f2b 一并入 main) — 功能落地正确"]
    :wave-20-status-summary
      ["task-scope-index-guard v1 → code-aligned-partial (wave 20 task 01 commit 1fc0fd6; staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发 + 9+3 fixtures + 0 mutating git; 默认未 git config core.hooksPath .githooks 启用)"
       "renderer scoped-commit guard v2 → code-aligned (wave 20 task 02 commit b36cf6c; Commit 节加 task-scope-guard --mode staged 子步 + MISSIOND_TASK_CONTRACT env prefix; brief → preflight → staged guard → commit → verifier 五段闭环)"
       "execution preflight task-contract scope v1 → code-aligned (wave 20 task 03 commit fe835e8; 8 新 preflight 字段 task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path; 无新错误码 信息性; grep proof 0 mutating git; legacy byte-compat)"
       "machine-driven dispatch v0 → code-aligned (wave 20 task 04 commit 681c95d; DispatchContractMode {Rendered, Machine} + dispatch_contract_mode arg / render_markdown shorthand; Machine 模式接通 wave-19/07 dormant consumer; **Lisp 真正成 dispatch SSOT**, Markdown brief 不再 load-bearing; 默认 Rendered 不破 wave-15..19 byte-shape; unified_entry forward 4 emitter knobs)"
       "unified-entry machine-loop smoke v2 → code-aligned (wave 20 task 05 commit d308fae; 6 smoke tests + build_artifact_refs lift 8 个 machine-contract 字段 + wave-15..19 byte-compat 钉死 + Markdown non-load-bearing assertion + malformed refusal smoke)"
       "cross-plan distill auto-trigger v1 → code-aligned (wave 20 task 06 commit 3669ebc; auto_chain_trigger default 'never' / 'deterministic_only'; 6 deterministic trigger rule 全过才触发; sonnet 仍仅显式; default off byte-compat)"
       "LLM-augmented plan inference v0 → code-aligned-partial (wave 20 task 07 commit 6bb935a; infer_plan_fields=sonnet_suggest mode opt-in / **suggest only / applied=false 永远钉死**; Sonnet 不可用 llm_unavailable no fallback; DAG mode refuse_llm_inference_in_dag_mode 拒; default off byte-compat)"
       "review auto-answer policy v0 → code-aligned (wave 20 task 08 commit 8adb0a8; auto_answer_policy off|deterministic_safe|dry_run; 5 inherited rules from wave-18/07 + 2 new guards destructive_action / caller_decision_present; **3 hard invariants test-pinned**: I1 NEVER auto-reject / I2 destructive NEVER auto-promote / I3 NO LLM; unified_entry forward + build_artifact_refs lift 5 outcome keys; default off byte-compat)"
       "ExecutionEvent legacy metadata sweep v0 → code-aligned (wave 20 task 09 commit 6e01e3f; 8 legacy variants 全加 dispatch trio Heartbeat/Released/DeviationRecorded/DecisionRecorded/IssueRecorded/Audited/Repaired/StaleClaim; 从 companion log read_dispatch_metadata_from_log 自动继承; **wave14-02 启动的 ExecutionEvent metadata 协议在 wave 20-09 全 11 variants 闭环**)"
       "review-automation-policy-v0 status upgrade → code-aligned (wave 20 task 08 加 auto_answer_policy 闭环 5+2 rule + 3 invariants; LLM 自动 approve 仍 v0 non-goal)"
       "ExecutionEvent dispatch-metadata-v1 status note 扩 → code-aligned (wave 20 task 09 完成 8 legacy variants 全覆盖; 11 variants 全 dispatch trio 闭环)"
       "machine-contract task-contract-v1 status note 扩 → code-aligned (wave 20 task 04 machine 模式接通后 Lisp 真正成为 dispatch SSOT, Markdown 仍存为兼容视图但 non-load-bearing)"]
    :wave-21-status-summary
      ["hooks-path installer v1 → code-aligned-partial (wave 21 task 01 commit 44c74df; scripts/install-missiond-hooks.mjs --check/--install/--json/--dry-fixture/--strict + scripts/check-missiond-hooks.mjs 只读 doctor + .githooks/pre-commit 保 MISSIOND_TASK_CONTRACT env-gated; --install 跑 git config --local core.hooksPath .githooks 一次 + already-aligned no-op + never --global/--system; **opt-in repo-local only — 不擅自 default-on**)"
       "task-run verifier v1 → code-aligned (wave 21 task 02 commit 1335fa7; scripts/verify-task-run.mjs 三合一 task contract + report task_id + commit_hash + memory completion + 14 forbidden git verb proof + dogfood self-verify; verify-task-contract.mjs main() gated by import.meta.url 让 importers 不触发 CLI parsing)"
       "execution report-verifier integration v1 → code-aligned (wave 21 task 03 commit 308426e; mission_execution(complete) 加 4 新字段 task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified + verified=true 触发 daemon-internal sexp cross-check 无新 dep + 4 structured error codes VERIFIED_REQUIRES_* / TASK_REPORT_TASK_ID_MISMATCH / TASK_REPORT_COMMIT_HASH_MISMATCH / TASK_CONTRACT_MALFORMED + 4 字段持久化为 Lisp keyword + daemon NEVER spawns Node + daemon_never_invokes_mutating_git unit-pinned)"
       "autonomous workstation LLM proposal v0 → code-aligned-partial (wave 21 task 04 commit 68b84f1; workstation_inference_mode=off|sonnet_suggest opt-in + WorkstationProposalGate 仅在 caller+PLAN 全空才 propose + 4 propose 字段 × 3 confidence × 4 safety status + workstation_proposals[] cap 6; **propose only never auto-spawn — applied=false / auto_spawn=false 永钉死**; conservative whitelists; Sonnet unavailable surfaces llm_unavailable + reason 钉 'no fallback to claude -p / prompt mode'; DAG mode preflight rejects sonnet_suggest INVALID_PARAM)"
       "plan inference apply gate v1 → code-aligned-partial (wave 21 task 05 commit a18200b; apply_inferred_fields=true + llm_caller_approved + persist_inference opt-in apply gate; **6 道严格 gate** caller_approval / master_flag / confidence ∈ {high (deterministic) | high+medium (LLM)} / conflict_status=none / per-field safety / slot availability; **8 skip reason canonical**; apply_gate block 在每 dispatch 分支 surface; **persisted plan.sexp_text 永不 mutate** persist_inference_applied=false 永钉死; strict shape (typo string \"true\" → INVALID_PARAM); ApplySafe back-compat preserved)"
       "LLM auto-approve proposal v0 → code-aligned-partial (wave 21 task 06 commit e140773; auto_approve_mode=off|sonnet_suggest opt-in for directive (approve|archive) + plan (approve|mark|supersede) review surfaces; ORTHOGONAL to wave-18/07 + wave-20/08 三 review knob 共存; **5 invariants test-pinned**: I1 NEVER auto-reject / I2 destructive (archive|supersede|remove) blocked WITHOUT calling Sonnet / I3 applied=false + requires_human=true 永钉死 / I4 sonnet unavailable no fallback / I5 destructive_check ALWAYS from is_destructive_review_action helper; 22 unit tests; 10 dispatch branch sites; strict-enum parse INVALID_PARAM; caller-supplied review_decision ALWAYS wins)"
       "sonnet distill chain auto-apply v1 → code-aligned-partial (wave 21 task 07 commit 4d494db; auto_sonnet=true + auto_sonnet_approved=true 双 opt-in apply-gate; **7 重 gate** 双 opt-in + auto_chain_trigger=auto_safe + ALL 6 wave-20 deterministic safety rule + caller distill_mode != sonnet; auto-promote inner distill from dry_run to sonnet via direct call; **8 status taxonomy** + **4 model_call_status taxonomy**; **7 invariants test-pinned** I1-I7 default-off / dual opt-in / reuse trigger never relax / caller-already-sonnet refusal / sonnet failure preserve inner / review_required=true PINNED / wave-19/20 blocks UNCHANGED purely additive; strict shape validation INVALID_PARAM)"
       "machine-contract autonomous loop smoke v3 → code-aligned (wave 21 task 08 commit 8ba8723; 15 deterministic e2e tests across 4 handler files; pins wave21-03 verifier 5-rule cross-check 5 path / wave21-04 I3+I4+I5 / wave21-06 I1+I2+I3+I5 / wave21-07 I1+I3+I7 / Markdown task_brief_preview NEVER 进 artifact_refs (二度钉死) / machine dispatch SSOT task_contract_path == task_contract_source_path 钉死; no LLM, no spawn, no shell)"
       "review-automation-policy-v0 status note 扩 → code-aligned (wave 21 task 06 加 第三 review knob auto_approve_mode propose-only with 5 invariants; 三 review knob 共存独立观测 review_automation_policy + auto_answer_policy + auto_approve_mode; LLM 自动 approve apply 仍 v0 non-goal future)"
       "machine-contract task-contract-v1 status note 扩 → code-aligned (wave 21 task 02/03 加 task-run verifier 三合一 + execution report-verifier daemon-internal verified gate; **真正 6 段闭环 brief → preflight → staged guard → commit → verifier (post-commit) → execution complete verify (daemon-internal)**)"]
    :wave-22-status-summary
      ["hooks default-on doctor v2 → code-aligned-partial (wave 22 task 01 commit 49555c4; install-missiond-hooks.mjs default mode 无 flag = --check 只读 doctor (read-only); --install 仍唯一 mutation entry; doctor JSON payload 加 :severity (ok|preflight-drift), :reason (4 codes: aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing), :advice, :install_command; renderer renderHooksDoctorPreflight() 块在每 :commit :required brief commit section 上方; dry-fixture 8→11; **opt-in repo-local only — git config 真 mutation 仍 caller 显式 --install**)"
       "execution auto-run-verifier v2 → code-aligned (wave 22 task 02 commit 02ac627; daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process (task_contract_loadable / task_report_loadable / task_report_schema / task_id_matches_contract / commit_hash_matches_report / shared_memory_loadable / shared_memory_schema / shared_memory_completion_for_task); 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 (verification_source='daemon-auto-verifier'); legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' 不硬拒; 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威; daemon_never_invokes_mutating_git unit-pinned)"
       "review LLM approve apply gate v1 → code-aligned-partial (wave 22 task 03 commit 4b55cb4; apply_llm_auto_approve=true + proposal_hash (32-hex deterministic SHA-256) + caller_approved=true 4 opt-in; **6 道严格 gate** G1 apply flag G2 deterministic SHA-256 hash matches G3 caller_approved=true G4 deterministic non-destructive G5 decision=approved G6 confidence=high; 3 新错误码 APPLY_GATE_MISSING_PROPOSAL_HASH / APPLY_GATE_PROPOSAL_HASH_MISMATCH / APPLY_GATE_INVALID_PARAM (hash mismatch fail-fast BEFORE DB mutation); 只 legacy quiet `action=approve` 路径触发 DB transition; **wave-21/06 5 invariants 全 preserved 5 dedicated tests**; explicit review_decision 路径 gate 仍仅 informational; destructive actions ALWAYS skip with SkippedDestructiveAction)"
       "persisted plan inference apply v2 → code-aligned (wave 22 task 04 commit fee6567; persist_inference=true + caller_approved=true + proposal_hash + apply_inferred_fields=true 4 opt-in apply gate; compute_inference_proposal_hash 32-hex SHA-256 over (plan_id, original_sexp_hash, sorted applied_fields); execute_persisted_apply DB path: plan_list_by_task → plan_insert(next-version) with original sexp 加 (plan-inference-applied :inference-version v2 ...) annotation → plan_supersede(old_id) → append_plan_evidence_entry typed source/kind=plan_inference_persisted_apply with rollback_plan_id=predecessor; 2 新错误码 PERSIST_APPLY_MISSING_PROPOSAL_HASH / PERSIST_APPLY_PROPOSAL_HASH_MISMATCH; **wave-21/05 6 invariants 全 preserved 7 dedicated tests**; **I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死**, v2 用 SEPARATE `persisted_apply` block surface 状态)"
       "autonomous workstation true spawn v1 → code-aligned-partial (wave 22 task 05 commit 162a303; auto_spawn=true + workstation_caller_approved=true + preflight_status_acceptable=true + workstation_proposal_hash 4 opt-in (mode=sonnet_suggest 是前置必填); **12-rule gate matrix** G1 auto_spawn opt-in / G2 bundle Suggested / G3 hash matches / G4 ALL safety_status=safe / G5 ALL confidence=high / G6 caller_approved=true / G7 preflight_acceptable=true / G8 task_contract_path supplied / G9 contract loads ok / G10 :write-scope non-empty / G11 :write-scope non-overlap with :must-not-touch / G12 proposed target=mission_task_delegate; 走 `mission_task_delegate` substrate **绝不 `claude -p`**; 3 新错误码 AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH; 15 status taxonomy; **wave-21/04 4 invariants 全 preserved 4 dedicated tests** default off / Sonnet unavailable no fallback / propose-only fields preserved on wave-21/04 surface / DAG mode rejects sonnet_suggest at preflight)"
       "distill chain policy auto-sonnet v2 → code-aligned (wave 22 task 06 commit 2423d4b; auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} 单一 policy 选择即 attestation; **dual opt-in 移除** — policy 选择即 explicit operator attestation; legacy auto_sonnet=true + auto_sonnet_approved=true 双 opt-in 路径仍 back-compat coexists; safe_after_rules 触发要 ALL 6 wave-20 rule pass + trigger=auto_safe + distill_mode != sonnet; dry_run 完整 evaluate 仅 surface 不 spawn; 8 AUTO_SONNET_POLICY_STATUS_* wire-string constants; strict closed-enum parse rejects 10 typo / camelCase / case-mismatch / shape-mismatch inputs; **wave-21/07 7 invariants 全 preserved 7 dedicated tests** (I7 验证 4 块 coexistence wave-19 auto_chain + wave-20 auto_chain_trigger + wave-21/07 dual opt-in path + wave-22/06 policy path))"
       "autonomous loop apply smoke v4 → code-aligned (wave 22 task 07 commit 6b2125c; 9 new deterministic smoke tests across 5 in-scope handler files 覆盖 wave22-02/03/04/05/06; unified_entry +1 envelope smoke pinning all 22 cross-wave invariants + Markdown task_brief_preview NEVER 进 artifact_refs 三度钉死; agent_execution +2 verifier failure paths SHARED_MEMORY_NO_COMPLETION_FOR_TASK + TASK_REPORT_COMMIT_HASH_MISMATCH; plan +2 persisted apply gate fixture hash accept + missing hash reject + wave-21/05 6 invariants pinned; workstation_dispatch +2 auto-spawn gate fixture hash accept + missing hash reject + wave-21/04 4 invariants pinned; review_gate +2 review apply gate fixture hash accept + missing hash reject + wave-21/06 5 invariants pinned; **22 cross-wave invariants pinned** (wave21-04 4 + wave21-05 6 + wave21-06 5 + wave21-07 7 = 22); no real LLM (synthesised proposal/bundle structs); no real spawn (gate evaluators 终止于 Spawned 不调 substrate); no mutating git (tempfile fixtures))"
       "machine-contract task-contract-v1 status note 扩 → code-aligned (wave 22 task 01 加 hooks default-on doctor v2 + wave 22 task 02 加 daemon-internal auto-run-verifier 8 cross-checks; **brief→preflight→staged guard→commit→out-of-process verifier (wave21-02)→execution complete + daemon-internal auto-verifier (wave22-02) 真正 6 段闭环 升级到 daemon 自跑 verifier**)"]
    :wave-23-status-summary
      ["session-trace v1 → code-aligned-partial (wave 23 tasks 01/04/05/06; schema/checker + daemon append + plan/workstation propagation + analyzer; :session-trace-writable default false opt-in)"
       "report-contract trace explanation fields → code-aligned (wave 23 task 02; five optional prose fields + trace_refs; facts remain in session-trace.lisp)"
       "verify-task-run --trace / --require-trace → code-aligned (wave 23 task 03; checker can require trace proof)"
       "trace analyzer → code-aligned-partial (wave 23 task 06; descriptive bottleneck rules only, no routing/model policy)"
       "trace-derived router policy → architecture-designed (wave 23 task 07; backend classes drafted, no runtime router replacement)"
       "lisp backfill wave23 status → in-progress by Codex; frontend Lisp still postponed"]
    :wave-24-status-summary
      ["router-policy v1 → code-aligned (wave 24 task 01 commit 988f7d8; schema/checker/seed + 3 advisory rules; dry-run-only=true and runtime-replacement=false required)"
       "trace corpus index v0 → code-aligned (wave 24 task 02 commit e61088b; 8 top-level JSON keys; reuses wave23 bottleneck thresholds; no backend recommendation)"
       "router recommendation CLI v0 → code-aligned (wave 24 task 03 commits d6d8e10+ef8fc5f; read-only deterministic; dry_run_only=true; no LLM/spawn/git; fallback claudecode low confidence when no rule matches)"
       "mission_plan router dry-run surface → code-aligned-partial (wave 24 task 04 commit b8721ab; router_policy_mode=dry_run returns applied=false literal; apply/auto/unknown INVALID_PARAM before plan lookup; Rust and Node algorithms independent)"
       "renderer router advisory context → code-aligned (wave 24 task 05 commit 294a92a; :router-policy-path + Router Policy (advisory) / dry-run only block; Session Trace section byte-identical)"
       "router dry-run full-chain smoke → code-aligned (wave 24 task 06 commit 6afe541; pins Node/Rust/renderer invariants, no runtime backend replacement)"
       "trace-derived router policy status upgrade → code-aligned-partial for advisory dry-run chain; runtime replacement remains pending"]
    :wave-25-status-summary
      ["router-policy corpus evaluator → code-aligned (wave 25 task 01 commit 8dbe85f; 67 real task contracts; backend counts claudecode=49 / deterministic-checker=14 / verifier-worker=4; confidence high=6 / medium=5 / low=56; read-only no shell/git/LLM)"
       "report-contract router fields → code-aligned (wave 25 task 02 commit 7709031; seven optional fields; strict atom booleans reject string true/false bypass)"
       "mission_plan trace-index confidence → code-aligned-partial (wave 25 task 03 commit bd2b5a3; router_policy_trace_index_path optional; used/missing/unreadable/malformed statuses; absent/off preserve byte-shape and block I/O)"
       "renderer recommendation command → code-aligned (wave 25 task 04 commit e1fdbe4; parameterized recommend-task-backend command + MAY report fields; renderer never shells out)"
       "router measurement smoke → code-aligned (wave 25 task 05 commit 0f5d857; CLI/Rust parity for rich trace evidence >=5; dry_run_only/applied/runtime_replacement invariants pinned)"
       "trace-derived router policy status remains code-aligned-partial: measurement loop improves observability, not runtime backend replacement"]
    :wave-26-status-summary
      ["router backend readiness registry → code-aligned (wave 26 task 01 commit 2ac6a5b; 5 backend enum + current-default/advisory-only/runtime-ready/unavailable readiness taxonomy + checker)"
       "recommend/evaluate readiness annotations → code-aligned (wave 26 task 02 commit ad8ec04; --backend-registry + apply blockers; real corpus 75 tasks apply_eligible_count=0)"
       "mission_plan router_backend_registry_path → code-aligned-partial (wave 26 task 03 commit fcd937a; dry-run only, off/default no-I/O byte-shape)"
       "report/renderer readiness fields → code-aligned (wave 26 tasks 04/05 commits 44ddf9d/43df6230; five report fields + check-router-backend-registry command + MUST NOT switch backend)"
       "router readiness smoke → code-aligned (wave 26 task 06 commit 4bfa710; 5-layer smoke pins current-default not apply-eligible and no shell/LLM/git/network)"
       "trace-derived router policy remains code-aligned-partial: readiness improves evidence but no runtime consumer applies router_apply_eligible"]
    :wave-27-status-summary
      ["router dispatch descriptor schema/checker → code-aligned (wave 27 task 01 commit f451b04; 13 required fields + literal dry_run_only/runtime_replacement/no_execution invariants)"
       "router dispatch descriptor CLI → code-aligned (wave 27 task 02 commits 14fdf5a + 752fe40; read-only build from task/policy/registry, validates before emit)"
       "mission_plan router_dispatch_descriptor → code-aligned-partial (wave 27 task 03 commit 6e4f14d; dry-run evidence only, off/default no-I/O byte-shape)"
       "report/renderer descriptor fields → code-aligned (wave 27 tasks 04/05 commits afb5ffb/17cb401; six report fields + build/check commands, no static descriptor path field)"
       "router dispatch descriptor smoke → code-aligned (wave 27 task 06 commit 7f65f05; 5-layer smoke pins no-execution descriptor invariants)"
       "trace-derived router policy remains code-aligned-partial: descriptor improves machine handoff but runtime backend replacement stays pending"]
    :next-step
      ["条件全满足后 (11 条件全 code-aligned; 完整 11-stage PLAN DAG scheduler 已 close 主线; wave 19 加 machine-contract task SSOT 全闭环; wave 20 加 machine-driven dispatch + scoped-commit 全 5 段闭环 + ExecutionEvent metadata 11 variants 闭环 + review auto-answer deterministic_safe; wave 21 加 propose+apply-gate 范式覆盖 4 路 LLM/inference 通道 + execution 端 daemon-internal verified gate 6 段闭环 + hooks-path installer + 三合一 run verifier; **wave 22 加 explicit-gate-promotion 范式 — wave-21 propose-only 通道全部升级到 explicit-apply-gate (review LLM approve apply gate v1 6 道 gate / persisted plan inference apply v2 plan_supersede rollback handle / autonomous workstation true spawn v1 12-rule gate matrix mission_task_delegate substrate 绝不 claude -p / distill chain policy auto-sonnet v2 dual opt-in 移除 policy 即 attestation) + execution auto-run-verifier v2 daemon-internal 8 cross-checks (verification_source taxonomy) + hooks default-on doctor v2 (--check 只读 doctor + 4 reason codes + renderer renderHooksDoctorPreflight() 块) + autonomous loop apply smoke v4 22 cross-wave invariants pinned** — 留 完全 LLM 自主无任何 caller opt-in (wave22-03/04/05/06 仍 require explicit caller_approval/proposal_hash/policy/auto_spawn opt-in) / Sonnet 真无任何 attestation (wave22-06 policy=safe_after_rules 仍是 explicit policy 选择即 attestation) / git hooks default-on real install (wave22-01 仍 doctor only — caller 必须显式 git config core.hooksPath .githooks 才生效) / Auto-seed shared-memory ledger claim entry on parallel workstation spawn (wave22-05 真 spawn 已落, ledger seed 仍 future) / report-contract checker auto-invoke without caller-supplied 4 paths (wave22-02 仍要求 caller 提供 4 路径才触发 daemon-internal verifier) / frontend Lisp 仍 future), 由 lisp-review skill 牵头, 按 compression-policy.allowed 三类做批量压缩"
       "wave 22 explicit-gate-promotion 范式 — 后续可推: (a) wave22-05 workstation true spawn 升级到 完全 LLM 自主 (无 caller_approved opt-in, plan-runner 全局推断 + 真 spawn — 尚需 ledger auto-seed claim entry); (b) wave22-04 persisted plan inference apply 升级到 自主 persist (无 caller_approved opt-in, 自动判断 deterministic high-confidence 即 persist + 自动 rollback policy); (c) wave22-03 review LLM approve apply gate 升级到 完全自主 (无 caller_approved opt-in, 自动 6 gate pass 即 apply); (d) wave22-06 distill chain policy 升级到 真无任何 attestation (plan-runner 全局推断 + auto-promote sonnet 而无需 policy 选择); (e) wave22-01 hooks default-on doctor 升级到 default-on real install (default-mode = --install 自动跑 git config core.hooksPath .githooks); (f) wave22-02 daemon-internal auto-verifier 升级到 自动 spawn (verifier 路径无需 caller 提供 4 路径, daemon 从 task contract / report cache 自动 resolve)"
       "wave 21 propose+apply-gate 范式 — 后续可推: (a) workstation propose 升级到 真 spawn (caller approval-gated → 真 mission_task_delegate spawn) — wave 22-05 已闭环; (b) plan inference apply gate 升级到 persist (persist_inference_applied=true 真改 plan.sexp_text 落盘) — wave 22-04 已闭环; (c) LLM auto-approve propose 升级到 真落 review row state transition — wave 22-03 已闭环 (caller_approved 显式仍必填); (d) sonnet distill chain 升级到 完全自动 (无需 caller dual opt-in) — wave 22-06 已部分闭环 (policy 选择即 attestation, dual opt-in 移除); (e) hooks-path installer 升级到 default-on (git config core.hooksPath .githooks 默认安装) — wave 22-01 仅升级到 default-on doctor, real install 仍 caller 显式; (f) report-contract checker auto-invoke from daemon — wave 22-02 已部分闭环 (caller 仍需提供 4 路径)"
       "wave 20 闭环 machine-driven dispatch / wave 19 闭环 task-contract dispatch SSOT 仍 hold; 后续 shared-memory ledger 在并行工位 spawn 时自动 seed claim entry"
       "压缩 PR 必须带 git diff --check + checker --all-v2 + 对应 *-execution.lisp D-deviation"
       "继续切 enforce default — claim-lease / scoped-commit 切 enforce-by-default + git pre-commit hook 默认启用 + git hooksPath 默认指向 .githooks (worktree-level scope check)"
       "把 5 wave-15-shard 内若干 status-only 文本压缩 — 只压 status 句子, schema/contract 段不动"
       "压缩前以 :compression-safe? 字段做白名单过滤 — false 的 section 即使在 compression-policy.allowed 之内也保留正文"
       "frontend Lisp 仍 postpone 直到 MissionD loop 稳"]))
