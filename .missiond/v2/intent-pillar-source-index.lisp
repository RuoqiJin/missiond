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
      :effect "出现重复 D010, 说明共享执行层必须引入正式 ID allocator 与 claim lease")))
